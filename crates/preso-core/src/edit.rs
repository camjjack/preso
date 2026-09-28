//! Structural rewrites of a deck's source: insert, duplicate, move and hide
//! slides; split a slide into columns and back; set its kind. The language
//! server offers these as code actions.
//!
//! Every action is a rewrite into syntax the parser already understands —
//! none adds vocabulary — and touches only the lines it must, so the diff an
//! author reviews is exactly the change they asked for. Edits are whole-line
//! replacements against the *original* source: an action's edits never
//! overlap, so an editor can apply them together.

use crate::model::Layout;
use crate::outline::{self, LineKind, Outline, SlideSpan};
use crate::parser;
use std::ops::Range;

/// Replace `lines` (0-based, end-exclusive; empty to insert before
/// `lines.start`) with `text`. `text` is whole lines, each terminated —
/// except when the edit reaches the end of a file that has no final newline,
/// where it isn't either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEdit {
    pub lines: Range<usize>,
    pub text: String,
}

/// An action available at a cursor, with the edits that perform it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub action: Action,
    /// The menu title: [`Action::title`], or a more specific one when the
    /// cursor shapes the edit.
    pub title: &'static str,
    pub edits: Vec<LineEdit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Split the slide into two columns, the right one starting at the
    /// block under the cursor.
    TwoColumns,
    /// Undo a two-column split.
    SingleColumn,
    TitleSlide,
    SectionSlide,
    /// Clear `kind=`.
    NormalSlide,
    InsertSlideAfter,
    DuplicateSlide,
    MoveSlideUp,
    MoveSlideDown,
    HideSlide,
    UnhideSlide,
    /// Insert an image as its own paragraph (see [`insert_media`]).
    InsertImage,
    /// Make an image the slide's full-bleed background.
    BackgroundImage,
    /// Set the slide's video clip.
    InsertVideo,
}

impl Action {
    pub const ALL: [Action; 14] = [
        Action::TwoColumns,
        Action::SingleColumn,
        Action::TitleSlide,
        Action::SectionSlide,
        Action::NormalSlide,
        Action::InsertSlideAfter,
        Action::DuplicateSlide,
        Action::MoveSlideUp,
        Action::MoveSlideDown,
        Action::HideSlide,
        Action::UnhideSlide,
        Action::InsertImage,
        Action::BackgroundImage,
        Action::InsertVideo,
    ];

    /// A stable code-action kind, for binding an action to a key.
    pub fn kind(self) -> &'static str {
        match self {
            Action::TwoColumns => "refactor.preso.layout.twoColumns",
            Action::SingleColumn => "refactor.preso.layout.singleColumn",
            Action::TitleSlide => "refactor.preso.kind.title",
            Action::SectionSlide => "refactor.preso.kind.section",
            Action::NormalSlide => "refactor.preso.kind.normal",
            Action::InsertSlideAfter => "refactor.preso.slide.insertAfter",
            Action::DuplicateSlide => "refactor.preso.slide.duplicate",
            Action::MoveSlideUp => "refactor.preso.slide.moveUp",
            Action::MoveSlideDown => "refactor.preso.slide.moveDown",
            Action::HideSlide => "refactor.preso.slide.hide",
            Action::UnhideSlide => "refactor.preso.slide.unhide",
            Action::InsertImage => "refactor.preso.insert.image",
            Action::BackgroundImage => "refactor.preso.insert.background",
            Action::InsertVideo => "refactor.preso.insert.video",
        }
    }

    /// The menu title. Layout changes, inserts and slide operations are
    /// prefixed so an editor's flat code-action menu reads as groups. (The
    /// language server appends the file to an insert's title.)
    pub fn title(self) -> &'static str {
        match self {
            Action::TwoColumns => "Layout: two columns",
            Action::SingleColumn => "Layout: single column",
            Action::TitleSlide => "Layout: title slide",
            Action::SectionSlide => "Layout: section slide",
            Action::NormalSlide => "Layout: normal slide",
            Action::InsertSlideAfter => "Slide: insert a new slide after",
            Action::DuplicateSlide => "Slide: duplicate",
            Action::MoveSlideUp => "Slide: move up",
            Action::MoveSlideDown => "Slide: move down",
            Action::HideSlide => "Slide: hide",
            Action::UnhideSlide => "Slide: unhide",
            Action::InsertImage => "Insert image",
            Action::BackgroundImage => "Background image",
            Action::InsertVideo => "Insert video",
        }
    }
}

/// The heading a freshly inserted slide starts with.
const NEW_SLIDE_HEADING: &str = "# New slide";

/// Every action available with the cursor on `line`, most specific first.
pub fn actions(source: &str, line: usize) -> Vec<Edit> {
    let doc = Doc::new(source);
    let Some(index) = doc.outline.slide_at(line) else {
        return Vec::new();
    };
    let slide = &doc.outline.slides[index];
    let mut out = Vec::new();
    if slide.layout == Layout::Content
        && let Some((title, edits)) = two_columns(&doc, slide, line)
    {
        out.push(Edit {
            action: Action::TwoColumns,
            title,
            edits,
        });
    }
    let mut offer = |action: Action, edits: Option<Vec<LineEdit>>| {
        if let Some(edits) = edits.filter(|e| !e.is_empty()) {
            out.push(Edit {
                action,
                title: action.title(),
                edits,
            });
        }
    };

    if let Layout::TwoColumn { .. } = slide.layout {
        offer(Action::SingleColumn, Some(single_column(&doc, slide)));
    }
    let kind = slide.kind.as_deref();
    let is_kind = |t: &str| t.starts_with("kind=");
    if kind != Some("title") {
        offer(
            Action::TitleSlide,
            Some(set_tokens(&doc, slide, is_kind, Some("kind=title"))),
        );
    }
    if kind != Some("section") {
        offer(
            Action::SectionSlide,
            Some(set_tokens(&doc, slide, is_kind, Some("kind=section"))),
        );
    }
    if kind.is_some() {
        offer(
            Action::NormalSlide,
            Some(set_tokens(&doc, slide, is_kind, None)),
        );
    }

    // A slide whose code fence never closes runs to the end of the file:
    // a `---` after it would land in the code block, and moving it would
    // swallow its neighbour.
    let open_fence = |i: usize| doc.ends_in_fence(&doc.outline.slides[i]);
    if !open_fence(index) {
        let blank_slide = vec![String::new(), NEW_SLIDE_HEADING.to_string(), String::new()];
        offer(
            Action::InsertSlideAfter,
            Some(insert_after(&doc, slide, blank_slide)),
        );
        let copy = doc.lines[slide.lines.clone()]
            .iter()
            .map(|l| l.to_string())
            .collect();
        offer(
            Action::DuplicateSlide,
            Some(insert_after(&doc, slide, copy)),
        );
    }
    if index > 0 && !open_fence(index) {
        offer(
            Action::MoveSlideUp,
            Some(vec![swap(&doc, index - 1, index)]),
        );
    }
    if index + 1 < doc.outline.slides.len() && !open_fence(index + 1) {
        offer(
            Action::MoveSlideDown,
            Some(vec![swap(&doc, index, index + 1)]),
        );
    }

    let is_hidden = |t: &str| t == "hidden";
    if slide.hidden() {
        offer(
            Action::UnhideSlide,
            Some(set_tokens(&doc, slide, is_hidden, None)),
        );
    } else {
        offer(
            Action::HideSlide,
            Some(set_tokens(&doc, slide, is_hidden, Some("hidden"))),
        );
    }
    out
}

/// Apply non-overlapping `edits` to `source`.
pub fn apply(source: &str, edits: &[LineEdit]) -> String {
    let mut starts: Vec<usize> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .filter(|&i| i < source.len())
        .collect();
    starts.push(source.len());
    let offset = |line: usize| starts[line.min(starts.len() - 1)];

    let mut sorted: Vec<&LineEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.lines.start));
    let mut out = source.to_string();
    for edit in sorted {
        out.replace_range(offset(edit.lines.start)..offset(edit.lines.end), &edit.text);
    }
    out
}

/// The source split into lines, with what edits need to write lines back.
pub(crate) struct Doc<'a> {
    pub(crate) lines: Vec<&'a str>,
    eol: &'static str,
    final_newline: bool,
    pub(crate) outline: Outline,
}

impl<'a> Doc<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        Doc {
            lines: source.lines().collect(),
            eol: if source.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            },
            final_newline: source.is_empty() || source.ends_with('\n'),
            outline: outline::outline(source),
        }
    }

    pub(crate) fn edit(&self, mut lines: Range<usize>, mut new: Vec<String>) -> LineEdit {
        let n = self.lines.len();
        if self.final_newline || lines.end < n {
            let text = new.iter().map(|l| format!("{l}{}", self.eol)).collect();
            return LineEdit { lines, text };
        }
        // The last line has no terminator. Fold it into the edit when
        // appending, so the text never has to begin mid-line, and leave the
        // file unterminated as it was.
        if lines.start == n {
            lines.start = n - 1;
            new.insert(0, self.lines[n - 1].to_string());
        }
        LineEdit {
            lines,
            text: new.join(self.eol),
        }
    }

    fn ends_in_fence(&self, slide: &SlideSpan) -> bool {
        let mut fence = crate::fence::Tracker::default();
        for line in &self.lines[slide.lines.clone()] {
            fence.process(line);
        }
        fence.in_fence()
    }

    /// Whether a line added at the end of `slide` would land inside a code
    /// fence, math block or note that never closes, instead of being
    /// markdown.
    fn ends_open(&self, slide: &SlideSpan) -> bool {
        let mut lines = self.lines[slide.lines.clone()].to_vec();
        lines.push("***");
        outline::classify(&lines).last() != Some(&LineKind::Text)
    }

    pub(crate) fn kinds(&self, slide: &SlideSpan) -> Vec<LineKind> {
        outline::classify(&self.lines[slide.lines.clone()])
    }

    /// The slide's blocks: runs of lines between blank markdown lines. A
    /// blank line inside a fence, note or math block doesn't end one.
    fn blocks(&self, slide: &SlideSpan) -> Vec<Range<usize>> {
        let mut blocks: Vec<Range<usize>> = Vec::new();
        for (i, kind) in slide.lines.clone().zip(self.kinds(slide)) {
            if kind == LineKind::Text && self.lines[i].trim().is_empty() {
                continue;
            }
            match blocks.last_mut() {
                Some(b) if b.end == i => b.end = i + 1,
                _ => blocks.push(i..i + 1),
            }
        }
        blocks
    }

    /// `(line number, trimmed text)` of the slide's markdown lines — those a
    /// directive or `***` can sit on.
    fn text_lines(&self, slide: &SlideSpan) -> Vec<(usize, &'a str)> {
        slide
            .lines
            .clone()
            .zip(self.kinds(slide))
            .filter(|(_, kind)| *kind == LineKind::Text)
            .map(|(i, _)| (i, self.lines[i].trim()))
            .collect()
    }
}

/// Insert a slide holding `body` after `slide`.
fn insert_after(doc: &Doc, slide: &SlideSpan, body: Vec<String>) -> Vec<LineEdit> {
    let mut edits = vec![insert_delimited(doc, slide, body)];
    // A file opening with a `---` that never closes has no frontmatter —
    // the line is a delimiter in front of an empty first slide. The `---`
    // being added would close it, turning the first slide into YAML; drop
    // the leading one instead, which means nothing on its own.
    if doc.outline.frontmatter.is_none() && doc.outline.delimiters.first() == Some(&0) {
        edits.push(doc.edit(0..1, vec![]));
    }
    edits
}

fn insert_delimited(doc: &Doc, slide: &SlideSpan, mut body: Vec<String>) -> LineEdit {
    let end = slide.lines.end;
    let mut new = Vec::new();
    if end < doc.lines.len() {
        // `end` is the next slide's delimiter: land in front of it.
        new.push("---".to_string());
        new.append(&mut body);
    } else {
        if doc.lines.last().is_some_and(|l| !l.trim().is_empty()) {
            new.push(String::new());
        }
        new.push("---".to_string());
        new.append(&mut body);
        while new.last().is_some_and(|l| l.trim().is_empty()) {
            new.pop();
        }
    }
    doc.edit(end..end, new)
}

/// Swap slides `a` and `b` (`a < b`). Only their content moves; the blank
/// lines around each, and the delimiters between, stay where they are.
fn swap(doc: &Doc, a: usize, b: usize) -> LineEdit {
    let (first, second) = (&doc.outline.slides[a], &doc.outline.slides[b]);
    let own = |r: Range<usize>| doc.lines[r].iter().map(|l| l.to_string());
    let new = own(second.content.clone())
        .chain(own(first.content.end..second.content.start))
        .chain(own(first.content.clone()))
        .collect();
    doc.edit(first.content.start..second.content.end, new)
}

/// Rewrite the slide's `<!-- slide: … -->` directives: drop every token
/// `remove` matches, then add `add` to the first directive (or a new one at
/// the top of the slide). A directive left with no tokens is deleted.
fn set_tokens(
    doc: &Doc,
    slide: &SlideSpan,
    remove: impl Fn(&str) -> bool,
    add: Option<&str>,
) -> Vec<LineEdit> {
    let mut edits = Vec::new();
    let mut added = false;
    let directives = doc
        .text_lines(slide)
        .into_iter()
        .filter_map(|(i, t)| parser::directive(t, "slide").map(|spec| (i, spec)));
    for (i, spec) in directives {
        let before: Vec<&str> = spec.split_whitespace().collect();
        let mut after: Vec<&str> = before.iter().copied().filter(|t| !remove(t)).collect();
        if !added && let Some(token) = add {
            after.push(token);
            added = true;
        }
        if after == before {
            continue;
        }
        let new = if after.is_empty() {
            vec![]
        } else {
            vec![format!(
                "{}<!-- slide: {} -->",
                indent(doc.lines[i]),
                after.join(" ")
            )]
        };
        edits.push(doc.edit(i..i + 1, new));
    }
    keep_slide(doc, slide, &mut edits, "<!-- slide: -->");
    if !added && let Some(token) = add {
        let at = slide.content.start;
        edits.push(doc.edit(at..at, vec![format!("<!-- slide: {token} -->")]));
    }
    edits
}

/// Place a media file on the slide under `line`. `path` is relative to the
/// directory the deck's assets resolve against (for a chapter, its
/// top-level deck's):
///
/// - [`Action::InsertImage`]: `![alt](path)` as its own paragraph after the
///   block under the cursor, alt text from the file name;
/// - [`Action::BackgroundImage`]: `background=path` in the slide's
///   `<!-- slide: … -->`, replacing any other background;
/// - [`Action::InsertVideo`]: `<!-- video: path -->` after the block under
///   the cursor, or in place of the slide's clip if it has one (titled
///   *Replace video*).
///
/// `None` for any other action, off any slide, where the line would be
/// swallowed by a fence or comment that never closes, or for a path
/// markdown can't carry bare (whitespace, parentheses, angle brackets).
pub fn insert_media(source: &str, line: usize, action: Action, path: &str) -> Option<Edit> {
    if !is_bare_path(path) {
        return None;
    }
    let doc = Doc::new(source);
    let slide = &doc.outline.slides[doc.outline.slide_at(line)?];
    let edit = |title, edits| {
        Some(Edit {
            action,
            title,
            edits,
        })
    };
    match action {
        Action::InsertImage => {
            let stem = std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let alt: String = stem
                .chars()
                .filter(|c| !"[]".contains(*c))
                .map(|c| if c == '-' || c == '_' { ' ' } else { c })
                .collect();
            let at = media_point(&doc, slide, line)?;
            edit(
                action.title(),
                vec![insert_paragraph(
                    &doc,
                    slide,
                    at,
                    format!("![{alt}]({path})"),
                )],
            )
        }
        Action::BackgroundImage if !path.starts_with('#') => {
            let token = format!("background={path}");
            let is_background = |t: &str| t.starts_with("background=");
            edit(
                action.title(),
                set_tokens(&doc, slide, is_background, Some(&token)),
            )
        }
        Action::InsertVideo => {
            let directive = format!("<!-- video: {path} -->");
            // The last clip directive is the one that plays.
            let current = doc
                .text_lines(slide)
                .into_iter()
                .rev()
                .find(|(_, t)| parser::directive(t, "video").is_some());
            match current {
                Some((i, _)) => edit(
                    "Replace video",
                    vec![doc.edit(
                        i..i + 1,
                        vec![format!("{}{directive}", indent(doc.lines[i]))],
                    )],
                ),
                None => {
                    let at = media_point(&doc, slide, line)?;
                    edit(
                        action.title(),
                        vec![insert_paragraph(&doc, slide, at, directive)],
                    )
                }
            }
        }
        _ => None,
    }
}

/// Whether markdown and directives can carry `path` as written: no
/// whitespace, parentheses or angle brackets.
pub fn is_bare_path(path: &str) -> bool {
    !path.is_empty() && !path.contains(|c: char| c.is_whitespace() || "()<>".contains(c))
}

/// Where inserted media goes: after the block under the cursor (or the last
/// block above it), unless a line there would be code or a comment.
fn media_point(doc: &Doc, slide: &SlideSpan, cursor: usize) -> Option<usize> {
    let blocks = doc.blocks(slide);
    let block = blocks
        .iter()
        .rev()
        .find(|b| b.start <= cursor)
        .or(blocks.first())?;
    let mut lines = doc.lines[slide.lines.start..block.end].to_vec();
    lines.push("![](x.png)");
    (outline::classify(&lines).last() == Some(&LineKind::Text)).then_some(block.end)
}

/// `line` as a paragraph of its own, inserted before line `at`.
fn insert_paragraph(doc: &Doc, slide: &SlideSpan, at: usize, line: String) -> LineEdit {
    let mut new = vec![String::new(), line];
    // Keep a blank line before a following slide delimiter.
    if at == slide.lines.end && at < doc.lines.len() {
        new.push(String::new());
    }
    doc.edit(at..at, new)
}

/// The title when the cursor chose the split.
const SPLIT_HERE: &str = "Layout: two columns, split here";

/// Split the slide into two columns. With the cursor on a later block, or a
/// later item of a list, the right column starts there. Otherwise the split
/// follows the content: an image after other content goes right, a lone
/// list splits at its middle item, several blocks split in half, and
/// anything else gains an empty right column to fill. `None` only when the
/// slide already has a `***` line, a rule the split would silently take
/// over. Returns the menu title with the edits.
fn two_columns(
    doc: &Doc,
    slide: &SlideSpan,
    cursor: usize,
) -> Option<(&'static str, Vec<LineEdit>)> {
    let kinds = doc.kinds(slide);
    let at = |i: usize| (doc.lines[i], kinds[i - slide.lines.start]);
    let has_rule = slide
        .lines
        .clone()
        .any(|i| matches!(at(i), (l, LineKind::Text) if l.trim() == "***"));
    // After a fence, math block or note that never closes, a `***` would be
    // swallowed by it rather than split anything.
    if has_rule || doc.ends_open(slide) {
        return None;
    }

    let blocks = doc.blocks(slide);
    // A block of nothing but comments renders nothing; it can't be a column.
    let is_comment = |i: usize| match at(i) {
        (_, LineKind::Note) => true,
        (l, LineKind::Text) => {
            let t = l.trim();
            t.starts_with("<!--") && t.ends_with("-->")
        }
        _ => false,
    };
    let rendered: Vec<Range<usize>> = blocks
        .into_iter()
        .filter(|b| b.clone().any(|i| !is_comment(i)))
        .collect();
    // The slide's heading stays on the left, where the two-column renderer
    // lifts it into a band across both columns.
    let heading_only =
        |b: &Range<usize>| b.len() == 1 && crate::model::atx_heading(at(b.start).0).is_some();
    let skip = usize::from(rendered.first().is_some_and(heading_only));
    let body = &rendered[skip..];
    // A block's top-level list items: markers at the first marker's indent.
    let items = |b: &Range<usize>| -> Vec<usize> {
        let is_item =
            |i: &usize| at(*i).1 == LineKind::Text && parser::bullet_split(at(*i).0).is_some();
        let Some(first) = b.clone().find(is_item) else {
            return Vec::new();
        };
        let base = indent(doc.lines[first]).len();
        b.clone()
            .filter(|i| is_item(i) && indent(doc.lines[*i]).len() == base)
            .collect()
    };
    let image_only = |b: &Range<usize>| {
        b.clone()
            .all(|i| is_comment(i) || at(i).0.trim().starts_with("!["))
    };

    // Where the right column starts (`None`: an empty one, appended).
    let chosen = body.iter().enumerate().find_map(|(k, b)| {
        if b.contains(&cursor) {
            let item = items(b).into_iter().skip(1).rev().find(|&l| l <= cursor);
            Some(item.or((k > 0).then_some(b.start)))
        } else if b.start > cursor && k > 0 && body[k - 1].end <= cursor {
            Some(Some(b.start)) // In the gap before this block.
        } else {
            None
        }
    });
    // Move a split up past any pause markers or notes between the two
    // columns' content, so the `***` comes before them: otherwise the earlier
    // reveal steps have no split, and the slide jumps from one column to two.
    let blank = |i: usize| at(i).1 == LineKind::Text && at(i).0.trim().is_empty();
    let before_comments = |line: usize| {
        let mut split = line;
        let mut j = line;
        while j > slide.lines.start && (blank(j - 1) || is_comment(j - 1)) {
            j -= 1;
            if is_comment(j) {
                split = j;
            }
        }
        split
    };
    let (title, split) = match chosen.flatten() {
        Some(line) => (SPLIT_HERE, Some(line)),
        None => {
            let auto = if body.len() >= 2 {
                let k = (1..body.len())
                    .find(|&k| image_only(&body[k]))
                    .unwrap_or(body.len().div_ceil(2));
                Some(body[k].start)
            } else {
                body.first()
                    .map(items)
                    .filter(|items| items.len() >= 2)
                    .map(|items| items[items.len().div_ceil(2)])
            };
            (Action::TwoColumns.title(), auto)
        }
    };
    let split = split.map(before_comments);

    // One edit spanning both changes — the directive (replaced in place, or
    // inserted at the top) and the separator — so they can never overlap:
    // they would at the end of a file without a final newline, where an
    // append folds in the last line. The directive can sit either side of
    // the split.
    let directive = "<!-- layout: TwoColumn -->";
    let insert_directive = slide.layout_line.is_none().then_some(slide.content.start);
    let separator_at = split.unwrap_or(slide.content.end);
    let lo = [slide.layout_line, insert_directive, Some(separator_at)]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(separator_at);
    let hi = slide
        .layout_line
        .map_or(separator_at, |l| (l + 1).max(separator_at));
    // Blank lines only where the source already had one before the split —
    // between blocks — never between list items, so removing the `***`
    // later restores the text exactly (see `single_column`).
    let separator: Vec<String> = match split {
        Some(line) if blank(line - 1) && !blank(line) => vec!["***".into(), String::new()],
        Some(_) => vec!["***".into()],
        None => vec![String::new(), "***".into()],
    };
    let mut new = Vec::new();
    for i in lo..=hi {
        if insert_directive == Some(i) {
            new.push(directive.to_string());
        }
        if separator_at == i {
            new.extend(separator.iter().cloned());
        }
        if i == hi {
            break;
        }
        new.push(match slide.layout_line {
            Some(l) if l == i => format!("{}{directive}", indent(doc.lines[i])),
            _ => doc.lines[i].to_string(),
        });
    }
    Some((title, vec![doc.edit(lo..hi, new)]))
}

/// Remove the layout directive and the `***` split (with the blank line
/// after it, so the column's two surrounding blank lines don't double up).
fn single_column(doc: &Doc, slide: &SlideSpan) -> Vec<LineEdit> {
    let text = doc.text_lines(slide);
    let mut edits: Vec<LineEdit> = text
        .iter()
        .filter(|(_, t)| parser::directive(t, "layout").is_some())
        .map(|&(i, _)| doc.edit(i..i + 1, vec![]))
        .collect();
    if let Some(&(i, _)) = text.iter().find(|(_, t)| *t == "***") {
        let inside = |j: usize| slide.lines.contains(&j);
        let blank = |j: usize| inside(j) && doc.lines[j].trim().is_empty();
        let (before, after) = (i.checked_sub(1).is_some_and(blank), blank(i + 1));
        let lines = if before && after {
            i..i + 2 // Between blocks: one blank line stays.
        } else if before && !inside(i + 1) {
            i - 1..i + 1 // An empty right column at the end.
        } else {
            i..i + 1 // Between list items.
        };
        edits.push(doc.edit(lines, vec![]));
    }
    // The layout directive comes first, so it's the deletion kept.
    edits.reverse();
    keep_slide(doc, slide, &mut edits, "<!-- layout: Content -->");
    edits
}

/// If `edits` would delete every non-blank line of `slide` — a slide of
/// nothing but directives, which still counts as a (blank) slide — keep the
/// last deletion's line as `neutral` instead, so the slide stays.
fn keep_slide(doc: &Doc, slide: &SlideSpan, edits: &mut [LineEdit], neutral: &str) {
    let deleted = |i: usize| {
        edits
            .iter()
            .any(|e| e.text.is_empty() && e.lines.contains(&i))
    };
    let survives = slide
        .lines
        .clone()
        .any(|i| !doc.lines[i].trim().is_empty() && !deleted(i));
    if survives {
        return;
    }
    if let Some(last) = edits.iter_mut().rev().find(|e| e.text.is_empty()) {
        let at = last.lines.start;
        let rest: Vec<String> = doc.lines[at + 1..last.lines.end]
            .iter()
            .map(|l| l.to_string())
            .collect();
        let mut new = vec![neutral.to_string()];
        new.extend(rest);
        *last = doc.edit(last.lines.clone(), new);
    }
}

pub(crate) fn indent(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str, line: usize, action: Action) -> Option<String> {
        actions(source, line)
            .into_iter()
            .find(|e| e.action == action)
            .map(|e| apply(source, &e.edits))
    }

    fn offered(source: &str, line: usize) -> Vec<Action> {
        actions(source, line)
            .into_iter()
            .map(|e| e.action)
            .collect()
    }

    const DECK: &str = "---\ntitle: T\n---\n\n# A\n\nalpha\n\n---\n\n# B\n\nbeta\n";

    #[test]
    fn insert_after_a_middle_slide() {
        let out = run(DECK, 4, Action::InsertSlideAfter).unwrap();
        assert_eq!(
            out,
            "---\ntitle: T\n---\n\n# A\n\nalpha\n\n---\n\n# New slide\n\n---\n\n# B\n\nbeta\n"
        );
    }

    #[test]
    fn insert_after_the_last_slide() {
        let out = run(DECK, 12, Action::InsertSlideAfter).unwrap();
        assert!(out.ends_with("beta\n\n---\n\n# New slide\n"), "{out}");
    }

    #[test]
    fn insert_after_last_slide_without_final_newline() {
        let out = run("# A\nalpha", 0, Action::InsertSlideAfter).unwrap();
        assert_eq!(out, "# A\nalpha\n\n---\n\n# New slide");
    }

    #[test]
    fn duplicate_copies_the_whole_segment() {
        let out = run(DECK, 6, Action::DuplicateSlide).unwrap();
        assert_eq!(
            out,
            "---\ntitle: T\n---\n\n# A\n\nalpha\n\n---\n\n# A\n\nalpha\n\n---\n\n# B\n\nbeta\n"
        );
        let out = run(DECK, 10, Action::DuplicateSlide).unwrap();
        assert!(out.ends_with("beta\n\n---\n\n# B\n\nbeta\n"), "{out}");
    }

    #[test]
    fn moves_swap_content_and_keep_the_spacing() {
        let down = run(DECK, 4, Action::MoveSlideDown).unwrap();
        assert_eq!(
            down,
            "---\ntitle: T\n---\n\n# B\n\nbeta\n\n---\n\n# A\n\nalpha\n"
        );
        let up = run(&down, 10, Action::MoveSlideUp).unwrap();
        assert_eq!(up, DECK);
        assert!(!offered(DECK, 4).contains(&Action::MoveSlideUp));
        assert!(!offered(DECK, 12).contains(&Action::MoveSlideDown));
    }

    #[test]
    fn no_actions_in_frontmatter_or_an_empty_file() {
        assert!(actions(DECK, 1).is_empty());
        assert!(actions("", 0).is_empty());
    }

    #[test]
    fn two_columns_split_at_the_cursor_block() {
        let src = "# Title\n\nleft text\n\n![pic](a.png)\n";
        let out = run(src, 4, Action::TwoColumns).unwrap();
        assert_eq!(
            out,
            "<!-- layout: TwoColumn -->\n# Title\n\nleft text\n\n***\n\n![pic](a.png)\n"
        );
        let slide = &crate::parser::parse(&out).unwrap().slides[0];
        let (left, right) = slide.columns_at(0).unwrap();
        assert!(left.contains("left text") && right.contains("pic"));
    }

    fn two_columns(source: &str, line: usize) -> (&'static str, String) {
        let e = actions(source, line)
            .into_iter()
            .find(|e| e.action == Action::TwoColumns)
            .expect("two columns offered");
        (e.title, apply(source, &e.edits))
    }

    /// The (left, right) column sources after splitting.
    fn columns(source: &str) -> (String, String) {
        let slide = &crate::parser::parse(source).unwrap().slides[0];
        slide.columns_at(slide.steps.len() - 1).unwrap()
    }

    const LIST: &str = "# Title\n\n- one\n- two\n  - nested\n- three\n- four\n- five\n";

    #[test]
    fn two_columns_is_offered_on_every_single_column_slide() {
        for line in 0..LIST.lines().count() {
            assert!(
                offered(LIST, line).contains(&Action::TwoColumns),
                "line {line}"
            );
        }
        assert!(offered("# Only a heading\n", 0).contains(&Action::TwoColumns));
    }

    #[test]
    fn a_list_splits_at_the_item_under_the_cursor() {
        let (title, out) = two_columns(LIST, 6); // on `- four`
        assert_eq!(title, SPLIT_HERE);
        let (left, right) = columns(&out);
        assert!(left.contains("- three") && !left.contains("four"), "{left}");
        assert!(right.trim_start().starts_with("- four"), "{right}");
        // A nested line belongs to its item.
        let (_, out) = two_columns(LIST, 4);
        let (_, right) = columns(&out);
        assert!(right.trim_start().starts_with("- two"), "{right}");
    }

    #[test]
    fn a_lone_list_splits_at_its_middle_item_by_default() {
        for line in [0, 2] {
            let (title, out) = two_columns(LIST, line);
            assert_eq!(title, Action::TwoColumns.title());
            let (left, right) = columns(&out);
            assert!(left.contains("- three"), "{left}");
            assert!(right.trim_start().starts_with("- four"), "{right}");
        }
    }

    #[test]
    fn an_image_after_the_text_goes_right() {
        let src = "# Title\n\nintro\n\nmore\n\n![pic](a.png)\n\ncaption\n";
        let (_, out) = two_columns(src, 0);
        let (left, right) = columns(&out);
        assert!(left.contains("more") && right.trim_start().starts_with("![pic]"));
    }

    #[test]
    fn a_single_paragraph_gains_an_empty_right_column() {
        let src = "# Title\n\nJust one paragraph.\n";
        let (title, out) = two_columns(src, 2);
        assert_eq!(title, Action::TwoColumns.title());
        assert_eq!(
            out,
            "<!-- layout: TwoColumn -->\n# Title\n\nJust one paragraph.\n\n***\n"
        );
        assert_eq!(columns(&out).1.trim(), "");
    }

    #[test]
    fn the_split_comes_before_pause_markers() {
        let src = "## Why?\n\n- one\n<!-- pause -->\n- two\n<!-- pause -->\n- three\n\n<!-- note: n -->\n";
        let (_, out) = two_columns(src, 0);
        assert_eq!(
            out,
            "<!-- layout: TwoColumn -->\n## Why?\n\n- one\n<!-- pause -->\n- two\n***\n<!-- pause -->\n- three\n\n<!-- note: n -->\n"
        );
        // Split from the first step on, so the layout doesn't jump.
        let slide = &crate::parser::parse(&out).unwrap().slides[0];
        for step in 0..slide.steps.len() {
            assert!(slide.columns_at(step).is_some(), "step {step}");
        }
        assert_eq!(run(&out, 1, Action::SingleColumn).unwrap(), src);
    }

    #[test]
    fn a_layout_directive_below_the_split_is_still_replaced() {
        let src = "intro\n\n<!-- layout: Content -->\n# Title\n";
        let (_, out) = two_columns(src, 0);
        assert_eq!(out, "intro\n\n***\n\n<!-- layout: TwoColumn -->\n# Title\n");
        assert!(columns(&out).1.contains("# Title"));
    }

    #[test]
    fn a_one_line_slide_without_a_final_newline() {
        let (_, out) = two_columns("Hello", 0);
        assert_eq!(out, "<!-- layout: TwoColumn -->\nHello\n\n***");
        let (_, out) = two_columns("<!-- layout: Content -->\nHello", 1);
        assert_eq!(out, "<!-- layout: TwoColumn -->\nHello\n\n***");
    }

    #[test]
    fn two_columns_not_offered_after_an_unclosed_block() {
        for src in [
            "text\n\n```\ncode",
            "text\n\n$$\nx^2",
            "text\n<!-- note: open",
        ] {
            assert!(!offered(src, 0).contains(&Action::TwoColumns), "{src:?}");
        }
    }

    #[test]
    fn single_column_undoes_every_kind_of_split() {
        let deck = format!("{LIST}\n---\n\n# Next\n");
        for src in [
            LIST,
            deck.as_str(),
            "# Title\n\nJust one paragraph.\n",
            "# Title\n\nJust one paragraph.\n\n---\n# B\n",
            "# Title\n\nleft text\n\n![pic](a.png)\n",
            "# Loose\n\n- a\n\n- b\n\n- c\n\n- d\n",
            "# T\n\ntext\n\n<!-- pause -->\n\n![img](x.png)\n",
        ] {
            for line in 0..3 {
                let (_, split) = two_columns(src, line);
                let back = run(&split, 1, Action::SingleColumn).unwrap();
                assert_eq!(back, src, "split at line {line}: {split:?}");
            }
        }
    }

    #[test]
    fn two_columns_not_offered_over_an_existing_rule() {
        let src = "a\n\n***\n\nb\n";
        assert!(!offered(src, 4).contains(&Action::TwoColumns));
    }

    #[test]
    fn two_columns_ignores_blank_lines_inside_fences() {
        let src = "intro\n\n```\ncode\n\nmore code\n```\n\nafter\n";
        // The cursor on the fence's inner blank line splits before the
        // whole fence, not in the middle of it.
        let out = run(src, 4, Action::TwoColumns).unwrap();
        assert!(
            out.contains("intro\n\n***\n\n```\ncode\n\nmore code\n```"),
            "{out}"
        );
    }

    #[test]
    fn two_columns_replaces_an_existing_layout_directive() {
        let src = "<!-- layout: Content -->\nleft\n\nright\n";
        let out = run(src, 3, Action::TwoColumns).unwrap();
        assert_eq!(out, "<!-- layout: TwoColumn -->\nleft\n\n***\n\nright\n");
    }

    #[test]
    fn single_column_undoes_two_columns() {
        let src = "# Title\n\nleft text\n\n![pic](a.png)\n";
        let split = run(src, 4, Action::TwoColumns).unwrap();
        let back = run(&split, 1, Action::SingleColumn).unwrap();
        assert_eq!(back, src);
    }

    #[test]
    fn kinds_add_replace_and_clear_the_token() {
        let src = "# A\n";
        let title = run(src, 0, Action::TitleSlide).unwrap();
        assert_eq!(title, "<!-- slide: kind=title -->\n# A\n");
        let section = run(&title, 1, Action::SectionSlide).unwrap();
        assert_eq!(section, "<!-- slide: kind=section -->\n# A\n");
        assert_eq!(run(&section, 1, Action::NormalSlide).unwrap(), src);
        assert!(!offered(src, 0).contains(&Action::NormalSlide));
    }

    #[test]
    fn kinds_keep_other_slide_tokens() {
        let src = "<!-- slide: align=center kind=title -->\n# A\n";
        let out = run(src, 1, Action::SectionSlide).unwrap();
        assert_eq!(out, "<!-- slide: align=center kind=section -->\n# A\n");
        let out = run(src, 1, Action::NormalSlide).unwrap();
        assert_eq!(out, "<!-- slide: align=center -->\n# A\n");
    }

    #[test]
    fn hide_and_unhide() {
        let src = "# A\n---\n# B\n";
        let hidden = run(src, 2, Action::HideSlide).unwrap();
        assert_eq!(hidden, "# A\n---\n<!-- slide: hidden -->\n# B\n");
        assert_eq!(crate::parser::parse(&hidden).unwrap().slides.len(), 1);
        assert_eq!(run(&hidden, 3, Action::UnhideSlide).unwrap(), src);
    }

    #[test]
    fn directives_inside_fences_are_left_alone() {
        let src = "# A\n\n```md\n<!-- slide: kind=title -->\n```\n";
        let out = run(src, 0, Action::TitleSlide).unwrap();
        assert_eq!(
            out,
            "<!-- slide: kind=title -->\n# A\n\n```md\n<!-- slide: kind=title -->\n```\n"
        );
    }

    #[test]
    fn a_leading_unclosed_delimiter_is_not_turned_into_frontmatter() {
        let src = "---\n# A\n";
        let out = run(src, 1, Action::DuplicateSlide).unwrap();
        assert_eq!(out, "# A\n\n---\n# A\n");
        assert_eq!(crate::parser::parse(&out).unwrap().slides.len(), 2);
    }

    #[test]
    fn nothing_lands_inside_an_unclosed_fence() {
        let src = "# A\n---\n# B\n```\ncode\n";
        let on_b = offered(src, 2);
        for action in [
            Action::InsertSlideAfter,
            Action::DuplicateSlide,
            Action::MoveSlideUp,
        ] {
            assert!(!on_b.contains(&action), "{action:?}");
        }
        assert!(!offered(src, 0).contains(&Action::MoveSlideDown));
    }

    #[test]
    fn a_slide_of_only_directives_survives_their_removal() {
        let src = "# A\n---\n<!-- slide: hidden -->\n";
        let out = run(src, 2, Action::UnhideSlide).unwrap();
        assert_eq!(out, "# A\n---\n<!-- slide: -->\n");
        let out = run("<!-- layout: TwoColumn -->", 0, Action::SingleColumn).unwrap();
        assert_eq!(out, "<!-- layout: Content -->");
    }

    fn media(
        source: &str,
        line: usize,
        action: Action,
        path: &str,
    ) -> Option<(&'static str, String)> {
        insert_media(source, line, action, path).map(|e| (e.title, apply(source, &e.edits)))
    }

    #[test]
    fn images_land_after_the_block_under_the_cursor() {
        let src = "# A\n\n- one\n- two\n\nafter\n\n---\n\n# B\n";
        let (title, out) = media(src, 3, Action::InsertImage, "img/net-diagram_v2.png").unwrap();
        assert_eq!(title, "Insert image");
        assert_eq!(
            out,
            "# A\n\n- one\n- two\n\n![net diagram v2](img/net-diagram_v2.png)\n\nafter\n\n---\n\n# B\n"
        );
        // On the heading: after the heading.
        let (_, out) = media("# A\n", 0, Action::InsertImage, "a.png").unwrap();
        assert_eq!(out, "# A\n\n![a](a.png)\n");
        // At the end of a slide, a blank line stays before the delimiter.
        let (_, out) = media("# A\ntext\n---\n# B\n", 1, Action::InsertImage, "a.png").unwrap();
        assert_eq!(out, "# A\ntext\n\n![a](a.png)\n\n---\n# B\n");
        // End of a file without a final newline.
        let (_, out) = media("# A", 0, Action::InsertImage, "a.png").unwrap();
        assert_eq!(out, "# A\n\n![a](a.png)");
    }

    #[test]
    fn videos_insert_or_replace_the_clip() {
        let (title, out) = media("# A\n\ntext\n", 2, Action::InsertVideo, "demo.mp4").unwrap();
        assert_eq!(title, "Insert video");
        assert_eq!(out, "# A\n\ntext\n\n<!-- video: demo.mp4 -->\n");
        let src = "# A\n<!-- video: old.mp4 -->\ntext\n";
        let (title, out) = media(src, 2, Action::InsertVideo, "new.mp4").unwrap();
        assert_eq!(title, "Replace video");
        assert_eq!(out, "# A\n<!-- video: new.mp4 -->\ntext\n");
        let slide = &crate::parser::parse(&out).unwrap().slides[0];
        assert_eq!(slide.video.as_deref(), Some("new.mp4"));
    }

    #[test]
    fn background_replaces_the_background_and_keeps_its_fit() {
        let (_, out) = media("# A\n", 0, Action::BackgroundImage, "bg.jpg").unwrap();
        assert_eq!(out, "<!-- slide: background=bg.jpg -->\n# A\n");
        let src = "<!-- slide: background=#112233 fit=contain -->\n# A\n";
        let (_, out) = media(src, 1, Action::BackgroundImage, "bg.jpg").unwrap();
        assert_eq!(out, "<!-- slide: fit=contain background=bg.jpg -->\n# A\n");
    }

    #[test]
    fn media_is_refused_where_markdown_cant_hold_it() {
        assert!(media("# A\n", 0, Action::InsertImage, "my pic.png").is_none());
        assert!(media("# A\n", 0, Action::InsertImage, "a(1).png").is_none());
        assert!(media("# A\n```\ncode\n", 2, Action::InsertImage, "a.png").is_none());
        assert!(media("---\ntitle: T\n---\n# A\n", 1, Action::InsertImage, "a.png").is_none());
        assert!(media("# A\n", 0, Action::TwoColumns, "a.png").is_none());
    }

    #[test]
    fn crlf_files_stay_crlf() {
        let src = "# A\r\n\r\n---\r\n\r\n# B\r\n";
        let out = run(src, 0, Action::MoveSlideDown).unwrap();
        assert_eq!(out, "# B\r\n\r\n---\r\n\r\n# A\r\n");
        let out = run(src, 0, Action::InsertSlideAfter).unwrap();
        assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
    }

    #[test]
    fn action_kinds_are_unique_and_namespaced() {
        let kinds: std::collections::HashSet<_> = Action::ALL.iter().map(|a| a.kind()).collect();
        assert_eq!(kinds.len(), Action::ALL.len());
        assert!(kinds.iter().all(|k| k.starts_with("refactor.preso.")));
    }
}
