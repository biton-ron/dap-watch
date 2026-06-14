use anyhow::{Context, Result, bail};
use tokio::select;

use crate::{
    dap_adapter::{AdapterStatus, DapAdapter},
    dap_message::DapMessage,
    dap_stream::DapStream,
    file_watcher::{FileWatcher, WatcherConfig},
    ide_server::{IdeServer, IdeStatus},
    log,
    logger::LogSource,
};

#[derive(Default)]
struct DebugState {
    initialize: Option<DapMessage>,
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
                message = DapStream::read_stream(&mut self.ide_stream) => {
                    match message {
                        Ok(None) => {},
                        Ok(Some(message)) => {
                            if let Some(adapter_stream) = &mut self.adapter_stream {
                                let _ = adapter_stream.write(&message).await;
                                log!(LogSource::Ide, "{}", message);
                            }
                        },
                        Err(e) => {
                            bail!(e);
                        }
                    }
                },

                // Adapter Lifecycle
                message = DapStream::read_stream(&mut self.adapter_stream) => {
                    match message {
                        Ok(None) => {},
                        Ok(Some(message)) => {
                            if let Some(stream) = &mut self.ide_stream {
                                let _ = stream.write(&message).await;
                                log!(LogSource::Adapter, "{}", message);
                            }
                        },
                        Err(e) => {
                            bail!(e);
                        }
                    }
                },

                // File Watching
                _ = self.watcher.next() => {
                    log!(LogSource::Watcher, "File changed, rebuilding...");
                },
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
