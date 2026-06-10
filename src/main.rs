use anyhow::{Result, bail};

use crate::{dap_adapter::DapAdapter, dap_stream::DapMessage, ide_server::IdeServer};

mod dap_adapter;
mod dap_stream;
mod ide_server;

#[tokio::main(flavor = "current_thread")]

async fn main() -> Result<()> {
    let mut dap_adapter = DapAdapter::new();
    dap_adapter.spawn().await?;
    let mut adapter_stream = dap_adapter.connect().await?;

    let mut ide = IdeServer::new(2500).await?;
    let mut ide_stream = ide.connect().await?;

    loop {
        tokio::select! {
            message = ide_stream.read() => {
                match message {
                    Ok(message) => {
                        match message {
                            DapMessage::Event(buffer) | DapMessage::Response(buffer) => {
                                adapter_stream.write(&buffer).await;
                            },
                            DapMessage::Request { raw_bytes, .. } => {
                                adapter_stream.write(&raw_bytes).await;
                            },
                        }
                    },
                    Err(e) => {
                        bail!(e);
                    }
                }
            },
            message = adapter_stream.read() => {
                match message {
                    Ok(message) => {
                        match message {
                            DapMessage::Event(buffer) | DapMessage::Response(buffer) => {
                                ide_stream.write(&buffer).await;
                            },
                            DapMessage::Request { raw_bytes, .. } => {
                                ide_stream.write(&raw_bytes).await;
                            },
                        }
                    },
                    Err(e) => {
                        bail!(e);
                    }
                }
            },
        }
    }

    Ok(())
}
