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

/// A playback time as the scrub bar shows it: `m:ss`, or `h:mm:ss` from an
/// hour up. Whole seconds, rounded down, so the clock never runs ahead.
pub fn clock(time: std::time::Duration) -> String {
    let secs = time.as_secs();
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Where stepping `frames` frames (negative: back) from `position` lands, at
/// `fps` frames a second — clamped to the clip. A stream that reports no
/// frame rate steps as if it were 30 fps.
#[cfg_attr(not(feature = "video"), allow(dead_code))]
pub fn step_target(
    position: std::time::Duration,
    frames: i32,
    fps: f64,
    duration: std::time::Duration,
) -> std::time::Duration {
    let fps = if fps.is_finite() && fps > 0.0 {
        fps
    } else {
        30.0
    };
    let target = position.as_secs_f64() + f64::from(frames) / fps;
    std::time::Duration::from_secs_f64(target.clamp(0.0, duration.as_secs_f64()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn clock_reads_like_a_player() {
        assert_eq!(clock(Duration::ZERO), "0:00");
        assert_eq!(clock(Duration::from_millis(12_900)), "0:12");
        assert_eq!(clock(Duration::from_secs(64)), "1:04");
        assert_eq!(clock(Duration::from_secs(3_725)), "1:02:05");
    }

    #[test]
    fn steps_are_a_frame_long_and_stay_in_the_clip() {
        let at = |ms| Duration::from_millis(ms);
        let len = at(10_000);
        assert_eq!(step_target(at(1_000), 1, 25.0, len), at(1_040));
        assert_eq!(step_target(at(1_000), -5, 25.0, len), at(800));
        // Clamped at both ends.
        assert_eq!(step_target(at(20), -5, 25.0, len), Duration::ZERO);
        assert_eq!(step_target(at(9_990), 5, 25.0, len), len);
        // No frame rate reported: 30 fps.
        let step = step_target(Duration::ZERO, 3, 0.0, len);
        assert!((step.as_secs_f64() - 0.1).abs() < 1e-9, "{step:?}");
        assert_eq!(
            step_target(at(500), 1, f64::NAN, len),
            step_target(at(500), 1, 30.0, len)
        );
    }
}

#[cfg(feature = "video")]
mod embed {
    use iced_video_player::Video;
    use std::path::Path;

    /// A GStreamer-backed video clip, for inline playback under the wgpu
    /// backend. Owns the pipeline; dropping it tears the pipeline down. The
    /// app keys these by deck-relative path in a map.
    pub struct Embedded {
        video: Video,
        /// Played at least once. Until then the slide shows its ▶ poster;
        /// after, a pause freezes on the current frame instead.
        started: bool,
        /// Whether it's been told to play. GStreamer changes state
        /// asynchronously, so its own reading lags a toggle (and a load) for a
        /// moment; this doesn't.
        playing: bool,
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
            Ok(Self {
                video,
                started: false,
                playing: false,
            })
        }

        /// The underlying player, for the `VideoPlayer` widget.
        pub fn video(&self) -> &Video {
            &self.video
        }

        /// Toggle play/pause.
        pub fn toggle_pause(&mut self) {
            let play = !self.is_playing();
            self.video.set_paused(!play);
            self.playing = play;
            self.started |= play;
        }

        /// Whether it's playing: told to, and not stopped at its end.
        pub fn is_playing(&self) -> bool {
            self.playing && !self.video.eos()
        }

        /// Whether the clip has been played at least once, so its frame (not
        /// the poster) belongs on the slide even while it's paused.
        pub fn started(&self) -> bool {
            self.started
        }

        /// Pause the clip if it's playing (idempotent). Used when its slide
        /// leaves the screen so it stops without being torn down.
        pub fn pause(&mut self) {
            if self.playing {
                self.video.set_paused(true);
                self.playing = false;
            }
        }

        /// How far into the clip playback is.
        pub fn position(&self) -> std::time::Duration {
            self.video.position()
        }

        /// The clip's length (zero until GStreamer knows it).
        pub fn duration(&self) -> std::time::Duration {
            self.video.duration()
        }

        /// Jump to `to` (clamped to the clip). `accurate` lands on the exact
        /// frame; otherwise the nearest keyframe, which keeps up with a drag.
        /// It shows that frame even paused, so it counts as starting the clip.
        /// A clip that played to its end is un-ended first, so pressing play
        /// afterwards carries on from `to` instead of starting over.
        pub fn seek_to(&mut self, to: std::time::Duration, accurate: bool) {
            let to = to.min(self.video.duration());
            if self.video.eos() {
                let _ = self.video.restart_stream();
                self.video.set_paused(true);
                self.playing = false;
            }
            let _ = self.video.seek(to, accurate);
            self.started = true;
        }

        /// Pause and step `frames` frames forward (negative: back).
        pub fn step(&mut self, frames: i32) {
            self.pause();
            let to = super::step_target(
                self.video.position(),
                frames,
                self.video.framerate(),
                self.video.duration(),
            );
            self.seek_to(to, true);
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

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A one-second, 10 fps test clip of its own for test `name`, made
        /// with ffmpeg; `None` (and the test skips) where ffmpeg isn't
        /// installed.
        fn clip(name: &str) -> Option<std::path::PathBuf> {
            let path =
                std::env::temp_dir().join(format!("preso-clip-{name}-{}.mp4", std::process::id()));
            let made = std::process::Command::new("ffmpeg")
                .args(["-y", "-loglevel", "error", "-f", "lavfi"])
                .args(["-i", "testsrc=duration=1:size=64x48:rate=10"])
                .args(["-pix_fmt", "yuv420p"])
                .arg(&path)
                .status()
                .is_ok_and(|s| s.success());
            made.then_some(path)
        }

        #[test]
        fn a_clip_stays_started_once_played() {
            let Some(path) = clip("started") else {
                eprintln!("skipping: ffmpeg isn't installed");
                return;
            };
            let mut clip = Embedded::load(&path).expect("clip loads");
            // GStreamer changes state asynchronously; a presenter's key
            // presses are far apart, so let each change land.
            let settle = |clip: &Embedded, paused: bool| {
                for _ in 0..100 {
                    if clip.video().paused() == paused {
                        return true;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                false
            };
            // Loaded paused, never played: the poster's turn — and that's
            // known at once, whatever GStreamer is still settling.
            assert!(!clip.is_playing() && !clip.started());
            assert!(settle(&clip, true));
            clip.toggle_pause();
            assert!(clip.is_playing() && clip.started());
            assert!(settle(&clip, false));
            // Paused again, it's still started, so its frame stays up.
            clip.toggle_pause();
            assert!(!clip.is_playing() && clip.started());
            assert!(settle(&clip, true));
            // Leaving the slide pauses it; that doesn't un-start it either.
            clip.toggle_pause();
            clip.pause();
            assert!(!clip.is_playing() && clip.started());
            assert!(settle(&clip, true));
            std::fs::remove_file(path).ok();
        }

        /// Wait (briefly) for the paused pipeline to settle on `want`.
        fn settles_at(clip: &Embedded, want: std::time::Duration) -> bool {
            let tolerance = std::time::Duration::from_millis(60);
            for _ in 0..100 {
                if clip.position().abs_diff(want) <= tolerance {
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            false
        }

        #[test]
        fn seeking_and_stepping_move_a_paused_clip() {
            let Some(path) = clip("seek") else {
                eprintln!("skipping: ffmpeg isn't installed");
                return;
            };
            let mut clip = Embedded::load(&path).expect("clip loads");
            assert!(clip.duration() > std::time::Duration::from_millis(900));
            // A seek before ever playing shows that frame: the clip's started.
            let half = std::time::Duration::from_millis(500);
            clip.seek_to(half, true);
            assert!(clip.started() && !clip.is_playing());
            assert!(settles_at(&clip, half), "at {:?}", clip.position());
            // One frame on at 10 fps is 100 ms; two back is 300 ms.
            clip.step(1);
            assert!(
                settles_at(&clip, std::time::Duration::from_millis(600)),
                "at {:?}",
                clip.position()
            );
            clip.step(-3);
            assert!(
                settles_at(&clip, std::time::Duration::from_millis(300)),
                "at {:?}",
                clip.position()
            );
            assert!(!clip.is_playing(), "stepping leaves it paused");
            // Past the end clamps to it.
            clip.seek_to(std::time::Duration::from_secs(60), true);
            assert!(clip.position() <= clip.duration());
            std::fs::remove_file(path).ok();
        }
    }
}
