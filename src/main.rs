use anyhow::Result;
use proxy::Proxy;

mod dap_adapter;
mod dap_stream;
mod file_watcher;
mod ide_server;
mod logger;
mod proxy;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    Proxy::run().await?;

    Ok(())
}
