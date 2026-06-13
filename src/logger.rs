use colored::Colorize;
use std::fmt::{Arguments, Display};

pub enum Direction {
    In,
    Out,
}

pub enum LogSource {
    Proxy,
    Watcher,
    Program,
    Adapter(Direction),
    Ide(Direction),
}

impl LogSource {
    /// This function can be (and should be) used through the log! macro exported from this module.
    /// The enum allow for a clearer logging that distinguish the differnet possible sources, this helps debugging the proxy easier.
    /// log! macro expands that by allowing inline formatting of the log message as well.
    pub fn log(&self, message: Arguments) {
        let tag = match self {
            LogSource::Proxy => "[Proxy]".yellow(),
            LogSource::Watcher => "[Watcher]".green(),
            LogSource::Program => "[Program]".cyan(),
            LogSource::Adapter(Direction::In) => "[Adapter -> Proxy]".blue(),
            LogSource::Adapter(Direction::Out) => "[Proxy -> Adapter]".bright_blue(),
            LogSource::Ide(Direction::In) => "[IDE -> Proxy]".magenta(),
            LogSource::Ide(Direction::Out) => "[Proxy -> IDE]".bright_magenta(),
        };

        println!("{}: {}", tag, message);
    }
}

/// Calls the .log function on the passed $source (expected to be a LogSource variant).
/// Example usage: ``log!(LogSource::Proxy, "Lisening on port :{}", port)``.
#[macro_export]
macro_rules! log {
    ($source: expr, $($arg:tt)*) => {
        $source.log(format_args!($($arg)*))
    };
}
