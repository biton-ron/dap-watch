use std::process::ExitStatus;

use anyhow::{Context, Result, bail};
use tokio::{process::Command, select, task::JoinHandle};

use crate::{
    config::Config,
    dap_adapter::{
        AdapterStatus::{self, Replaying},
        DapAdapter,
    },
    dap_message::DapMessage::{self},
    dap_stream::DapStream,
    file_watcher::FileWatcher,
    ide_server::{IdeServer, IdeStatus},
    log,
    logger::LogSource,
    proxy_state::ProxyState,
};

#[derive(PartialEq)]
enum StreamSources {
    Ide,
    Adapter,
}

pub struct Proxy {
    config: Config,
    state: ProxyState,

    // IDE
    ide: IdeServer,
    ide_stream: Option<DapStream>,
    ide_status: IdeStatus,

    // Runtime
    adapter: DapAdapter,
    adapter_stream: Option<DapStream>,
    adapter_status: AdapterStatus,
    build_handle: Option<JoinHandle<Result<ExitStatus>>>,

    // Watcher
    watcher: FileWatcher,
}

impl Proxy {
    pub async fn new(config: Config) -> Result<Proxy> {
        let watcher = FileWatcher::new(&config.watcher).context("Failed to launch watcher")?;
        let ide = IdeServer::new(config.port)
            .await
            .context("Launching IdeServer has failed")?;

        log!(
            LogSource::Proxy,
            "Initiated, proxy is listening on :{}",
            config.port
        );

        Ok(Proxy {
            state: ProxyState::default(),
            ide,
            ide_stream: None,
            ide_status: IdeStatus::Listening,
            adapter: DapAdapter::new(&config.runtime),
            adapter_stream: None,
            adapter_status: AdapterStatus::Pending,
            build_handle: None,
            watcher,
            config,
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

                message = DapStream::read_stream(&mut self.ide_stream), if self.adapter_status == AdapterStatus::Connected => {
                    self.handle_streaming(StreamSources::Ide, message).await?
                },

                // Adapter Lifecycle
                message = DapStream::read_stream(&mut self.adapter_stream) => self.handle_streaming(StreamSources::Adapter, message).await?,

                // File Watching
                _ = self.watcher.next() => {
                    log!(LogSource::Watcher, "File changed, rebuilding...");
                    self.rebuild().await?;
                },

                // Wait for building to complete
                result = Proxy::await_build(&mut self.build_handle), if self.adapter_status == AdapterStatus::Building => {

                    match result {
                        Ok(_status) => {
                            self.adapter_status = AdapterStatus::Pending;
                        },
                        Err(_) => {}
                    }
                }
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
                    if let Some(command) = self.state.capture(&message) {
                        log!(LogSource::Proxy, "State captured: {:?}", command);
                    }
                } else if source == StreamSources::Adapter && self.adapter_status == Replaying {
                    log!(
                        LogSource::Adapter,
                        "Suppressed message during replay: {}",
                        message
                    );

                    if self.state.is_last_replay_response(&message) {
                        self.adapter_status = AdapterStatus::Connected;
                        log!(LogSource::Proxy, "Message replay has finished successfuly");
                    }

                    return Ok(());
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

            self.replay_state()
                .await
                .context("Failed to replay messages")?;
        }

        Ok(())
    }

    async fn replay_state(&mut self) -> Result<()> {
        if let Some(stream) = &mut self.adapter_stream {
            log!(LogSource::Proxy, "Init state replay to the new debugger");

            self.adapter_status = AdapterStatus::Replaying;

            let replay_sequence = self.state.get_replay_sequence();

            if replay_sequence.len() > 0 {
                for message in replay_sequence {
                    stream
                        .write(message)
                        .await
                        .context("Replaying a message has failed")?;
                }
            } else {
                self.adapter_status = AdapterStatus::Connected;
                log!(LogSource::Proxy, "Message replay has finished successfuly");
            }
        }

        Ok(())
    }

    async fn rebuild(&mut self) -> Result<()> {
        self.adapter
            .kill()
            .await
            .context("Failed to kill debug adapter")?;

        self.adapter_stream = None;
        self.adapter_status = AdapterStatus::Building;

        let build_cmd = self.config.runtime.build.clone();

        self.build_handle = Some(tokio::spawn(async move {
            // Execute the build command configured by the user
            build_command(build_cmd)
                .status()
                .await
                .context("Proxy has failed re-building the program")
        }));

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
}

#[cfg(unix)]
fn build_command(cmd: String) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd);
    c
}

#[cfg(windows)]
fn build_command(cmd: String) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/C").arg(cmd);
    c
}
