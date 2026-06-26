use crate::cli;
use anyhow::Result;
use config::{Config, ConfigError};
use serde::{Deserialize, Serialize};
use std::{fs, vec};

/// File watcher configuration
#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct WatcherConfig {
    /// Paths to watch for file changes, supports glob patterns (e.g., "src/**/*.rs")
    pub paths: Vec<String>,
    /// Paths to exclude from watching, supports glob patterns
    pub ignore_paths: Vec<String>,
    /// When true, files matching .gitignore rules are automatically excluded from watching
    pub gitignore: bool,
    /// Debounce duration in milliseconds - file changes within this window are batched into a single rebuild
    pub debounce_ms: u64,
}

#[derive(Clone, Deserialize, Serialize)]
pub enum RuntimeModes {
    /// Headless means dap-watch is starting the program itself, no need for a debugger to be attached.
    /// A debugger can later than connect to the provided port using an "attach" request.
    Headless {
        /// Path to the compiled program binary
        program: String,
        /// Arguments passed to the program when launched
        program_args: Vec<String>,
        /// Path to an environment file loaded before running the program
        program_env_file: Option<String>,
        /// Port the proxy listens on for IDE connections
        port: u16,
    },
    /// Stdio means that dap-watch was started as a child-process by the IDE, and communciation between them is done through stdin/stdout rather than TCP.
    /// This mode should be used together with "launch" debug request along with the program related configuration (which program to run? etc)
    Stdio,
}

/// Runtime configuration for the build, program, and debug adapter
#[derive(Clone, Deserialize, Serialize)]
pub struct RuntimeConfig {
    /// Path to the debug adapter binary (e.g., "/path/to/codelldb")
    pub adapter: String,
    /// Additional arguments passed to the debug adapter, appended after the port flag managed by dap-watch
    pub adapter_args: Vec<String>,
    /// Shell command to build the program (e.g., "cargo build")
    pub build: String,
    pub mode: RuntimeModes,
}

/// Min configuration for dap-watch
#[derive(Deserialize, Serialize)]
pub struct MainConfig {
    pub runtime: RuntimeConfig,
    pub watcher: WatcherConfig,
}

pub const DEFAULT_CONFIG_FILE_PATH: &str = "dap-watch.toml";

impl MainConfig {
    pub fn build(cli_args: cli::MainArgs) -> Result<MainConfig, ConfigError> {
        // Defaults
        let defaults = MainConfig::detect_defaults();
        let mut builder = Config::builder().add_source(config::Config::try_from(&defaults)?);

        // Config file
        if let Ok(true) = fs::exists(&cli_args.config) {
            builder = builder.add_source(config::File::with_name(&cli_args.config));
        } else if cli_args.config != DEFAULT_CONFIG_FILE_PATH {
            // In case a config file was explicitly provided, it must exist
            return Err(ConfigError::Message(format!(
                "Config file '{}' not found",
                cli_args.config
            )));
        }

        // CLI overrides
        if let Some(port) = &cli_args.port {
            builder = builder.set_override("port", *port)?;
        }

        if let Some(program) = &cli_args.program {
            builder = builder.set_override("runtime.program", program.as_str())?;
        }

        if let Some(build) = &cli_args.build {
            builder = builder.set_override("runtime.build", build.as_str())?;
        }

        let config = builder.build()?.try_deserialize::<MainConfig>();

        config
    }

    /// TODO: Auto detect defaults by folder structure
    fn detect_defaults() -> MainConfig {
        MainConfig {
            runtime: RuntimeConfig {
                adapter: String::from(
                    "/Users/ronbiton/.vscode/extensions/vadimcn.vscode-lldb-1.12.2/adapter/codelldb",
                ),
                adapter_args: vec![],
                build: String::from("cargo build"),
                mode: RuntimeModes::Headless {
                    program: String::from("target/debug/test_app"),
                    program_args: vec![],
                    program_env_file: None,
                    port: 2500,
                },
            },
            watcher: WatcherConfig {
                paths: vec![String::from("./src/**.rs")],
                ignore_paths: vec![],
                gitignore: true,
                debounce_ms: 500,
            },
        }
    }

    pub fn init() {
        println!("Hello!");
    }
}
