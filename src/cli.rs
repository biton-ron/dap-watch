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
    /// Port the proxy listens on for IDE connections (overrides config file)
    #[arg(long, short)]
    pub port: Option<u16>,

    /// Path to configuration file
    #[arg(long, short, default_value = "dap-watch.toml")]
    pub config: String,

    /// Build command override (overrides config file)
    #[arg(long)]
    pub build: Option<String>,

    /// Program binary path override (overrides config file)
    #[arg(long)]
    pub program: Option<String>,

    /// Verbose logging output
    #[arg(long, short)]
    pub verbose: bool,
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
