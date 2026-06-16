use anyhow::Result;
use proxy::Proxy;

use crate::config::Config;

mod config;
mod dap_adapter;
mod dap_message;
mod dap_stream;
mod file_watcher;
mod ide_server;
mod logger;
mod proxy;
mod proxy_state;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let config = Config::build(None);
    let mut proxy = Proxy::new(config).await?;
    proxy.run().await?;

    Ok(())
}
