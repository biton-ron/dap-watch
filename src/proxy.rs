use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use tokio::select;

use crate::{
    dap_adapter::{AdapterStatus, DapAdapter},
    dap_message::{
        DapMessage::{self, Request},
        RequestCommandTypes::{self},
    },
    dap_stream::DapStream,
    file_watcher::{FileWatcher, WatcherConfig},
    ide_server::{IdeServer, IdeStatus},
    log,
    logger::LogSource,
};

#[derive(PartialEq)]
enum StreamSources {
    Ide,
    Adapter,
}

#[derive(Default)]
struct DebugState {
    initialize: Option<DapMessage>,
    launch: Option<DapMessage>,
    attach: Option<DapMessage>,
    configuration_done: Option<DapMessage>,
    function_breakpoints: Option<DapMessage>,
    exception_breakpoints: Option<DapMessage>,
    breakpoints: HashMap<String, DapMessage>, // Hashed by file path
}

pub struct Proxy {
    state: DebugState,

    // IDE
    ide: IdeServer,
    ide_stream: Option<DapStream>,
    ide_queue: Vec<DebugState>, // Stores a queue of messages to be processed when bi-directional streaming is temporarly disabled (during replay)
    ide_status: IdeStatus,

    // Adapter
    adapter: DapAdapter,
    adapter_stream: Option<DapStream>,
    adapter_status: AdapterStatus,

    // Watcher
    watcher: FileWatcher,
}

impl Proxy {
    pub async fn new() -> Result<Proxy> {
        let watcher = FileWatcher::new(WatcherConfig {}).context("Failed to launch watcher")?;
        let ide_port = 2500; // TODO: Should come from configuration
        let ide = IdeServer::new(ide_port)
            .await
            .context("Launching IdeServer has failed")?;

        log!(
            LogSource::Proxy,
            "Initiated, proxy is listening on :{}",
            ide_port
        );

        Ok(Proxy {
            state: DebugState::default(),
            ide,
            ide_stream: None,
            ide_queue: Vec::new(),
            ide_status: IdeStatus::Listening,
            adapter: DapAdapter::new(),
            adapter_stream: None,
            adapter_status: AdapterStatus::Pending,
            watcher,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            self.spawn_adapter()
                .await
                .context("Debugger spawning has failed")?;

            select! {
                // IDE Lifecycle
                stream = self.ide.connect(), if self.ide_status == IdeStatus::Listening => {
                    log!(LogSource::Proxy, "IDE established connection");
                    self.ide_stream = Some(stream.context("Launching IdeServer has failed")?);
                    self.ide_status = IdeStatus::Connected;
                },
                message = DapStream::read_stream(&mut self.ide_stream) => self.handle_streaming(StreamSources::Ide, message).await?,

                // Adapter Lifecycle
                message = DapStream::read_stream(&mut self.adapter_stream) => self.handle_streaming(StreamSources::Adapter, message).await?,

                // File Watching
                _ = self.watcher.next() => {
                    log!(LogSource::Watcher, "File changed, rebuilding...");
                    self.adapter.kill().await.context("Failed to kill debug adapter")?;
                    self.adapter_stream = None;
                    self.adapter_status = AdapterStatus::Pending;
                },
            }
        }
    }

    // Intercept messages from both streaming sources and deal with forwarding and state management
    async fn handle_streaming(
        &mut self,
        source: StreamSources,
        message: Result<Option<DapMessage>>,
    ) -> Result<()> {
        match message {
            Ok(None) => {}
            Ok(Some(message)) => {
                if source == StreamSources::Ide {
                    self.capture_state(&message);
                }

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

    /// Takes a message and updates Proxy state accordingly.
    /// State is only affected by messages that are sent from the IDE, so there is no need to call capture_state when message source is debug adapter.
    fn capture_state(&mut self, message: &DapMessage) {
        // Only requests have some effect on the state
        if let Request { command, .. } = message {
            let message = message.clone();

            match command {
                RequestCommandTypes::Attach => self.state.attach = Some(message),
                RequestCommandTypes::Initialize => self.state.initialize = Some(message),
                RequestCommandTypes::Launch => self.state.launch = Some(message),
                RequestCommandTypes::ConfigurationDone => {
                    self.state.configuration_done = Some(message)
                }
                RequestCommandTypes::SetExceptionBreakpoints => {
                    self.state.exception_breakpoints = Some(message)
                }
                RequestCommandTypes::SetFunctionBreakpoints => {
                    self.state.function_breakpoints = Some(message)
                }
                RequestCommandTypes::SetBreakpoints(file_path) => {
                    self.state
                        .breakpoints
                        .insert(String::from(file_path), message);
                }
                RequestCommandTypes::PassForward(_) => {}
            }

            if !matches!(command, RequestCommandTypes::PassForward(_)) {
                log!(LogSource::Proxy, "State captured: {:?}", command);
            }
        }
    }

    // Spawn the debugger, connect
    async fn spawn_adapter(&mut self) -> Result<()> {
        if self.adapter_status == AdapterStatus::Pending {
            self.adapter
                .spawn()
                .await
                .context("Failed spawning debug process")?;

            log!(LogSource::Proxy, "Debug adapter spawned");

            self.adapter_stream = Some(
                self.adapter
                    .connect()
                    .await
                    .context("Unable to connect to the debugger process")?,
            );

            log!(LogSource::Proxy, "Proxy is connected to debug adapter");

            self.adapter_status = AdapterStatus::Connected;
        }

        Ok(())
    }
}
