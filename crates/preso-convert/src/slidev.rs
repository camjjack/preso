//! A minimal Slidev deck parser.
//!
//! Slidev decks are markdown with `---` separators. Every slide may carry a
//! leading YAML *frontmatter* block (delimited by `---`); the first slide's
//! frontmatter doubles as deck-level *headmatter* (theme, title, …). We split
//! the document on `---` lines (outside code fences) into chunks, then pair a
//! chunk that parses as a YAML mapping with the body chunk that follows it.
//!
//! This handles well-formed decks. The one known ambiguity: a body that is a
//! single `key: value` line parses as a mapping and would be mistaken for
//! frontmatter — rare enough to accept, and surfaced as a conversion warning
//! downstream if a slide ends up with no body.

use serde_norway::{Mapping, Value};

/// A parsed Slidev deck.
pub struct SlidevDeck {
    /// Deck-level config from the first slide's frontmatter (theme, title, …).
    /// The same keys also remain on the first slide's frontmatter.
    pub headmatter: Mapping,
    pub slides: Vec<SlidevSlide>,
}

/// One Slidev slide: optional frontmatter plus its markdown body.
pub struct SlidevSlide {
    pub frontmatter: Mapping,
    pub body: String,
}

/// Split `source` into chunks on `---` lines that sit outside code fences.
/// The text before the first separator is the first (possibly empty) chunk.
fn split_on_separators(source: &str) -> Vec<String> {
    let mut chunks = vec![String::new()];
    let mut in_fence = false;
    let mut fence_marker: Option<&str> = None;
    for line in source.lines() {
        let trimmed = line.trim();
        // Track fenced code so a `---` inside it is literal.
        if let Some(marker) = fence_marker {
            if trimmed.starts_with(marker) {
                in_fence = false;
                fence_marker = None;
            }
        } else if trimmed.starts_with("```") {
            in_fence = true;
            fence_marker = Some("```");
        } else if trimmed.starts_with("~~~") {
            in_fence = true;
            fence_marker = Some("~~~");
        }

        if !in_fence && is_separator(trimmed) {
            chunks.push(String::new());
            continue;
        }
        let last = chunks.last_mut().expect("seeded with one chunk");
        last.push_str(line);
        last.push('\n');
    }
    chunks
}

/// A Slidev separator/frontmatter delimiter: a line of three or more dashes.
fn is_separator(trimmed: &str) -> bool {
    trimmed.len() >= 3 && trimmed.chars().all(|c| c == '-')
}

/// `true` if `chunk` parses as a non-empty YAML mapping — our test for "this
/// chunk is frontmatter, not body". Markdown bodies (`# heading`, `- list`,
/// prose) parse as comments/sequences/scalars, not mappings.
fn as_frontmatter(chunk: &str) -> Option<Mapping> {
    if chunk.trim().is_empty() {
        return None;
    }
    match serde_norway::from_str::<Value>(chunk) {
        Ok(Value::Mapping(m)) if !m.is_empty() => Some(m),
        _ => None,
    }
}

/// Parse a Slidev deck.
pub fn parse(source: &str) -> SlidevDeck {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let chunks = split_on_separators(source);

    // Drop the leading empty chunk produced when the file opens with `---`.
    let mut chunks: &[String] = &chunks;
    if chunks.first().is_some_and(|c| c.trim().is_empty()) {
        chunks = &chunks[1..];
    }

    let mut slides = Vec::new();
    let mut i = 0;
    while i < chunks.len() {
        let (frontmatter, body) = match as_frontmatter(&chunks[i]) {
            // A trailing frontmatter chunk with no following body is treated
            // as a body instead (degenerate deck); otherwise pair them.
            Some(fm) if i + 1 < chunks.len() => {
                i += 1;
                (fm, chunks[i].clone())
            }
            _ => (Mapping::new(), chunks[i].clone()),
        };
        // Skip wholly empty trailing slides (e.g. a final separator).
        if frontmatter.is_empty() && body.trim().is_empty() {
            i += 1;
            continue;
        }
        slides.push(SlidevSlide { frontmatter, body });
        i += 1;
    }

    let headmatter = slides
        .first()
        .map(|s| s.frontmatter.clone())
        .unwrap_or_default();
    SlidevDeck { headmatter, slides }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headmatter_and_per_slide_frontmatter() {
        let src = "\
---
theme: seriph
title: My Talk
---

# Slide One

---
layout: center
---

# Slide Two

---

# Slide Three
";
        let deck = parse(src);
        assert_eq!(
            deck.headmatter.get("theme").and_then(Value::as_str),
            Some("seriph")
        );
        assert_eq!(deck.slides.len(), 3);
        assert!(deck.slides[0].body.contains("# Slide One"));
        assert_eq!(
            deck.slides[1]
                .frontmatter
                .get("layout")
                .and_then(Value::as_str),
            Some("center")
        );
        assert!(deck.slides[1].body.contains("# Slide Two"));
        assert!(deck.slides[2].frontmatter.is_empty());
        assert!(deck.slides[2].body.contains("# Slide Three"));
    }

    #[test]
    fn no_headmatter() {
        let deck = parse("# Just a slide\n\nsome text\n");
        assert!(deck.headmatter.is_empty());
        assert_eq!(deck.slides.len(), 1);
    }

    #[test]
    fn separator_inside_code_fence_is_literal() {
        let src = "\
# Code

```yaml
foo: bar
---
baz: qux
```
";
        let deck = parse(src);
        assert_eq!(deck.slides.len(), 1);
        assert!(deck.slides[0].body.contains("baz: qux"));
    }
}
