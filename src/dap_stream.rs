use std::io::Error;
use tokio::{io::AsyncReadExt, net::TcpStream};

enum DapMessage {}

pub struct DapStream {
    stream: Option<TcpStream>,
    buffer: Vec<u8>,
}

impl DapStream {
    pub fn new(stream: TcpStream) -> DapStream {
        DapStream {
            stream: Some(stream),
            buffer: Vec::<u8>::new(),
        }
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
