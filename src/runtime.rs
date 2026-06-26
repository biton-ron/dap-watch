use crate::{config::RuntimeConfig, dap_stream::DapStream};

use anyhow::{Context, Result, bail};
use std::{
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    process::{Child, Command},
    task::JoinHandle,
    time::sleep,
};

const CONNECTION_LOOP_MAX_ERRORS: u16 = 30;

pub struct Runtime {
    port: Option<u16>,
    process: Option<Child>,
    config: RuntimeConfig,
}

impl Runtime {
    pub fn new(config: &RuntimeConfig) -> Runtime {
        Runtime {
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
                    let (reader, writer) = stream.into_split();
                    let stream = DapStream::new(Box::new(reader), Box::new(writer));
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

    pub fn build(&self) -> JoinHandle<Result<ExitStatus>> {
        let build_cmd = self.config.build.clone();

        tokio::spawn(async {
            // Execute the build command configured by the user
            shell(build_cmd)
                .status()
                .await
                .context("Proxy has failed re-building the program")
        })
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

// Platform specific shells
#[cfg(unix)]
fn shell(cmd: String) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd);
    c
}

#[cfg(windows)]
fn shell(cmd: String) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/C").arg(cmd);
    c
}
