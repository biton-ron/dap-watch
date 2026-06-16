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

        let mut file_watcher = FileWatcher {
            config: config.clone(),
            receiver,
            watcher,
        };

        // TODO: Take watch patterns from configuration
        // TODO: Optionally respect .gitignore
        file_watcher
            .watcher
            .watch(Path::new("./src/bin"), RecursiveMode::Recursive)
            .context("Failed to watch ./src/bin directory")?;

        Ok(file_watcher)
    }

    pub async fn next(&mut self) -> Result<()> {
        // TODO: Add debounce logic, extract event information for logging
        let _ = self.receiver.recv().await;

        Ok(())
    }
}
