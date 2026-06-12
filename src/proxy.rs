use anyhow::{Context, Result, bail};
use tokio::select;

use crate::{
    dap_adapter::{AdapterStatus, DapAdapter},
    dap_stream::{DapMessage, DapStream},
    file_watcher::{FileWatcher, WatcherConfig},
    ide_server::{self, IdeServer, IdeStatus},
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
    adapter: Option<DapAdapter>,
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
            adapter: None,
            adapter_stream: None,
            adapter_status: AdapterStatus::Spawned,
            watcher,
        }
    }

    pub async fn run() -> Result<()> {
        let ide = IdeServer::new(2500)
            .await
            .context("Launching IdeServer has failed")?;

        let watcher = FileWatcher::new(WatcherConfig {}).context("Failed to launch watcher")?;
        let mut proxy = Proxy::new(ide, watcher);

        loop {
            select! {
                stream = proxy.ide.connect(), if proxy.ide_status == IdeStatus::Listening => {
                    println!("IDE is connected!");
                    proxy.ide_stream = Some(stream.context("Launching IdeServer has failed")?);
                    proxy.ide_status = IdeStatus::Connected;
                },
                message = DapStream::read_stream(&mut proxy.ide_stream) => {
                    match message {
                        Ok(message) => {
                            if let Some(adapter_stream) = &mut proxy.adapter_stream {
                                let _ = adapter_stream.write(message).await;
                            }
                        },
                        Err(e) => {
                            bail!(e);
                        }
                    }
                },
                message = DapStream::read_stream(&mut proxy.adapter_stream) => {
                    match message {
                        Ok(message) => {
                            if let Some(stream) = &mut proxy.ide_stream {
                                let _ = stream.write(message).await;
                            }
                        },
                        Err(e) => {
                            bail!(e);
                        }
                    }
                },
                _ = proxy.watcher.next() => {
                    println!("Event was detected!");
                },
            }
        }
    }
}
