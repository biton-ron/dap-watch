use std::process::ExitStatus;

use anyhow::{Context, Result, bail};
use tokio::{select, task::JoinHandle};

use crate::{
    config::{MainConfig, RuntimeModes},
    dap_stream::{DapStream, ReadResult},
    file_watcher::FileWatcher,
    ide::{IdeHandler, IdeStatus},
    log,
    logger::LogSource,
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
    Live,
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
            watcher,
            config,
        })
    }

    /// Main proxy loop, orchestrates IDE <-> Adapter communication, FileWatcher events handling, and Runtime spawning & rebuilds.
    pub async fn run(&mut self) -> Result<()> {
        if let RuntimeModes::Headless { port, .. } = self.config.runtime.mode {
            log!(LogSource::Proxy, "Proxy is listening on :{}", port);
            self.capture_headless_launch_state();
        }

        loop {
            self.start_runtime().await?;

            select! {
                // IDE Lifecycle
                stream = self.ide.connect(), if self.ide_status == IdeStatus::Listening => {
                    log!(LogSource::Proxy, "IDE established connection");
                    self.ide_stream = Some(stream.context("Launching IdeServer has failed")?);
                    self.ide_status = IdeStatus::Connected;
                },

                message = DapStream::read_stream(&mut self.ide_stream), if self.runtime_status == RuntimeStatus::Live => {
                    self.handle_streaming(StreamSources::Ide, message).await?
                },

                // Adapter Lifecycle
                message = DapStream::read_stream(&mut self.adapter_stream) => {
                    self.handle_streaming(StreamSources::Adapter, message).await?;
                },

                // File Watching
                _ = self.watcher.next() => {
                    log!(LogSource::Watcher, "File changed, rebuilding...");
                    self.rebuild().await?;
                },

                // Wait for building to complete
                result = Proxy::await_build(&mut self.build_handle), if self.runtime_status == RuntimeStatus::Building => {
                    match result {
                        Ok(status) => {
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

    /// On headless mode, there are 3 different possibilities for the runtime to operate:
    /// 1. No debugging - program is started as a child process, no debug adapter is spawned at this point.
    /// 2. First debugging - when program is already spawned, attaching to dap-watch means spawning a debug adapter - and forwarding the attach request to it.
    /// 3. Rebuild while debugging - in this case, we want the adapter itself to launch the program, so no code execution will be missed, in order to do that
    ///    we "inject" a DapMessage to ProxyState, so it would be replayed to those freshly spawned adapters after rebuild.
    async fn capture_headless_launch_state(&mut self) {
        // self.state.capture() // TODO: construct a launch message using something like DapMessage::launch
    }

    /// Intercept messages from both streaming sources and deal with forwarding and state management
    async fn handle_streaming(
        &mut self,
        source: StreamSources,
        message: Result<ReadResult>,
    ) -> Result<()> {
        match message {
            Ok(ReadResult::EOF) => {
                if source == StreamSources::Ide {
                    match self.config.runtime.mode {
                        RuntimeModes::Headless { .. } => {
                            // TODO: need to handle disconnection graecfully, meaning: clear breakpoints, and continue the program if paused
                        }

                        // In launch mode - disconnection means shutting down dap-watch and the adapter altogether, only in headless mode the program survives IDE disconnection.
                        RuntimeModes::Launch => return self.graceful_shutdown().await,
                    }
                }
            }
            Ok(ReadResult::Message(message)) => {
                if source == StreamSources::Ide {
                    if let Some(command) = self.state.capture(&message) {
                        log!(LogSource::Proxy, "State captured: {:?}", command);
                    }
                } else if source == StreamSources::Adapter {
                    // While replaying state to a new debug adapter, we will surparss the messages coming back from the adapter.
                    // The reason is simple - we don't want these messages to reach the IDE, as they were not actually requested on its behalf - the IDE is not aware of them.
                    if self.runtime_status == RuntimeStatus::Replaying {
                        log!(
                            LogSource::Adapter,
                            "Suppressed message during replay: {}",
                            message
                        );

                        if self.state.is_last_replay_response(&message) {
                            self.runtime_status = RuntimeStatus::Live;
                            log!(LogSource::Proxy, "Message replay has finished successfuly");
                        }

                        return Ok(());
                    }
                }

                // Forward logic is simple - as long as the forward target is alive (IDE/Adapter), and we're not surprassing messages (during replay) -> write to target.
                let (forward_stream, log_source) = match source {
                    StreamSources::Adapter => (&mut self.ide_stream, LogSource::Adapter),
                    StreamSources::Ide => (&mut self.adapter_stream, LogSource::Ide),
                };

                if let Some(forward_stream) = forward_stream {
                    forward_stream
                        .write(&message)
                        .await
                        .context("Could not write a message to a stream")?;

                    log!(log_source, "{}", message);
                }
            }
            Err(e) => bail!(e),
        }

        Ok(())
    }

    /// Spawn the debugger, connect
    async fn start_runtime(&mut self) -> Result<()> {
        if self.runtime_status == RuntimeStatus::Pending {
            // TODO: Pending OR debugging but no debugger is live
            if self.ide_status == IdeStatus::Connected {
                self.runtime
                    .spawn_adapter()
                    .await
                    .context("Failed spawning debug process")?;

                log!(LogSource::Proxy, "Debug adapter spawned");

                self.runtime_status = RuntimeStatus::Spawned;

                self.adapter_stream = Some(
                    self.runtime
                        .connect()
                        .await
                        .context("Unable to connect to the debugger process")?,
                );

                log!(LogSource::Proxy, "Proxy is connected to debug adapter");

                self.replay_state()
                    .await
                    .context("Failed to replay messages")?;
            } else {
                // TODO: This should only happen once
                self.runtime.spawn_program().await?;
            }
        }

        Ok(())
    }

    /// Replaying the last stored state the a newly spawned adatper, this is called right after launching a fresh adapter.
    /// ProxyState prepares a sequence of DapMessage requests that needs to be sent to the adapter in order to match the
    /// state in the previous run, this includes: file breakpoints, function breakpoints and launch configuration.
    async fn replay_state(&mut self) -> Result<()> {
        if let Some(stream) = &mut self.adapter_stream {
            log!(LogSource::Proxy, "Init state replay to the new debugger");

            self.runtime_status = RuntimeStatus::Replaying;

            let replay_sequence = self.state.get_replay_sequence();

            if replay_sequence.len() > 0 {
                for message in replay_sequence {
                    stream
                        .write(message)
                        .await
                        .context("Replaying a message has failed")?;
                }
            } else {
                self.runtime_status = RuntimeStatus::Live;
                log!(LogSource::Proxy, "Message replay has finished successfuly");
            }
        }

        Ok(())
    }

    /// Rebuild is in charge of killing the adapter or/and the program's child process (depends on headless/launch mode)
    /// and to trigger a rebuild for the program. The handler for the rebuild process is kept and is then awaited on the
    /// main proxy loop until completed.
    async fn rebuild(&mut self) -> Result<()> {
        self.runtime.kill().await?;

        self.adapter_stream = None;
        self.runtime_status = RuntimeStatus::Building;
        self.build_handle = Some(self.runtime.build());

        Ok(())
    }

    /// Wraps an optional build_handler as a standalone future, so it could be easily used in a select! arm
    async fn await_build(
        build_handler: &mut Option<JoinHandle<Result<ExitStatus>>>,
    ) -> Result<ExitStatus> {
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
