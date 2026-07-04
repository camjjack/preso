//! File watching: notify-debouncer-full on a dedicated thread, bridged into
//! an iced subscription stream.
//!
//! The *parent directory* is watched rather than the file itself: editors that
//! save atomically (write temp file, then rename over the original) would
//! otherwise detach the watch on the first save. It's watched recursively and
//! a reload fires on the deck file or any `.md` under it, so editing an
//! `<!-- include: … -->` chapter file reloads the deck too.

use crate::app::Message;
use iced::Subscription;
use notify_debouncer_full::notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(300);

/// Watch `dir` (recursively) and emit [`Message::FileChanged`] when any deck
/// or theme file under it changes. Used for both the deck's directory and the
/// theme file's directory.
pub fn subscription(dir: &Path) -> Subscription<Message> {
    Subscription::run_with(dir.to_path_buf(), |dir| {
        let dir = dir.clone();
        iced::stream::channel(16, async move |tx| {
            watch_thread(dir, tx);
            // Keep the stream alive; items arrive from the watcher thread.
            std::future::pending::<()>().await;
        })
    })
    .map(|()| Message::FileChanged)
}

fn watch_thread(dir: PathBuf, tx: futures_channel_sender::Sender) {
    std::thread::Builder::new()
        .name("preso-file-watch".into())
        .spawn(move || {
            let dir = if dir.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                dir
            };

            let (raw_tx, raw_rx) = std::sync::mpsc::channel();
            let mut debouncer =
                match new_debouncer(DEBOUNCE, None, move |result: DebounceEventResult| {
                    let _ = raw_tx.send(result);
                }) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::error!(error = %e, "hot reload disabled: cannot create watcher");
                        return;
                    }
                };
            // Recursive so chapter files pulled in with `<!-- include: … -->`
            // (including those in subdirectories) are watched too.
            if let Err(e) = debouncer.watch(&dir, RecursiveMode::Recursive) {
                tracing::error!(error = %e, dir = %dir.display(), "hot reload disabled");
                return;
            }
            tracing::debug!(dir = %dir.display(), "watching for changes");

            let mut tx = tx;
            while let Ok(result) = raw_rx.recv() {
                match result {
                    Ok(events) => {
                        // Reload on any markdown (deck or included chapter) or
                        // TOML (theme) change under the watched directory.
                        let ours = events.iter().any(|e| {
                            e.event.paths.iter().any(|p| {
                                matches!(
                                    p.extension().and_then(std::ffi::OsStr::to_str),
                                    Some("md" | "toml")
                                )
                            })
                        });
                        if ours && tx.try_send(()).is_err() {
                            tracing::debug!("subscription channel closed; stopping watcher");
                            return;
                        }
                    }
                    Err(errors) => {
                        for e in errors {
                            tracing::warn!(error = %e, "file watch error");
                        }
                    }
                }
            }
        })
        .expect("spawn watcher thread");
}

/// Local alias module so the thread function signature stays readable.
mod futures_channel_sender {
    pub type Sender = iced::futures::channel::mpsc::Sender<()>;
}
