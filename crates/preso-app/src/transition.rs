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
//!   slide. `cover` maps here too.
//! - `ContentWipe` wipes only what's on the slide: the background, accent bars
//!   and logo stay put. No screenshot here — both slides' content layers are
//!   rendered live, each clipped to its side of the edge (see `clip.rs`).
//! - `Pan` moves the camera along a strip of slides: the outgoing content
//!   slides off one side as the incoming slides in from the other, over a
//!   design that holds still. Also rendered live — a renderer translation
//!   *can* move live content, which a screenshot-era transition couldn't.
//!   `slide`/`push` and Slidev's `slide-left`/… names map here.
//!
//! A transition belongs to the boundary between two slides: moving on plays
//! the incoming slide's, going back plays the outgoing slide's [reversed]
//! (see `Kind::reversed`), so a pan going back retraces its path.

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
    /// A wipe of the slide content alone, over a design that holds still.
    /// Where the design itself changes between the two slides (a title slide,
    /// a background image) there's nothing to hold, so the whole slide wipes.
    ContentWipe,
    /// A camera pan along the deck: the content slides out one side and the
    /// next slide's slides in from the other, over a design that holds still
    /// (or, where the design changes, with the whole slide). The direction is
    /// the way the content moves.
    Pan(Direction),
}

/// Every name `transition:` accepts. One table, so the language server's list
/// (`preso_core::lint::TRANSITIONS`) can be checked against it exactly.
const NAMES: &[(&str, Kind)] = &[
    ("none", Kind::None),
    // `fade` kept as the long-standing alias for the dissolve.
    ("fade", Kind::Dissolve),
    ("dissolve", Kind::Dissolve),
    ("wipe", Kind::Wipe),
    ("cover", Kind::Wipe),
    ("wipe-content", Kind::ContentWipe),
    // Motion names are pans; Slidev's directional `slide-*` keep their
    // direction.
    ("pan", Kind::Pan(Direction::Left)),
    ("pan-left", Kind::Pan(Direction::Left)),
    ("pan-right", Kind::Pan(Direction::Right)),
    ("pan-up", Kind::Pan(Direction::Up)),
    ("pan-down", Kind::Pan(Direction::Down)),
    ("slide", Kind::Pan(Direction::Left)),
    ("slide-left", Kind::Pan(Direction::Left)),
    ("slide-right", Kind::Pan(Direction::Right)),
    ("slide-up", Kind::Pan(Direction::Up)),
    ("slide-down", Kind::Pan(Direction::Down)),
    ("push", Kind::Pan(Direction::Left)),
    ("push-left", Kind::Pan(Direction::Left)),
    ("push-right", Kind::Pan(Direction::Right)),
    ("push-up", Kind::Pan(Direction::Up)),
    ("push-down", Kind::Pan(Direction::Down)),
];

/// Which way a pan moves the content (Slidev's convention): `Left` slides
/// the outgoing content off to the left and brings the incoming in from the
/// right — the camera moving right along the deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    pub fn reversed(self) -> Self {
        match self {
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
        }
    }

    /// The way the content moves, as a unit (x, y) — y downwards.
    pub fn vector(self) -> (f32, f32) {
        match self {
            Direction::Left => (-1.0, 0.0),
            Direction::Right => (1.0, 0.0),
            Direction::Up => (0.0, -1.0),
            Direction::Down => (0.0, 1.0),
        }
    }
}

impl Kind {
    pub fn from_frontmatter(value: Option<&str>) -> Self {
        let value = value.map(str::trim);
        NAMES
            .iter()
            .find(|(name, _)| Some(*name) == value)
            .map_or(Self::None, |(_, kind)| *kind)
    }

    /// How long this transition runs. The wipe is slower than the dissolve: a
    /// moving hard edge reads as jittery if it's too quick, whereas an opacity
    /// blend is fine fast. A pan moves the whole slide's worth of content, so
    /// it gets longer still to read as a glide rather than a jump.
    pub fn duration(self) -> Duration {
        match self {
            Kind::None => Duration::ZERO,
            Kind::Dissolve => Duration::from_millis(300),
            Kind::Wipe | Kind::ContentWipe => Duration::from_millis(480),
            Kind::Pan(_) => Duration::from_millis(600),
        }
    }

    /// This transition played backwards, for going back across the boundary
    /// it belongs to: a pan retraces its path; the rest look the same either
    /// way.
    pub fn reversed(self) -> Self {
        match self {
            Kind::Pan(direction) => Kind::Pan(direction.reversed()),
            other => other,
        }
    }

    /// Whether this transition animates a screenshot of the outgoing slide,
    /// which the app then has to capture ahead of time.
    pub fn needs_frame(self) -> bool {
        matches!(self, Kind::Dissolve | Kind::Wipe)
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
        assert_eq!(Kind::from_frontmatter(Some("cover")), Kind::Wipe);
        assert_eq!(
            Kind::from_frontmatter(Some("wipe-content")),
            Kind::ContentWipe
        );
        let pan = |d| Kind::Pan(d);
        for name in ["pan", "pan-left", "slide", "slide-left", "push"] {
            assert_eq!(
                Kind::from_frontmatter(Some(name)),
                pan(Direction::Left),
                "{name}"
            );
        }
        assert_eq!(
            Kind::from_frontmatter(Some("pan-right")),
            pan(Direction::Right)
        );
        assert_eq!(Kind::from_frontmatter(Some("slide-up")), pan(Direction::Up));
        assert_eq!(
            Kind::from_frontmatter(Some("pan-down")),
            pan(Direction::Down)
        );
        assert_eq!(Kind::from_frontmatter(Some("none")), Kind::None);
        assert_eq!(Kind::from_frontmatter(None), Kind::None);
    }

    #[test]
    fn going_back_reverses_a_pan() {
        assert_eq!(
            Kind::Pan(Direction::Left).reversed(),
            Kind::Pan(Direction::Right)
        );
        assert_eq!(
            Kind::Pan(Direction::Up).reversed(),
            Kind::Pan(Direction::Down)
        );
        // Undirected transitions are their own reverse.
        assert_eq!(Kind::Dissolve.reversed(), Kind::Dissolve);
        for d in [
            Direction::Left,
            Direction::Right,
            Direction::Up,
            Direction::Down,
        ] {
            let (x, y) = d.vector();
            assert_eq!(d.reversed().vector(), (-x, -y));
        }
    }

    #[test]
    fn language_server_knows_every_transition() {
        // preso-lsp warns on a `transition=` outside `lint::TRANSITIONS`, so
        // it must list exactly the names this module accepts — no more (a
        // name it offers would silently cut) and no fewer (a working name
        // would be flagged).
        let listed: std::collections::BTreeSet<&str> =
            preso_core::lint::TRANSITIONS.iter().copied().collect();
        let accepted: std::collections::BTreeSet<&str> =
            NAMES.iter().map(|(name, _)| *name).collect();
        assert_eq!(listed, accepted);
        for name in preso_core::lint::TRANSITIONS {
            let animates = Kind::from_frontmatter(Some(name)) != Kind::None;
            assert_eq!(animates, *name != "none", "{name}");
        }
    }

    #[test]
    fn only_screenshot_transitions_need_a_frame() {
        assert!(Kind::Dissolve.needs_frame());
        assert!(Kind::Wipe.needs_frame());
        assert!(!Kind::ContentWipe.needs_frame());
        assert!(!Kind::Pan(Direction::Up).needs_frame());
        assert!(!Kind::None.needs_frame());
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
