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
/// `![](preso-imagerow:N)` → the corresponding marker block.
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
    None
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
    let indent = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();
    let level = (indent / 2).min(4) as u8;
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return Some(ListItem {
            level,
            ordered: false,
            runs: parse_inlines(rest.trim()),
        });
    }
    // `1. ` numbered items.
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0
        && let Some(rest) = trimmed[digits..].strip_prefix(". ")
    {
        return Some(ListItem {
            level,
            ordered: true,
            runs: parse_inlines(rest.trim()),
        });
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
    runs.retain(|r| !r.text.is_empty());
    if runs.is_empty() {
        runs.push(Run::plain(""));
    }
    runs
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
        if let Some(start) = text.find(open)
            && let Some(len) = text[start + open.len()..].find(open)
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
