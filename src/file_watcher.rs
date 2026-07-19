use std::path::Path;

use crate::config::WatcherConfig;
use anyhow::Context;
use anyhow::Result;
use notify::Event;
use notify::FsEventWatcher;
use notify::RecursiveMode;
use notify::Watcher;
use tokio::sync::mpsc::Receiver;

pub struct FileWatcher {
    config: WatcherConfig,
    receiver: Receiver<Event>,
    watcher: FsEventWatcher,
}

impl FileWatcher {
    pub fn new(config: &WatcherConfig) -> Result<FileWatcher> {
        let (sender, receiver) = tokio::sync::mpsc::channel::<Event>(100);

        let watcher = notify::recommended_watcher(move |event| {
            if let Ok(e) = event {
                // This does not block the main thread, only notify's thread.
                // Our tokio reciever remains async and is processing these events without blocking.
                let _ = sender.blocking_send(e);
            }
        })
        .context("notify could not establish a watcher")?;

        Ok(FileWatcher {
            config: config.clone(),
            receiver,
            watcher,
        })
    }

    /// Iterates through the configured paths and register them for file change events through the watcher
    pub fn watch(&mut self) -> Result<()> {
        for path in self.config.paths.iter() {
            self.watcher
                .watch(Self::get_path_from_pattern(&path), RecursiveMode::Recursive)
                .with_context(|| format!("Failed to watch path: {}", path))?;
        }

        Ok(())
    }

    /// Extracts the base directory from a glob pattern by splitting at the first wildcard.
    /// For example: `src/**/*.rs` → `src/`, `*.rs` → `` (current directory), `some_file.rs` -> `some_file.rs` (exact match, no pattern).
    fn get_path_from_pattern(pattern: &str) -> &Path {
        let base_path = pattern
            .split("*")
            .next()
            .expect("Any string must have at least one part when splitting");

        Path::new(base_path)
    }

    pub async fn next(&mut self) -> Result<()> {
        // TODO: Add debounce logic, extract event information for logging
        let _ = self.receiver.recv().await;

        Ok(())
    }
}
