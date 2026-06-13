use crate::dap_stream::DapStream;

use anyhow::{Context, Result, bail};
use std::{env, process::Stdio, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    process::{Child, Command},
    time::sleep,
};

#[derive(Default)]
struct AdapterConfig {}

const TMP_CODELLB_PATH: &str =
    "/Users/ronbiton/.vscode/extensions/vadimcn.vscode-lldb-1.12.2/adapter/codelldb";

const CONNECTION_LOOP_MAX_ERRORS: u16 = 30;

#[derive(Default, PartialEq)]
pub enum AdapterStatus {
    #[default]
    Pending,
    Spawned,
    Connected,
    Replaying,
    Alive,
}

#[derive(Default)]
pub struct DapAdapter {
    port: Option<u16>,
    process: Option<Child>,
    config: AdapterConfig,
    status: AdapterStatus,
}

impl DapAdapter {
    pub fn new() -> DapAdapter {
        DapAdapter::default()
    }

    pub async fn spawn(&mut self) -> Result<()> {
        let port = port_selection()
            .await
            .context("Port selection for debugger has failed")?;

        let child = Command::new(TMP_CODELLB_PATH)
            .arg("--port")
            .arg(port.to_string())
            .kill_on_drop(true)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Unable to spawn debugger as a child process")?;

        self.process = Some(child);
        self.status = AdapterStatus::Spawned;
        self.port = Some(port);

        Ok(())
    }

    pub async fn connect(&mut self) -> Result<DapStream> {
        let port = self.port.context("Could not find a port to connect to")?;
        let mut errors_count = 0;

        loop {
            let stream = TcpStream::connect(("127.0.0.1", port)).await;

            match stream {
                Ok(stream) => {
                    let stream = DapStream::new(stream);

                    self.status = AdapterStatus::Connected;

                    return Ok(stream);
                }
                Err(_) => {
                    errors_count += 1;

                    if errors_count == CONNECTION_LOOP_MAX_ERRORS {
                        bail!("Could not establish connection to debug adapter");
                    }

                    sleep(Duration::from_millis(50)).await;
                }
            }
        }
    }
}

async fn port_selection() -> Result<u16> {
    let tmp_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("Unable to bind a TcpListener to a port")?;

    let port: u16 = tmp_listener.local_addr()?.port();

    drop(tmp_listener);

    return Ok(port);
}
