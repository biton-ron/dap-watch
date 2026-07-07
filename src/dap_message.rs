use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use serde_json::{Number, Value};

use crate::{
    dap_stream::{HEADER_DELIMITER, HEADER_DELIMITER_LENGTH},
    log,
    logger::{LogLevel, LogSource},
};

/// We only specify in this enum commands are required for state preservation.
/// As an example - setBreakpoints is a crucial part of the state, and will be replayed to debuggers when re-spawned.
/// Everything else falls into the PassForward() category which will be sent directly by the proxy without thouching state at all.
#[derive(Debug, PartialEq, Clone)]
pub enum RequestCommandTypes {
    Initialize,
    Launch,
    /// Attach holds the parsed arguments from the full message JSON.
    /// These arguments are used to re-construct a Launch message during rebuild.
    Attach(serde_json::Value),
    /// SetBreakpoints is different as it holds file_path as String.
    SetBreakpoints(String),
    SetExceptionBreakpoints,
    SetFunctionBreakpoints,
    ConfigurationDone,
    Disconnect,
    /// Fallback for all other command types
    PassForward(String),
}

#[derive(Debug, PartialEq, Clone)]
pub enum EventTypes {
    Output(String),
    Other(String),
}

#[derive(Debug, PartialEq, Clone)]
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
        request_seq: u64,
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
                write!(formatter, "[SEQ: {}] DapMessage::Request | {:?}", seq, command)
            }
        }
    }
}

/// Arbitrarily high enough to not conflict with actual IDE messages
const SEQ_COUNTER_INITIAL_VALUE: u64 = 9000000;

/// Keep tracks of the manually constracted messages in DapMessage, makes sure seq is not used twice to avoid conflicts.
static SEQ_COUNTER: AtomicU64 = AtomicU64::new(SEQ_COUNTER_INITIAL_VALUE);

impl DapMessage {
    /// Extract the sequence nubmer from self
    pub fn seq(&self) -> u64 {
        match self {
            DapMessage::Event { seq, .. } | DapMessage::Request { seq, .. } | DapMessage::Response { seq, .. } => *seq,
        }
    }

    /// Construct a DapMessage directly from a json string, wrapping the string with Content-Length for proper raw_bytes representation.
    fn from_str(body: &str) -> Result<DapMessage> {
        let raw_bytes = format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes();
        return Self::parse_body(body.as_bytes(), &raw_bytes);
    }

    /// Get the next sequence and bump its value
    fn get_next_seq() -> u64 {
        SEQ_COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    fn get_raw_bytes(&self) -> &Vec<u8> {
        match self {
            DapMessage::Event { raw_bytes, .. }
            | DapMessage::Request { raw_bytes, .. }
            | DapMessage::Response { raw_bytes, .. } => raw_bytes,
        }
    }

    /// Clone a DapMessage and assigns a new seq id to the cloned instance
    /// - `request_seq`:  Only acceptable when the original message is a `DapMessage::Response`.
    pub fn clone_with_new_seq(&self, request_seq: Option<u64>) -> DapMessage {
        let raw_bytes = self.get_raw_bytes();
        let new_line_index = raw_bytes
            .windows(HEADER_DELIMITER_LENGTH)
            .position(|w| w == HEADER_DELIMITER)
            .expect("Header delimeter is always expected in a message raw_bytes as DapMessage was originally constructed from it");

        let json_part = &raw_bytes[new_line_index + HEADER_DELIMITER_LENGTH..];
        let mut parsed_json: serde_json::Value = serde_json::from_slice(json_part)
            .expect("a DapMessage's body is expected to parse as it was already parsed before");

        parsed_json["seq"] = serde_json::Value::from(Self::get_next_seq());

        // If provided, and if message is a Response, recieved request_sec will override the original one.
        if let Some(request_seq) = request_seq
            && let DapMessage::Response { .. } = self
        {
            parsed_json["request_seq"] = serde_json::Value::from(request_seq);
        }

        let body = parsed_json.to_string();

        Self::from_str(&body).expect("Construction of a DapMessage on the same data structure is not expected to fail")
    }

    /// Construct a launch message with the provided list of arguments
    pub fn make_launch_request(arguments: &serde_json::Value) -> Result<DapMessage> {
        let json = serde_json::json!({
            "seq": Self::get_next_seq(),
            "type": "request",
            "command": "launch",
            "arguments": arguments
        });

        let body = json.to_string();

        Self::from_str(&body)
    }

    /// Construct a "continue" request, continue explicitly is intended to resume (continue) execution on all threads.
    pub fn make_continue_request() -> Result<DapMessage> {
        let json = serde_json::json!({
            "seq": Self::get_next_seq(),
            "type": "request",
            "command": "continue",
            "arguments": {
                "threadId": 0,
                // Explicit to make sure we resume all threads.
                // The reason this is important is the use case - we only build a "continue" message when IDE disconnects and we want to make sure debugging is paused.
                // We release all breakpoints and then send this request, which will resume execution on all threads if any was paused.
                "singleThread": false,
            }
        });

        let body = json.to_string();

        Self::from_str(&body)
    }

    /// Construct a response without a body for the provided request_sec, this is only a good fit for "fake" responses when there is no "body" required.
    pub fn make_acknowledgement_response(request_seq: u64) -> Result<DapMessage> {
        let json = serde_json::json!({
            "seq": Self::get_next_seq(),
            "request_seq": request_seq,
            "type": "response",
        });

        let body = json.to_string();

        Self::from_str(&body)
    }

    /// Construct an output message
    pub fn make_output_event(output: &str) -> Result<DapMessage> {
        let json = serde_json::json!({
            "seq": Self::get_next_seq(),
            "type": "event",
            "event": "output",
            "body": {
                "output": output,
            }
        });

        let body = json.to_string();

        Self::from_str(&body)
    }

    /// Construct a launch message (DapMessage::Request with RequestCommandTypes::Launch).
    pub fn clear_breakpoints(command: RequestCommandTypes) -> Result<DapMessage> {
        let command_name = match command {
            RequestCommandTypes::SetBreakpoints(_) => "setBreakpoints",
            RequestCommandTypes::SetExceptionBreakpoints => "setExceptionBreakpoints",
            RequestCommandTypes::SetFunctionBreakpoints => "setFunctionBreakpoints",
            _ => bail!("Only breakpoints related commands are expected"),
        };

        let arguments = match command {
            RequestCommandTypes::SetBreakpoints(path) => {
                serde_json::json!({ "breakpoints": [], "source": { "path": path } })
            }
            RequestCommandTypes::SetFunctionBreakpoints | RequestCommandTypes::SetExceptionBreakpoints => {
                serde_json::json!({ "breakpoints": [] })
            }
            _ => bail!("Only breakpoints related commands are expected"),
        };

        let json = serde_json::json!({
            "seq": Self::get_next_seq(),
            "type": "request",
            "command": command_name,
            "arguments": arguments,
        });

        let body = json.to_string();

        Self::from_str(&body)
    }

    /// Takes a complete body and parse it as a DapMessage.
    ///
    /// - `body`: JSON payload only (no headers).
    /// - `full_message`: Complete DAP message including headers, stored on the resulting [`DapMessage`] for forwarding between streams
    ///
    /// Body is assuemd to be a valid JSON buffer, if JSON parsing failed or DapMessage could not be constructed, and error would be returned instead.
    pub fn parse_body(body: &[u8], full_message: &[u8]) -> Result<DapMessage> {
        log!(
            LogSource::Proxy,
            LogLevel::Debug,
            "DAP message to parse: {:?}",
            str::from_utf8(body)
        );

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
            "response" => {
                let request_seq = parsed_json["request_seq"]
                    .as_u64()
                    .context("Could not find request sequence id in resposne message")?;

                Ok(DapMessage::Response {
                    seq,
                    request_seq,
                    raw_bytes: Vec::from(full_message),
                })
            }
            "request" => {
                let command_type_str = parsed_json["command"]
                    .as_str()
                    .context("Could not parse request: a Request should have a command attached to it as a string")?;

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
                    "launch" => RequestCommandTypes::Launch,
                    "attach" => RequestCommandTypes::Attach(parsed_json["arguments"].clone()),
                    "configurationDone" => RequestCommandTypes::ConfigurationDone,
                    "disconnect" => RequestCommandTypes::Disconnect,
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
}

#[cfg(test)]
mod tests {
    use crate::dap_message::{DapMessage, EventTypes};

    // Tests for DapMessage::parse_body code paths that are not covered by dap_stream's tests suite.
    #[test]
    fn test_parse_body_invalid_json_returns_error() {
        let buffer = r#"not valid json at all"#.as_bytes();
        let parsed = DapMessage::parse_body(buffer, buffer);

        assert!(parsed.is_err());
    }

    #[test]
    fn test_parse_body_missing_type_returns_error() {
        let buffer = r#"{"seq":1,"command":"initialize"}"#.as_bytes();
        let parsed = DapMessage::parse_body(buffer, buffer);

        assert!(parsed.is_err());
    }

    #[test]
    fn test_parse_body_request_missing_command_returns_error() {
        let buffer = r#"{"seq":1,"type":"request"}"#.as_bytes();
        let parsed = DapMessage::parse_body(buffer, buffer);

        assert!(parsed.is_err());
    }

    #[test]
    fn test_parse_body_set_breakpoints_missing_source_path_returns_error() {
        let buffer = r#"{"seq":1,"type":"request","command":"setBreakpoints","arguments":{}}"#.as_bytes();
        let parsed = DapMessage::parse_body(buffer, buffer);

        assert!(parsed.is_err());
    }

    #[test]
    fn test_parse_output_event_stdout() {
        let buffer =
            r#"{"seq":1,"type":"event","event":"output","body":{"category":"stdout","output":"hello world\n"}}"#
                .as_bytes();
        let parsed = DapMessage::parse_body(buffer, buffer);

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
        let parsed = DapMessage::parse_body(buffer, buffer);

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
        let parsed = DapMessage::parse_body(buffer, buffer);

        assert!(parsed.is_err());
    }

    #[test]
    fn test_parse_output_event_missing_output_returns_error() {
        let buffer = r#"{"seq":1,"type":"event","event":"output","body":{"category":"stdout"}}"#.as_bytes();
        let parsed = DapMessage::parse_body(buffer, buffer);

        assert!(parsed.is_err());
    }
}
