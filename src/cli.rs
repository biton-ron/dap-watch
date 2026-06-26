use crate::config::DEFAULT_CONFIG_FILE_PATH;
use clap::{Args, Parser, Subcommand};

#[derive(Subcommand)]
pub enum Commands {
    /// Generate a dap-watch.toml configuration file in the current directory
    Init(InitArgs),
}

// Root
#[derive(Parser)]
#[command(
    name = "dap-watch",
    version,
    about = "DAP proxy with file watching and auto-rebuild for compiled languages"
)]
pub struct Cli {
    #[command(flatten)]
    pub main: MainArgs,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

// Main
#[derive(Args)]
pub struct MainArgs {
    /// Program binary path override (overrides config file)
    #[arg()]
    pub program: Option<String>,

    /// Port the proxy listens on for IDE connections (overrides config file)
    #[arg(long, short)]
    pub port: Option<u16>,

    /// Path to configuration file
    #[arg(long, short, default_value = DEFAULT_CONFIG_FILE_PATH)]
    pub config: String,

    /// Build command override (overrides config file)
    #[arg(long)]
    pub build: Option<String>,

    /// Verbose logging output
    #[arg(long, short)]
    pub verbose: bool,

    /// --stdio is specifically for launch mode, it allows IDEs to spawn dap-watch as child-process and communicate through stdin/out
    #[arg(long, conflicts_with_all = ["program", "port"])]
    pub stdio: bool,

    /// Remaining args are passed through to adapter
    #[arg(trailing_var_arg = true, hide = true)]
    pub adapter_args: Vec<String>,
}

// Init
#[derive(Args)]
pub struct InitArgs {
    /// Language preset to generate config for (e.g., "rust", "go", "cpp")
    #[arg(long, short)]
    pub language: Option<String>,

    /// Force overwrite if config file already exists
    #[arg(long)]
    pub force: bool,
}
