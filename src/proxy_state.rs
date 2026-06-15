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

        // Storing the last sequence id, this is used to identify the moment all replay responses have completed
        self.last_replayed_seq = replay_sequence.last().map(|m| m.seq());

        return replay_sequence;
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
