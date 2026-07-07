use std::collections::HashMap;

use anyhow::Result;

use crate::dap_message::{
    DapMessage::{self},
    RequestCommandTypes,
};

#[derive(Default, Debug)]
pub struct ProxyState {
    initialize: Option<DapMessage>,

    /// On RuntimeMode::Headless, the "attach" request from the IDE, will be convert to a launch
    /// request and will be stored here, on subsequent rebuilds, only the launch will be replayed.
    launch: Option<DapMessage>,
    configuration_done: Option<DapMessage>,
    function_breakpoints: Option<DapMessage>,
    exception_breakpoints: Option<DapMessage>,
    breakpoints: HashMap<String, DapMessage>, // Hashed by file path
    last_replayed_seq: Option<u64>,

    /// Captured initialize response, faked back to IDE on headless reconnect.
    initialize_response: Option<DapMessage>,
}

impl ProxyState {
    /// Captures relevant IDE messages into debug state for replay.
    /// Returns the message back if state was affected, `None` otherwise.
    pub fn capture(&mut self, message: &DapMessage) -> Result<Option<DapMessage>> {
        match message {
            DapMessage::Request { command, .. } => {
                let stored_message = message.clone();

                match &command {
                    RequestCommandTypes::Initialize => self.initialize = Some(stored_message),
                    RequestCommandTypes::Launch => self.launch = Some(stored_message),

                    // When recieveing an "attach" request from the IDE, it means we're in ::Headless mode.
                    // Which means that only the very-first "attach" request is sent as it-is to the adapter.
                    // But in cases of rebuild, we're transforming that attach into a "launch" request, since
                    // there is no process to attach to, and we want to make sure that the program starts after
                    // breakpoints and the rest of the state is set, this way, no code execution will be missed.
                    RequestCommandTypes::Attach(arguments) => {
                        // TODO: Write a unit test for attach -> launch convertion
                        self.launch = Some(DapMessage::make_launch_request(&arguments));
                    }
                    RequestCommandTypes::ConfigurationDone => self.configuration_done = Some(stored_message),
                    RequestCommandTypes::SetExceptionBreakpoints => self.exception_breakpoints = Some(stored_message),
                    RequestCommandTypes::SetFunctionBreakpoints => self.function_breakpoints = Some(stored_message),
                    RequestCommandTypes::SetBreakpoints(file_path) => {
                        self.breakpoints.insert(String::from(file_path), stored_message);
                    }
                    RequestCommandTypes::Disconnect | RequestCommandTypes::PassForward(_) => {}
                }

                if !matches!(command, RequestCommandTypes::PassForward(_)) {
                    return Ok(Some(message.clone()));
                }
            }
            DapMessage::Response { request_seq, .. } => {
                // We're only storing the response that matches the initialize request.
                // This response will be used to properly allow re-connection from IDE on live debugging session (in headless mode).
                if let Some(initialize) = &self.initialize
                    && initialize.seq() == *request_seq
                {
                    self.initialize_response = Some(message.clone());
                    return Ok(Some(message.clone()));
                }
            }
            DapMessage::Event { .. } => {}
        }

        Ok(None)
    }

    /// Get breakpoints file list
    pub fn get_breakpoints_file_paths(&self) -> Vec<&String> {
        self.breakpoints.keys().collect()
    }

    /// Captured initialize response, faked back to IDE on headless reconnect
    pub fn get_initialize_response(&self) -> Option<&DapMessage> {
        self.initialize_response.as_ref()
    }

    /// Bring state back to its default values
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Produces a sequence of DapMessage to be sent to the adapter when its respawned, based on the proxy state.
    pub fn get_replay_sequence(&mut self) -> Vec<&DapMessage> {
        let mut replay_sequence = vec![
            self.initialize.as_ref(),
            self.launch.as_ref(),
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
        let launch = make_request(3, RequestCommandTypes::Launch);
        let exception_breakpoints = make_request(4, RequestCommandTypes::SetExceptionBreakpoints);
        let function_breakpoints = make_request(5, RequestCommandTypes::SetFunctionBreakpoints);
        let breakpoints = make_request(6, RequestCommandTypes::SetBreakpoints(String::from("testfile.rs")));
        let configuration_done = make_request(7, RequestCommandTypes::ConfigurationDone);

        state.capture(&initialize).unwrap();
        state.capture(&launch).unwrap();
        state.capture(&exception_breakpoints).unwrap();
        state.capture(&function_breakpoints).unwrap();
        state.capture(&breakpoints).unwrap();
        state.capture(&configuration_done).unwrap();

        assert_eq!(state.initialize, Some(initialize));
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

        let file_a_breakpoints = make_request(1, RequestCommandTypes::SetBreakpoints(String::from("file_a.rs")));

        let file_b_breakpoints = make_request(2, RequestCommandTypes::SetBreakpoints(String::from("file_b.rs")));

        state.capture(&file_a_breakpoints).unwrap();
        state.capture(&file_b_breakpoints).unwrap();

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

        assert!(state.capture(&message).unwrap().is_none());
    }

    #[test]
    fn test_capture_response_returns_none() {
        let mut state = ProxyState::default();
        let message = DapMessage::Response {
            seq: 1,
            request_seq: 2,
            raw_bytes: vec![],
        };

        assert!(state.capture(&message).unwrap().is_none());
    }

    #[test]
    fn test_capture_event_returns_none() {
        let mut state = ProxyState::default();
        let message = DapMessage::Event {
            seq: 1,
            raw_bytes: vec![],
            event: EventTypes::Output(String::from("Test Output")),
        };

        assert!(state.capture(&message).unwrap().is_none());
    }

    // get_replay_sequence tests
    #[test]
    fn test_replay_sequence_order_initialize_first_config_done_last() {
        let mut state = ProxyState::default();

        let initialize = make_request(1, RequestCommandTypes::Initialize);
        let breakpoints_a = make_request(2, RequestCommandTypes::SetBreakpoints(String::from("testfile_a.rs")));
        let configuration_done = make_request(3, RequestCommandTypes::ConfigurationDone);
        let breakpoints_b = make_request(4, RequestCommandTypes::SetBreakpoints(String::from("testfile_b.rs")));

        state.capture(&initialize).unwrap();
        state.capture(&breakpoints_a).unwrap();
        state.capture(&configuration_done).unwrap();
        state.capture(&breakpoints_b).unwrap();

        let sequence = state.get_replay_sequence();

        assert_eq!(sequence.first(), Some(&&initialize));
        assert_eq!(sequence.last(), Some(&&configuration_done));
    }

    #[test]
    fn test_replay_sequence_empty_state_returns_empty() {
        let mut state = ProxyState::default();
        let sequence = state.get_replay_sequence();

        assert_eq!(sequence.len(), 0);
    }

    #[test]
    fn test_replay_sequence_full_correct() {
        let mut state = ProxyState::default();

        let initialize = make_request(1, RequestCommandTypes::Initialize);
        let launch = make_request(2, RequestCommandTypes::Launch);
        let exception_breakpoints = make_request(3, RequestCommandTypes::SetExceptionBreakpoints);
        let function_breakpoints = make_request(4, RequestCommandTypes::SetFunctionBreakpoints);
        let breakpoints_a = make_request(5, RequestCommandTypes::SetBreakpoints(String::from("testfile_a.rs")));
        let configuration_done = make_request(6, RequestCommandTypes::ConfigurationDone);
        let breakpoints_b = make_request(7, RequestCommandTypes::SetBreakpoints(String::from("testfile_b.rs")));

        state.capture(&initialize).unwrap();
        state.capture(&launch).unwrap();
        state.capture(&exception_breakpoints).unwrap();
        state.capture(&function_breakpoints).unwrap();
        state.capture(&breakpoints_a).unwrap();
        state.capture(&configuration_done).unwrap();
        state.capture(&breakpoints_b).unwrap();

        let sequence = state.get_replay_sequence();

        assert_eq!(sequence.len(), 7);

        // Sequence always starts with initation related messages
        let start = &sequence[0..=1];
        assert_eq!(start[0], &initialize);
        assert_eq!(start[1], &launch); // In real usage, state should never have both launch and attach at the same time, this is why order does not matter here.

        // Mid-section always have breakpoints (order of breakpoints is not important)
        let middle = &sequence[2..=5];
        assert!(middle.contains(&&exception_breakpoints));
        assert!(middle.contains(&&function_breakpoints));
        assert!(middle.contains(&&breakpoints_a));
        assert!(middle.contains(&&breakpoints_b));

        // Confirms that configuration is always last
        assert_eq!(sequence.last(), Some(&&configuration_done));
        assert_eq!(state.last_replayed_seq.unwrap(), configuration_done.seq()); // Regardless of order of capture, if configuration done is present, its seq should be last
    }

    // is_last_replay_response tests
    #[test]
    fn test_is_last_replay_response() {
        let mut state = ProxyState::default();

        let initialize = make_request(1, RequestCommandTypes::Initialize);
        let configuration_done = make_request(3, RequestCommandTypes::ConfigurationDone);

        state.capture(&initialize).unwrap();
        state.capture(&configuration_done).unwrap();

        _ = state.get_replay_sequence();

        // Response with request_seq matching last replay message
        let response = DapMessage::Response {
            seq: 5,
            request_seq: 3,
            raw_bytes: vec![],
        };

        assert!(state.is_last_replay_response(&response));

        // Response not matching
        let response = DapMessage::Response {
            seq: 5,
            request_seq: 2,
            raw_bytes: vec![],
        };

        assert!(!state.is_last_replay_response(&response));

        // Message types is not response
        let another_request = make_request(4, RequestCommandTypes::SetExceptionBreakpoints);
        let some_event = DapMessage::Event {
            seq: 5,
            raw_bytes: vec![],
            event: EventTypes::Output(String::from("Test output")),
        };

        assert!(!state.is_last_replay_response(&another_request));
        assert!(!state.is_last_replay_response(&some_event));
    }

    #[test]
    fn test_is_last_replay_response_no_replay_seq_returns_false() {
        let mut state = ProxyState::default();

        let initialize = make_request(1, RequestCommandTypes::Initialize);
        let configuration_done = make_request(3, RequestCommandTypes::ConfigurationDone);

        state.capture(&initialize).unwrap();
        state.capture(&configuration_done).unwrap();

        let response = DapMessage::Response {
            seq: 5,
            request_seq: 3,
            raw_bytes: vec![],
        };

        // Even though request_seq matches last request (configuration), it should retrun false, since state.get_replay_sequence wasn't called
        assert!(!state.is_last_replay_response(&response));
    }
}
