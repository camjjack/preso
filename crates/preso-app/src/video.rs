//! Playing video clips referenced by `<!-- video: … -->`.
//!
//! Two paths:
//!
//! - **Embedded** ([`Embedded`], the `video` cargo feature): the clip plays
//!   inline on the slide via `iced_video_player` (GStreamer → wgpu texture).
//!   Requires the wgpu backend, so the app only uses it when wgpu is live.
//! - **External** ([`play`], always available): the `V` key hands the clip to
//!   an external player — `mpv --fullscreen` when on `PATH` (ideal for the
//!   audience monitor), else the platform's default opener. The fallback when
//!   the `video` feature is off or the software backend is in use.

use std::path::Path;
use std::process::Command;

/// Launch an external player for `path`. Prefers a fullscreen `mpv`; falls
/// back to the platform's default opener. Returns an error only if the file is
/// missing or no launcher could be spawned.
pub fn play(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
    }
    // mpv opens borderless fullscreen, which is what we want on the audience
    // display. If it isn't installed the spawn fails fast and we fall back to
    // the OS default app (windowed, on whatever monitor it prefers).
    if Command::new("mpv")
        .arg("--fullscreen")
        .arg(path)
        .spawn()
        .is_ok()
    {
        return Ok(());
    }
    open_default(path)
}

#[cfg(target_os = "macos")]
fn open_default(path: &Path) -> std::io::Result<()> {
    Command::new("open").arg(path).spawn().map(|_| ())
}

#[cfg(target_os = "windows")]
fn open_default(path: &Path) -> std::io::Result<()> {
    // `start` is a cmd builtin, not an executable; the empty "" is the window
    // title that `start` expects before the path.
    Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_default(path: &Path) -> std::io::Result<()> {
    Command::new("xdg-open").arg(path).spawn().map(|_| ())
}

#[cfg(feature = "video")]
pub use embed::Embedded;

#[cfg(feature = "video")]
mod embed {
    use iced_video_player::Video;
    use std::path::Path;

    /// A GStreamer-backed video clip, for inline playback under the wgpu
    /// backend. Owns the pipeline; dropping it tears the pipeline down. The
    /// app keys these by deck-relative path in a map.
    pub struct Embedded {
        video: Video,
    }

    impl Embedded {
        /// Load (and preroll) the clip at `path`. Starts **paused** — playback
        /// begins when the presenter presses `V` — so entering a video slide
        /// doesn't blast audio unprompted. This blocks on the GStreamer preroll
        /// (why the app calls it up front, off the presentation path).
        pub fn load(path: &Path) -> Result<Self, String> {
            // `Url::from_file_path` demands an absolute path; the deck path can
            // be relative (e.g. `preso deck.md`), so resolve it first. This
            // also surfaces a missing file as a clear error.
            let absolute = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
            let url = url::Url::from_file_path(&absolute)
                .map_err(|()| format!("invalid video path: {}", absolute.display()))?;
            let mut video = Video::new(&url).map_err(|e| e.to_string())?;
            video.set_paused(true);
            Ok(Self { video })
        }

        /// The underlying player, for the `VideoPlayer` widget.
        pub fn video(&self) -> &Video {
            &self.video
        }

        /// Toggle play/pause.
        pub fn toggle_pause(&mut self) {
            self.video.set_paused(!self.video.paused());
        }

        /// Pause the clip if it's playing (idempotent). Used when its slide
        /// leaves the screen so it stops without being torn down.
        pub fn pause(&mut self) {
            if !self.video.paused() {
                self.video.set_paused(true);
            }
        }

        /// Seek backwards: `to_start` jumps to the beginning (full rewind),
        /// otherwise scrubs back a fixed step, clamped at zero. A failed seek
        /// (e.g. a non-seekable source) is ignored — nothing to do about it.
        pub fn seek_back(&mut self, to_start: bool) {
            const SCRUB: std::time::Duration = std::time::Duration::from_secs(5);
            let target = if to_start {
                std::time::Duration::ZERO
            } else {
                self.video.position().saturating_sub(SCRUB)
            };
            let _ = self.video.seek(target, false);
        }
    }
}
