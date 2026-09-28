//! Where things are in a deck's source: slide and heading line spans, for
//! tools that point back into the file (the language server's outline,
//! folding and code actions) rather than render it.
//!
//! Built on the parser's own splitter ([`parser::segments`]) and slide
//! processor, so a slide here is exactly a slide on screen — including which
//! ones `<!-- slide: hidden -->` drops and how `number=` renumbers the rest.
//!
//! All line numbers are 0-based indices into `source.lines()`.

use crate::fence;
use crate::model::{Layout, Slide, atx_heading};
use crate::parser::{self, NoteOpen, parse_note_open};
use std::ops::Range;

/// A deck file's structure.
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    /// The frontmatter block, delimiters included, when the file has one.
    pub frontmatter: Option<Range<usize>>,
    /// Every slide in the file, hidden ones included, in order.
    pub slides: Vec<SlideSpan>,
    /// Lines holding a `---` slide delimiter.
    pub delimiters: Vec<usize>,
    /// `source.lines().count()`.
    pub line_count: usize,
}

/// One slide's place in the source.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideSpan {
    /// Every line between the delimiters either side (which aren't included),
    /// blank lines and all.
    pub lines: Range<usize>,
    /// First to last non-blank line.
    pub content: Range<usize>,
    /// ATX headings outside code fences and comments, in order.
    pub headings: Vec<Heading>,
    /// The number the slide displays, or `None` for a hidden slide, which
    /// the presentation drops.
    pub number: Option<usize>,
    /// `<!-- slide: kind=… -->`, if any.
    pub kind: Option<String>,
    /// From `<!-- layout: … -->`.
    pub layout: Layout,
    /// The directive line setting the layout, if any.
    pub layout_line: Option<usize>,
}

impl SlideSpan {
    /// The slide's name: its first heading's text.
    pub fn title(&self) -> Option<&str> {
        self.headings
            .first()
            .map(|h| h.text.as_str())
            .filter(|t| !t.is_empty())
    }

    /// The line to point at for the whole slide: its first heading, else
    /// its first content line.
    pub fn anchor(&self) -> usize {
        self.headings.first().map_or(self.content.start, |h| h.line)
    }

    pub fn hidden(&self) -> bool {
        self.number.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub line: usize,
    pub level: u8,
    pub text: String,
}

/// How the slide processor reads a line — which lines are literal code,
/// which sit inside a comment, which are markdown (directives included).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineKind {
    /// Markdown or a single-line directive.
    Text,
    /// A fence marker or a line inside a fenced code block.
    Code,
    /// Inside a `$$ … $$` display-math block, delimiters included.
    Math,
    /// A multi-line `<!-- note: … -->`, from its opening line to its close.
    Note,
}

/// Classify a slide segment's lines the way `parser::process_slide` walks
/// them: each segment starts outside any fence, note or math block, and the
/// checks run in the same order, so a line is only a directive here if it
/// is one there.
pub(crate) fn classify(lines: &[&str]) -> Vec<LineKind> {
    let mut fence = fence::Tracker::default();
    let mut in_note = false;
    let mut in_math = false;
    lines
        .iter()
        .map(|line| {
            if fence.in_fence() {
                fence.process(line);
                return LineKind::Code;
            }
            if in_note {
                in_note = !line.contains("-->");
                return LineKind::Note;
            }
            let trimmed = line.trim();
            if in_math {
                in_math = trimmed != "$$";
                return LineKind::Math;
            }
            if let Some(NoteOpen::Continued(..)) = parse_note_open(trimmed) {
                in_note = true;
                return LineKind::Note;
            }
            if trimmed == "$$" {
                in_math = true;
                return LineKind::Math;
            }
            if trimmed.len() > 4 && trimmed.starts_with("$$") && trimmed.ends_with("$$") {
                return LineKind::Math;
            }
            if fence.process(line) {
                return LineKind::Code;
            }
            LineKind::Text
        })
        .collect()
}

/// A deck's structure: frontmatter, slides and headings.
pub fn outline(source: &str) -> Outline {
    let lines: Vec<&str> = source.lines().collect();
    let (frontmatter, body_start) = match parser::frontmatter_span(source) {
        Some((_, body_start)) => (Some(0..body_start), body_start),
        None => (None, 0),
    };

    let mut spans = Vec::new();
    let mut processed: Vec<Slide> = Vec::new();
    for range in parser::segments(source, body_start) {
        let segment = &lines[range.clone()];
        let Some(first) = segment.iter().position(|l| !l.trim().is_empty()) else {
            continue; // Empty: not a slide.
        };
        let last = segment
            .iter()
            .rposition(|l| !l.trim().is_empty())
            .unwrap_or(first);

        let kinds = classify(segment);
        let mut headings = Vec::new();
        let mut layout_line = None;
        for (i, (line, kind)) in segment.iter().zip(&kinds).enumerate() {
            if *kind != LineKind::Text {
                continue;
            }
            if let Some((level, text)) = atx_heading(line) {
                headings.push(Heading {
                    line: range.start + i,
                    level,
                    text: text.to_string(),
                });
            } else if parser::directive(line.trim(), "layout").is_some() {
                layout_line = Some(range.start + i);
            }
        }

        let mut text = String::new();
        for line in segment {
            text.push_str(line);
            text.push('\n');
        }
        let slide = parser::process_slide(&text, range.start + 1);
        spans.push(SlideSpan {
            lines: range.clone(),
            content: range.start + first..range.start + last + 1,
            headings,
            number: None,
            kind: slide.overrides.kind.clone(),
            layout: slide.layout,
            layout_line,
        });
        processed.push(slide);
    }

    // Number the visible slides as the presentation does (see
    // [`crate::display_number`]): count from 1, restarting at any `number=`.
    let mut counter = 1;
    for (span, slide) in spans.iter_mut().zip(&processed) {
        if slide.overrides.hidden {
            continue;
        }
        counter = slide.overrides.number.unwrap_or(counter);
        span.number = Some(counter);
        counter += 1;
    }

    Outline {
        frontmatter,
        slides: spans,
        delimiters: parser::delimiter_lines(source, body_start),
        line_count: lines.len(),
    }
}

impl Outline {
    /// The slide a cursor on `line` belongs to: the one containing it, or
    /// for a delimiter or blank gap, the slide that follows (the last slide
    /// past the end). `None` inside the frontmatter or in a deck without
    /// slides.
    pub fn slide_at(&self, line: usize) -> Option<usize> {
        if self.frontmatter.as_ref().is_some_and(|f| f.contains(&line)) {
            return None;
        }
        self.slides
            .iter()
            .position(|s| line < s.lines.end)
            .or_else(|| self.slides.len().checked_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECK: &str = "\
---
title: Demo
---

# Intro

Hello

---

<!-- layout: TwoColumn -->
## Left and right ##

left

***

right

---

```md
---
# not a heading
```
";

    #[test]
    fn slides_and_their_spans() {
        let o = outline(DECK);
        assert_eq!(o.frontmatter, Some(0..3));
        assert_eq!(o.delimiters, vec![8, 19]);
        assert_eq!(o.slides.len(), 3);

        let intro = &o.slides[0];
        assert_eq!(intro.lines, 3..8);
        assert_eq!(intro.content, 4..7);
        assert_eq!(intro.title(), Some("Intro"));
        assert_eq!(intro.number, Some(1));

        let cols = &o.slides[1];
        assert_eq!(cols.title(), Some("Left and right"));
        assert_eq!(cols.layout, Layout::TwoColumn { left: 1, right: 1 });
        assert_eq!(cols.layout_line, Some(10));
        assert_eq!(cols.anchor(), 11);
    }

    #[test]
    fn fenced_lines_are_neither_delimiters_nor_headings() {
        let o = outline(DECK);
        let code = &o.slides[2];
        assert_eq!(code.lines, 20..25);
        assert!(code.headings.is_empty());
        assert_eq!(code.title(), None);
        assert_eq!(code.anchor(), 21);
    }

    #[test]
    fn hidden_slides_are_kept_but_unnumbered() {
        let src = "# A\n---\n<!-- slide: hidden -->\n# B\n---\n# C\n";
        let o = outline(src);
        let numbers: Vec<_> = o.slides.iter().map(|s| s.number).collect();
        assert_eq!(numbers, vec![Some(1), None, Some(2)]);
        assert!(o.slides[1].hidden());
    }

    #[test]
    fn numbering_follows_number_resets() {
        let src = "# A\n---\n<!-- slide: number=10 -->\n# B\n---\n# C\n";
        let numbers: Vec<_> = outline(src).slides.iter().map(|s| s.number).collect();
        assert_eq!(numbers, vec![Some(1), Some(10), Some(11)]);
    }

    #[test]
    fn kind_comes_from_the_slide_directive() {
        let o = outline("<!-- slide: kind=section -->\n# Part two\n");
        assert_eq!(o.slides[0].kind.as_deref(), Some("section"));
    }

    #[test]
    fn slide_at_maps_delimiters_and_gaps_to_the_next_slide() {
        let o = outline(DECK);
        assert_eq!(o.slide_at(1), None); // frontmatter
        assert_eq!(o.slide_at(3), Some(0));
        assert_eq!(o.slide_at(8), Some(1)); // the delimiter before slide 2
        assert_eq!(o.slide_at(24), Some(2));
        assert_eq!(o.slide_at(999), Some(2));
    }

    #[test]
    fn consecutive_delimiters_leave_no_empty_slide() {
        let o = outline("# A\n---\n\n---\n# B\n");
        assert_eq!(o.slides.len(), 2);
        assert_eq!(o.slide_at(2), Some(1));
    }

    #[test]
    fn headings_inside_notes_and_math_are_ignored() {
        let src = "# Real\n<!-- note: starts here\n# not a heading\n-->\n$$\n# nope\n$$\n";
        let o = outline(src);
        let titles: Vec<_> = o.slides[0]
            .headings
            .iter()
            .map(|h| h.text.as_str())
            .collect();
        assert_eq!(titles, vec!["Real"]);
    }

    #[test]
    fn empty_and_unclosed_frontmatter_sources() {
        assert!(outline("").slides.is_empty());
        assert_eq!(outline("").slide_at(0), None);
        // An unclosed frontmatter opener is a delimiter, not frontmatter.
        let o = outline("---\n# A\n");
        assert_eq!(o.frontmatter, None);
        assert_eq!(o.slides.len(), 1);
    }

    #[test]
    fn atx_heading_closing_sequences() {
        assert_eq!(atx_heading("## Title ##"), Some((2, "Title")));
        assert_eq!(atx_heading("# C#"), Some((1, "C#")));
        assert_eq!(atx_heading("#"), Some((1, "")));
        assert_eq!(atx_heading("#hashtag"), None);
        assert_eq!(atx_heading("    # code"), None);
    }
}
