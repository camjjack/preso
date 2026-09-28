//! Styling for the element under the cursor — an image's `{…}` attributes,
//! a table's text size and column alignment, a code or diagram fence's
//! annotation. Offered only where the cursor is on something that takes the
//! style, so the language server's menu grows an *Image:*, *Table:* or
//! *Code:* group exactly when it means something.
//!
//! Like [`crate::edit`], each style is a line rewrite into syntax the parser
//! already reads, using the parser's own helpers to find images, tables and
//! fences. Table *structure* (rows, columns, pipe alignment) is left to the
//! editor's markdown tooling; only what preso adds — text size, and the
//! column alignment it renders — is here.

use crate::edit::{Doc, LineEdit, indent};
use crate::fence;
use crate::lint;
use crate::outline::{LineKind, SlideSpan};
use crate::parser;
use std::collections::BTreeSet;
use std::ops::Range;

/// A style available at the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleEdit {
    /// A stable code-action kind (`refactor.preso.image.width.50`, …).
    pub kind: &'static str,
    pub title: String,
    pub edits: Vec<LineEdit>,
}

/// Image widths offered, in percent of the content width.
const IMAGE_WIDTHS: [u8; 4] = [25, 50, 75, 100];
/// Table text sizes offered (design units; the theme's body size is the
/// default, and a table that needs a size usually needs to shrink).
const TABLE_SIZES: [u8; 3] = [28, 24, 20];
/// Code and diagram widths offered.
const BLOCK_WIDTHS: [u8; 3] = [50, 75, 100];
/// Fence languages preso renders as diagrams.
const DIAGRAMS: [&str; 3] = ["mermaid", "dot", "graphviz"];

/// Every style for whatever sits under the cursor at `line`/`col` (a byte
/// column). Empty when it's nothing stylable.
pub fn styles(source: &str, line: usize, col: usize) -> Vec<StyleEdit> {
    let doc = Doc::new(source);
    let Some(slide) = doc
        .outline
        .slide_at(line)
        .map(|i| &doc.outline.slides[i])
        .filter(|s| s.lines.contains(&line))
    else {
        return Vec::new();
    };
    let kinds = doc.kinds(slide);
    let kind_at = |i: usize| kinds[i - slide.lines.start];
    let mut out = Vec::new();
    if let Some(fence) = fence_at(&doc, slide, &kinds, line) {
        code_styles(&doc, &fence, line, &mut out);
    } else if kind_at(line) == LineKind::Text {
        if let Some(table) = table_at(&doc, slide, &kinds, line) {
            table_styles(&doc, slide, &kinds, &table, line, col, &mut out);
        } else {
            image_styles(&doc, slide, &kinds, line, col, &mut out);
        }
    }
    out
}

fn style(kind: &'static str, title: impl Into<String>, edit: LineEdit) -> StyleEdit {
    StyleEdit {
        kind,
        title: title.into(),
        edits: vec![edit],
    }
}

fn align_title(align: &str) -> &'static str {
    match align {
        "center" => "centre",
        "right" => "align right",
        _ => "align left",
    }
}

// --- Images -------------------------------------------------------------

/// One `![alt](url)` on a line, with its glued-on `{…}` group, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageSpan {
    pub(crate) start: usize,
    /// The `)` that ends the url.
    pub(crate) close: usize,
    /// Inside the braces of a `){…}` group.
    pub(crate) attrs: Option<Range<usize>>,
    /// Just past the image, attributes included.
    pub(crate) end: usize,
}

/// The images on a line, outside inline code. As the parser reads them, an
/// image ends at the first `)` after its `](`, and a `{…}` group belongs to
/// it only when glued on.
pub(crate) fn image_spans(line: &str) -> Vec<ImageSpan> {
    let code = lint::code_spans(line);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = line[from..].find("![") {
        let start = from + found;
        from = start + 2;
        if code.iter().any(|c| c.contains(&start)) {
            continue;
        }
        let Some(open) = line[from..].find("](") else {
            break;
        };
        let Some(close) = line[from + open + 2..].find(')') else {
            break;
        };
        let close = from + open + 2 + close;
        let attrs = line[close + 1..]
            .strip_prefix('{')
            .and_then(|rest| rest.find('}'))
            .map(|len| close + 2..close + 2 + len);
        let end = attrs.as_ref().map_or(close + 1, |a| a.end + 1);
        out.push(ImageSpan {
            start,
            close,
            attrs,
            end,
        });
        from = end;
    }
    out
}

fn image_styles(
    doc: &Doc,
    slide: &SlideSpan,
    kinds: &[LineKind],
    line: usize,
    col: usize,
    out: &mut Vec<StyleEdit>,
) {
    let text = doc.lines[line];
    if text.trim_start().starts_with("<!--") {
        return;
    }
    let spans = image_spans(text);
    let Some(span) = spans
        .iter()
        .find(|s| (s.start..=s.end).contains(&col))
        .or(spans.first())
    else {
        return;
    };
    let tokens: Vec<&str> = span
        .attrs
        .clone()
        .map(|a| text[a].split_whitespace().collect())
        .unwrap_or_default();
    let rewrite = |tokens: Vec<String>| {
        let group = if tokens.is_empty() {
            String::new()
        } else {
            format!("{{{}}}", tokens.join(" "))
        };
        let new = format!("{}{group}{}", &text[..=span.close], &text[span.end..]);
        doc.edit(line..line + 1, vec![new])
    };
    // Replace the `key=` token (in place, else appended); `None` removes it.
    let set = |key: &str, value: Option<String>| {
        let mut new: Vec<String> = Vec::new();
        let mut placed = false;
        for t in &tokens {
            if t.starts_with(&format!("{key}=")) {
                if let Some(v) = value.as_ref().filter(|_| !placed) {
                    new.push(format!("{key}={v}"));
                    placed = true;
                }
            } else {
                new.push(t.to_string());
            }
        }
        if let Some(v) = value.filter(|_| !placed) {
            new.push(format!("{key}={v}"));
        }
        rewrite(new)
    };
    let toggle = |flag: &str| {
        let mut new: Vec<String> = tokens
            .iter()
            .filter(|t| **t != flag)
            .map(|t| t.to_string())
            .collect();
        if !tokens.contains(&flag) {
            new.push(flag.to_string());
        }
        rewrite(new)
    };

    let width = tokens
        .iter()
        .find_map(|t| t.strip_prefix("width="))
        .and_then(|v| v.trim_end_matches('%').parse::<f32>().ok());
    for w in IMAGE_WIDTHS {
        if width != Some(f32::from(w)) {
            let kind = match w {
                25 => "refactor.preso.image.width.25",
                50 => "refactor.preso.image.width.50",
                75 => "refactor.preso.image.width.75",
                _ => "refactor.preso.image.width.100",
            };
            out.push(style(
                kind,
                format!("Image: width {w}%"),
                set("width", Some(format!("{w}%"))),
            ));
        }
    }
    let align = tokens
        .iter()
        .find_map(|t| t.strip_prefix("align="))
        .unwrap_or("left");
    for (value, kind) in [
        ("center", "refactor.preso.image.align.center"),
        ("right", "refactor.preso.image.align.right"),
        ("left", "refactor.preso.image.align.left"),
    ] {
        if align != value {
            // Left is the default: say so by dropping the token.
            let v = (value != "left").then(|| value.to_string());
            out.push(style(
                kind,
                format!("Image: {}", align_title(value)),
                set("align", v),
            ));
        }
    }
    let has = |flag: &str| tokens.contains(&flag);
    let flag = |f: &str, kind, on: &str, off: &str| {
        let title = if has(f) { off } else { on };
        style(kind, format!("Image: {title}"), toggle(f))
    };
    out.push(flag(
        "border",
        "refactor.preso.image.border",
        "add border",
        "remove border",
    ));
    out.push(flag(
        "shadow",
        "refactor.preso.image.shadow",
        "add shadow",
        "remove shadow",
    ));
    out.push(flag(
        "plain",
        "refactor.preso.image.plain",
        "plain (no theme border or shadow)",
        "theme border and shadow (remove plain)",
    ));
    // `fit` only means something in a row of adjacent image lines.
    let image_line = |i: usize| {
        slide.lines.contains(&i)
            && kinds[i - slide.lines.start] == LineKind::Text
            && parser::is_image_line(doc.lines[i])
    };
    let in_row =
        image_line(line) && (line.checked_sub(1).is_some_and(image_line) || image_line(line + 1));
    if in_row {
        out.push(flag(
            "fit",
            "refactor.preso.image.fit",
            "row at natural widths (fit)",
            "row at equal widths (remove fit)",
        ));
    }
    if span.attrs.is_some() {
        out.push(style(
            "refactor.preso.image.clear",
            "Image: remove styling",
            rewrite(Vec::new()),
        ));
    }
}

// --- Tables -------------------------------------------------------------

/// A table's lines: the header, its delimiter row, and body rows.
struct Table {
    lines: Range<usize>,
}

/// The slide's tables, found as the parser finds them: a line with a `|`
/// followed by a delimiter row, then every following row line.
fn tables(doc: &Doc, slide: &SlideSpan, kinds: &[LineKind]) -> Vec<Table> {
    let text = |i: usize| {
        (slide.lines.contains(&i) && kinds[i - slide.lines.start] == LineKind::Text)
            .then(|| doc.lines[i])
    };
    let mut out = Vec::new();
    let mut i = slide.lines.start;
    while i < slide.lines.end {
        let header = text(i).is_some_and(|l| l.contains('|'));
        if header && text(i + 1).is_some_and(|l| parser::table_delimiter(l).is_some()) {
            let mut end = i + 2;
            while text(end).is_some_and(parser::is_table_row) {
                end += 1;
            }
            out.push(Table { lines: i..end });
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

fn table_at(doc: &Doc, slide: &SlideSpan, kinds: &[LineKind], line: usize) -> Option<Table> {
    tables(doc, slide, kinds)
        .into_iter()
        .find(|t| t.lines.contains(&line))
}

fn table_styles(
    doc: &Doc,
    slide: &SlideSpan,
    kinds: &[LineKind],
    table: &Table,
    line: usize,
    col: usize,
    out: &mut Vec<StyleEdit>,
) {
    let header = table.lines.start;

    // The `<!-- table: size=… -->` that applies: the nearest above the
    // table, with no other table in between to consume it.
    let previous_end = tables(doc, slide, kinds)
        .iter()
        .map(|t| t.lines.end)
        .filter(|&end| end <= header)
        .max()
        .unwrap_or(slide.lines.start);
    let directive = (previous_end..header).rev().find(|&i| {
        kinds[i - slide.lines.start] == LineKind::Text
            && parser::directive(doc.lines[i].trim(), "table").is_some()
    });
    let size = directive.and_then(|i| {
        parser::directive(doc.lines[i].trim(), "table")?
            .split_whitespace()
            .find_map(|t| t.strip_prefix("size="))
            .and_then(|v| v.parse::<f32>().ok())
    });
    let set_size = |value: Option<u8>| match (directive, value) {
        (Some(i), Some(n)) => doc.edit(
            i..i + 1,
            vec![format!("{}<!-- table: size={n} -->", indent(doc.lines[i]))],
        ),
        (Some(i), None) => doc.edit(i..i + 1, vec![]),
        (None, Some(n)) => doc.edit(header..header, vec![format!("<!-- table: size={n} -->")]),
        (None, None) => unreachable!("only offered with a size to set or clear"),
    };
    for n in TABLE_SIZES {
        if size != Some(f32::from(n)) {
            let kind = match n {
                28 => "refactor.preso.table.size.28",
                24 => "refactor.preso.table.size.24",
                _ => "refactor.preso.table.size.20",
            };
            out.push(style(
                kind,
                format!("Table: text size {n}"),
                set_size(Some(n)),
            ));
        }
    }
    if size.is_some() {
        out.push(style(
            "refactor.preso.table.size.normal",
            "Table: normal text size",
            set_size(None),
        ));
    }

    // The column under the cursor, and its delimiter cell.
    let delimiter = header + 1;
    let cells = delimiter_cells(doc.lines[delimiter]);
    if cells.is_empty() {
        return;
    }
    let text = doc.lines[line];
    let lead = usize::from(text.trim_start().starts_with('|'));
    let pipes = unescaped_pipes(&text[..col.min(text.len())]);
    let column = pipes.saturating_sub(lead).min(cells.len() - 1);
    let cell = cells[column].clone();
    let row = doc.lines[delimiter];
    let content = row[cell.clone()].trim();
    let current = match (content.starts_with(':'), content.ends_with(':')) {
        (true, true) if content.len() > 1 => "center",
        (_, true) => "right",
        _ => "left",
    };
    let name = parser::table_cells(doc.lines[header])
        .get(column)
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .map_or(String::new(), |h| format!(" ({h})"));
    // Keep the cell's width, so a pipe-aligned table stays aligned.
    let lead_ws = &row[cell.start..cell.end - row[cell.clone()].trim_start().len()];
    let trail_ws = &row[cell.start + lead_ws.len() + content.len()..cell.end];
    let len = content.len().max(3);
    for (value, kind) in [
        ("center", "refactor.preso.table.align.center"),
        ("right", "refactor.preso.table.align.right"),
        ("left", "refactor.preso.table.align.left"),
    ] {
        if value == current {
            continue;
        }
        let dashes = match value {
            "center" => format!(":{}:", "-".repeat(len.saturating_sub(2).max(1))),
            "right" => format!("{}:", "-".repeat(len - 1)),
            _ => "-".repeat(len),
        };
        let new = format!(
            "{}{lead_ws}{dashes}{trail_ws}{}",
            &row[..cell.start],
            &row[cell.end..]
        );
        let n = column + 1;
        let title = match value {
            "center" => format!("Table: centre column {n}{name}"),
            "right" => format!("Table: align column {n} right{name}"),
            _ => format!("Table: align column {n} left{name}"),
        };
        out.push(style(
            kind,
            title,
            doc.edit(delimiter..delimiter + 1, vec![new]),
        ));
    }
}

/// Byte ranges of a delimiter row's cells, between the pipes, dropping the
/// empty edges outside a leading or trailing `|`.
fn delimiter_cells(row: &str) -> Vec<Range<usize>> {
    let mut cells = Vec::new();
    let mut start = 0;
    for (i, c) in row.char_indices() {
        if c == '|' {
            cells.push(start..i);
            start = i + 1;
        }
    }
    cells.push(start..row.len());
    if cells
        .first()
        .is_some_and(|c| row[c.clone()].trim().is_empty())
    {
        cells.remove(0);
    }
    if cells
        .last()
        .is_some_and(|c| row[c.clone()].trim().is_empty())
    {
        cells.pop();
    }
    cells
}

/// `|` characters not escaped with a backslash.
fn unescaped_pipes(text: &str) -> usize {
    let bytes = text.as_bytes();
    (0..bytes.len())
        .filter(|&i| bytes[i] == b'|' && (i == 0 || bytes[i - 1] != b'\\'))
        .count()
}

// --- Code and diagram fences --------------------------------------------

struct Fence {
    /// The opening fence line.
    open: usize,
    /// The closing fence line, or the last code line when it never closes.
    close: usize,
}

/// The fenced block containing `line`, opening and closing fences included.
fn fence_at(doc: &Doc, slide: &SlideSpan, kinds: &[LineKind], line: usize) -> Option<Fence> {
    let mut tracker = fence::Tracker::default();
    let mut open: Option<usize> = None;
    for i in slide.lines.clone() {
        if kinds[i - slide.lines.start] != LineKind::Code {
            continue;
        }
        let starting = !tracker.in_fence();
        tracker.process(doc.lines[i]);
        if starting {
            open = Some(i);
        }
        let closed = !tracker.in_fence();
        let block_end = closed || i + 1 == slide.lines.end;
        if let Some(o) = open
            && block_end
            && (o..=i).contains(&line)
        {
            return Some(Fence { open: o, close: i });
        }
        if closed {
            open = None;
        }
    }
    None
}

fn code_styles(doc: &Doc, fence: &Fence, line: usize, out: &mut Vec<StyleEdit>) {
    let text = doc.lines[fence.open];
    let body = text.trim_start();
    let marker_len = body.chars().take_while(|&c| c == '`' || c == '~').count();
    let info = &body[marker_len..];
    let brace = info.find('{');
    let language = info[..brace.unwrap_or(info.len())]
        .split_whitespace()
        .next();
    let annotation = brace.map(|b| {
        let inner = &info[b + 1..];
        inner[..inner.rfind('}').unwrap_or(inner.len())].to_string()
    });
    // The annotation's words, exactly as written. Commas stay inside the
    // word they're in: they separate line numbers (`2,4-6`) and — in a
    // diagram's zoom stages — node labels that may hold spaces (`Read, Parse
    // input`), so splitting on them and joining with spaces would turn two
    // labels into one. A rewrite only ever adds, drops or replaces a word.
    let words: Vec<String> = annotation
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let is_line_spec = |t: &str| {
        !t.is_empty()
            && t.split('-').count() <= 2
            && t.split('-')
                .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    // A word of line numbers: `2`, `4-6`, `2,4-6`, or `2,` before a space.
    let is_line_list = |w: &str| {
        w.split(',').any(|t| !t.is_empty()) && w.split(',').all(|t| t.is_empty() || is_line_spec(t))
    };
    let staged = words.iter().any(|t| t.contains('|'));
    let diagram = language.is_some_and(|l| DIAGRAMS.contains(&l));
    let noun = if diagram { "Diagram" } else { "Code" };

    // Write the opening line back with `lines` highlighted and `others` as
    // the remaining tokens, in their original order.
    let rewrite = |lines: &BTreeSet<usize>, others: Vec<String>| {
        let mut parts: Vec<String> = Vec::new();
        if !lines.is_empty() {
            parts.push(compress(lines));
        }
        parts.extend(others);
        let head = match brace {
            Some(b) => text[..text.len() - info.len() + b].trim_end().to_string(),
            None => text.trim_end().to_string(),
        };
        let new = if parts.is_empty() {
            head
        } else {
            format!("{head} {{{}}}", parts.join(" "))
        };
        doc.edit(fence.open..fence.open + 1, vec![new])
    };
    let lines: BTreeSet<usize> = words
        .iter()
        .filter(|w| is_line_list(w))
        .flat_map(|w| w.split(','))
        .filter(|t| !t.is_empty())
        .flat_map(|t| match t.split_once('-') {
            Some((a, b)) => {
                let (a, b): (usize, usize) = (a.parse().unwrap_or(0), b.parse().unwrap_or(0));
                (a..=b).collect::<Vec<_>>()
            }
            None => t.parse().into_iter().collect(),
        })
        .collect();
    let others: Vec<String> = words.iter().filter(|w| !is_line_list(w)).cloned().collect();

    // Highlight the line under the cursor (code, not diagrams; not
    // click-through stages, whose lines differ per step).
    if !diagram && !staged && line > fence.open && line < fence.close {
        let n = line - fence.open;
        let mut toggled = lines.clone();
        let title = if toggled.remove(&n) {
            format!("Code: stop highlighting line {n}")
        } else {
            toggled.insert(n);
            format!("Code: highlight line {n}")
        };
        out.push(style(
            "refactor.preso.code.highlightLine",
            title,
            rewrite(&toggled, others.clone()),
        ));
    }

    let with = |key: &str, value: Option<String>| -> Vec<String> {
        let mut new: Vec<String> = others
            .iter()
            .filter(|t| !t.starts_with(&format!("{key}=")))
            .cloned()
            .collect();
        new.extend(value.map(|v| format!("{key}={v}")));
        new
    };
    let width = others
        .iter()
        .find_map(|t| t.strip_prefix("width="))
        .and_then(|v| v.trim_end_matches('%').parse::<f32>().ok());
    for w in BLOCK_WIDTHS {
        if width != Some(f32::from(w)) {
            let kind = match w {
                50 => "refactor.preso.code.width.50",
                75 => "refactor.preso.code.width.75",
                _ => "refactor.preso.code.width.100",
            };
            out.push(style(
                kind,
                format!("{noun}: width {w}%"),
                rewrite(&lines, with("width", Some(format!("{w}%")))),
            ));
        }
    }
    // Stages can zoom as well as highlight (code) or name nodes (diagrams).
    if staged {
        let zooms = others.iter().any(|t| t == "zoom");
        let mut new: Vec<String> = others.iter().filter(|t| *t != "zoom").cloned().collect();
        if !zooms {
            new.push("zoom".to_string());
        }
        let title = match (diagram, zooms) {
            (_, true) => format!("{noun}: stop zooming"),
            (true, false) => "Diagram: zoom onto the nodes each stage names".to_string(),
            (false, false) => "Code: zoom onto each stage's lines".to_string(),
        };
        out.push(style(
            "refactor.preso.code.zoom",
            title,
            rewrite(&lines, new),
        ));
    }
    if diagram {
        let transparent = others.iter().any(|t| t == "transparent");
        let mut new: Vec<String> = others
            .iter()
            .filter(|t| *t != "transparent")
            .cloned()
            .collect();
        if !transparent {
            new.push("transparent".to_string());
        }
        let title = if transparent {
            "Diagram: card background (remove transparent)"
        } else {
            "Diagram: transparent background"
        };
        out.push(style(
            "refactor.preso.code.transparent",
            title,
            rewrite(&lines, new),
        ));
    } else {
        let align = others
            .iter()
            .find_map(|t| t.strip_prefix("align="))
            .unwrap_or("left");
        for (value, kind) in [
            ("center", "refactor.preso.code.align.center"),
            ("right", "refactor.preso.code.align.right"),
            ("left", "refactor.preso.code.align.left"),
        ] {
            if align != value {
                let v = (value != "left").then(|| value.to_string());
                out.push(style(
                    kind,
                    format!("Code: {}", align_title(value)),
                    rewrite(&lines, with("align", v)),
                ));
            }
        }
    }
}

/// `{2, 4, 5, 6}` → `2,4-6`.
fn compress(lines: &BTreeSet<usize>) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut iter = lines.iter().copied().peekable();
    while let Some(start) = iter.next() {
        let mut end = start;
        while iter.peek() == Some(&(end + 1)) {
            end = iter.next().unwrap_or(end);
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
    }
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::apply;

    fn titles(src: &str, line: usize, col: usize) -> Vec<String> {
        styles(src, line, col)
            .into_iter()
            .map(|s| s.title)
            .collect()
    }

    fn run(src: &str, line: usize, col: usize, title: &str) -> String {
        let s = styles(src, line, col)
            .into_iter()
            .find(|s| s.title == title)
            .unwrap_or_else(|| panic!("no {title:?} in {:?}", titles(src, line, col)));
        apply(src, &s.edits)
    }

    #[test]
    fn nothing_to_style_on_plain_text() {
        assert!(styles("# Title\n\nJust words.\n", 2, 0).is_empty());
        assert!(styles("<!-- note: ![x](y.png) -->\n", 0, 12).is_empty());
        assert!(styles("", 0, 0).is_empty());
    }

    #[test]
    fn image_widths_alignment_and_flags() {
        let src = "![Logo](logo.png)\n";
        let t = titles(src, 0, 3);
        assert!(t.contains(&"Image: width 50%".to_string()));
        assert!(t.contains(&"Image: centre".to_string()));
        assert!(!t.contains(&"Image: align left".to_string())); // the default
        assert!(!t.contains(&"Image: remove styling".to_string()));
        assert!(!t.iter().any(|t| t.contains("fit"))); // not in a row

        let styled = run(src, 0, 3, "Image: width 50%");
        assert_eq!(styled, "![Logo](logo.png){width=50%}\n");
        let styled = run(&styled, 0, 3, "Image: centre");
        assert_eq!(styled, "![Logo](logo.png){width=50% align=center}\n");
        let styled = run(&styled, 0, 3, "Image: add shadow");
        assert_eq!(styled, "![Logo](logo.png){width=50% align=center shadow}\n");
        let styled = run(&styled, 0, 3, "Image: width 75%");
        assert_eq!(styled, "![Logo](logo.png){width=75% align=center shadow}\n");
        let styled = run(&styled, 0, 3, "Image: align left");
        assert_eq!(styled, "![Logo](logo.png){width=75% shadow}\n");
        assert_eq!(run(&styled, 0, 3, "Image: remove styling"), src);
        // The parser reads what was written.
        let slide = &crate::parser::parse(&styled).unwrap().slides[0];
        assert!(
            slide.source.contains("#preso-img=width:75+shadow"),
            "{}",
            slide.source
        );
    }

    #[test]
    fn the_image_under_the_cursor_is_the_one_styled() {
        let src = "![a](a.png) text ![b](b.png){border} more\n";
        let out = run(src, 0, 20, "Image: remove border");
        assert_eq!(out, "![a](a.png) text ![b](b.png) more\n");
    }

    #[test]
    fn fit_is_offered_in_image_rows() {
        let src = "![a](a.png)\n![b](b.png)\n";
        let out = run(src, 1, 0, "Image: row at natural widths (fit)");
        assert_eq!(out, "![a](a.png)\n![b](b.png){fit}\n");
    }

    const TABLE: &str = "\
Intro

| Phase | Status |
|-------|:------:|
| Parse | done   |
";

    #[test]
    fn table_text_size() {
        let out = run(TABLE, 4, 0, "Table: text size 24");
        assert_eq!(
            out,
            TABLE.replace("\n| Phase", "\n<!-- table: size=24 -->\n| Phase")
        );
        let out2 = run(&out, 5, 0, "Table: text size 20");
        assert!(out2.contains("<!-- table: size=20 -->\n| Phase"));
        assert!(!titles(&out, 5, 0).contains(&"Table: text size 24".to_string()));
        assert_eq!(run(&out, 5, 0, "Table: normal text size"), TABLE);
        let slide = &crate::parser::parse(&out2).unwrap().slides[0];
        assert_eq!(slide.tables[0].font_size, Some(20.0));
    }

    #[test]
    fn table_column_alignment_follows_the_cursor() {
        // Cursor in the first column of a body row.
        let t = titles(TABLE, 4, 3);
        assert!(
            t.contains(&"Table: centre column 1 (Phase)".to_string()),
            "{t:?}"
        );
        assert!(
            t.contains(&"Table: align column 1 right (Phase)".to_string()),
            "{t:?}"
        );
        let out = run(TABLE, 4, 3, "Table: align column 1 right (Phase)");
        assert!(out.contains("|------:|:------:|"), "{out}");
        // The second column is centred already.
        let t = titles(TABLE, 4, 12);
        assert!(!t.iter().any(|t| t.contains("centre column 2")), "{t:?}");
        let out = run(TABLE, 4, 12, "Table: align column 2 left (Status)");
        assert!(out.contains("|-------|--------|"), "{out}");
        let slide = &crate::parser::parse(&out).unwrap().slides[0];
        assert_eq!(slide.tables[0].aligns[1], crate::TableAlign::Left);
    }

    #[test]
    fn a_size_directive_belongs_to_the_next_table_only() {
        let src = "<!-- table: size=20 -->\n| a |\n|---|\n| 1 |\n\n| b |\n|---|\n| 2 |\n";
        // The second table has no size of its own.
        assert!(!titles(src, 5, 0).contains(&"Table: normal text size".to_string()));
        let out = run(src, 5, 0, "Table: text size 28");
        assert!(
            out.contains("| 1 |\n\n<!-- table: size=28 -->\n| b |"),
            "{out}"
        );
    }

    #[test]
    fn code_line_highlighting_toggles() {
        let src = "```rust {2,4-6 size=20}\na\nb\nc\nd\ne\nf\n```\n";
        let out = run(src, 3, 0, "Code: highlight line 3");
        assert_eq!(out.lines().next(), Some("```rust {2-6 size=20}"));
        let out = run(src, 5, 0, "Code: stop highlighting line 5");
        assert_eq!(out.lines().next(), Some("```rust {2,4,6 size=20}"));
        let slide = &crate::parser::parse(&out).unwrap().slides[0];
        let lines = slide.code_blocks[0].highlighted_lines().unwrap();
        assert_eq!(lines.into_iter().collect::<Vec<_>>(), vec![2, 4, 6]);
        // Not on the fence lines themselves.
        assert!(!titles(src, 0, 0).iter().any(|t| t.contains("highlight")));
    }

    #[test]
    fn code_width_and_alignment() {
        let src = "```python\nprint()\n```\n";
        let out = run(src, 1, 0, "Code: centre");
        assert_eq!(out.lines().next(), Some("```python {align=center}"));
        let out = run(&out, 1, 0, "Code: width 100%");
        assert_eq!(
            out.lines().next(),
            Some("```python {align=center width=100%}")
        );
        assert_eq!(
            run(&out, 1, 0, "Code: align left").lines().next(),
            Some("```python {width=100%}")
        );
    }

    #[test]
    fn click_through_stages_are_left_alone() {
        let src = "```js {1|2-3|all}\na\nb\nc\n```\n";
        assert!(!titles(src, 2, 0).iter().any(|t| t.contains("highlight")));
    }

    #[test]
    fn diagrams_get_width_and_transparency() {
        let src = "```mermaid\ngraph LR; a-->b\n```\n";
        let t = titles(src, 1, 0);
        assert!(t.contains(&"Diagram: width 75%".to_string()));
        assert!(!t.iter().any(|t| t.starts_with("Code:")), "{t:?}");
        let out = run(src, 0, 0, "Diagram: transparent background");
        assert_eq!(out.lines().next(), Some("```mermaid {transparent}"));
        let slide = &crate::parser::parse(&out).unwrap().slides[0];
        assert!(slide.code_blocks[0].transparent_background());
    }

    #[test]
    fn restyling_keeps_zoom_stages_as_written() {
        // Commas separate a diagram stage's node labels, which may hold
        // spaces; restyling must not merge `Read, Parse input` into one label.
        let src = "```mermaid {all|Layout|Read, Parse input zoom}\ngraph LR\n  a[Read]\n```\n";
        for title in [
            "Diagram: width 50%",
            "Diagram: width 100%",
            "Diagram: transparent background",
        ] {
            let out = run(src, 1, 0, title);
            let fence = out.lines().next().unwrap();
            assert!(
                fence.contains("{all|Layout|Read, Parse input zoom "),
                "{title}: {fence}"
            );
            let slide = &crate::parser::parse(&out).unwrap().slides[0];
            assert_eq!(
                slide.code_blocks[0].zoom_labels_at(2),
                Some(vec!["Read".to_string(), "Parse input".to_string()]),
                "{title}"
            );
        }

        // Code zoom stages with comma-separated lines keep them too.
        let src = "```rust {all|2,4-5|6 zoom}\na\nb\nc\nd\ne\nf\n```\n";
        let out = run(src, 1, 0, "Code: centre");
        assert_eq!(
            out.lines().next(),
            Some("```rust {all|2,4-5|6 zoom align=center}")
        );
    }

    #[test]
    fn stages_can_zoom() {
        let src = "```rust {all|2|4-5}\na\nb\nc\nd\ne\n```\n";
        let out = run(src, 1, 0, "Code: zoom onto each stage's lines");
        assert_eq!(out.lines().next(), Some("```rust {all|2|4-5 zoom}"));
        let slide = &crate::parser::parse(&out).unwrap().slides[0];
        assert_eq!(slide.code_blocks[0].zoom_lines_at(2), Some((4, 5)));
        let back = run(&out, 1, 0, "Code: stop zooming");
        assert_eq!(back.lines().next(), Some("```rust {all|2|4-5}"));

        let src = "```mermaid {all|Read, Parse input}\ngraph LR\n```\n";
        let out = run(src, 1, 0, "Diagram: zoom onto the nodes each stage names");
        assert_eq!(
            out.lines().next(),
            Some("```mermaid {all|Read, Parse input zoom}")
        );
        // No stages, nothing to offer.
        assert!(
            !titles("```rust {2}\na\nb\n```\n", 1, 0)
                .iter()
                .any(|t| t.contains("zoom"))
        );
    }

    #[test]
    fn compress_line_sets() {
        let set = |v: &[usize]| v.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(compress(&set(&[1, 2, 3, 5, 7, 8])), "1-3,5,7-8");
        assert_eq!(compress(&set(&[])), "");
    }
}
