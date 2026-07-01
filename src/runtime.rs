use crate::{
    config::{RuntimeConfig, RuntimeModes},
    dap_stream::DapStream,
    log,
    logger::LogSource,
};

use anyhow::{Context, Result, bail};
use std::{
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    net::{TcpListener, TcpStream},
    process::{Child, Command},
    task::JoinHandle,
    time::sleep,
};

const CONNECTION_LOOP_MAX_ERRORS: u16 = 30;

pub struct Runtime {
    /// The user's application process. Only populated when dap-watch owns the
    /// process directly (headless, not debugging). When the adapter owns the
    /// process (rebuild during debug), this is None.
    program: Option<Child>,

    /// The debug adapter process (e.g. codelldb, delve). Spawned when
    /// the IDE connects, killed when IDE disconnects or on rebuild.
    adapter: Option<Child>,

    /// Port the adapter is listening on for DAP connections, this port is managed
    /// by dap-watch (picked automatically by the OS) and is not configurable.
    adapter_port: Option<u16>,

    config: RuntimeConfig,
}

impl Runtime {
    pub fn new(config: &RuntimeConfig) -> Runtime {
        Runtime {
            program: None,
            adapter_port: None,
            adapter: None,
            config: config.clone(),
        }
    }

    pub async fn spawn_adapter(&mut self) -> Result<()> {
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

        self.adapter = Some(child);
        self.adapter_port = Some(port);

        Ok(())
    }

    async fn kill_adapter(&mut self) -> Result<()> {
        if let Some(adapter) = &mut self.adapter {
            adapter.kill().await?;
            self.adapter = None;
        }

        Ok(())
    }

    pub async fn spawn_program(&mut self) -> Result<()> {
        if let RuntimeModes::Headless {
            program,
            program_args,
            ..
        } = &self.config.mode
        {
            let mut child = Command::new(&program)
                .arg("--port")
                .args(program_args)
                // TODO: we need to add environment variables support here
                .kill_on_drop(true)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("Unable to spawn program")?;

            Self::pipe_program_logs(child.stdout.take().unwrap());
            Self::pipe_program_logs(child.stderr.take().unwrap());

            self.program = Some(child);
        }

        Ok(())
    }

    async fn kill_program(&mut self) -> Result<()> {
        if let Some(program) = &mut self.program {
            // Process may have already been killed by the adapter — only kill if still running.
            match program.try_wait() {
                Ok(None) => program
                    .kill()
                    .await
                    .context("Could not kill program's child process")?,
                Ok(Some(_)) => {}
                Err(_) => {}
            }

            self.program = None;
        }

        Ok(())
    }

    fn pipe_program_logs(source: impl AsyncRead + Unpin + Send + 'static) {
        tokio::spawn(async {
            let reader = BufReader::new(source);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                log!(LogSource::Program, "{}", line);
            }
        });
    }

    pub async fn kill(&mut self) -> Result<()> {
        self.kill_adapter()
            .await
            .context("Failed to kill debug adapter")?;

        self.kill_program().await.context("Can't kill program")?;

        Ok(())
    }

    pub async fn connect(&mut self) -> Result<DapStream> {
        let port = self
            .adapter_port
            .context("Could not find a port to connect to")?;
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
