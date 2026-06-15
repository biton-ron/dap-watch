use anyhow::{Context, Result, bail};

/// We only specify in this enum commands are required for state preservation.
/// As an example - setBreakpoints is a crucial part of the state, and will be replayed to debuggers when re-spawned.
/// Everything else falls into the PassForward() category which will be sent directly by the proxy without thouching state at all.
#[derive(Debug, PartialEq)]
pub enum RequestCommandTypes {
    Initialize,
    Attach,
    Launch,
    ConfigurationDone,
    SetBreakpoints(String), // String is the file_path
    SetExceptionBreakpoints,
    SetFunctionBreakpoints,
    PassForward(String), // Fallback for all the rest
}

#[derive(Debug, PartialEq)]
pub enum EventTypes {
    Output(String),
    Other(String),
}

#[derive(Debug, PartialEq)]
pub enum DapMessage {
    Request {
        seq: u64,
        raw_bytes: Vec<u8>,
        command: RequestCommandTypes,
    },
    Event {
        seq: u64,
        raw_bytes: Vec<u8>,
        event: EventTypes,
    },
    Response {
        seq: u64,
        raw_bytes: Vec<u8>,
    },
}

impl std::fmt::Display for DapMessage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DapMessage::Event { event, seq, .. } => {
                write!(formatter, "[SEQ: {}] DapMessage::Event | {:?}", seq, event)
            }
            DapMessage::Response { seq, .. } => {
                write!(formatter, "[SEQ: {}] DapMessage::Response", seq)
            }
            DapMessage::Request { command, seq, .. } => {
                write!(
                    formatter,
                    "[SEQ: {}] DapMessage::Request | {:?}",
                    seq, command
                )
            }
        }
    }
}

/// Takes a complete body and parse it as a DapMessage.
///
/// `body` is only the JSON part of the message (no headers, full JSON).  
///
/// `full_message` is full message including headers (it is kept on DapMessage for forward-passing between DapStreams).
///
/// Body is assuemd to be a valid JSON buffer, if JSON parsing failed or DapMessage could not be constructed, and error would be returned instead.
pub fn parse_dap_body(body: &[u8], full_message: &[u8]) -> Result<DapMessage> {
    let parsed_json: serde_json::Value =
        serde_json::from_slice(body).context("Could not parse DAP message: Invalid JSON")?;

    let message_type_str = parsed_json["type"]
        .as_str()
        .context("Could not parse DAP message type")?;

    let seq = parsed_json["seq"]
        .as_u64()
        .context("Unabled to extract seq id from message body")?;

    match message_type_str {
        "event" => {
            let event_type = parsed_json["event"]
                .as_str()
                .context("Parser could not find an event type")?;

            Ok(DapMessage::Event {
                seq,
                raw_bytes: Vec::from(full_message),
                event: match event_type {
                    "output" => {
                        let output = parsed_json["body"]["output"]
                            .as_str()
                            .context("Parser could not find output on event with output type")?;

                        EventTypes::Output(String::from(output))
                    }
                    _ => EventTypes::Other(String::from(event_type)),
                },
            })
        }
        "response" => Ok(DapMessage::Response {
            seq,
            raw_bytes: Vec::from(full_message),
        }),
        "request" => {
            let command_type_str = parsed_json["command"].as_str().context(
                "Could not parse request: a Request should have a command attached to it as a string",
            )?;

            let command: RequestCommandTypes = match command_type_str {
                "setBreakpoints" => {
                    let file_path = parsed_json["arguments"]["source"]["path"]
                        .as_str()
                        .context("DAP parsing failed: setBreakpoints is missing source path")?;

                    RequestCommandTypes::SetBreakpoints(file_path.to_string())
                }
                "setExceptionBreakpoints" => RequestCommandTypes::SetExceptionBreakpoints,
                "setFunctionBreakpoints" => RequestCommandTypes::SetFunctionBreakpoints,
                "initialize" => RequestCommandTypes::Initialize,
                "attach" => RequestCommandTypes::Attach,
                "launch" => RequestCommandTypes::Launch,
                "configurationDone" => RequestCommandTypes::ConfigurationDone,
                _ => RequestCommandTypes::PassForward(String::from(command_type_str)),
            };

            Ok(DapMessage::Request {
                seq,
                raw_bytes: Vec::from(full_message),
                command,
            })
        }
        _ => bail!("Invalid DAP message type"),
    }
}

#[cfg(test)]
mod tests {
    use crate::dap_message::{DapMessage, EventTypes, parse_dap_body};

    // Tests for parse_dap_body code paths that are not covered by dap_stream's tests suite.
    #[test]
    fn test_parse_body_invalid_json_returns_error() {
        let buffer = r#"not valid json at all"#.as_bytes();
        let parsed = parse_dap_body(buffer, buffer);

        assert!(parsed.is_err());
    }

    #[test]
    fn test_parse_body_missing_type_returns_error() {
        let buffer = r#"{"seq":1,"command":"initialize"}"#.as_bytes();
        let parsed = parse_dap_body(buffer, buffer);

        assert!(parsed.is_err());
    }

    #[test]
    fn test_parse_body_request_missing_command_returns_error() {
        let buffer = r#"{"seq":1,"type":"request"}"#.as_bytes();
        let parsed = parse_dap_body(buffer, buffer);

        assert!(parsed.is_err());
    }

    #[test]
    fn test_parse_body_set_breakpoints_missing_source_path_returns_error() {
        let buffer =
            r#"{"seq":1,"type":"request","command":"setBreakpoints","arguments":{}}"#.as_bytes();
        let parsed = parse_dap_body(buffer, buffer);

        assert!(parsed.is_err());
    }

    #[test]
    fn test_parse_output_event_stdout() {
        let buffer = r#"{"seq":1,"type":"event","event":"output","body":{"category":"stdout","output":"hello world\n"}}"#.as_bytes();
        let parsed = parse_dap_body(buffer, buffer);

        match parsed {
            Ok(message) => {
                assert_eq!(
                    message,
                    DapMessage::Event {
                        seq: 1,
                        raw_bytes: Vec::from(buffer),
                        event: EventTypes::Output(String::from("hello world\n"))
                    }
                )
            }
            Err(_) => panic!("Expected event to be parsed"),
        }
    }

    #[test]
    fn test_parse_non_output_event_returns_other() {
        let buffer = r#"{"seq":500,"type":"event","event":"initialized"}"#.as_bytes();
        let parsed = parse_dap_body(buffer, buffer);

        match parsed {
            Ok(message) => {
                assert_eq!(
                    message,
                    DapMessage::Event {
                        seq: 500,
                        raw_bytes: Vec::from(buffer),
                        event: EventTypes::Other(String::from("initialized")),
                    }
                )
            }
            Err(_) => panic!("Expected event to be parsed"),
        }
    }

    #[test]
    fn test_parse_output_event_missing_body_returns_error() {
        let buffer = r#"{"seq":1,"type":"event","event":"output"}"#.as_bytes();
        let parsed = parse_dap_body(buffer, buffer);

        assert!(parsed.is_err());
    }

    #[test]
    fn test_parse_output_event_missing_output_returns_error() {
        let buffer =
            r#"{"seq":1,"type":"event","event":"output","body":{"category":"stdout"}}"#.as_bytes();
        let parsed = parse_dap_body(buffer, buffer);

        assert!(parsed.is_err());
    }
}
