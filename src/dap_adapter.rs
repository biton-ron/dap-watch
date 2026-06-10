use anyhow::{Context, Result};
use tokio::process::{Child, Command};

struct AdapterConfig {}

const TMP_CODELLB_PATH: &str = "~/.vscode/extensions/vadimcn.vscode-lldb-1.12.2/adapter/codelldb";

enum AdapterStatus {
    Pending,
    Started,
    Replaying,
    Live,
}

pub struct DapAdapter {
    process: Option<Child>,
    config: AdapterConfig,
    status: AdapterStatus,
}

impl DapAdapter {
    pub fn new() -> DapAdapter {
        DapAdapter {
            process: None,
            config: AdapterConfig {},
            status: AdapterStatus::Pending,
        }
    }

    pub async fn spawn(&mut self) -> Result<()> {
        let child = Command::new(TMP_CODELLB_PATH)
            .arg("--port")
            .arg("0")
            .spawn()
            .context("Unable to spawn debugger as a child process")?;

        self.process = Some(child);
        self.status = AdapterStatus::Started;

        Ok(())
    }
}
