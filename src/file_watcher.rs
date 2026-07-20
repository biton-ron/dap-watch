use std::env::current_dir;
use std::path::Path;
use std::time::Duration;

use crate::config::WatcherConfig;
use anyhow::Context;
use anyhow::Result;
use globset::Glob;
use globset::GlobSet;
use globset::GlobSetBuilder;
use notify::Event;
use notify::FsEventWatcher;
use notify::RecursiveMode;
use notify::Watcher;
use tokio::select;
use tokio::sync::mpsc::Receiver;
use tokio::time::sleep;

pub struct FileWatcher {
    config: WatcherConfig,
    receiver: Receiver<Event>,
    watcher: FsEventWatcher,
    includes_matcher: GlobSet,
    excludes_matcher: GlobSet,
}

impl FileWatcher {
    pub fn new(config: &WatcherConfig) -> Result<FileWatcher> {
        let (sender, receiver) = tokio::sync::mpsc::channel::<Event>(100);

        // Establishing a new wathcer with notify, sending events to to the mpsc channel created above
        let watcher = notify::recommended_watcher(move |event| {
            if let Ok(e) = event {
                // This does not block the main thread, only notify's thread.
                // Our tokio reciever remains async and is processing these events without blocking.
                let _ = sender.blocking_send(e);
            }
        })
        .context("notify could not establish a watcher")?;

        // Building matchers based on include and exclude patterns (glob patterns).
        // These matchers can then be used to evaluate if a file should trigger a notification or not.
        let (includes_matcher, excludes_matcher) =
            Self::build_matchers(&config).context("Could not build matchers for the provided watch config")?;

        Ok(Self {
            config: config.clone(),
            receiver,
            watcher,
            excludes_matcher,
            includes_matcher,
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

    /// Builds two GlobSet from config, (includes, excludes) - each can be matched against
    /// an exact file name to check if matches the patterns added to them.
    fn build_matchers(config: &WatcherConfig) -> Result<(GlobSet, GlobSet)> {
        let mut includes = GlobSetBuilder::new();
        let mut excludes = GlobSetBuilder::new();

        for pattern in &config.ignore_paths {
            let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
            excludes.add(Glob::new(&pattern)?);
        }

        for pattern in &config.paths {
            let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
            includes.add(Glob::new(&pattern)?);
        }

        Ok((
            includes.build().context("Could not build a set for included files")?,
            excludes.build().context("Could not build a set for excluded files")?,
        ))
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

    /// Waits for an event of files that are trackable, trackable means that they're part of `config.paths`
    /// but are not part of `config.ignore_paths` (and `.gitignore` when `config.gitignore=true`).
    pub async fn next(&mut self) {
        loop {
            let event = self.receiver.recv().await;

            if let Some(event) = event {
                if self.is_event_matches(event) {
                    // A matching file has changed, now we debounce before returning to avoid events noise
                    loop {
                        select! {
                            _ = sleep(Duration::from_millis(self.config.debounce_ms)) => {
                                return;
                            },
                            _ = self.receiver.recv() => {},
                        }
                    }
                }
            }
        }
    }

    /// Tests event's paths against includes and excludes matchers
    fn is_event_matches(&self, event: Event) -> bool {
        let cwd = current_dir().expect("Could not evaluate current working directory");

        event.paths.iter().any(|f| {
            let relative_path = f.strip_prefix(&cwd).unwrap_or(f);
            return self.includes_matcher.is_match(relative_path) && !self.excludes_matcher.is_match(relative_path);
        })
    }
}
