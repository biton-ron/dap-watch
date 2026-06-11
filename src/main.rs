use anyhow::{Context, Result, bail};

use crate::{
    dap_adapter::DapAdapter,
    dap_stream::DapMessage,
    file_watcher::{FileWatcher, WatcherConfig},
    ide_server::IdeServer,
};

mod dap_adapter;
mod dap_stream;
mod file_watcher;
mod ide_server;

#[tokio::main(flavor = "current_thread")]

async fn main() -> Result<()> {
    let mut dap_adapter = DapAdapter::new();
    dap_adapter.spawn().await?;
    let mut adapter_stream = dap_adapter.connect().await?;

    let mut ide = IdeServer::new(2500).await?;
    let mut ide_stream = ide.connect().await?;

    let mut watcher = FileWatcher::new(WatcherConfig {}).context("Failed to launch watcher")?;
    println!("Watcher is live!");

    loop {
        tokio::select! {
            message = ide_stream.read() => {
                match message {
                    Ok(message) => {
                        adapter_stream.write(message).await;
                    },
                    Err(e) => {
                        bail!(e);
                    }
                }
            },
            message = adapter_stream.read() => {
                match message {
                    Ok(message) => {
                        ide_stream.write(message).await;
                    },
                    Err(e) => {
                        bail!(e);
                    }
                }
            },
            _ = watcher.next() => {
                println!("Event was detected!");
            }
        }
    }

    Ok(())
}
