//! Block and inline structure of a (cleaned) preso slide source, for the
//! editable `.pptx` exporter. The input is `Slide::step_source` — the
//! parser has already lifted math/tables/image-rows into markers and
//! reduced fence info strings to the language — so this only has to
//! recognize block shapes and inline emphasis.

use preso_core::fence;

/// One block-level element of a slide, in document order.
#[derive(Debug, PartialEq)]
pub enum Block {
    /// ATX heading: level 1–6.
    Heading(u8, Vec<Run>),
    Paragraph(Vec<Run>),
    /// A run of list items (bullet or numbered), with nesting levels.
    List(Vec<ListItem>),
    /// Fenced code: language + verbatim lines.
    Code(Option<String>, Vec<String>),
    /// Blockquote: one entry per quoted line.
    Quote(Vec<Vec<Run>>),
    /// `![alt](url)` on its own line; `url` still carries any
    /// `#preso-img=` fragment.
    Image {
        url: String,
        alt: String,
    },
    /// `![math](preso-math:N)` marker → `Slide::math_blocks[N]`.
    Math(usize),
    /// `![table](preso-table:N)` marker → `Slide::tables[N]`.
    Table(usize),
    /// `![](preso-imagerow:N)` marker → `Slide::image_rows[N]`.
    ImageRow(usize),
    /// `![](preso-imagetext:N)` marker → `Slide::image_texts[N]`: images with
    /// the text beside them.
    ImageText {
        index: usize,
        /// `(level, ordered)` when the marker was a list item's whole
        /// content, so the text can keep its bullet.
        list: Option<(u8, bool)>,
    },
}

#[derive(Debug, PartialEq)]
pub struct ListItem {
    /// Nesting level, 0-based (from leading indentation).
    pub level: u8,
    /// Numbered (`1.`) rather than bulleted.
    pub ordered: bool,
    pub runs: Vec<Run>,
}

/// One inline run: text plus formatting.
#[derive(Debug, PartialEq, Clone)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: Option<String>,
}

impl Run {
    fn plain(text: &str) -> Self {
        Run {
            text: text.to_string(),
            bold: false,
            italic: false,
            code: false,
            link: None,
        }
    }
}

/// Parse a cleaned slide (or column) source into blocks.
pub fn parse_blocks(source: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut fence_state = fence::Tracker::default();
    let mut code: Option<(Option<String>, Vec<String>)> = None;
    let mut para: Vec<String> = Vec::new();
    let mut list: Vec<ListItem> = Vec::new();
    let mut quote: Vec<Vec<Run>> = Vec::new();

    fn flush_para(para: &mut Vec<String>, blocks: &mut Vec<Block>) {
        if !para.is_empty() {
            let joined = para.join(" ");
            blocks.push(Block::Paragraph(parse_inlines(&joined)));
            para.clear();
        }
    }
    fn flush_list(list: &mut Vec<ListItem>, blocks: &mut Vec<Block>) {
        if !list.is_empty() {
            blocks.push(Block::List(std::mem::take(list)));
        }
    }
    fn flush_quote(quote: &mut Vec<Vec<Run>>, blocks: &mut Vec<Block>) {
        if !quote.is_empty() {
            blocks.push(Block::Quote(std::mem::take(quote)));
        }
    }

    for line in source.lines() {
        let was_in = fence_state.in_fence();
        if fence_state.process(line) {
            if !was_in {
                // Opening fence: flush and start collecting.
                flush_para(&mut para, &mut blocks);
                flush_list(&mut list, &mut blocks);
                flush_quote(&mut quote, &mut blocks);
                let info = line.trim_start().trim_start_matches(['`', '~']).trim();
                let language = (!info.is_empty()).then(|| info.to_string());
                code = Some((language, Vec::new()));
            } else if fence_state.in_fence() {
                if let Some((_, lines)) = &mut code {
                    lines.push(line.to_string());
                }
            } else {
                // Closing fence.
                if let Some((lang, lines)) = code.take() {
                    blocks.push(Block::Code(lang, lines));
                }
            }
            continue;
        }

        let trimmed = line.trim();

        // Blank line (or the parser's non-breaking-space list spacer):
        // block boundary.
        if trimmed.is_empty() || trimmed.chars().all(|c| c == '\u{a0}') {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut list, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            continue;
        }

        // Markers the preso parser left in the source.
        if let Some(block) = marker(trimmed) {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut list, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            blocks.push(block);
            continue;
        }

        // ATX heading.
        if let Some((level, text)) = heading(trimmed) {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut list, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            blocks.push(Block::Heading(level, parse_inlines(text)));
            continue;
        }

        // Blockquote line.
        if let Some(rest) = trimmed.strip_prefix('>') {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut list, &mut blocks);
            quote.push(parse_inlines(rest.trim_start()));
            continue;
        }

        // A list item that is nothing but an image-text marker: its own
        // block, since the picture needs placing beside its own text.
        if let Some(block) = list_marker(line) {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut list, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            blocks.push(block);
            continue;
        }

        // List item (bulleted or numbered), nesting from indentation.
        if let Some(item) = list_item(line) {
            flush_para(&mut para, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            list.push(item);
            continue;
        }

        // Whole-line image (possibly a marker fragment on a real path).
        if let Some((alt, url)) = whole_line_image(trimmed) {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut list, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            blocks.push(Block::Image {
                url: url.to_string(),
                alt: alt.to_string(),
            });
            continue;
        }

        // Continuation of a list item?
        if !list.is_empty()
            && line.starts_with([' ', '\t'])
            && let Some(last) = list.last_mut()
        {
            last.runs.push(Run::plain(" "));
            last.runs.extend(parse_inlines(trimmed));
            continue;
        }

        flush_list(&mut list, &mut blocks);
        flush_quote(&mut quote, &mut blocks);
        para.push(trimmed.to_string());
    }
    flush_para(&mut para, &mut blocks);
    flush_list(&mut list, &mut blocks);
    flush_quote(&mut quote, &mut blocks);
    if let Some((lang, lines)) = code.take() {
        blocks.push(Block::Code(lang, lines)); // unterminated fence
    }
    blocks
}

/// `![math](preso-math:N)` / `![table](preso-table:N)` /
/// `![](preso-imagerow:N)` / `![](preso-imagetext:N)` → the corresponding
/// marker block.
fn marker(trimmed: &str) -> Option<Block> {
    let (_, url) = whole_line_image(trimmed)?;
    if let Some(n) = url.strip_prefix("preso-math:") {
        return n.parse().ok().map(Block::Math);
    }
    if let Some(n) = url.strip_prefix("preso-table:") {
        return n.parse().ok().map(Block::Table);
    }
    if let Some(n) = url.strip_prefix("preso-imagerow:") {
        return n.parse().ok().map(Block::ImageRow);
    }
    if let Some(n) = url.strip_prefix("preso-imagetext:") {
        return n
            .parse()
            .ok()
            .map(|index| Block::ImageText { index, list: None });
    }
    None
}

/// A list item whose whole content is an image-text marker — the parser
/// leaves the bullet in place and lifts only what follows it. The item can't
/// stay in the list shape (a picture has to be placed beside its own text),
/// so it becomes a block of its own that remembers the bullet it had.
fn list_marker(line: &str) -> Option<Block> {
    let (marker_len, level, ordered) = list_prefix(line)?;
    let rest = line.trim_start()[marker_len..].trim();
    match marker(rest) {
        Some(Block::ImageText { index, .. }) => Some(Block::ImageText {
            index,
            list: Some((level, ordered)),
        }),
        _ => None,
    }
}

fn heading(trimmed: &str) -> Option<(u8, &str)> {
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &trimmed[hashes..];
    rest.strip_prefix(' ')
        .map(|text| (hashes as u8, text.trim()))
        .or_else(|| rest.is_empty().then_some((hashes as u8, "")))
}

fn list_item(line: &str) -> Option<ListItem> {
    let (marker_len, level, ordered) = list_prefix(line)?;
    Some(ListItem {
        level,
        ordered,
        runs: parse_inlines(line.trim_start()[marker_len..].trim()),
    })
}

/// A list item's marker: `(its byte length, nesting level, ordered)`.
/// Measured from the trimmed line, with the level from the indentation.
fn list_prefix(line: &str) -> Option<(usize, u8, bool)> {
    let indent = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();
    let level = (indent / 2).min(4) as u8;
    if ["- ", "* ", "+ "].iter().any(|m| trimmed.starts_with(m)) {
        return Some((2, level, false));
    }
    // `1. ` numbered items.
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && trimmed[digits..].starts_with(". ") {
        return Some((digits + 2, level, true));
    }
    None
}

fn whole_line_image(trimmed: &str) -> Option<(&str, &str)> {
    let rest = trimmed.strip_prefix("![")?;
    let close = rest.find("](")?;
    let url = rest[close + 2..].strip_suffix(')')?;
    // Exactly one image on the line.
    (!url.contains("](")).then_some((&rest[..close], url))
}

/// Parse inline markdown into formatted runs: `` `code` ``, `**bold**`,
/// `*italic*` / `_italic_`, and `[text](url)` links. Code spans are
/// tokenized first (their content is verbatim); emphasis nests.
pub fn parse_inlines(text: &str) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        if let Some(len) = rest[start + 1..].find('`') {
            emphasis(&rest[..start], false, false, &mut runs);
            let span = &rest[start + 1..start + 1 + len];
            // A sentinel tag means this "code" span is really `==marked==`
            // text (see preso-core's `replace_marks`); PowerPoint has no
            // portable run highlight, so it degrades to a plain run.
            let (text, code) = match span.strip_prefix(preso_core::parser::MARK_SENTINEL) {
                Some(marked) => (marked, false),
                None => (span, true),
            };
            runs.push(Run {
                text: text.to_string(),
                bold: false,
                italic: false,
                code,
                link: None,
            });
            rest = &rest[start + len + 2..];
        } else {
            break;
        }
    }
    emphasis(rest, false, false, &mut runs);
    // Markdown's backslash escapes have done their job by now — they kept the
    // parser off punctuation the author meant literally (`\$100`, `\_word\_`,
    // which is exactly what the PowerPoint *importer* writes). PowerPoint has
    // no such convention, so the backslash has to go or it shows up on the
    // slide. Code spans are verbatim and keep theirs.
    for run in &mut runs {
        if !run.code {
            run.text = unescape_md(&run.text);
        }
    }
    runs.retain(|r| !r.text.is_empty());
    if runs.is_empty() {
        runs.push(Run::plain(""));
    }
    runs
}

/// Drop the backslash from a markdown escape — `\<punctuation>` → that
/// character. A backslash before anything else isn't an escape and stands.
fn unescape_md(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.clone().next() {
            Some(next) if next.is_ascii_punctuation() => {
                out.push(next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// Emphasis + links within a code-free segment. The earliest construct in
/// the text wins, so `**[x](y)**` (emphasis outside) and `[**x**](y)`
/// (emphasis inside) both nest correctly.
fn emphasis(text: &str, bold: bool, italic: bool, out: &mut Vec<Run>) {
    let link = find_link(text);
    let span = find_span(text);
    let link_first = match (&link, &span) {
        (Some(l), Some(s)) => l.0 < s.0,
        (Some(_), None) => true,
        _ => false,
    };
    if link_first {
        let (start, label, url, after) = link.expect("checked");
        emphasis(&text[..start], bold, italic, out);
        let before = out.len();
        emphasis(label, bold, italic, out);
        for run in &mut out[before..] {
            run.link = Some(url.to_string());
        }
        emphasis(after, bold, italic, out);
        return;
    }
    if let Some((start, inner, after, is_bold)) = span {
        emphasis(&text[..start], bold, italic, out);
        emphasis(inner, bold || is_bold, italic || !is_bold, out);
        emphasis(after, bold, italic, out);
        return;
    }
    if !text.is_empty() {
        out.push(Run {
            text: text.to_string(),
            bold,
            italic,
            code: false,
            link: None,
        });
    }
}

/// `[label](url)` → `(start, label, url, rest-after)`.
fn find_link(text: &str) -> Option<(usize, &str, &str, &str)> {
    let start = text.find('[')?;
    let mid = text[start..].find("](")?;
    let end = text[start + mid + 2..].find(')')?;
    Some((
        start,
        &text[start + 1..start + mid],
        &text[start + mid + 2..start + mid + 2 + end],
        &text[start + mid + 3 + end..],
    ))
}

/// Earliest closed emphasis span → `(start, inner, rest-after, is_bold)`.
/// `**` is tried before `*` so they can't be confused at the same offset.
fn find_span(text: &str) -> Option<(usize, &str, &str, bool)> {
    let mut best: Option<(usize, &str, &str, bool)> = None;
    for (open, is_bold) in [("**", true), ("*", false), ("_", false)] {
        if let Some(start) = find_delimiter(text, open, 0)
            && let Some(len) =
                find_delimiter(text, open, start + open.len()).map(|at| at - start - open.len())
            && len > 0
            && best.as_ref().is_none_or(|b| start < b.0)
        {
            best = Some((
                start,
                &text[start + open.len()..start + open.len() + len],
                &text[start + open.len() + len + open.len()..],
                is_bold,
            ));
        }
    }
    best
}

/// Byte offset of the next `delimiter` at or after `from` that really marks
/// emphasis. Two things disqualify one, both of which the PowerPoint
/// *importer* relies on when it writes markdown back out:
///
/// - a backslash before it — `\_word\_` is a literal underscore either side;
/// - for `_` only, alphanumerics on both sides. CommonMark's flanking rules
///   don't let an underscore inside a word open or close emphasis, which is
///   what keeps `snake_case_fn` from turning into "snake *case* fn".
fn find_delimiter(text: &str, delimiter: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut at = from;
    while let Some(found) = text.get(at..)?.find(delimiter) {
        let start = at + found;
        let escaped = bytes[..start]
            .iter()
            .rev()
            .take_while(|&&b| b == b'\\')
            .count()
            % 2
            == 1;
        let intraword = delimiter == "_"
            && text[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
            && text[start + delimiter.len()..]
                .chars()
                .next()
                .is_some_and(char::is_alphanumeric);
        if !escaped && !intraword {
            return Some(start);
        }
        at = start + delimiter.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_split_and_classify() {
        let src = "# Title\n\nA **bold** para\nwith a wrap.\n\n- one\n- two\n  - nested\n1. first\n\n```rust\nfn main() {}\n```\n\n> quoted\n\n![math](preso-math:0)\n![alt](x.png#preso-img=width:40)\n";
        let blocks = parse_blocks(src);
        assert!(matches!(&blocks[0], Block::Heading(1, _)));
        assert!(matches!(&blocks[1], Block::Paragraph(runs) if runs.len() == 3));
        let Block::List(items) = &blocks[2] else {
            panic!("list, got {:?}", blocks[2])
        };
        assert_eq!(items.len(), 4);
        assert_eq!(items[2].level, 1);
        assert!(items[3].ordered);
        assert!(
            matches!(&blocks[3], Block::Code(Some(l), lines) if l == "rust" && lines.len() == 1)
        );
        assert!(matches!(&blocks[4], Block::Quote(lines) if lines.len() == 1));
        assert!(matches!(&blocks[5], Block::Math(0)));
        assert!(matches!(&blocks[6], Block::Image { url, .. } if url.starts_with("x.png")));
    }

    #[test]
    fn escaped_and_intraword_punctuation_is_not_emphasis() {
        let text = |runs: &[Run]| runs.iter().map(|r| r.text.as_str()).collect::<String>();

        // What the PowerPoint importer writes for literal punctuation has to
        // survive the trip back out: the backslash goes (PowerPoint has no
        // such convention) and nothing turns italic on the way.
        let runs = parse_inlines("the \\_emphasis\\_ case");
        assert_eq!(text(&runs), "the _emphasis_ case");
        assert!(runs.iter().all(|r| !r.italic));

        // An underscore inside a word never opened emphasis in markdown, and
        // must not here either.
        let runs = parse_inlines("call snake_case_fn now");
        assert_eq!(text(&runs), "call snake_case_fn now");
        assert!(runs.iter().all(|r| !r.italic));

        // Escaped dollars lose their backslash too (preso-core leaves it in
        // the source for the markdown renderer, which PowerPoint isn't).
        assert_eq!(
            text(&parse_inlines("costs \\$100-\\$200")),
            "costs $100-$200"
        );

        // Emphasis at a word boundary still works, as does bold.
        let runs = parse_inlines("an _italic_ and **bold** word");
        assert!(runs.iter().any(|r| r.text == "italic" && r.italic));
        assert!(runs.iter().any(|r| r.text == "bold" && r.bold));

        // A code span is verbatim: its backslashes are content.
        let runs = parse_inlines("run `printf \\$1` here");
        assert!(runs.iter().any(|r| r.code && r.text == "printf \\$1"));
    }

    #[test]
    fn image_text_markers_become_blocks() {
        // Standing on its own, and as a list item's whole content — the
        // parser leaves the bullet in place and lifts only what follows it.
        let blocks = parse_blocks(
            "![](preso-imagetext:0)\n\n- ![](preso-imagetext:1)\n  1. ![](preso-imagetext:2)\n- plain\n",
        );
        assert!(matches!(
            &blocks[0],
            Block::ImageText {
                index: 0,
                list: None
            }
        ));
        assert!(matches!(
            &blocks[1],
            Block::ImageText {
                index: 1,
                list: Some((0, false))
            }
        ));
        // The bullet it had comes with it, nesting level and all.
        assert!(matches!(
            &blocks[2],
            Block::ImageText {
                index: 2,
                list: Some((1, true))
            }
        ));
        // An ordinary item after one still forms a list.
        assert!(matches!(&blocks[3], Block::List(items) if items.len() == 1));
    }

    #[test]
    fn inline_formatting_combines() {
        let runs = parse_inlines("plain **bold** *it* `code` [link](https://x)");
        let texts: Vec<(&str, bool, bool, bool, bool)> = runs
            .iter()
            .map(|r| (r.text.as_str(), r.bold, r.italic, r.code, r.link.is_some()))
            .collect();
        assert!(texts.contains(&("bold", true, false, false, false)));
        assert!(texts.contains(&("it", false, true, false, false)));
        assert!(texts.contains(&("code", false, false, true, false)));
        assert!(texts.contains(&("link", false, false, false, true)));
        // Nested: bold link label keeps both.
        let runs = parse_inlines("**[x](u)**");
        assert!(
            runs.iter()
                .any(|r| r.text == "x" && r.bold && r.link.is_some())
        );
    }

    #[test]
    fn paragraph_lines_join_and_spacers_split() {
        let blocks = parse_blocks("line one\nline two\n\n\u{a0}\n\nnext\n");
        assert_eq!(blocks.len(), 2);
        assert!(
            matches!(&blocks[0], Block::Paragraph(r) if r[0].text.contains("line one line two"))
        );
    }
}
