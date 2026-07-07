use anyhow::{Context, Result, bail};
use std::process::ExitStatus;
use tokio::{select, task::JoinHandle};

use crate::{
    config::{MainConfig, RuntimeModes},
    dap_message::{DapMessage, RequestCommandTypes},
    dap_stream::{DapStream, ReadResult},
    file_watcher::FileWatcher,
    ide::{IdeHandler, IdeStatus},
    log,
    logger::{LogLevel, LogSource},
    proxy_state::ProxyState,
    runtime::Runtime,
};

#[derive(PartialEq)]
enum StreamSources {
    Ide,
    Adapter,
}

#[derive(Default, PartialEq, Debug)]
pub enum RuntimeStatus {
    #[default]
    Pending,
    Building,
    BuildingFailed,
    Spawned,
    Replaying,
    Debugging,
}

pub struct Proxy {
    config: MainConfig,
    state: ProxyState,

    // IDE
    ide: IdeHandler,
    ide_stream: Option<DapStream>,
    ide_status: IdeStatus,

    // Runtime
    runtime: Runtime,
    runtime_status: RuntimeStatus,
    adapter_stream: Option<DapStream>,
    build_handle: Option<JoinHandle<Result<ExitStatus>>>,
    needs_replay: bool,

    // Watcher
    watcher: FileWatcher,
}

impl Proxy {
    pub async fn new(config: MainConfig) -> Result<Proxy> {
        let watcher = FileWatcher::new(&config.watcher).context("Failed to launch watcher")?;
        let ide = IdeHandler::new(config.runtime.mode.clone())
            .await
            .context("Launching IdeServer has failed")?;

        Ok(Proxy {
            state: ProxyState::default(),
            ide,
            ide_stream: None,
            ide_status: IdeStatus::Listening,
            runtime: Runtime::new(&config.runtime),
            adapter_stream: None,
            runtime_status: RuntimeStatus::Pending,
            build_handle: None,
            needs_replay: false,
            watcher,
            config,
        })
    }

    /// Main proxy loop, orchestrates IDE <-> Adapter communication, FileWatcher events handling, and Runtime spawning & rebuilds.
    pub async fn run(&mut self) -> Result<()> {
        if let RuntimeModes::Headless { port, .. } = self.config.runtime.mode {
            log!(LogSource::Proxy, "Proxy is listening on :{}", port);
        }

        loop {
            self.start_runtime().await?;

            select! {
                // IDE Lifecycle
                stream = self.ide.connect(), if self.ide_status == IdeStatus::Listening => {
                    log!(LogSource::Proxy, "IDE established connection");
                    self.ide_stream = Some(stream.context("Launching IdeServer has failed")?);
                    self.ide_status = IdeStatus::Connected;

                    // First adapter spawn after IDE connects — skip replay as state is empty (no prior IDE messages captured yet),
                    // and in headless mode the injected launch message must not be replayed since the IDE is sending its own attach request.
                    self.needs_replay = false;
                },

                message = DapStream::read_stream(&mut self.ide_stream), if self.runtime_status == RuntimeStatus::Debugging => {
                    self.handle_streaming(StreamSources::Ide, message).await?
                },

                // Adapter Lifecycle
                message = DapStream::read_stream(&mut self.adapter_stream) => {
                    self.handle_streaming(StreamSources::Adapter, message).await?;
                },

                // Output piping to the adapter
                output = self.runtime.read_program_output() => {
                    if let Some(output) = output {
                        self.log_and_notify(LogSource::Program, output).await?;
                    }
                },

                // File Watching
                _ = self.watcher.next() => {
                    self.log_and_notify(LogSource::Watcher, "File changed, rebuilding...").await?;
                    self.rebuild().await?;
                },

                // Wait for building to complete
                result = Proxy::await_build(&mut self.build_handle), if self.runtime_status == RuntimeStatus::Building => {
                    match result {
                        Ok(status) => {
                            self.build_handle = None;
                            self.runtime_status = match status.success() {
                                true => RuntimeStatus::Pending, // Success - next loop iteration will spawn a new process.
                                false => RuntimeStatus::BuildingFailed // Build failed, another attempt on the next file change.
                            };
                        },
                        Err(_) => {}
                    }
                }
            }
        }
    }

    /// Intercept messages from both streaming sources and deal with forwarding and state management
    async fn handle_streaming(&mut self, source: StreamSources, message: Result<ReadResult>) -> Result<()> {
        match message {
            // Launch mode: IDE disconnect means full shutdown.
            // Headless mode: program survives, only debug session ends.
            Ok(ReadResult::EOF) => {
                if source == StreamSources::Ide {
                    match self.config.runtime.mode {
                        RuntimeModes::Headless { .. } => return self.handle_ide_disconnection().await,
                        RuntimeModes::Launch => return self.graceful_shutdown().await,
                    }
                }
            }

            // When a message from the IDE needs to be absorbed by the proxy - the adapter will never see the message (forward is blocked),
            // and at the same time we also need to fake the response to the IDE, so the IDE does not suspect anything.
            Ok(ReadResult::Message(message)) if self.should_absorb_request(&message) => {
                return self.fake_adapter_response(message).await;
            }

            // Forwarding logic
            Ok(ReadResult::Message(message)) => {
                let (forward_stream, log_source) = match source {
                    StreamSources::Ide => {
                        if let Ok(Some(command)) = self.state.capture(&message) {
                            log!(LogSource::Proxy, LogLevel::Debug, "State captured: {:?}", command);
                        }

                        (&mut self.adapter_stream, LogSource::Ide)
                    }
                    StreamSources::Adapter => {
                        // While replaying state to a new debug adapter, suppress responses — they
                        // weren't requested by the IDE and shouldn't reach it.
                        if self.runtime_status == RuntimeStatus::Replaying {
                            log!(
                                LogSource::Adapter,
                                LogLevel::Debug,
                                "Suppressed message during replay: {}",
                                message
                            );

                            if self.state.is_last_replay_response(&message) {
                                self.runtime_status = RuntimeStatus::Debugging;
                                log!(
                                    LogSource::Proxy,
                                    LogLevel::Debug,
                                    "Message replay has finished successfully"
                                );
                            }

                            return Ok(());
                        }

                        (&mut self.ide_stream, LogSource::Adapter)
                    }
                };

                // Forward to the other side — as long as the target stream is alive and we're not suppressing (replay).
                if let Some(forward_stream) = forward_stream {
                    forward_stream
                        .write(&message)
                        .await
                        .context("Could not write a message to a stream")?;

                    log!(log_source, LogLevel::Verbose, "{}", message);
                }
            }
            Err(e) => bail!(e),
        }

        Ok(())
    }

    /// Sends a fake response on behalf of the adapter, based on the received message from the IDE.
    async fn fake_adapter_response(&mut self, message: DapMessage) -> Result<()> {
        log!(
            LogSource::Ide,
            LogLevel::Verbose,
            "{} request from IDE absorbed, sending fake response",
            message
        );

        if let Some(stream) = &mut self.ide_stream {
            match message {
                // Specifically for Initialize we want a differnet behavior:
                // 1. Instead of generic "acknowledgement" response, we send the response to the first "initialize" request the IDE made in this session.
                // 2. On top of the response message, we also send an "Initialized" event message, to let the IDE know the adapter is ready for the rest of configuration.
                DapMessage::Request {
                    seq,
                    command: RequestCommandTypes::Initialize,
                    ..
                } => {
                    if let Some(response) = self.state.get_initialize_response() {
                        // We clone the response to the original "initiate" request, and making sure it's "request_seq" is the new request's seq.
                        let response = response.clone_with_new_seq(Some(seq));

                        stream
                            .write(&response)
                            .await
                            .context("Could not write a message to a stream")?;

                        // TODO: Send "initialized" event at this point.
                    }
                }

                // In all other cases, we just send an aknowledgement response
                _ => {
                    let response = DapMessage::make_acknowledgement_response(message.seq())
                        .context("Failed to create a response message")?;

                    stream
                        .write(&response)
                        .await
                        .context("Could not write a message to a stream")?;
                }
            };
        }

        Ok(())
    }

    /// Determines weither a message coming from the IDE shoudld be forwarded to the adapter, or weither we would like the adapter to not know about this message.
    fn should_absorb_request(&self, message: &DapMessage) -> bool {
        if let RuntimeModes::Headless { .. } = &self.config.runtime.mode {
            return match message {
                // Intercept disconnect so the adapter stays alive.
                // A fake response lets the IDE close gracefully, then the subsequent EOF triggers handle_ide_disconnection for cleanup.
                DapMessage::Request {
                    command: RequestCommandTypes::Disconnect,
                    ..
                } => true,

                // If one of the following requests were received from the IDE, while runtime status is ::Debugging
                // it can only mean one thing - the IDE is in the process of re-connection (which is different than
                // initial connection), we know it is re-connection because the first-connection is what started the
                // debug session, when these messages are coming in the first time, runtime is still in ::Spawned or
                // ::Pending status, and in that case, all messages should be forwarded normally (not absorbed).
                DapMessage::Request {
                    command:
                        RequestCommandTypes::Initialize
                        | RequestCommandTypes::Attach(_)
                        | RequestCommandTypes::Launch
                        | RequestCommandTypes::ConfigurationDone,
                    ..
                } if self.runtime_status == RuntimeStatus::Debugging => true,

                _ => false,
            };
        }

        false
    }

    /// Starts the runtime based on current state and mode.
    ///
    /// Two paths depending on whether an IDE is connected:
    ///
    /// **IDE connected** (launch mode, or headless after IDE connects):
    ///   `Pending`/`Spawned` -> spawn adapter -> connect -> `Replaying` -> `Debugging`
    ///
    /// **No IDE** (headless startup):
    ///   `Pending` -> spawn program as child process -> `Spawned`
    async fn start_runtime(&mut self) -> Result<()> {
        match self.runtime_status {
            RuntimeStatus::Spawned | RuntimeStatus::Pending => {
                if self.ide_status == IdeStatus::Connected {
                    self.runtime
                        .spawn_adapter()
                        .await
                        .context("Failed spawning debug process")?;

                    log!(LogSource::Proxy, LogLevel::Verbose, "Debug adapter spawned");

                    self.adapter_stream = Some(
                        self.runtime
                            .connect()
                            .await
                            .context("Unable to connect to the debugger process")?,
                    );

                    log!(
                        LogSource::Proxy,
                        LogLevel::Verbose,
                        "Proxy is connected to debug adapter"
                    );

                    self.replay_state().await.context("Failed to replay messages")?;
                } else if self.runtime_status == RuntimeStatus::Pending
                    && let RuntimeModes::Headless { .. } = self.config.runtime.mode
                {
                    self.runtime.spawn_program().await?;
                    self.runtime_status = RuntimeStatus::Spawned;
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// This is only triggered on `::Headless` mode (on `::Launch` mode, IDE disconnections means graceful shutdown).
    /// We need to make sure that state is cleared (from both state & the live adapter) - which means all breakpoints are cleared.
    /// and we also need to make sure that if the program is paused at this point in time we continue execution immediately.
    /// from here, subsequent `start_runtime()` calls will not need the adapter, and just start the program directly (until another connection is made by the IDE).
    async fn handle_ide_disconnection(&mut self) -> Result<()> {
        self.ide_stream = None;
        self.ide_status = IdeStatus::Listening;

        match self.runtime_status {
            RuntimeStatus::Debugging | RuntimeStatus::Replaying => {
                if let Some(stream) = &mut self.adapter_stream {
                    // Clear sequence will make sure of both: no breakpoints are active & app execution continues if currently breaking.
                    let mut clear_sequence: Vec<DapMessage> = vec![];

                    clear_sequence.push(DapMessage::clear_breakpoints(
                        RequestCommandTypes::SetExceptionBreakpoints,
                    )?);

                    clear_sequence.push(DapMessage::clear_breakpoints(
                        RequestCommandTypes::SetFunctionBreakpoints,
                    )?);

                    for file in self.state.get_breakpoints_file_paths() {
                        clear_sequence.push(DapMessage::clear_breakpoints(RequestCommandTypes::SetBreakpoints(
                            file.to_string(),
                        ))?);
                    }

                    // After clearing all breakpoints, the final message would be "continue" to resume execution on all threads.
                    clear_sequence
                        .push(DapMessage::make_continue_request().context("Could not create a continue request")?);

                    for message in clear_sequence {
                        stream
                            .write(&message)
                            .await
                            .context("Could not send a clear message to adapter")?;
                    }
                }
            }
            _ => {}
        }

        self.state.clear();

        Ok(())
    }

    /// Replaying the last stored state the a newly spawned adatper, this is called right after launching a fresh adapter.
    /// ProxyState prepares a sequence of DapMessage requests that needs to be sent to the adapter in order to match the
    /// state in the previous run, this includes: file breakpoints, function breakpoints and launch configuration.
    async fn replay_state(&mut self) -> Result<()> {
        if let Some(stream) = &mut self.adapter_stream {
            if self.needs_replay {
                log!(
                    LogSource::Proxy,
                    LogLevel::Verbose,
                    "Init state replay to the new debugger"
                );

                self.runtime_status = RuntimeStatus::Replaying;

                let replay_sequence = self.state.get_replay_sequence();

                if replay_sequence.len() > 0 {
                    for message in replay_sequence {
                        stream.write(message).await.context("Replaying a message has failed")?;
                    }

                    return Ok(());
                }
            }

            self.runtime_status = RuntimeStatus::Debugging;

            log!(
                LogSource::Proxy,
                LogLevel::Verbose,
                "Message replay has finished successfuly"
            );
        }

        Ok(())
    }

    /// Log to temrinal when in ::Headless mode.
    /// Notify to IDE as an Output event if currently debugging.
    async fn log_and_notify(&mut self, log_source: LogSource, message: impl Into<String>) -> Result<()> {
        let message = message.into();

        if let RuntimeModes::Headless { .. } = &self.config.runtime.mode {
            log!(log_source, "{}", message);
        }

        if let Some(stream) = &mut self.ide_stream {
            let output = format!("{}\n", message);
            let message = DapMessage::make_output_event(&output).context("Could not create an output event")?;

            stream
                .write(&message)
                .await
                .context("Could not send a message to IDE stream")?;
        }

        Ok(())
    }

    /// Rebuild is in charge of killing the adapter or/and the program's child process (depends on headless/launch mode)
    /// and to trigger a rebuild for the program. The handler for the rebuild process is kept and is then awaited on the
    /// main proxy loop until completed.
    async fn rebuild(&mut self) -> Result<()> {
        self.runtime.kill().await?;

        if let Some(handle) = &mut self.build_handle {
            handle.abort();
        }

        self.needs_replay = true;
        self.adapter_stream = None;
        self.runtime_status = RuntimeStatus::Building;
        self.build_handle = Some(self.runtime.build());

        Ok(())
    }

    /// Wraps an optional build_handler as a standalone future, so it could be easily used in a select! arm
    async fn await_build(build_handler: &mut Option<JoinHandle<Result<ExitStatus>>>) -> Result<ExitStatus> {
        match build_handler {
            Some(handler) => handler.await?,
            None => std::future::pending().await,
        }
    }

    async fn graceful_shutdown(&mut self) -> Result<()> {
        self.runtime.kill().await?;

        Ok(())
    }
}
