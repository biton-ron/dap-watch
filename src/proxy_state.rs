use std::collections::HashMap;

use crate::dap_message::{
    DapMessage::{self, Request},
    RequestCommandTypes,
};

#[derive(Default)]
pub struct ProxyState {
    // Command specific state
    initialize: Option<DapMessage>,
    launch: Option<DapMessage>,
    attach: Option<DapMessage>,
    configuration_done: Option<DapMessage>,
    function_breakpoints: Option<DapMessage>,
    exception_breakpoints: Option<DapMessage>,
    breakpoints: HashMap<String, DapMessage>, // Hashed by file path

    // Logical state
    last_replayed_seq: Option<u64>,
}

impl ProxyState {
    /// Captures relevant IDE messages into debug state for replay.
    /// Returns the command type if state was updated, `None` otherwise.
    pub fn capture_state(&mut self, message: &DapMessage) -> Option<RequestCommandTypes> {
        // Only requests have some effect on the state
        if let Request { command, .. } = message {
            let message = message.clone();

            match command {
                RequestCommandTypes::Attach => self.attach = Some(message),
                RequestCommandTypes::Initialize => self.initialize = Some(message),
                RequestCommandTypes::Launch => self.launch = Some(message),
                RequestCommandTypes::ConfigurationDone => self.configuration_done = Some(message),
                RequestCommandTypes::SetExceptionBreakpoints => {
                    self.exception_breakpoints = Some(message)
                }
                RequestCommandTypes::SetFunctionBreakpoints => {
                    self.function_breakpoints = Some(message)
                }
                RequestCommandTypes::SetBreakpoints(file_path) => {
                    self.breakpoints.insert(String::from(file_path), message);
                }
                RequestCommandTypes::PassForward(_) => {}
            }

            if !matches!(command, RequestCommandTypes::PassForward(_)) {
                return Some(command.clone());
            }
        }

        None
    }

    /// Produces a sequence of DapMessage to be sent to the adapter when its respawned, based on the proxy state.
    pub fn get_replay_sequence(&mut self) -> Vec<&DapMessage> {
        let mut replay_sequence = vec![
            self.initialize.as_ref(),
            self.launch.as_ref(),
            self.attach.as_ref(),
            self.exception_breakpoints.as_ref(),
            self.function_breakpoints.as_ref(),
        ];

        for (.., message) in &self.breakpoints {
            replay_sequence.push(Some(message));
        }

        replay_sequence.push(self.configuration_done.as_ref());

        // Stripping None values from the replay_sequence, leaving only populated state messages
        let replay_sequence: Vec<&DapMessage> = replay_sequence.into_iter().flatten().collect();

        // Storing the last sequence id, this is used to identify the moment all replay responses have arrived back from the adapter
        self.last_replayed_seq = replay_sequence.last().map(|m| m.seq());

        replay_sequence
    }

    /// Checks if the message is a DapMessage::Response that matches the seq id from the last message in the replay sequence
    pub fn is_last_replay_response(&self, message: &DapMessage) -> bool {
        if let DapMessage::Response { request_seq, .. } = message {
            return self
                .last_replayed_seq
                .as_ref()
                .is_some_and(|last_replay_seq| request_seq == last_replay_seq);
        }

        false
    }
}

#[cfg(test)]
mod test {
    use std::vec;

    use crate::{
        dap_message::{
            DapMessage, EventTypes,
            RequestCommandTypes::{self, PassForward},
        },
        proxy_state::ProxyState,
    };

    fn make_request(seq: u64, command: RequestCommandTypes) -> DapMessage {
        DapMessage::Request {
            seq,
            raw_bytes: vec![],
            command,
        }
    }

    // capture_state tests
    #[test]
    fn test_capture_stores_each_command_in_correct_field() {
        let mut state = ProxyState::default();

        let initialize = make_request(1, RequestCommandTypes::Initialize);
        let attach = make_request(2, RequestCommandTypes::Attach);
        let launch = make_request(3, RequestCommandTypes::Launch);
        let configuration_done = make_request(5, RequestCommandTypes::ConfigurationDone);
        let exception_breakpoints = make_request(6, RequestCommandTypes::SetExceptionBreakpoints);
        let function_breakpoints = make_request(7, RequestCommandTypes::SetFunctionBreakpoints);
        let breakpoints = make_request(
            8,
            RequestCommandTypes::SetBreakpoints(String::from("testfile.rs")),
        );

        state.capture_state(&initialize);
        state.capture_state(&attach);
        state.capture_state(&launch);
        state.capture_state(&configuration_done);
        state.capture_state(&exception_breakpoints);
        state.capture_state(&function_breakpoints);
        state.capture_state(&breakpoints);

        assert_eq!(state.initialize, Some(initialize));
        assert_eq!(state.attach, Some(attach));
        assert_eq!(state.launch, Some(launch));
        assert_eq!(state.configuration_done, Some(configuration_done));
        assert_eq!(state.exception_breakpoints, Some(exception_breakpoints));
        assert_eq!(state.function_breakpoints, Some(function_breakpoints));

        assert!(
            state
                .breakpoints
                .get("testfile.rs")
                .is_some_and(|message| message == &breakpoints)
        );
    }

    #[test]
    fn test_capture_set_breakpoints_different_files_coexist() {
        let mut state = ProxyState::default();

        let file_a_breakpoints = make_request(
            1,
            RequestCommandTypes::SetBreakpoints(String::from("file_a.rs")),
        );

        let file_b_breakpoints = make_request(
            2,
            RequestCommandTypes::SetBreakpoints(String::from("file_b.rs")),
        );

        state.capture_state(&file_a_breakpoints);
        state.capture_state(&file_b_breakpoints);

        assert!(
            state
                .breakpoints
                .get("file_a.rs")
                .is_some_and(|message| message == &file_a_breakpoints)
        );

        assert!(
            state
                .breakpoints
                .get("file_b.rs")
                .is_some_and(|message| message == &file_b_breakpoints)
        );
    }

    #[test]
    fn test_capture_pass_forward_returns_none() {
        let mut state = ProxyState::default();
        let message = make_request(1, PassForward(String::from("Evaluate")));

        assert_eq!(state.capture_state(&message), None);
    }

    #[test]
    fn test_capture_response_returns_none() {
        let mut state = ProxyState::default();
        let message = DapMessage::Response {
            seq: 1,
            request_seq: 2,
            raw_bytes: vec![],
        };

        assert_eq!(state.capture_state(&message), None);
    }

    #[test]
    fn test_capture_event_returns_none() {
        let mut state = ProxyState::default();
        let message = DapMessage::Event {
            seq: 1,
            raw_bytes: vec![],
            event: EventTypes::Output(String::from("Test Output")),
        };

        assert_eq!(state.capture_state(&message), None);
    }

    // get_replay_sequence tests
    #[test]
    fn test_replay_sequence_order_initialize_first_config_done_last() {}

    #[test]
    fn test_replay_sequence_empty_state_returns_empty() {}

    #[test]
    fn test_replay_sequence_skips_none_fields() {}

    #[test]
    fn test_replay_sequence_includes_breakpoints() {}

    #[test]
    fn test_replay_sequence_sets_last_replayed_seq() {}

    // is_last_replay_response tests
    #[test]
    fn test_is_last_replay_response_matching_seq_returns_true() {}

    #[test]
    fn test_is_last_replay_response_non_matching_seq_returns_false() {}

    #[test]
    fn test_is_last_replay_response_request_message_returns_false() {}

    #[test]
    fn test_is_last_replay_response_event_message_returns_false() {}

    #[test]
    fn test_is_last_replay_response_no_replay_seq_returns_false() {}
}
