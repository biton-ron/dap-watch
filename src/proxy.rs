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
    adapter: DapAdapter,
    adapter_stream: Option<DapStream>,
    adapter_status: AdapterStatus,

    // Watcher
    watcher: FileWatcher,
}

impl Proxy {
    pub fn new(ide: IdeServer, adapter: DapAdapter, watcher: FileWatcher) -> Proxy {
        Proxy {
            state: DebugState::default(),
            ide,
            ide_stream: None,
            ide_queue: Vec::new(),
            ide_status: IdeStatus::Listening,
            adapter,
            adapter_stream: None,
            adapter_status: AdapterStatus::Spawned,
            watcher,
        }
    }

    pub async fn run() -> Result<()> {
        let mut proxy = Proxy::default();

        loop {
            select! {
                ide = IdeServer::new(2500), if proxy.ide_status == IdeStatus::Pending => {
                    proxy.ide = Some(ide.context("Launching IdeServer has failed")?);
                },
                stream = proxy.ide.connect(), if proxy.ide_status == IdeStatus::Listening => {
                    proxy.ide = Some(ide.context("Launching IdeServer has failed")?);
                }
            }
        }
    }
}

// pub async fn run() -> Result<()> {
//     let mut state = DebugState::default();

//     let mut dap_adapter = DapAdapter::new();
//     dap_adapter.spawn().await?;
//     let mut adapter_stream = dap_adapter.connect().await?;

//     let mut ide = IdeServer::new(2500).await?;
//     let mut ide_stream = ide.connect().await?;

//     let mut watcher = FileWatcher::new(WatcherConfig {}).context("Failed to launch watcher")?;
//     println!("Watcher is live!");

//     loop {
//         tokio::select! {
//             message = ide_stream.read() => {
//                 match message {
//                     Ok(message) => {
//                         adapter_stream.write(message).await;
//                     },
//                     Err(e) => {
//                         bail!(e);
//                     }
//                 }
//             },
//             message = adapter_stream.read() => {
//                 match message {
//                     Ok(message) => {
//                         ide_stream.write(message).await;
//                     },
//                     Err(e) => {
//                         bail!(e);
//                     }
//                 }
//             },
//             _ = watcher.next() => {
//                 println!("Event was detected!");
//             }
//         }
//     }

//     Ok(())
// }
