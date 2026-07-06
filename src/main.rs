use anyhow::Result;
use clap::Parser;
use proxy::Proxy;

use crate::{
    cli::{Cli, Commands},
    config::MainConfig,
    logger::LogLevel,
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

    // Log level
    if cli.verbose {
        logger::set_log_level(LogLevel::Verbose);
    } else if cli.debug {
        logger::set_log_level(LogLevel::Debug);
    }

    match cli.command {
        Some(Commands::Init(_)) => MainConfig::init(),
        None => {
            let config = MainConfig::build(cli.main)?;
            let mut proxy = Proxy::new(config).await?;

            proxy.run().await?;
        }
    }

    Ok(())
}
