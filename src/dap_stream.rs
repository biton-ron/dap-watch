///! This module is mainly for the DapStream struct, its implementation and all other supporting utlities.
///! DapStream is a wrapper around TcpStream that can read and parse buffers as DapMessage structures.
use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::dap_message::DapMessage;

pub struct DapStream {
    reader: Box<dyn AsyncRead + Unpin>,
    writer: Box<dyn AsyncWrite + Unpin>,
    buffer: Vec<u8>,
}

pub enum ReadResult {
    Message(DapMessage),
    EOF, // End-of-file
}

impl DapStream {
    pub fn new(
        reader: Box<dyn AsyncRead + Unpin>,
        writer: Box<dyn AsyncWrite + Unpin>,
    ) -> DapStream {
        DapStream {
            reader,
            writer,
            buffer: Vec::<u8>::new(),
        }
    }

    pub async fn read(&mut self) -> Result<ReadResult> {
        loop {
            let parsed = DapStream::parse_message(&self.buffer);

            match parsed {
                Ok(None) => {}
                Ok(Some((message, leftovers))) => {
                    self.buffer = leftovers;
                    return Ok(ReadResult::Message(message));
                }
                Err(e) => {
                    return Err(e);
                }
            }

            let mut next_buffer: [u8; 1024] = [0; 1024];
            let next_buffer_length = match self
                .reader
                .read(&mut next_buffer)
                .await
                .context("Could not read form IDE stream")?
            {
                0 => return Ok(ReadResult::EOF),
                n => n,
            };

            self.buffer
                .extend_from_slice(&next_buffer[..next_buffer_length]);
        }
    }

    pub async fn write(&mut self, message: &DapMessage) -> Result<()> {
        let stream = self.writer.as_mut();

        match message {
            DapMessage::Event { raw_bytes, .. }
            | DapMessage::Response { raw_bytes, .. }
            | DapMessage::Request { raw_bytes, .. } => {
                stream.write_all(&raw_bytes).await?;
                stream.flush().await?;
            }
        }

        Ok(())
    }

    /// Helper that wraps an optional DapStream as a Future.
    /// This makes it easier to use DapStram.read on select! loop while DapStream is optionally None (Future will not resolve if thats the case).
    pub async fn read_stream(stream: &mut Option<DapStream>) -> Result<ReadResult> {
        match stream {
            Some(stream) => stream.read().await,
            None => std::future::pending().await,
        }
    }

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
    fn parse_message(buffer: &[u8]) -> Result<Option<(DapMessage, Vec<u8>)>> {
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
            let body_end_index = body_start_index + body_length;

            if buffer.len() < body_start_index + body_length {
                // Buffer does not include the full body of the message, Ok(None) since its not neceserally an error, but most likely an incomplete buffer
                return Ok(None);
            }

            let body = &buffer[body_start_index..body_end_index];
            let parsed_message = DapMessage::parse_body(body, &buffer[0..body_end_index])
                .context("Failed to parse DAP message body")?;

            // Anything in the buffer that did not belong to the parsed message is kept as leftovers (if any)
            let buffer_leftovers = if body_end_index == buffer.len() {
                vec![]
            } else {
                Vec::from(&buffer[body_start_index + body_length..buffer.len()])
            };

            return Ok(Some((parsed_message, buffer_leftovers)));
        }

        Ok(None)
    }
}

const HEADER_DELIMITER: &[u8] = b"\r\n\r\n";
const HEADER_DELIMITER_LENGTH: usize = 4;

#[cfg(test)]
mod tests {
    use crate::{
        dap_message::{DapMessage, EventTypes, RequestCommandTypes},
        dap_stream::DapStream,
    };

    fn make_dap_message(body: &str) -> Vec<u8> {
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
    }

    // DapStream::parse_message tests
    #[test]
    fn test_parse_complete_request() {
        let buffer = make_dap_message(r#"{"seq":152,"type":"request","command":"initialize"}"#);
        let parsed = DapStream::parse_message(&buffer).unwrap();

        match parsed {
            Some((message, leftovers)) => {
                assert_eq!(leftovers.len(), 0); // Complete message, should not have leftovers

                match message {
                    DapMessage::Request {
                        seq,
                        command,
                        raw_bytes,
                    } => {
                        assert_eq!(seq, 152);
                        assert_eq!(command, RequestCommandTypes::Initialize);
                        assert_eq!(raw_bytes, buffer);
                    }
                    _ => panic!("Message type is expected to be a Request"),
                }
            }
            None => panic!("Expected parsing to result in DapMessage"),
        }
    }

    #[test]
    fn test_parse_complete_event() {
        let buffer = make_dap_message(r#"{"seq":1,"type":"event","event":"initialized"}"#);
        let parsed = DapStream::parse_message(&buffer).unwrap();

        match parsed {
            Some((message, leftovers)) => {
                assert_eq!(leftovers.len(), 0); // Complete message, should not have leftovers
                assert_eq!(
                    message,
                    DapMessage::Event {
                        seq: 1,
                        raw_bytes: buffer,
                        event: EventTypes::Other(String::from("initialized"))
                    }
                );
            }
            None => panic!("Expected parsing to result in DapMessage"),
        }
    }

    #[test]
    fn test_parse_complete_response() {
        let buffer =
            make_dap_message(r#"{"seq":1,"type":"response","request_seq":15,"success":true}"#);
        let parsed = DapStream::parse_message(&buffer).unwrap();

        match parsed {
            Some((message, leftovers)) => {
                assert_eq!(leftovers.len(), 0); // Complete message, should not have leftovers
                assert_eq!(
                    message,
                    DapMessage::Response {
                        seq: 1,
                        request_seq: 15,
                        raw_bytes: buffer
                    }
                );
            }
            None => panic!("Expected parsing to result in DapMessage"),
        }
    }

    #[test]
    fn test_parse_incomplete_body_returns_none() {
        let buffer = b"Content-Length: 999\r\n\r\n{\"seq\":1,\"type\":\"reque";
        let parsed = DapStream::parse_message(buffer).unwrap();

        assert_eq!(parsed, None);
    }

    #[test]
    fn test_parse_no_header_returns_none() {
        let buffer = r#"{"seq":1,"type":"request","command":"initialize"}"#.as_bytes();
        let parsed = DapStream::parse_message(buffer).unwrap();

        assert_eq!(parsed, None);
    }

    #[test]
    fn test_parse_empty_buffer_returns_none() {
        let buffer = b"";
        let parsed = DapStream::parse_message(buffer).unwrap();

        assert_eq!(parsed, None);
    }

    #[test]
    fn test_parse_message_with_leftovers() {
        let initial_message =
            make_dap_message(r#"{"seq":500,"type":"request","command":"initialize"}"#);
        let follow_up_message = make_dap_message(r#"{"seq":2,"type":"event","event":"stopped"}"#);

        // Buffer contains two messages at once, leftovers should include the follow up message
        let buffer = [initial_message.as_slice(), follow_up_message.as_slice()].concat();
        let parsed = DapStream::parse_message(&buffer).unwrap();

        match parsed {
            Some((message, leftovers)) => {
                assert_eq!(leftovers, follow_up_message);

                match message {
                    DapMessage::Request {
                        seq: 500,
                        command,
                        raw_bytes,
                    } => {
                        assert_eq!(command, RequestCommandTypes::Initialize);
                        assert_eq!(raw_bytes, initial_message);
                    }
                    _ => panic!("Message type is expected to be a Request"),
                }
            }
            None => panic!("Expected a message to be parsed succsesfully"),
        }
    }

    #[test]
    fn test_parse_set_breakpoints_with_source_path() {
        let buffer = make_dap_message(
            r#"{"seq":1,"type":"request","command":"setBreakpoints","arguments":{"source":{"path":"/test/path.rs"},"breakpoints":[{"line":10}]}}"#,
        );

        let parsed = DapStream::parse_message(&buffer).unwrap();

        match parsed {
            Some((message, leftovers)) => {
                assert_eq!(leftovers.len(), 0); // Complete message, should not have leftovers

                match message {
                    DapMessage::Request {
                        seq,
                        command,
                        raw_bytes,
                    } => {
                        assert_eq!(
                            command,
                            RequestCommandTypes::SetBreakpoints(String::from("/test/path.rs"))
                        );
                        assert_eq!(seq, 1);
                        assert_eq!(raw_bytes, buffer);
                    }
                    _ => panic!("Message type is expected to be a Request"),
                }
            }
            None => panic!("Expected parsing to result in DapMessage"),
        }
    }

    #[test]
    fn test_parse_unknown_command_returns_pass_forward() {
        let buffer = make_dap_message(r#"{"seq":1,"type":"request","command":"continue"}"#);
        let parsed = DapStream::parse_message(&buffer).unwrap();

        match parsed {
            Some((message, leftovers)) => {
                assert_eq!(leftovers.len(), 0); // Complete message, should not have leftovers

                match message {
                    DapMessage::Request {
                        seq,
                        command,
                        raw_bytes,
                    } => {
                        assert_eq!(seq, 1);
                        assert_eq!(
                            command,
                            RequestCommandTypes::PassForward(String::from("continue"))
                        );
                        assert_eq!(raw_bytes, buffer);
                    }
                    _ => panic!("Message type is expected to be a Request"),
                }
            }
            None => panic!("Expected parsing to result in DapMessage"),
        }
    }

    #[test]
    fn test_parse_invalid_content_length_returns_error() {
        let buffer = b"Content-Length: abc\r\n\r\n{\"seq\":1}";
        let parsed = DapStream::parse_message(buffer);

        assert!(parsed.is_err());
    }
}
