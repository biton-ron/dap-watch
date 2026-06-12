use crate::dap_stream::DapStream;

use std::io::Error;
use tokio::net::TcpListener;

#[derive(Default, PartialEq)]
pub enum IdeStatus {
    #[default]
    Pending,
    Listening,
    Connected,
}

pub struct IdeServer {
    listener: TcpListener,
}

impl IdeServer {
    pub async fn new(port: u16) -> Result<IdeServer, Error> {
        let listener = TcpListener::bind(("127.0.0.1", port)).await?;

        println!("IDE Server is listening on 127.0.0.1:{}", port);

        Ok(IdeServer { listener })
    }

    pub async fn connect(&mut self) -> Result<DapStream, Error> {
        let (raw_stream, _) = self.listener.accept().await?;
        let dap_stream = DapStream::new(raw_stream);

        println!("Connection has been established");

        Ok(dap_stream)
    }
}
