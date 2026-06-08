use anyhow::{Context, Ok, Result, bail};
use tokio::{io::AsyncReadExt, net::TcpStream};

/// We only specify in this enum commands are required for state preservation.
/// As an example - setBreakpoints is a crucial part of the state, and will be replayed to debuggers when re-spawned.
/// Everything else falls into the PassForward() category which will be sent directly by the proxy without thouching state at all.
#[derive(Debug, PartialEq)]
pub enum RequestCommandTypes {
    Initialize,
    Attach,
    Launch,
    SetBreakpoints(String),
    SetDataBreakpoints,
    SetExecutionBreakpoints,
    SetFunctionBreakpoints,
    SetInstructionBreakpoints,
    PassForward, // Fallback for all the rest
}

#[derive(Debug, PartialEq)]
pub enum DapMessage {
    Request {
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

    pub async fn read(&mut self) -> Result<Vec<u8>> {
        loop {
            if self.buffer.len() >= 500 {
                let message = self.buffer.clone();
                self.buffer.clear();
                return Ok(message);
            }

            let stream = match &mut self.stream {
                Some(stream) => stream,
                None => bail!("IDE has not yet established connection"),
            };

            let mut next_buffer: [u8; 1024] = [0; 1024];
            let next_buffer_length = match stream
                .read(&mut next_buffer)
                .await
                .context("Could not read form IDE stream")?
            {
                0 => {
                    self.stream = None;
                    return Ok(Vec::<u8>::new());
                }
                n => n,
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
/// When parsing is sucessful (no errors), this function returns Option<DapMessage, Vec<u8>>, why optional? because buffer is inhertily incomplete, which means:
/// 1. There is no gurantee that buffer holds a full message yet.
/// 2. Buffer in many cases will include some bytes for the next message.
///
/// So to answer both:
/// 1. If a complete message could not be found in the buffer, return None.
/// 2. If a message was found in the buffer - return a DapMessage and a subset buffer with the leftover bytes.
///
/// If there is an error, the Result::Err would be returned.
fn parse_dap_message(buffer: &[u8]) -> Result<Option<(DapMessage, Vec<u8>)>> {
    // Looking for the first new line ("\r\n\r\n") in the buffer, this marks the end of the Content-Length header
    let new_line_index = buffer
        .windows(HEADER_DELIMITER_LENGTH)
        .position(|w| w == HEADER_DELIMITER);

    if let Some(new_line_index) = new_line_index {
        let header = &buffer[0..new_line_index];
        let body_length_str = String::from_utf8_lossy(header).replace("Content-Length: ", "");
        let body_length = body_length_str
            .parse::<usize>()
            .context("Parsing Content-Length header has failed, no length was found")?;
        let body_start_index = new_line_index + HEADER_DELIMITER_LENGTH;

        if buffer.len() < body_start_index + body_length {
            // Buffer does not include the full body of the message, Ok(None) since its not neceserally an error, but most likely an incomplete buffer
            return Ok(None);
        }

        let body = &buffer[body_start_index..body_start_index + body_length];
        let parsed_message = parse_dap_body(body).context("Failed to parse DAP message body")?;
        let buffer_leftovers = Vec::from(&buffer[body_start_index + body_length + 1..buffer.len()]);

        return Ok(Some((parsed_message, buffer_leftovers)));
    }

    Ok(None)
}

/// Takes a complete body and parse it as a DapMessage.
/// Body is assuemd to be a valid JSON buffer, if JSON parsing failed or DapMessage could not be constructed, and error would be returned instead.
fn parse_dap_body(body: &[u8]) -> Result<DapMessage> {
    let parsed_json: serde_json::Value =
        serde_json::from_slice(body).context("Could not parse DAP message: Invalid JSON")?;

    let message_type_str = parsed_json["type"]
        .as_str()
        .context("Could not parse DAP message type")?;

    match message_type_str {
        "event" => Ok(DapMessage::Event(Vec::from(body))),
        "response" => Ok(DapMessage::Response(Vec::from(body))),
        "request" => {
            let command_type_str = parsed_json["command"].as_str().context(
                "Request should have a command attached to it as a string, could not parse request",
            )?;

            let command: RequestCommandTypes = match command_type_str {
                "setBreakpoints" => {
                    let file_path = parsed_json["arguments"]["source"]["path"]
                        .as_str()
                        .context("DAP parsing failed: setBreakpoints is missing source path")?;

                    RequestCommandTypes::SetBreakpoints(file_path.to_string())
                }
                "setDataBreakpoints" => RequestCommandTypes::SetDataBreakpoints,
                "setExecutionBreakpoints" => RequestCommandTypes::SetExecutionBreakpoints,
                "setFunctionBreakpoints" => RequestCommandTypes::SetFunctionBreakpoints,
                "setInstructionBreakpoints" => RequestCommandTypes::SetInstructionBreakpoints,
                "initialize" => RequestCommandTypes::Initialize,
                "attach" => RequestCommandTypes::Attach,
                "launch" => RequestCommandTypes::Launch,
                _ => RequestCommandTypes::PassForward,
            };

            Ok(DapMessage::Request {
                raw_bytes: Vec::from(body),
                command,
            })
        }
        _ => bail!("Invalid DAP message type"),
    }
}

#[cfg(test)]
mod tests {
    use crate::dap_stream::{DapMessage, RequestCommandTypes, parse_dap_body};

    #[test]
    fn test_parse_dap_body() {
        let dap_message =
            parse_dap_body(b"{\"seq\": 1, \"type\": \"request\", \"command\": \"initialize\"}")
                .unwrap();

        match dap_message {
            DapMessage::Request { command, .. } => {
                assert_eq!(command, RequestCommandTypes::Initialize);
            }
            _ => panic!("Expected message type to be ::Request"),
        }
    }
}
