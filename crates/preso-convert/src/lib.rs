//! Convert a Slidev deck or PowerPoint file into preso markdown.
//!
//! [`convert`] parses a Slidev source, maps deck headmatter to preso
//! frontmatter, then runs each slide through the [`rules`] pipeline and
//! renders the result. [`convert_pptx`] reads a `.pptx` directly. Anything
//! that can't be represented in preso is reported as a warning rather than
//! failing the conversion.

mod pptx;
mod rules;
mod slidev;

pub use pptx::convert as convert_pptx;

use rules::{DECK_KEYS, SlideCtx, as_string, default_rules};
use serde_norway::{Mapping, Value};

/// The result of a conversion: the preso markdown plus any non-fatal notices.
pub struct Conversion {
    pub output: String,
    pub warnings: Vec<String>,
    /// Extracted binary assets to write alongside the output, as
    /// `(path relative to the output file, bytes)`. Empty for Slidev (its
    /// images are referenced in place, not extracted); the PowerPoint
    /// importer fills it with images pulled out of the `.pptx`.
    pub media: Vec<(String, Vec<u8>)>,
}

/// Convert Slidev `source` to preso markdown.
pub fn convert(source: &str) -> Conversion {
    let deck = slidev::parse(source);
    let rules = default_rules();
    let mut warnings = Vec::new();

    let mut out = String::new();
    let frontmatter = deck_frontmatter(&deck.headmatter, &mut warnings);
    if !frontmatter.is_empty() {
        out.push_str("---\n");
        out.push_str(&frontmatter);
        out.push_str("---\n\n");
    }

    for (i, slide) in deck.slides.iter().enumerate() {
        let mut front = slide.frontmatter.clone();
        // Slide 0's frontmatter doubles as deck headmatter; those keys are
        // emitted as deck frontmatter above, so don't reprocess them per-slide.
        if i == 0 {
            for key in DECK_KEYS {
                front.remove(*key);
            }
        }
        let mut ctx = SlideCtx {
            index: i,
            front,
            overrides: Vec::new(),
            layout: None,
            body: slide.body.trim().to_string(),
            notes: Vec::new(),
            warnings: Vec::new(),
        };
        for rule in &rules {
            rule.apply(&mut ctx);
        }
        warnings.append(&mut ctx.warnings);

        if i > 0 {
            out.push_str("\n---\n\n");
        }
        out.push_str(&render_slide(&ctx));
    }

    Conversion {
        output: out.trim_end().to_string() + "\n",
        warnings,
        media: Vec::new(),
    }
}

/// Render one converted slide: directives, body, then notes.
fn render_slide(ctx: &SlideCtx) -> String {
    let mut s = String::new();
    if let Some(layout) = &ctx.layout {
        s.push_str(&format!("<!-- layout: {layout} -->\n"));
    }
    if !ctx.overrides.is_empty() {
        let parts: Vec<String> = ctx
            .overrides
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        s.push_str(&format!("<!-- slide: {} -->\n", parts.join(" ")));
    }
    let has_directives = ctx.layout.is_some() || !ctx.overrides.is_empty();
    if has_directives && !ctx.body.is_empty() {
        s.push('\n');
    }
    s.push_str(ctx.body.trim_end());
    if !ctx.body.is_empty() {
        s.push('\n');
    }
    for note in &ctx.notes {
        s.push_str(&format!("\n<!-- note: {note} -->\n"));
    }
    s
}

/// Map Slidev deck headmatter to preso frontmatter (returns the YAML body,
/// without the `---` fences).
fn deck_frontmatter(headmatter: &Mapping, warnings: &mut Vec<String>) -> String {
    let mut lines = Vec::new();

    if let Some(title) = headmatter.get("title").and_then(as_string) {
        lines.push(format!("title: {}", yaml_scalar(&title)));
    }

    if let Some(theme) = headmatter.get("theme").and_then(as_string) {
        if theme == "dark" || theme == "light" {
            lines.push(format!("theme: {theme}"));
        } else {
            warnings.push(format!(
                "deck: Slidev theme '{theme}' has no preso equivalent; using the default theme \
                 (set `theme:` to a preso theme name or .toml path)"
            ));
        }
    }

    if let Some(transition) = headmatter.get("transition").and_then(as_string) {
        // Map Slidev's transition names onto preso's set (fade | wipe | none).
        // Slidev's directional `slide-*` become a wipe (preso has no transforms
        // for true sliding); anything without an equivalent falls back to fade.
        let preso = match transition.as_str() {
            "none" => "none",
            "fade" | "fade-out" => "fade",
            t if t.starts_with("slide") => "slide", // preso renders as a wipe
            _ => "fade",
        };
        lines.push(format!("transition: {preso}"));
        if preso == "fade" && !matches!(transition.as_str(), "fade" | "fade-out") {
            warnings.push(format!(
                "deck: transition '{transition}' has no preso equivalent; using fade"
            ));
        }
    }

    if let Some(aspect) = headmatter.get("aspectRatio") {
        match aspect_to_preso(aspect) {
            Some(a) => lines.push(format!("aspect: {a:?}")),
            None => warnings.push("deck: could not map `aspectRatio`; dropped".to_string()),
        }
    }

    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    }
}

/// Slidev `aspectRatio` (`16/9`, `"16/9"`, or a float) → preso `"16:9"`.
fn aspect_to_preso(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => s
            .split_once('/')
            .map(|(w, h)| format!("{}:{}", w.trim(), h.trim())),
        Value::Number(_) => None, // a bare float (e.g. 1.7778) has no clean w:h
        _ => None,
    }
}

/// Quote a YAML scalar if it contains characters that would break a bare
/// `key: value` line.
fn yaml_scalar(s: &str) -> String {
    let needs_quotes = s.is_empty()
        || s.starts_with([
            ' ', '#', '"', '\'', '-', '[', '{', '*', '&', '!', '|', '>', '@', '`',
        ])
        || s.contains(": ")
        || s.ends_with(':')
        || s.trim() != s;
    if needs_quotes {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_to_end_basic_deck() {
        let src = "\
---
theme: seriph
title: My Talk
---

# My Talk

---
layout: section
---

# A Section

---
layout: two-cols
---

left content

::right::

right content

<!-- speaker reminder -->
";
        let result = convert(src);
        let out = &result.output;
        // Deck frontmatter (title kept, npm theme warned + dropped).
        assert!(out.contains("title: My Talk"));
        assert!(!out.contains("theme: seriph"));
        assert!(result.warnings.iter().any(|w| w.contains("seriph")));
        // Section layout → kind directive.
        assert!(out.contains("<!-- slide: kind=section -->"));
        // Two columns → layout directive + `***` split.
        assert!(out.contains("<!-- layout: TwoColumn -->"));
        assert!(out.contains("***"));
        // Trailing comment → preso note.
        assert!(out.contains("<!-- note: speaker reminder -->"));
        // Standalone `---` lines: 2 frontmatter delimiters + 2 slide separators.
        assert_eq!(out.lines().filter(|l| l.trim() == "---").count(), 4);
    }

    #[test]
    fn center_layout_maps_to_alignment() {
        let src = "---\nlayout: center\n---\n\n# Centered\n";
        let out = convert(src).output;
        assert!(out.contains("align=center"));
        assert!(out.contains("halign=center"));
    }

    #[test]
    fn transition_maps_to_preso_set() {
        // Directional slide-* → wipe, no warning.
        let r = convert("---\ntransition: slide-left\n---\n\n# A\n");
        assert!(r.output.contains("transition: slide"));
        assert!(!r.warnings.iter().any(|w| w.contains("transition")));

        // fade passes through unchanged.
        assert!(
            convert("---\ntransition: fade\n---\n\n# A\n")
                .output
                .contains("transition: fade")
        );

        // An unsupported transition falls back to fade and warns.
        let r = convert("---\ntransition: zoom\n---\n\n# A\n");
        assert!(r.output.contains("transition: fade"));
        assert!(r.warnings.iter().any(|w| w.contains("zoom")));
    }
}
