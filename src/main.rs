use anyhow::Result;
use clap::Parser;
use proxy::Proxy;

use crate::{cli::Cli, config::Config};

mod cli;
mod config;
mod dap_message;
mod dap_stream;
mod file_watcher;
mod ide_server;
mod logger;
mod proxy;
mod proxy_state;
mod runtime;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    Cli::parse();

    // let config = Config::build(None);
    // let mut proxy = Proxy::new(config).await?;
    // proxy.run().await?;

    Ok(())
}
