use colored::Colorize;
use std::{
    fmt::Arguments,
    sync::atomic::{AtomicU8, Ordering},
};

pub enum LogSource {
    Proxy,
    Watcher,
    Program,
    Adapter,
    Ide,
}

#[repr(u8)]
pub enum LogLevel {
    Default = 0,
    Verbose = 1,
    Debug = 2,
}

static LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Default as u8);

impl LogSource {
    /// This function can be (and should be) used through the log! macro exported from this module.
    /// The enum allow for a clearer logging that distinguish the differnet possible sources, this helps debugging the proxy easier.
    /// log! macro expands that by allowing inline formatting of the log message as well.
    pub fn log(&self, level: LogLevel, new_line: bool, message: Arguments) {
        if level as u8 <= LOG_LEVEL.load(Ordering::Relaxed) {
            let tag = match self {
                LogSource::Proxy => "Proxy".yellow(),
                LogSource::Watcher => "Watcher".green(),
                LogSource::Program => "Program".cyan(),
                LogSource::Adapter => "Adapter -> IDE".bright_blue(),
                LogSource::Ide => "IDE -> Adapter".bright_magenta(),
            };

            if new_line {
                println!("{:<15} | {}", tag, message);
            } else {
                print!("{:<15} | {}", tag, message);
            }
        }
    }
}

pub fn set_log_level(level: LogLevel) {
    LOG_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Calls the .log function on the passed $source (expected to be a LogSource variant).
/// Example usage: ``log!(LogSource::Proxy, "Lisening on port :{}", port)``.
#[macro_export]
macro_rules! log {
    // level + explicit newline turned off
    ($source:expr, $level:path, false, $($arg:tt)*) => {
        $source.log($level, false, format_args!($($arg)*))
    };

    // level, default newline = true
    ($source:expr, $level:path, $($arg:tt)*) => {
        $source.log($level, true, format_args!($($arg)*))
    };

    // no level, default level + newline
    ($source:expr, $($arg:tt)*) => {
        $source.log($crate::logger::LogLevel::Default, true, format_args!($($arg)*))
    };
}
