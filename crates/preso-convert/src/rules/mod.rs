//! The conversion pipeline: a list of [`Rule`]s, each handling one concern
//! (a frontmatter key, a body construct). To support a new Slidev feature —
//! or a new preso feature it can map onto — add a `Rule` and register it in
//! [`default_rules`]. Rules run in order and share a [`SlideCtx`].

use serde_norway::{Mapping, Value};

mod cleanup;
mod clicks;
mod code;
mod columns;
mod frontmatter;
mod images;
mod notes;

pub use frontmatter::DECK_KEYS;

/// Per-slide conversion state, threaded through every [`Rule`].
pub struct SlideCtx {
    /// 0-based slide index (slide 0's frontmatter is also deck headmatter).
    pub index: usize,
    /// Frontmatter keys not yet consumed. A rule removes the keys it handles;
    /// whatever remains is reported by [`frontmatter::LeftoverFrontmatter`].
    pub front: Mapping,
    /// `key=value` pairs collapsed into one `<!-- slide: … -->` directive.
    pub overrides: Vec<(String, String)>,
    /// Content of a `<!-- layout: … -->` directive, if any.
    pub layout: Option<String>,
    /// The markdown body (rewritten in place by body rules).
    pub body: String,
    /// Extracted speaker notes (each emitted as `<!-- note: … -->`).
    pub notes: Vec<String>,
    /// Human-readable notices about anything that couldn't be converted.
    pub warnings: Vec<String>,
}

impl SlideCtx {
    /// Record a non-fatal conversion notice, tagged with the slide number.
    pub fn warn(&mut self, msg: impl Into<String>) {
        self.warnings
            .push(format!("slide {}: {}", self.index + 1, msg.into()));
    }

    /// Add a `key=value` pair to this slide's `<!-- slide: … -->` directive.
    pub fn set_override(&mut self, key: &str, value: &str) {
        // Last write wins, so a later rule can refine an earlier guess.
        self.overrides.retain(|(k, _)| k != key);
        self.overrides.push((key.to_string(), value.to_string()));
    }

    /// Take a frontmatter value by key, removing it from `front`.
    pub fn take(&mut self, key: &str) -> Option<Value> {
        self.front.remove(key)
    }
}

/// One conversion step.
pub trait Rule {
    fn apply(&self, ctx: &mut SlideCtx);
}

/// The default pipeline, in execution order. Frontmatter rules emit
/// directives; body rules rewrite the markdown; the leftover rule warns last.
pub fn default_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(notes::Notes),
        Box::new(frontmatter::Layout),
        Box::new(frontmatter::Class),
        Box::new(frontmatter::Background),
        Box::new(frontmatter::Transition),
        Box::new(clicks::Clicks),
        Box::new(columns::Columns),
        Box::new(code::Code),
        Box::new(images::Images),
        Box::new(cleanup::Cleanup),
        Box::new(frontmatter::LeftoverFrontmatter),
    ]
}

/// Pull a frontmatter value as a string (scalars only).
pub(crate) fn as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}
