//! Slide transitions (Phase 4).
//!
//! iced 0.14 has no general per-widget opacity or transforms, so a transition
//! can't fade or move live slide content. The trick: capture the outgoing
//! slide as a bitmap with `window::screenshot` (works on both the wgpu and
//! tiny-skia backends), then animate that **image** over the live incoming
//! slide — `image` widgets *do* support opacity and can be clipped. The app
//! owns the capture/animation state; this module is the pure timing + kind
//! logic (instants injected per AGENTS.md).
//!
//! - `Dissolve` cross-fades the outgoing image's opacity 1→0 over the incoming
//!   slide (a true cross-dissolve, unlike the old background-coloured veil).
//! - `Wipe` clips the outgoing image away from one edge to reveal the incoming
//!   slide. Motion-style names (`slide`/`push`/`cover`) map here too — true
//!   translation needs transforms the renderer doesn't have.

use std::time::{Duration, Instant};

/// Which transition the deck requested (`transition:` frontmatter or a
/// per-slide `<!-- slide: transition=… -->` override).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    None,
    /// Cross-dissolve: outgoing image fades out over the live incoming slide.
    Dissolve,
    /// Directional reveal: outgoing image is clipped away from the left edge.
    Wipe,
}

impl Kind {
    pub fn from_frontmatter(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            // `fade` kept as the long-standing alias for the dissolve.
            Some("fade") | Some("dissolve") => Self::Dissolve,
            // Motion names collapse to a wipe: iced 0.14 can't translate live
            // content, so a directional reveal is the closest honest effect.
            Some("wipe") | Some("slide") | Some("push") | Some("cover") => Self::Wipe,
            _ => Self::None,
        }
    }

    /// How long this transition runs. The wipe is slower than the dissolve: a
    /// moving hard edge reads as jittery if it's too quick, whereas an opacity
    /// blend is fine fast.
    pub fn duration(self) -> Duration {
        match self {
            Kind::None => Duration::ZERO,
            Kind::Dissolve => Duration::from_millis(300),
            Kind::Wipe => Duration::from_millis(480),
        }
    }
}

/// Eased 0→1 progress for a transition started at `started`, or `None` once it
/// has finished. Smoothstep (ease-in-out): gentle at both ends so a wipe edge
/// or a dissolve doesn't lurch on the first frame.
pub fn progress(started: Instant, now: Instant, duration: Duration) -> Option<f32> {
    let linear =
        now.saturating_duration_since(started).as_secs_f32() / duration.as_secs_f32().max(1e-6);
    if linear >= 1.0 {
        return None;
    }
    Some(linear * linear * (3.0 - 2.0 * linear))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_parses_frontmatter() {
        assert_eq!(Kind::from_frontmatter(Some("fade")), Kind::Dissolve);
        assert_eq!(Kind::from_frontmatter(Some("dissolve")), Kind::Dissolve);
        assert_eq!(Kind::from_frontmatter(Some("wipe")), Kind::Wipe);
        assert_eq!(Kind::from_frontmatter(Some("slide")), Kind::Wipe);
        assert_eq!(Kind::from_frontmatter(Some("none")), Kind::None);
        assert_eq!(Kind::from_frontmatter(None), Kind::None);
    }

    #[test]
    fn progress_runs_zero_to_one_then_ends() {
        let t0 = Instant::now();
        let d = Duration::from_millis(200);

        let start = progress(t0, t0, d).unwrap();
        assert!(start.abs() < 1e-6, "starts at 0");

        let mid = progress(t0, t0 + Duration::from_millis(100), d).unwrap();
        assert!(mid > 0.0 && mid < 1.0);

        assert_eq!(progress(t0, t0 + Duration::from_millis(200), d), None);
        assert_eq!(progress(t0, t0 + Duration::from_millis(999), d), None);
    }

    #[test]
    fn progress_monotonically_increases() {
        let t0 = Instant::now();
        let d = Duration::from_millis(220);
        let mut last = -0.1_f32;
        for ms in (0..220).step_by(20) {
            let p = progress(t0, t0 + Duration::from_millis(ms), d).unwrap();
            assert!(p > last, "progress not increasing at {ms}ms");
            last = p;
        }
    }
}
