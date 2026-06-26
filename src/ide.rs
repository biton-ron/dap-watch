use crate::{config::RuntimeModes, dap_stream::DapStream};

use std::io::Error;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
};

#[derive(Default, PartialEq)]
pub enum IdeStatus {
    #[default]
    Pending,
    Listening,
    Connected,
}

pub enum IdeHandler {
    Tcp { listener: TcpListener },
    Stdio,
}

impl IdeHandler {
    pub async fn new(mode: RuntimeModes) -> Result<IdeHandler, Error> {
        match mode {
            RuntimeModes::Headless { port, .. } => {
                let listener = TcpListener::bind(("127.0.0.1", port)).await?;
                Ok(Self::Tcp { listener })
            }
            RuntimeModes::Stdio => Ok(Self::Stdio),
        }
    }

    pub async fn connect(&mut self) -> Result<DapStream, Error> {
        let (reader, writer): (Box<dyn AsyncRead + Unpin>, Box<dyn AsyncWrite + Unpin>) = match self
        {
            Self::Tcp { listener } => {
                let (stream, _) = listener.accept().await?;
                let (r, w) = stream.into_split();
                (Box::new(r), Box::new(w))
            }
            Self::Stdio => (Box::new(tokio::io::stdin()), Box::new(tokio::io::stdout())),
        };

        let dap_stream = DapStream::new(reader, writer);

        Ok(dap_stream)
    }
}
