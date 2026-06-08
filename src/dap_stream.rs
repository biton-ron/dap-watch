use std::io::Error;
use tokio::{io::AsyncReadExt, net::TcpStream};

/// We only specify in this enum commands are required for state preservation.
/// As an example - setBreakpoints is a crucial part of the state, and will be replayed to debuggers when re-spawned.
/// Everything else falls into the PassForward() category which will be sent directly by the proxy without thouching state at all.
enum RequestCommandTypes {
    Initialize,
    Attach,
    Launch,
    SetBreakpoints(),
    SetDataBreakpoints(),
    SetExecutionBreakpoints(),
    SetFunctionBreakpoints(),
    SetInstructionBreakpoints(),
    PassForward(), // Fallback for all the rest
}

enum DapMessage {
    Request {
        seq: usize,
        raw_bytes: Vec<u8>,
        command: RequestCommandTypes,
    },
    Event(Vec<u8>),
    Response(Vec<u8>),
}

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
            _parse_dap_message(&self.buffer);

            if self.buffer.len() >= 500 {
                let message = self.buffer.clone();
                self.buffer.clear();
                return Ok(message);
            }

            let stream = match &mut self.stream {
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

    pub fn write() {
        println!("write!");
    }
}

const HEADER_DELIMITER: &[u8] = b"\r\n\r\n";
const HEADER_DELIMITER_LENGTH: usize = 4;

/// Takes a bytes buffer and parse its headers and body looking for a DAP message.
/// DAP messages are plain JSON with Content-Length header as following:
/// "Content-Length: 12\r\n\r\n{ ... }"
///
/// When parsing is sucessful (no errors), this function returns Result::Ok(Option<DapMessage, Vec<u8>>), why optional? because buffer is inhertily incomplete, which means:
/// 1. There is no gurantee that buffer holds a full message yet.
/// 2. Buffer in many cases will include some bytes for the next message.
///
/// So to answer both:
/// 1. If a complete message could not be found in the buffer, return None.
/// 2. If a message was found in the buffer - return a DapMessage and a subset buffer with the leftover bytes.
///
/// If there is an error, the Result::Err would be returned.
fn _parse_dap_message(
    buffer: &[u8],
) -> Result<Option<(DapMessage, Vec<u8>)>, Box<dyn std::error::Error>> {
    // Looking for the first new line ("\r\n\r\n") in the buffer, this marks the end of the Content-Length header
    let new_line_index = buffer
        .windows(HEADER_DELIMITER_LENGTH)
        .position(|w| w == HEADER_DELIMITER);

    if let Some(new_line_index) = new_line_index {
        let header = &buffer[0..new_line_index];
        let body_length_str = String::from_utf8_lossy(header).replace("Content-Length: ", "");
        let body_length = body_length_str.parse::<usize>()?;
        let body_start_index = new_line_index + HEADER_DELIMITER_LENGTH;

        if buffer.len() < body_start_index + body_length {
            // Buffer does not include the full body of the message, Ok(None) since its not neceserally an error, but most likely an incomplete buffer
            return Ok(None);
        }

        let body = &buffer[body_start_index..body_start_index + body_length];
        let parsed_json = serde_json::from_slice(body);

        println!("{:#?}", parsed_json);

        return Ok(Some((DapMessage {}, Vec::<u8>::new())));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use crate::dap_stream::_parse_dap_message;

    #[test]
    fn test_dap_message_parsing() {
        println!("...");

        match _parse_dap_message(&Vec::from(
            b"Content-Length: 12\r\n\r\n{\"test_key\": \"test_value\"}",
        )) {
            Ok(None) => {
                println!("None!");
            }
            Ok(result) => {
                println!("Ok at least!");
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }
}
