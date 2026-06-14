/// We only specify in this enum commands are required for state preservation.
/// As an example - setBreakpoints is a crucial part of the state, and will be replayed to debuggers when re-spawned.
/// Everything else falls into the PassForward() category which will be sent directly by the proxy without thouching state at all.
#[derive(Debug, PartialEq)]
pub enum RequestCommandTypes {
    Initialize,
    Attach,
    Launch,
    SetBreakpoints(String), // String is the file_path
    SetDataBreakpoints,
    SetExecutionBreakpoints,
    SetFunctionBreakpoints,
    SetInstructionBreakpoints,
    PassForward, // Fallback for all the rest
}

#[derive(Debug, PartialEq)]
pub enum EventTypes {
    Output(String),
    Other,
}

#[derive(Debug, PartialEq)]
pub enum DapMessage {
    Request {
        raw_bytes: Vec<u8>,
        command: RequestCommandTypes,
    },
    Event {
        raw_bytes: Vec<u8>,
        event: EventTypes,
    },
    Response(Vec<u8>),
}
