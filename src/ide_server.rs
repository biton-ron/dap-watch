use std::io::Error;
use tokio::net::{TcpListener, TcpStream};

struct IdeServer {
    port: u16,
    listener: TcpListener,
    stream: Option<TcpStream>,
    buffer: Vec<u8>,
}

impl IdeServer {
    pub async fn new(port: u16) -> Result<IdeServer, Error> {
        let server = TcpListener::bind(("127.0.0.1", port)).await?;
        Ok(IdeServer {
            port,
            listener: server,
            stream: None(),
            buffer: vec![],
        })
    }

    pub async fn connect(&mut self) -> Result<(), Error> {
        let (stream, _) = self.listener.accept().await?;
        self.stream = Some(stream);
        Ok(())
    }

    pub async fn read(&self) -> Result<Vec<u8>, Error> {}
}
