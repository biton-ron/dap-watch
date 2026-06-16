use crate::{config::RuntimeConfig, dap_stream::DapStream};

use anyhow::{Context, Result, bail};
use std::{env, process::Stdio, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    process::{Child, Command},
    time::sleep,
};

const CONNECTION_LOOP_MAX_ERRORS: u16 = 30;

#[derive(Default, PartialEq)]
pub enum AdapterStatus {
    #[default]
    Building,
    Pending,
    Spawned,
    Replaying,
    Connected,
}

pub struct DapAdapter {
    port: Option<u16>,
    process: Option<Child>,
    config: RuntimeConfig,
}

impl DapAdapter {
    pub fn new(config: &RuntimeConfig) -> DapAdapter {
        DapAdapter {
            port: None,
            process: None,
            config: config.clone(),
        }
    }

    pub async fn spawn(&mut self) -> Result<()> {
        let port = port_selection()
            .await
            .context("Port selection for debugger has failed")?;

        let child = Command::new(&self.config.adapter)
            .arg("--port")
            .arg(port.to_string())
            .args(&self.config.adapter_args)
            .kill_on_drop(true)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Unable to spawn debugger as a child process")?;

        self.process = Some(child);
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

    pub async fn kill(&mut self) -> Result<()> {
        if let Some(process) = &mut self.process {
            process.kill().await?;
            self.process = None;
        }

        Ok(())
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
