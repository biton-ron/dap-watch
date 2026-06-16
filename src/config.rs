use std::vec;

/// File watcher configuration
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

/// Runtime configuration for the build, program, and debug adapter
pub struct RuntimeConfig {
    /// Path to the debug adapter binary (e.g., "/path/to/codelldb")
    pub adapter: String,
    /// Additional arguments passed to the debug adapter, appended after the port flag managed by dap-watch
    pub adapter_args: Vec<String>,
    /// Shell command to build the program (e.g., "cargo build")
    pub build: String,
    /// Path to the compiled program binary
    pub program: String,
    /// Arguments passed to the program when launched
    pub program_args: Vec<String>,
    /// Path to an environment file loaded before running the program
    pub program_env_file: Option<String>,
}

/// Root configuration for dap-watch
pub struct Config {
    /// Port the proxy listens on for IDE connections
    pub port: u16,
    pub runtime: RuntimeConfig,
    pub watcher: WatcherConfig,
}

impl Config {
    pub fn build(config_file: Option<String>) -> Config {
        return Config {
            port: 2500,
            runtime: RuntimeConfig {
                adapter: String::from(
                    "/Users/ronbiton/.vscode/extensions/vadimcn.vscode-lldb-1.12.2/adapter/codelldb",
                ),
                adapter_args: vec![],
                build: String::from("cargo build --bin test_app"),
                program: String::from("target/debug/test_app"),
                program_args: vec![],
                program_env_file: None,
            },
            watcher: WatcherConfig {
                paths: vec![String::from("./src/**.rs")],
                ignore_paths: vec![],
                gitignore: true,
                debounce_ms: 500,
            },
        };
    }
}
