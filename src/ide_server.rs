use std::io::Error;
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
};

pub struct IdeServer {
    port: u16,
    listener: TcpListener,
    stream: Option<TcpStream>,
    buffer: Vec<u8>,
}

impl IdeServer {
    pub async fn new(port: u16) -> Result<IdeServer, Error> {
        let server = TcpListener::bind(("127.0.0.1", port)).await?;

        println!("IDE Server is listening on 127.0.0.1:{}", port);

        Ok(IdeServer {
            port,
            listener: server,
            stream: None,
            buffer: Vec::new(),
        })
    }

    pub async fn connect(&mut self) -> Result<(), Error> {
        let (stream, _) = self.listener.accept().await?;

        println!("Connection has been established");

        self.stream = Some(stream);
        Ok(())
    }

    pub async fn read(&mut self) -> Result<Vec<u8>, Error> {
        loop {
            if self.buffer.len() >= 50 {
                let message = self.buffer.clone();
                self.buffer.clear();
                return Ok(message);
            }

            let mut stream = match &mut self.stream {
                Some(stream) => stream,
                None => {
                    return Err(Error::new(
                        std::io::ErrorKind::NotConnected,
                        "IDE has not yet established connection",
                    ));
                }
            };

            let mut next_buffer: [u8; 1024] = [0; 1024];
            let next_buffer_length = match stream.read(&mut next_buffer).await {
                Ok(0) => {
                    self.stream = None;
                    return Ok(Vec::<u8>::new());
                }
                Ok(n) => n,
                Err(e) => {
                    return Err(e);
                }
            };

            self.buffer
                .extend_from_slice(&next_buffer[..next_buffer_length]);

            println!("Current buffer length: {}", self.buffer.len())
        }
    }
}
