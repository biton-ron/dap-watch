use anyhow::{Context, Result, bail};
use tokio::select;

use crate::{
    dap_adapter::{AdapterStatus, DapAdapter},
    dap_message::DapMessage,
    dap_stream::DapStream,
    file_watcher::{FileWatcher, WatcherConfig},
    ide_server::{self, IdeServer, IdeStatus},
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
    pub fn new(ide: IdeServer, watcher: FileWatcher) -> Proxy {
        Proxy {
            state: DebugState::default(),
            ide,
            ide_stream: None,
            ide_queue: Vec::new(),
            ide_status: IdeStatus::Listening,
            adapter: DapAdapter::new(),
            adapter_stream: None,
            adapter_status: AdapterStatus::Pending,
            watcher,
        }
    }

    pub async fn run() -> Result<()> {
        let ide_port = 2500; // TODO: Should come from configuration
        let ide = IdeServer::new(ide_port)
            .await
            .context("Launching IdeServer has failed")?;

        log!(LogSource::Proxy, "Proxy is listening on :{}", ide_port);

        let watcher = FileWatcher::new(WatcherConfig {}).context("Failed to launch watcher")?;
        let mut proxy = Proxy::new(ide, watcher);

        loop {
            proxy
                .spawn_adapter()
                .await
                .context("Debugger spawning has failed")?;

            select! {
                // IDE Lifecycle
                stream = proxy.ide.connect(), if proxy.ide_status == IdeStatus::Listening => {
                    log!(LogSource::Proxy, "IDE established connection");
                    proxy.ide_stream = Some(stream.context("Launching IdeServer has failed")?);
                    proxy.ide_status = IdeStatus::Connected;
                },
                message = DapStream::read_stream(&mut proxy.ide_stream) => {
                    match message {
                        Ok(None) => {},
                        Ok(Some(message)) => {
                            if let Some(adapter_stream) = &mut proxy.adapter_stream {
                                let _ = adapter_stream.write(message).await;
                                // log!(LogSource::Ide, "{}", message);
                            }
                        },
                        Err(e) => {
                            bail!(e);
                        }
                    }
                },

                // Adapter Lifecycle
                message = DapStream::read_stream(&mut proxy.adapter_stream) => {
                    match message {
                        Ok(None) => {},
                        Ok(Some(message)) => {
                            if let Some(stream) = &mut proxy.ide_stream {
                                let _ = stream.write(message).await;
                            }
                        },
                        Err(e) => {
                            bail!(e);
                        }
                    }
                },

                // File Watching
                _ = proxy.watcher.next() => {
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
