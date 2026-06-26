use anyhow::Result;
use clap::Parser;
use proxy::Proxy;

use crate::{
    cli::{Cli, Commands},
    config::MainConfig,
};

mod cli;
mod config;
mod dap_message;
mod dap_stream;
mod file_watcher;
mod ide;
mod logger;
mod proxy;
mod proxy_state;
mod runtime;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    std::fs::write("/tmp/dap-watch-debug.log", "started").unwrap();

    match cli.command {
        Some(Commands::Init(_)) => MainConfig::init(),
        None => {
            let config = MainConfig::build(cli.main)?;

            std::fs::write("/tmp/dap-watch-debug.log", "config is good").unwrap();

            let mut proxy = Proxy::new(config).await?;

            proxy.run().await?;
        }
    }

    Ok(())
}
