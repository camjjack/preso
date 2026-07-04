use crate::error::ParseError;
use crate::fence;
use crate::model::{
    Anchor, CodeBlock, Frontmatter, ImageRef, ImageRow, LayerImage, Layout, MathBlock, Note, Slide,
    SlideOverrides, Table, TableAlign,
};

/// Result of parsing a deck source file.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedDeck {
    pub frontmatter: Frontmatter,
    pub slides: Vec<Slide>,
}

/// Parse a full markdown file into frontmatter + slides.
///
/// Rules (spec §6.2):
/// - YAML frontmatter: `---` on the very first line, closed by `---` or `...`.
/// - Slide delimiter: a line that is exactly `---` (trailing whitespace
///   allowed), **outside fenced code blocks**.
/// - Code fences per CommonMark: three or more backticks or tildes, up to
///   three spaces of indentation; closed by a fence of the same character
///   at least as long.
/// - `<!-- note: -->` / `<!-- speaker: -->` comments become speaker notes;
///   `<!-- note[n]: -->` attaches to reveal step `n`.
/// - `<!-- pause -->` / `<!-- v-click -->` on its own line splits a slide
///   into cumulative reveal steps.
/// - All of the above are literal text inside code fences.
pub fn parse(source: &str) -> Result<ParsedDeck, ParseError> {
    let (frontmatter, body_start) = extract_frontmatter(source)?;
    // Slides marked `<!-- slide: hidden -->` are dropped entirely, so they
    // appear in neither the presentation nor the exported PDF.
    let slides = split_slides(source, body_start)
        .into_iter()
        .map(|raw| process_slide(&raw.source, raw.start_line))
        .filter(|slide| !slide.overrides.hidden)
        .collect();
    Ok(ParsedDeck {
        frontmatter,
        slides,
    })
}

/// Extract frontmatter if the first line is `---`.
/// Returns the parsed frontmatter and the 0-based line index where the
/// slide body begins.
fn extract_frontmatter(source: &str) -> Result<(Frontmatter, usize), ParseError> {
    let mut lines = source.lines();
    let Some(first) = lines.next() else {
        return Ok((Frontmatter::default(), 0));
    };
    if first.trim_end() != "---" {
        return Ok((Frontmatter::default(), 0));
    }

    let mut yaml = String::new();
    for (i, line) in lines.enumerate() {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            let fm = if yaml.trim().is_empty() {
                Frontmatter::default()
            } else {
                serde_norway::from_str(&yaml)?
            };
            // body begins after the closing delimiter: line 0 is `---`,
            // lines 1..=i are yaml, line i+1 is the closer
            return Ok((fm, i + 2));
        }
        yaml.push_str(line);
        yaml.push('\n');
    }

    // A file that *opens* with `---` but never closes it is treated as a
    // deck whose first slide starts with a delimiter, not as an error:
    // this is what an author mid-edit most likely means.
    Ok((Frontmatter::default(), 0))
}

struct RawSlide {
    source: String,
    start_line: usize,
}

/// Split the body (starting at `body_start`, 0-based line index) into slides.
fn split_slides(source: &str, body_start: usize) -> Vec<RawSlide> {
    let mut slides = Vec::new();
    let mut current = String::new();
    // 1-based line where the current slide began
    let mut current_start = body_start + 1;
    // Fence tracking so `---` inside a fenced code block is ignored.
    let mut fence = fence::Tracker::default();

    for (i, line) in source.lines().enumerate().skip(body_start) {
        if !fence.process(line) && line.trim_end() == "---" {
            push_raw(&mut slides, &mut current, current_start);
            current_start = i + 2; // next line, 1-based
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }
    push_raw(&mut slides, &mut current, current_start);

    // An entirely empty file still yields one (empty) slide so the app
    // always has something to display during hot reload of a new file.
    if slides.is_empty() {
        slides.push(RawSlide {
            source: String::new(),
            start_line: body_start + 1,
        });
    }
    slides
}

fn push_raw(slides: &mut Vec<RawSlide>, current: &mut String, start_line: usize) {
    let source = std::mem::take(current);
    // Drop empty segments produced by leading/consecutive delimiters.
    if !source.trim().is_empty() {
        slides.push(RawSlide { source, start_line });
    }
}

/// Extract notes and reveal steps from a raw slide source.
fn process_slide(raw: &str, start_line: usize) -> Slide {
    // Content chunks between pause markers; chunks[i] is what step i *adds*.
    let mut chunks: Vec<String> = vec![String::new()];
    let mut notes: Vec<Note> = Vec::new();
    let mut code_blocks: Vec<CodeBlock> = Vec::new();
    let mut math_blocks: Vec<MathBlock> = Vec::new();
    let mut tables: Vec<Table> = Vec::new();
    let mut image_rows: Vec<ImageRow> = Vec::new();
    let mut layer_images: Vec<LayerImage> = Vec::new();
    let mut layout = Layout::default();
    let mut overrides = SlideOverrides::default();
    let mut footnote: Option<String> = None;
    let mut video: Option<String> = None;
    // Set by `<!-- table: size=NN -->`, consumed by the next table lifted.
    let mut pending_table_size: Option<f32> = None;
    let mut fence = fence::Tracker::default();
    // Some(step) while inside a multi-line note comment
    let mut open_note: Option<(Option<usize>, String)> = None;
    // Some(latex-so-far) while inside a `$$ ... $$` block
    let mut open_math: Option<String> = None;

    // Peekable so table detection can look ahead for the delimiter row and
    // consume the body rows; `continue` still advances normally.
    let mut lines = raw.lines().peekable();
    while let Some(line) = lines.next() {
        // Inside a fence everything is literal, including comments.
        if fence.in_fence() {
            fence.process(line); // may close the fence
            push_line(chunks.last_mut().expect("non-empty"), line);
            continue;
        }

        // Continuation of a multi-line note comment.
        if let Some((step, mut text)) = open_note.take() {
            match line.find("-->") {
                Some(end) => {
                    text.push(' ');
                    text.push_str(line[..end].trim());
                    notes.push(Note {
                        step,
                        text: text.trim().to_string(),
                    });
                }
                None => {
                    text.push(' ');
                    text.push_str(line.trim());
                    open_note = Some((step, text));
                }
            }
            continue;
        }

        // Continuation of a `$$` display-math block.
        if let Some(mut latex) = open_math.take() {
            if line.trim() == "$$" {
                push_math(
                    &mut math_blocks,
                    chunks.last_mut().expect("non-empty"),
                    latex,
                );
            } else {
                latex.push_str(line);
                latex.push('\n');
                open_math = Some(latex);
            }
            continue;
        }

        let trimmed = line.trim();
        if trimmed == "<!-- pause -->" || trimmed == "<!-- v-click -->" {
            chunks.push(String::new());
            continue;
        }
        if let Some(note) = parse_note_open(trimmed) {
            match note {
                NoteOpen::Complete(step, text) => notes.push(Note { step, text }),
                NoteOpen::Continued(step, text) => open_note = Some((step, text)),
            }
            continue;
        }
        if let Some(spec) = directive(trimmed, "layout") {
            layout = parse_layout(spec.trim());
            continue;
        }
        // Per-slide style overrides: `<!-- slide: align=center background=#112233 -->`
        if let Some(spec) = directive(trimmed, "slide") {
            for token in spec.split_whitespace() {
                match token.split_once('=') {
                    Some(("align", value)) => overrides.align = Some(value.to_string()),
                    Some(("halign", value)) => overrides.halign = Some(value.to_string()),
                    Some(("background", value)) => overrides.background = Some(value.to_string()),
                    Some(("kind", value)) => overrides.kind = Some(value.to_string()),
                    Some(("transition", value)) => overrides.transition = Some(value.to_string()),
                    Some(("number", value)) => overrides.number = value.parse().ok(),
                    Some(_) => {}
                    // Bare flags (no `=`).
                    None => {
                        if token == "hidden" {
                            overrides.hidden = true;
                        }
                    }
                }
            }
            continue;
        }
        // Per-slide footnote: a small disclaimer line (e.g. image credits).
        // Plain text; multiple directives on a slide are joined with a space.
        if let Some(text) = directive(trimmed, "footnote") {
            let text = text.trim();
            if !text.is_empty() {
                match &mut footnote {
                    Some(existing) => {
                        existing.push(' ');
                        existing.push_str(text);
                    }
                    None => footnote = Some(text.to_string()),
                }
            }
            continue;
        }
        // Per-slide video: `<!-- video: clip.mp4 -->` marks the slide as
        // playable. preso can't decode video frames on the tiny-skia backend,
        // so playback launches an external fullscreen player (see the app's
        // `video` module); the slide itself just shows a ▶ affordance. Last
        // directive wins if several appear on one slide.
        if let Some(spec) = directive(trimmed, "video") {
            let path = spec.trim();
            if !path.is_empty() {
                video = Some(path.to_string());
            }
            continue;
        }
        // Table directive: `<!-- table: size=NN -->` applies to the next
        // table on the slide (its cell font size, in design units).
        if let Some(spec) = directive(trimmed, "table") {
            for token in spec.split_whitespace() {
                if let Some(("size", value)) = token.split_once('=') {
                    pending_table_size = value.parse().ok().filter(|&n: &f32| n > 0.0);
                }
            }
            continue;
        }
        // Decoration image: `<!-- image: path position=center width=60 -->`
        // placed on a layer below the content (not in the text flow).
        if let Some(spec) = directive(trimmed, "image")
            && let Some(image) = parse_layer_image(spec)
        {
            layer_images.push(image);
            continue;
        }
        // Display math: `$$` opener (possibly one-line `$$ x $$`).
        if trimmed == "$$" {
            open_math = Some(String::new());
            continue;
        }
        if let Some(inner) = trimmed
            .strip_prefix("$$")
            .and_then(|r| r.strip_suffix("$$"))
            && !inner.is_empty()
        {
            push_math(
                &mut math_blocks,
                chunks.last_mut().expect("non-empty"),
                inner.trim().to_string(),
            );
            continue;
        }

        if fence.process(line) {
            // Opening fence: split the info string into language and
            // annotation. Renderers (and the iced markdown widget) choke
            // on "rust {2,4-6}" as a language, so the emitted line keeps
            // only the language token; the annotation is preserved in the
            // model for line highlighting (Phase 2).
            let (cleaned, block) = clean_fence_line(line);
            code_blocks.push(block);
            push_line(chunks.last_mut().expect("non-empty"), &cleaned);
            continue;
        }

        // GFM table: a header row immediately followed by a delimiter row
        // (`|---|:--:|`). preso renders tables itself (themeable), so we
        // lift them out and leave a `![table](preso-table:N)` marker.
        if line.contains('|') && lines.peek().is_some_and(|n| table_delimiter(n).is_some()) {
            let headers = table_cells(line);
            let aligns = table_delimiter(lines.next().expect("peeked")).expect("checked");
            let mut rows = Vec::new();
            while lines.peek().is_some_and(|n| is_table_row(n)) {
                rows.push(table_cells(lines.next().expect("peeked")));
            }
            push_line(
                chunks.last_mut().expect("non-empty"),
                &format!("![table](preso-table:{})", tables.len()),
            );
            tables.push(Table {
                headers,
                aligns,
                rows,
                font_size: pending_table_size.take(),
            });
            continue;
        }
        // Image row: a run of two or more adjacent image-only lines renders
        // side by side. A blank (or any non-image) line ends the run, so a
        // single image, or images separated by a blank line, still stack.
        if is_image_line(line) && lines.peek().is_some_and(|n| is_image_line(n)) {
            let mut run: Vec<&str> = vec![line];
            while lines.peek().is_some_and(|n| is_image_line(n)) {
                run.push(lines.next().expect("peeked"));
            }
            match run.iter().map(|l| image_ref(l)).collect::<Option<Vec<_>>>() {
                Some(images) => {
                    push_line(
                        chunks.last_mut().expect("non-empty"),
                        &format!("![](preso-imagerow:{})", image_rows.len()),
                    );
                    image_rows.push(ImageRow { images });
                }
                // One didn't parse cleanly: fall back to stacking them.
                None => {
                    for l in run {
                        push_line(
                            chunks.last_mut().expect("non-empty"),
                            &replace_inline_math(&rewrite_image_attrs(l)),
                        );
                    }
                }
            }
            continue;
        }
        push_line(
            chunks.last_mut().expect("non-empty"),
            &replace_inline_math(&rewrite_image_attrs(line)),
        );
    }

    // Unterminated `$$` at end of slide (author mid-edit): emit what we have.
    if let Some(latex) = open_math {
        push_math(
            &mut math_blocks,
            chunks.last_mut().expect("non-empty"),
            latex,
        );
    }

    // Unterminated note at end of slide: keep what we have.
    if let Some((step, text)) = open_note {
        notes.push(Note {
            step,
            text: text.trim().to_string(),
        });
    }

    // Build cumulative step sources.
    let mut steps = Vec::with_capacity(chunks.len());
    let mut acc = String::new();
    for chunk in &chunks {
        acc.push_str(chunk);
        steps.push(acc.trim_end().to_string() + "\n");
    }
    // Collapse trailing empty steps (e.g. a pause marker at the very end).
    while steps.len() > 1 && steps[steps.len() - 1] == steps[steps.len() - 2] {
        steps.pop();
    }

    // A blank line between top-level bullets means "gap here": split the run
    // into separate lists so the renderer spaces them apart (see `group_lists`).
    let steps: Vec<String> = steps.iter().map(|s| group_lists(s)).collect();

    Slide {
        source: steps.last().expect("non-empty").clone(),
        start_line,
        notes,
        footnote,
        video,
        steps,
        code_blocks,
        math_blocks,
        tables,
        image_rows,
        layer_images,
        layout,
        overrides,
    }
}

/// Parse a `<!-- image: path position=center width=60 opacity=0.5 padding=40 -->`
/// spec (the part between `image:` and `-->`). The first token is the path;
/// the rest are `key=value`. `None` if there's no path.
fn parse_layer_image(spec: &str) -> Option<LayerImage> {
    let mut tokens = spec.split_whitespace();
    let path = tokens.next()?.to_string();
    let mut image = LayerImage {
        path,
        position: Anchor::Center,
        width: None,
        opacity: 1.0,
        padding: [0.0; 4],
    };
    // `padding=` sets all sides; `padding-{top,right,bottom,left}=` override.
    let mut uniform = 0.0_f32;
    let mut sides: [Option<f32>; 4] = [None; 4];
    let pad = |v: &str| v.parse::<f32>().ok().map(|p| p.max(0.0));
    for token in tokens {
        match token.split_once('=') {
            Some(("position", v)) => image.position = parse_anchor(v),
            Some(("width", v)) => {
                image.width = v
                    .trim_end_matches('%')
                    .parse()
                    .ok()
                    .filter(|&n: &f32| n > 0.0);
            }
            Some(("opacity", v)) => {
                if let Ok(o) = v.parse::<f32>() {
                    image.opacity = o.clamp(0.0, 1.0);
                }
            }
            Some(("padding", v)) => {
                if let Some(p) = pad(v) {
                    uniform = p;
                }
            }
            Some(("padding-top", v)) => sides[0] = pad(v),
            Some(("padding-right", v)) => sides[1] = pad(v),
            Some(("padding-bottom", v)) => sides[2] = pad(v),
            Some(("padding-left", v)) => sides[3] = pad(v),
            _ => {}
        }
    }
    image.padding = [
        sides[0].unwrap_or(uniform),
        sides[1].unwrap_or(uniform),
        sides[2].unwrap_or(uniform),
        sides[3].unwrap_or(uniform),
    ];
    Some(image)
}

fn parse_anchor(s: &str) -> Anchor {
    use Anchor::{Bottom, BottomLeft, BottomRight, Center, Left, Right, Top, TopLeft, TopRight};
    match s {
        "top-left" => TopLeft,
        "top" => Top,
        "top-right" => TopRight,
        "left" => Left,
        "right" => Right,
        "bottom-left" => BottomLeft,
        "bottom" => Bottom,
        "bottom-right" => BottomRight,
        _ => Center,
    }
}

/// Whether a line is a single standalone image — `![alt](url)`, optionally
/// with a `{…}` attribute group. (Exactly one `](`, so two images on one
/// line aren't mistaken for one.)
fn is_image_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("![") && t.matches("](").count() == 1 && (t.ends_with(')') || t.ends_with('}'))
}

/// Parse an image line into a [`ImageRef`], rewriting any `{…}` attributes
/// into the URL fragment first (so the renderer sees the same form as a
/// stacked image). `None` if it doesn't parse as `![alt](url)`.
fn image_ref(line: &str) -> Option<ImageRef> {
    let rewritten = rewrite_image_attrs(line.trim());
    let rest = rewritten.strip_prefix("![")?;
    let close = rest.find("](")?;
    let alt = rest[..close].to_string();
    let url = rest[close + 2..].strip_suffix(')')?.to_string();
    Some(ImageRef { url, alt })
}

fn push_line(buf: &mut String, line: &str) {
    buf.push_str(line);
    buf.push('\n');
}

/// Honour blank lines between top-level bullets as a visual gap, scaled by how
/// many blank lines the author wrote.
///
/// CommonMark merges blank-separated bullets into one "loose" list and the
/// renderer spaces every item equally — so a blank line has no per-item
/// effect. To get a gap *only where the author put a blank line* (and a bigger
/// gap for more blank lines), a run of `K` blank lines between two top-level
/// bullets is replaced with a spacer paragraph of `K` blank (non-breaking-
/// space) lines. That paragraph both ends the first list and provides `K`
/// lines of vertical space before the next one, so one blank line = one line
/// break, two blank lines = double, and items with no blank between them stay
/// tight. Nested sub-bullets (a blank then indented content) and ordered lists
/// pass through unchanged.
fn group_lists(source: &str) -> String {
    let mut out = String::new();
    let mut fence = fence::Tracker::default();
    let mut in_list = false;
    // Blank lines seen since the last list item, withheld until we know
    // whether they precede another bullet (a gap) or item content (loose item).
    let mut pending_blanks = 0usize;

    for line in source.lines() {
        // Leave fenced code untouched.
        let was_in_fence = fence.in_fence();
        if fence.process(line) {
            if !was_in_fence {
                // Opening fence: flush withheld blanks and end any list run.
                push_blank_lines(&mut out, pending_blanks);
                pending_blanks = 0;
                in_list = false;
            }
            push_line(&mut out, line);
            continue;
        }

        if line.trim().is_empty() {
            if in_list {
                pending_blanks += 1;
            } else {
                push_line(&mut out, line);
            }
            continue;
        }

        if top_level_bullet(line) && in_list && pending_blanks > 0 {
            // Group boundary: a spacer paragraph of `pending_blanks` empty
            // lines separates the lists and provides the gap.
            out.push('\n');
            for _ in 0..pending_blanks {
                out.push('\u{a0}');
                out.push('\n');
            }
            out.push('\n');
            pending_blanks = 0;
            push_line(&mut out, line);
        } else if top_level_bullet(line) {
            push_blank_lines(&mut out, pending_blanks);
            pending_blanks = 0;
            in_list = true;
            push_line(&mut out, line);
        } else if line.starts_with([' ', '\t']) {
            // Indented: nested bullet or item continuation — the blank was
            // within an item, not a group boundary, so keep it verbatim.
            push_blank_lines(&mut out, pending_blanks);
            pending_blanks = 0;
            push_line(&mut out, line);
        } else {
            // Any other top-level content ends the list.
            push_blank_lines(&mut out, pending_blanks);
            pending_blanks = 0;
            in_list = false;
            push_line(&mut out, line);
        }
    }
    push_blank_lines(&mut out, pending_blanks);
    out
}

fn push_blank_lines(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push('\n');
    }
}

/// Whether `line` is an unindented unordered bullet (`- `, `* `, `+ `).
fn top_level_bullet(line: &str) -> bool {
    line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ")
}

/// Split a `| a | b |` table row into trimmed cell strings, dropping the
/// empty cells produced by the optional leading/trailing pipes. `\|` is an
/// escaped pipe (GFM): it becomes a literal `|` inside the cell rather than a
/// column separator — needed even within a `` `code` `` span.
fn table_cells(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = line.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                chars.next();
                cur.push('|');
            }
            '|' => {
                cells.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    cells.push(cur.trim().to_string());
    if cells.first().is_some_and(String::is_empty) {
        cells.remove(0);
    }
    if cells.last().is_some_and(String::is_empty) {
        cells.pop();
    }
    cells
}

/// If `line` is a GFM table delimiter (`|---|:--:|---:|`), return the
/// per-column alignment; otherwise `None`. Requires a pipe so a bare `---`
/// (never present inside a slide anyway) can't be mistaken for one.
fn table_delimiter(line: &str) -> Option<Vec<TableAlign>> {
    if !line.contains('|') {
        return None;
    }
    let cells = table_cells(line);
    if cells.is_empty() {
        return None;
    }
    cells
        .iter()
        .map(|cell| {
            let left = cell.starts_with(':');
            let right = cell.ends_with(':');
            let dashes = cell.trim_matches(':');
            (!dashes.is_empty() && dashes.bytes().all(|b| b == b'-')).then_some(
                match (left, right) {
                    (true, true) => TableAlign::Center,
                    (false, true) => TableAlign::Right,
                    _ => TableAlign::Left,
                },
            )
        })
        .collect()
}

/// A line that can be a table body row: non-blank and containing a pipe.
fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && t.contains('|')
}

/// Record a display math block and emit its image marker into the source.
fn push_math(math_blocks: &mut Vec<MathBlock>, buf: &mut String, latex: String) {
    let latex = latex.trim().to_string();
    if latex.is_empty() {
        return;
    }
    push_line(buf, &format!("![math](preso-math:{})", math_blocks.len()));
    math_blocks.push(MathBlock { latex });
}

/// If `trimmed` is a `<!-- name: … -->` directive comment, return the text
/// between the colon and the `-->` (untrimmed). Whitespace after `<!--` is
/// optional and flexible — `<!--layout:` and `<!--  layout:` both match —
/// the same tolerance note comments have always had via [`parse_note_open`].
fn directive<'a>(trimmed: &'a str, name: &str) -> Option<&'a str> {
    let body = trimmed.strip_prefix("<!--")?.trim_start();
    let rest = body.strip_prefix(name)?.strip_prefix(':')?;
    rest.strip_suffix("-->")
}

/// Parse a `<!-- layout: ... -->` spec. `TwoColumn` accepts an optional
/// `left:right` (or `left/right`) ratio of positive integers, e.g.
/// `TwoColumn 2:1`; it defaults to `1:1`. Anything else is `Content`.
fn parse_layout(spec: &str) -> Layout {
    let mut parts = spec.split_whitespace();
    match parts.next() {
        Some("TwoColumn") => {
            let (left, right) = parts.next().and_then(parse_ratio).unwrap_or((1, 1));
            Layout::TwoColumn { left, right }
        }
        _ => Layout::Content,
    }
}

/// `"2:1"` / `"60/40"` → `(2, 1)` / `(60, 40)`. Both parts must be
/// positive integers, else `None` (and the ratio falls back to 1:1).
fn parse_ratio(s: &str) -> Option<(u16, u16)> {
    let (a, b) = s.split_once([':', '/'])?;
    let a: u16 = a.trim().parse().ok().filter(|&n| n > 0)?;
    let b: u16 = b.trim().parse().ok().filter(|&n| n > 0)?;
    Some((a, b))
}

/// Rewrite Pandoc-style image attributes into a URL fragment the
/// renderer understands, since pulldown-cmark has no attribute support:
/// `![alt](x.png){width=30% border shadow}` →
/// `![alt](x.png#preso-img=width:30+border+shadow)`.
/// Recognized: `width=NN%`, `align=left|center|right`, `border`, `shadow`,
/// `plain`, `fit` (row layout). Groups with any unrecognized or malformed
/// token are left untouched.
fn rewrite_image_attrs(line: &str) -> String {
    const MARKER: &str = "){";
    if !line.contains(MARKER) {
        return line.to_string();
    }
    let mut out = line.to_string();
    let mut from = 0;
    while let Some(found) = out[from..].find(MARKER) {
        let start = from + found;
        let attrs_start = start + MARKER.len();
        let Some(close) = out[attrs_start..].find('}') else {
            break;
        };
        let group_end = attrs_start + close + 1;
        match encode_image_attrs(&out[attrs_start..attrs_start + close]) {
            Some(encoded) => {
                let replacement = format!("#preso-img={encoded})");
                out.replace_range(start..group_end, &replacement);
                from = start + replacement.len();
            }
            None => {
                from = group_end;
            }
        }
    }
    out
}

/// `width=30% border shadow` → `width:30+border+shadow`; `None` if any
/// token is unrecognized (the group is then left as literal text).
fn encode_image_attrs(spec: &str) -> Option<String> {
    let mut encoded: Vec<String> = Vec::new();
    for token in spec.split_whitespace() {
        if let Some(value) = token.strip_prefix("width=") {
            let pct = value
                .strip_suffix('%')
                .and_then(|v| v.trim().parse::<f32>().ok())
                .filter(|p| *p > 0.0 && *p <= 100.0)?;
            encoded.push(format!("width:{pct}"));
        } else if let Some(value) = token.strip_prefix("align=") {
            if !matches!(value, "left" | "center" | "right") {
                return None;
            }
            encoded.push(format!("align:{value}"));
        } else if matches!(token, "border" | "shadow" | "plain" | "fit") {
            encoded.push(token.to_string());
        } else {
            return None;
        }
    }
    if encoded.is_empty() {
        return None;
    }
    Some(encoded.join("+"))
}

/// Replace inline `$x^2$` math with inline-code styling so the markdown
/// renderer shows it distinctly. Conservative heuristics: the line must
/// contain an even number of `$`, and a math segment must be non-empty
/// with no leading/trailing whitespace (so `$5 and $6` stays currency).
fn replace_inline_math(line: &str) -> String {
    if !line.matches('$').count().is_multiple_of(2) || !line.contains('$') {
        return line.to_string();
    }
    let segments: Vec<&str> = line.split('$').collect();
    // segments alternate: text, math, text, math, ... text
    let valid = segments
        .iter()
        .skip(1)
        .step_by(2)
        .all(|m| !m.is_empty() && m.trim() == *m && !m.contains('`'));
    if !valid {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    for (i, segment) in segments.iter().enumerate() {
        if i % 2 == 1 {
            out.push('`');
            out.push_str(segment);
            out.push('`');
        } else {
            out.push_str(segment);
        }
    }
    out
}

enum NoteOpen {
    /// `<!-- note: text -->` on one line.
    Complete(Option<usize>, String),
    /// `<!-- note: text` — closes on a later line.
    Continued(Option<usize>, String),
}

/// Recognize `<!-- note: -->`, `<!-- speaker: -->`, `<!-- note[n]: -->`.
fn parse_note_open(trimmed: &str) -> Option<NoteOpen> {
    let body = trimmed.strip_prefix("<!--")?.trim_start();
    let (step, rest) = if let Some(rest) = body.strip_prefix("speaker:") {
        (None, rest)
    } else if let Some(rest) = body.strip_prefix("note:") {
        (None, rest)
    } else if let Some(rest) = body.strip_prefix("note[") {
        let close = rest.find("]:")?;
        let n: usize = rest[..close].trim().parse().ok()?;
        (Some(n), &rest[close + 2..])
    } else {
        return None;
    };

    match rest.find("-->") {
        Some(end) => Some(NoteOpen::Complete(step, rest[..end].trim().to_string())),
        None => Some(NoteOpen::Continued(step, rest.trim().to_string())),
    }
}

/// Split a fence-opening line into a renderer-safe line (language only)
/// and the parsed [`CodeBlock`] info.
///
/// `"```rust {2,4-6}"` → (`"```rust"`, language `rust`, annotation `{2,4-6}`)
fn clean_fence_line(line: &str) -> (String, CodeBlock) {
    let indent_len = line.len() - line.trim_start_matches(' ').len();
    let (indent, rest) = line.split_at(indent_len);
    let fence_ch = rest.chars().next().expect("caller verified fence");
    let fence_len = rest.chars().take_while(|&c| c == fence_ch).count();
    let (fence, info) = rest.split_at(fence_len);

    let info = info.trim();
    let mut parts = info.splitn(2, char::is_whitespace);
    let language = parts.next().filter(|s| !s.is_empty()).map(str::to_string);
    let annotation = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let cleaned = match &language {
        Some(lang) => format!("{indent}{fence}{lang}"),
        None => format!("{indent}{fence}"),
    };
    (
        cleaned,
        CodeBlock {
            language,
            annotation,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_dashes() {
        let deck = parse("# One\n\n---\n\n# Two\n").unwrap();
        assert_eq!(deck.slides.len(), 2);
        assert!(deck.slides[0].source.contains("# One"));
        assert!(deck.slides[1].source.contains("# Two"));
    }

    #[test]
    fn ignores_dashes_inside_backtick_fence() {
        let src = "# One\n\n```markdown\nfront\n\n---\n\nback\n```\n\n---\n\n# Two\n";
        let deck = parse(src).unwrap();
        assert_eq!(deck.slides.len(), 2);
        assert!(deck.slides[0].source.contains("---"));
    }

    #[test]
    fn ignores_dashes_inside_tilde_fence() {
        let src = "~~~\n---\n~~~\n\n---\n\n# Two\n";
        let deck = parse(src).unwrap();
        assert_eq!(deck.slides.len(), 2);
    }

    #[test]
    fn longer_fence_requires_longer_closer() {
        // ```` opens; ``` does not close it, so the --- stays inside
        let src = "````\n```\n---\n````\n\n---\n\n# Two\n";
        let deck = parse(src).unwrap();
        assert_eq!(deck.slides.len(), 2);
        assert!(deck.slides[0].source.contains("---"));
    }

    #[test]
    fn frontmatter_extracted_and_parsed() {
        let src = "---\ntitle: \"My Talk\"\ntheme: dark\naspect: \"16:9\"\n---\n\n# First\n";
        let deck = parse(src).unwrap();
        assert_eq!(deck.frontmatter.title.as_deref(), Some("My Talk"));
        assert_eq!(deck.frontmatter.theme.as_deref(), Some("dark"));
        assert_eq!(deck.frontmatter.aspect.as_deref(), Some("16:9"));
        assert_eq!(deck.slides.len(), 1);
        assert!(deck.slides[0].source.contains("# First"));
    }

    #[test]
    fn frontmatter_unknown_keys_ignored() {
        let src = "---\ntitle: x\nslidev_specific: whatever\n---\n# S\n";
        let deck = parse(src).unwrap();
        assert_eq!(deck.frontmatter.title.as_deref(), Some("x"));
    }

    #[test]
    fn invalid_frontmatter_is_an_error() {
        let src = "---\ntitle: [unclosed\n---\n# S\n";
        assert!(parse(src).is_err());
    }

    #[test]
    fn no_frontmatter_means_defaults() {
        let deck = parse("# Just a slide\n").unwrap();
        assert_eq!(deck.frontmatter, Frontmatter::default());
        assert_eq!(deck.slides.len(), 1);
    }

    #[test]
    fn empty_input_yields_one_empty_slide() {
        let deck = parse("").unwrap();
        assert_eq!(deck.slides.len(), 1);
        assert_eq!(deck.slides[0].steps.len(), 1);
    }

    #[test]
    fn consecutive_delimiters_do_not_create_empty_slides() {
        let deck = parse("# One\n---\n---\n# Two\n").unwrap();
        assert_eq!(deck.slides.len(), 2);
    }

    #[test]
    fn start_lines_are_recorded() {
        let src = "---\ntitle: t\n---\n# One\n\n---\n\n# Two\n";
        let deck = parse(src).unwrap();
        assert_eq!(deck.slides[0].start_line, 4);
        assert_eq!(deck.slides[1].start_line, 7);
    }

    // --- notes ---

    #[test]
    fn slide_note_extracted_and_stripped() {
        let src = "<!-- note: remember to smile -->\n# Hello\n";
        let deck = parse(src).unwrap();
        let slide = &deck.slides[0];
        assert_eq!(slide.notes.len(), 1);
        assert_eq!(slide.notes[0].text, "remember to smile");
        assert_eq!(slide.notes[0].step, None);
        assert!(!slide.source.contains("note:"));
        assert!(slide.source.contains("# Hello"));
    }

    #[test]
    fn speaker_alias_and_multiple_notes_concatenate() {
        let src = "<!-- note: first -->\n# H\n<!-- speaker: second -->\n";
        let deck = parse(src).unwrap();
        let texts: Vec<_> = deck.slides[0].notes.iter().map(|n| &n.text).collect();
        assert_eq!(texts, ["first", "second"]);
    }

    #[test]
    fn multiline_note_collected_until_close() {
        let src = "<!-- note: line one\nline two\nline three -->\n# H\n";
        let deck = parse(src).unwrap();
        assert_eq!(deck.slides[0].notes[0].text, "line one line two line three");
        assert!(deck.slides[0].source.contains("# H"));
    }

    #[test]
    fn step_note_parsed() {
        let src = "# H\n<!-- pause -->\n- a\n<!-- note[1]: shown at step 1 -->\n";
        let deck = parse(src).unwrap();
        let slide = &deck.slides[0];
        assert_eq!(slide.notes[0].step, Some(1));
        // visible at step 1, hidden at step 0
        assert_eq!(slide.notes_at(0).count(), 0);
        assert_eq!(slide.notes_at(1).count(), 1);
    }

    #[test]
    fn note_inside_fence_is_literal() {
        let src = "```html\n<!-- note: not a real note -->\n```\n";
        let deck = parse(src).unwrap();
        assert!(deck.slides[0].notes.is_empty());
        assert!(deck.slides[0].source.contains("not a real note"));
    }

    // --- steps ---

    #[test]
    fn pause_splits_into_cumulative_steps() {
        let src = "- one\n<!-- pause -->\n- two\n<!-- pause -->\n- three\n";
        let deck = parse(src).unwrap();
        let slide = &deck.slides[0];
        assert_eq!(slide.step_count(), 3);
        assert!(slide.step_source(0).contains("one"));
        assert!(!slide.step_source(0).contains("two"));
        assert!(slide.step_source(1).contains("two"));
        assert!(!slide.step_source(1).contains("three"));
        assert!(slide.step_source(2).contains("three"));
        assert_eq!(slide.source, slide.steps[2]);
    }

    #[test]
    fn v_click_alias_works() {
        let src = "a\n<!-- v-click -->\nb\n";
        let deck = parse(src).unwrap();
        assert_eq!(deck.slides[0].step_count(), 2);
    }

    #[test]
    fn no_pause_means_single_step() {
        let deck = parse("# Plain\n").unwrap();
        assert_eq!(deck.slides[0].step_count(), 1);
    }

    #[test]
    fn pause_inside_fence_is_literal() {
        let src = "```md\n<!-- pause -->\n```\n";
        let deck = parse(src).unwrap();
        let slide = &deck.slides[0];
        assert_eq!(slide.step_count(), 1);
        assert!(slide.source.contains("pause"));
    }

    #[test]
    fn trailing_pause_collapsed() {
        let src = "content\n<!-- pause -->\n";
        let deck = parse(src).unwrap();
        assert_eq!(deck.slides[0].step_count(), 1);
    }

    // --- code fences ---

    #[test]
    fn fence_annotation_stripped_for_renderer_kept_in_model() {
        let src = "```rust {2,4-6}\nfn main() {}\n```\n";
        let deck = parse(src).unwrap();
        let slide = &deck.slides[0];
        assert!(slide.source.contains("```rust\n"));
        assert!(!slide.source.contains("{2,4-6}"));
        assert_eq!(slide.code_blocks.len(), 1);
        assert_eq!(slide.code_blocks[0].language.as_deref(), Some("rust"));
        assert_eq!(slide.code_blocks[0].annotation.as_deref(), Some("{2,4-6}"));
    }

    #[test]
    fn plain_fence_unchanged() {
        let src = "```mermaid\ngraph TD\n```\n";
        let deck = parse(src).unwrap();
        let slide = &deck.slides[0];
        assert!(slide.source.contains("```mermaid\n"));
        assert_eq!(slide.code_blocks[0].language.as_deref(), Some("mermaid"));
        assert_eq!(slide.code_blocks[0].annotation, None);
    }

    #[test]
    fn bare_fence_has_no_language() {
        let deck = parse("```\nx\n```\n").unwrap();
        assert_eq!(deck.slides[0].code_blocks[0].language, None);
    }

    #[test]
    fn code_block_font_size_annotation() {
        let deck = parse("```rust {size=22}\nfn main() {}\n```\n").unwrap();
        let cb = &deck.slides[0].code_blocks[0];
        assert_eq!(cb.font_size(), Some(22.0));
        // No size token → None (theme default).
        let deck = parse("```rust {2,4}\nx\n```\n").unwrap();
        assert_eq!(deck.slides[0].code_blocks[0].font_size(), None);
        // Composes with line highlights in the same group.
        let deck = parse("```rust {2-3 size=18}\nx\n```\n").unwrap();
        let cb = &deck.slides[0].code_blocks[0];
        assert_eq!(cb.font_size(), Some(18.0));
        assert_eq!(cb.highlighted_lines().unwrap().len(), 2); // lines 2,3
    }

    #[test]
    fn code_block_highlight_style_flag() {
        let bg = parse("```rust {2 background}\nx\n```\n").unwrap();
        let cb = &bg.slides[0].code_blocks[0];
        assert_eq!(cb.highlight_style(), Some("background"));
        assert_eq!(cb.highlighted_lines().unwrap().len(), 1); // line 2 still parses

        let dim = parse("```rust {1|2 dim}\nx\n```\n").unwrap();
        assert_eq!(dim.slides[0].code_blocks[0].highlight_style(), Some("dim"));

        // No flag → None (theme default).
        let plain = parse("```rust {2,4}\nx\n```\n").unwrap();
        assert_eq!(plain.slides[0].code_blocks[0].highlight_style(), None);
    }

    #[test]
    fn code_block_align_annotation() {
        let c = parse("```rust {align=center}\nx\n```\n").unwrap();
        assert_eq!(c.slides[0].code_blocks[0].align(), Some("center"));
        // Composes with line highlights and size.
        let m = parse("```rust {2 align=right size=20}\nx\n```\n").unwrap();
        let cb = &m.slides[0].code_blocks[0];
        assert_eq!(cb.align(), Some("right"));
        assert_eq!(cb.font_size(), Some(20.0));
        assert_eq!(cb.highlighted_lines().unwrap().len(), 1);
        // Invalid value → None.
        let bad = parse("```rust {align=middle}\nx\n```\n").unwrap();
        assert_eq!(bad.slides[0].code_blocks[0].align(), None);
    }

    #[test]
    fn nested_fence_content_not_treated_as_code_block() {
        // The inner ``` lines are content of the ```` fence.
        let src = "````md\n```rust {1}\n```\n````\n";
        let deck = parse(src).unwrap();
        let slide = &deck.slides[0];
        assert_eq!(slide.code_blocks.len(), 1);
        assert_eq!(slide.code_blocks[0].language.as_deref(), Some("md"));
        assert!(slide.source.contains("{1}")); // inner annotation untouched
    }

    // --- math ---

    #[test]
    fn display_math_block_becomes_marker() {
        let src = "before\n\n$$\nE = mc^2\n$$\n\nafter\n";
        let deck = parse(src).unwrap();
        let slide = &deck.slides[0];
        assert_eq!(slide.math_blocks.len(), 1);
        assert_eq!(slide.math_blocks[0].latex, "E = mc^2");
        assert!(slide.source.contains("![math](preso-math:0)"));
        assert!(!slide.source.contains("$$"));
    }

    #[test]
    fn single_line_display_math() {
        let deck = parse("$$x = 1$$\n").unwrap();
        assert_eq!(deck.slides[0].math_blocks[0].latex, "x = 1");
    }

    #[test]
    fn multiple_math_blocks_indexed_in_order() {
        let src = "$$a$$\n\ntext\n\n$$b$$\n";
        let deck = parse(src).unwrap();
        let slide = &deck.slides[0];
        assert_eq!(slide.math_blocks.len(), 2);
        assert!(slide.source.contains("preso-math:0"));
        assert!(slide.source.contains("preso-math:1"));
    }

    #[test]
    fn inline_math_becomes_inline_code() {
        let deck = parse("the discriminant $b^2 - 4ac$ decides\n").unwrap();
        assert!(deck.slides[0].source.contains("`b^2 - 4ac`"));
        assert!(!deck.slides[0].source.contains('$'));
    }

    #[test]
    fn currency_is_not_math() {
        let deck = parse("it costs $5 and $6 respectively\n").unwrap();
        assert!(deck.slides[0].source.contains("$5 and $6"));
    }

    #[test]
    fn math_inside_fence_is_literal() {
        let src = "```latex\n$$x$$\n$y$\n```\n";
        let deck = parse(src).unwrap();
        let slide = &deck.slides[0];
        assert!(slide.math_blocks.is_empty());
        assert!(slide.source.contains("$$x$$"));
        assert!(slide.source.contains("$y$"));
    }

    // --- layout ---

    #[test]
    fn layout_directive_parsed_and_stripped() {
        use crate::model::Layout;
        let src = "<!-- layout: TwoColumn -->\nleft\n\n***\n\nright\n";
        let deck = parse(src).unwrap();
        let slide = &deck.slides[0];
        // Bare TwoColumn defaults to an even 1:1 split.
        assert_eq!(slide.layout, Layout::TwoColumn { left: 1, right: 1 });
        assert!(!slide.source.contains("layout:"));
        let (left, right) = slide.columns_at(0).unwrap();
        assert!(left.contains("left"));
        assert!(right.contains("right"));
    }

    #[test]
    fn two_column_ratio_parsed() {
        use crate::model::Layout;
        let deck = parse("<!-- layout: TwoColumn 2:1 -->\na\n\n***\n\nb\n").unwrap();
        assert_eq!(
            deck.slides[0].layout,
            Layout::TwoColumn { left: 2, right: 1 }
        );
        // `/` separator and multi-digit ratios also work.
        let deck = parse("<!-- layout: TwoColumn 60/40 -->\na\n\n***\n\nb\n").unwrap();
        assert_eq!(
            deck.slides[0].layout,
            Layout::TwoColumn {
                left: 60,
                right: 40
            }
        );
        // Malformed or zero ratios fall back to 1:1, still two-column.
        for spec in ["TwoColumn 0:1", "TwoColumn 2:x", "TwoColumn huh"] {
            let src = format!("<!-- layout: {spec} -->\na\n\n***\n\nb\n");
            assert_eq!(
                parse(&src).unwrap().slides[0].layout,
                Layout::TwoColumn { left: 1, right: 1 },
                "{spec}"
            );
        }
    }

    #[test]
    fn column_separator_inside_nested_fence_is_literal() {
        // A ```` fence whose *content* contains ``` lines and a `***`: the
        // shorter inner fence must not close the outer one, so the in-fence
        // `***` stays in the left column and only the later bare `***` splits.
        let src = "<!-- layout: TwoColumn -->\n\n\
            ````md\n```\n***\n```\n````\n\n\
            ***\n\n\
            right\n";
        let (left, right) = parse(src).unwrap().slides[0].columns_at(0).unwrap();
        assert!(
            left.contains("````"),
            "left keeps the outer fence: {left:?}"
        );
        assert!(left.contains("***"), "in-fence *** is content: {left:?}");
        assert_eq!(right.trim(), "right");
    }

    #[test]
    fn default_layout_has_no_columns() {
        let deck = parse("just text\n\n***\n\nmore\n").unwrap();
        assert_eq!(deck.slides[0].layout, crate::model::Layout::Content);
        assert!(deck.slides[0].columns_at(0).is_none());
    }

    // --- per-slide overrides ---

    #[test]
    fn slide_overrides_parsed_and_stripped() {
        let src = "<!-- slide: align=center halign=right background=#112233 -->\n# Big Statement\n";
        let deck = parse(src).unwrap();
        let slide = &deck.slides[0];
        assert_eq!(slide.overrides.align.as_deref(), Some("center"));
        assert_eq!(slide.overrides.halign.as_deref(), Some("right"));
        assert_eq!(slide.overrides.background.as_deref(), Some("#112233"));
        assert!(!slide.source.contains("slide:"));
    }

    #[test]
    fn directive_spacing_is_flexible() {
        use crate::model::Layout;
        // `<!--layout:` and `<!--  slide:` parse like their spaced forms,
        // matching the tolerance note comments have always had.
        let deck = parse("<!--layout: TwoColumn -->\na\n\n***\n\nb\n").unwrap();
        assert!(matches!(deck.slides[0].layout, Layout::TwoColumn { .. }));
        let deck = parse("<!--  slide: align=center -->\nx\n").unwrap();
        assert_eq!(deck.slides[0].overrides.align.as_deref(), Some("center"));
        // A longer comment name is not the directive; the line stays literal.
        let deck = parse("<!-- layouts: TwoColumn -->\nx\n").unwrap();
        assert_eq!(deck.slides[0].layout, Layout::Content);
        assert!(deck.slides[0].source.contains("layouts"));
    }

    #[test]
    fn unknown_override_keys_ignored() {
        let deck = parse("<!-- slide: sparkles=yes align=center -->\nx\n").unwrap();
        assert_eq!(deck.slides[0].overrides.align.as_deref(), Some("center"));

        let deck = parse("<!-- slide: kind=title -->\n# Big\n").unwrap();
        assert_eq!(deck.slides[0].overrides.kind.as_deref(), Some("title"));
    }

    #[test]
    fn footnote_directive_parsed_and_stripped() {
        let src = "# Slide\n\n![a](a.png)\n\n<!-- footnote: Images: Unsplash, CC BY 4.0 -->\n";
        let slide = &parse(src).unwrap().slides[0];
        assert_eq!(
            slide.footnote.as_deref(),
            Some("Images: Unsplash, CC BY 4.0")
        );
        // The directive is chrome, not body content.
        assert!(!slide.source.contains("footnote"));
    }

    #[test]
    fn slide_transition_override_parsed() {
        let slide = &parse("<!-- slide: transition=wipe -->\n# x\n")
            .unwrap()
            .slides[0];
        assert_eq!(slide.overrides.transition.as_deref(), Some("wipe"));
        // Absent by default.
        let plain = &parse("# y\n").unwrap().slides[0];
        assert_eq!(plain.overrides.transition, None);
    }

    #[test]
    fn video_directive_parsed_and_stripped() {
        let src = "# Demo\n\n![poster](poster.png)\n\n<!-- video: clips/demo.mp4 -->\n";
        let slide = &parse(src).unwrap().slides[0];
        assert_eq!(slide.video.as_deref(), Some("clips/demo.mp4"));
        // The directive is chrome, not body content.
        assert!(!slide.source.contains("video"));
    }

    #[test]
    fn multiple_footnotes_join() {
        let src = "<!-- footnote: First. -->\n<!-- footnote: Second. -->\n# x\n";
        assert_eq!(
            parse(src).unwrap().slides[0].footnote.as_deref(),
            Some("First. Second.")
        );
    }

    #[test]
    fn number_override_resets_the_display_counter() {
        use crate::model::{display_number, display_total};
        // Slide 3 (index 2) resets to 50; later slides continue from there.
        let src = "# 1\n---\n# 2\n---\n<!-- slide: number=50 -->\n# 3\n---\n# 4\n";
        let slides = &parse(src).unwrap().slides;
        assert_eq!(slides[2].overrides.number, Some(50));
        let nums: Vec<usize> = (0..slides.len())
            .map(|i| display_number(slides, i))
            .collect();
        assert_eq!(nums, vec![1, 2, 50, 51]);
        assert_eq!(display_total(slides), 51);
    }

    #[test]
    fn no_number_override_is_plain_sequential() {
        use crate::model::{display_number, display_total};
        let slides = &parse("# a\n---\n# b\n---\n# c\n").unwrap().slides;
        assert_eq!(
            (0..3)
                .map(|i| display_number(slides, i))
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(display_total(slides), 3);
    }

    #[test]
    fn layer_image_directive_parsed_and_stripped() {
        use crate::model::Anchor;
        let src = "<!-- image: pics/bg.png position=bottom-right width=40% opacity=0.5 padding=30 -->\n# Hi\n";
        let slide = &parse(src).unwrap().slides[0];
        assert_eq!(slide.layer_images.len(), 1);
        let li = &slide.layer_images[0];
        assert_eq!(li.path, "pics/bg.png");
        assert_eq!(li.position, Anchor::BottomRight);
        assert_eq!(li.width, Some(40.0));
        assert_eq!(li.opacity, 0.5);
        assert_eq!(li.padding, [30.0; 4]);
        // It's chrome, not body content.
        assert!(!slide.source.contains("<!-- image:"));
        assert!(slide.source.contains("# Hi"));
    }

    #[test]
    fn layer_image_defaults_and_multiple() {
        use crate::model::Anchor;
        let src = "<!-- image: a.png -->\n<!-- image: b.png position=top -->\n# H\n";
        let slide = &parse(src).unwrap().slides[0];
        assert_eq!(slide.layer_images.len(), 2);
        // Defaults: centered, natural width, opaque, no padding.
        assert_eq!(slide.layer_images[0].position, Anchor::Center);
        assert_eq!(slide.layer_images[0].width, None);
        assert_eq!(slide.layer_images[0].opacity, 1.0);
        assert_eq!(slide.layer_images[1].position, Anchor::Top);
    }

    #[test]
    fn layer_image_per_side_padding_overrides_uniform() {
        // `padding=` is the base; `padding-left=` etc. override that side.
        let src = "<!-- image: a.png padding=10 padding-left=40 padding-top=5 -->\n# H\n";
        let li = &parse(src).unwrap().slides[0].layer_images[0];
        // [top, right, bottom, left]
        assert_eq!(li.padding, [5.0, 10.0, 10.0, 40.0]);
    }

    #[test]
    fn hidden_slides_are_dropped_from_the_deck() {
        let src = "\
# One

---

<!-- slide: hidden -->

# Secret draft

---

# Three
";
        let deck = parse(src).unwrap();
        // The hidden slide is gone entirely; the rest keep their order.
        assert_eq!(deck.slides.len(), 2);
        assert!(deck.slides[0].source.contains("# One"));
        assert!(deck.slides[1].source.contains("# Three"));
        assert!(!deck.slides.iter().any(|s| s.source.contains("Secret")));
    }

    #[test]
    fn hidden_combines_with_other_overrides() {
        // `hidden` is a bare flag that can sit alongside key=value directives.
        let deck = parse("<!-- slide: kind=section hidden -->\n# Gone\n").unwrap();
        assert!(deck.slides.is_empty());
    }

    // --- line highlight annotations ---

    #[test]
    fn highlighted_lines_parse() {
        let deck = parse("```rust {1,3-5}\nx\n```\n").unwrap();
        let lines = deck.slides[0].code_blocks[0].highlighted_lines().unwrap();
        assert_eq!(lines.into_iter().collect::<Vec<_>>(), vec![1, 3, 4, 5]);
    }

    #[test]
    fn bad_annotation_yields_none() {
        let deck = parse("```rust {5-3}\nx\n```\n").unwrap();
        assert!(deck.slides[0].code_blocks[0].highlighted_lines().is_none());
    }

    #[test]
    fn click_through_highlight_adds_reveal_steps() {
        // A 3-stage code highlight makes the slide a 3-step reveal even
        // without any `<!-- pause -->` markers (the parallel step model).
        let deck = parse("```rust {1|2|3}\na\nb\nc\n```\n").unwrap();
        assert_eq!(deck.slides[0].step_count(), 3);
    }

    // --- tables ---

    #[test]
    fn table_extracted_with_alignment() {
        use crate::model::TableAlign::{Center, Left, Right};
        let deck = parse("| A | B | C |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n| x | y | z |\n").unwrap();
        let slide = &deck.slides[0];
        assert_eq!(slide.tables.len(), 1);
        let t = &slide.tables[0];
        assert_eq!(t.headers, ["A", "B", "C"]);
        assert_eq!(t.aligns, [Left, Center, Right]);
        assert_eq!(t.rows, [["1", "2", "3"], ["x", "y", "z"]]);
        // Source carries the marker, not the raw table.
        assert!(slide.source.contains("preso-table:0"));
        assert!(!slide.source.contains("| A |"));
    }

    #[test]
    fn escaped_pipe_stays_in_cell() {
        // `\|` is a literal pipe, not a column separator (even inside code).
        let deck = parse("| Cmd | Note |\n|---|---|\n| `grep a\\|b` | or |\n").unwrap();
        let t = &deck.slides[0].tables[0];
        assert_eq!(t.rows, [["`grep a|b`", "or"]]);
    }

    #[test]
    fn table_size_directive_applies_to_next_table() {
        let src = "<!-- table: size=20 -->\n| A | B |\n|---|---|\n| 1 | 2 |\n";
        let slide = &parse(src).unwrap().slides[0];
        assert_eq!(slide.tables[0].font_size, Some(20.0));
        assert!(!slide.source.contains("<!-- table:")); // directive stripped

        // No directive → None (theme default).
        let plain = &parse("| A | B |\n|---|---|\n| 1 | 2 |\n").unwrap().slides[0];
        assert_eq!(plain.tables[0].font_size, None);

        // Applies only to the *next* table, not a later one.
        let two = "<!-- table: size=18 -->\n| A |\n|---|\n| 1 |\n\ntext\n\n| B |\n|---|\n| 2 |\n";
        let s = &parse(two).unwrap().slides[0];
        assert_eq!(s.tables[0].font_size, Some(18.0));
        assert_eq!(s.tables[1].font_size, None);
    }

    #[test]
    fn pipe_text_without_delimiter_is_not_a_table() {
        let deck = parse("a | b is just prose\n\nmore text\n").unwrap();
        assert!(deck.slides[0].tables.is_empty());
    }

    // --- loose-bullet grouping ---

    #[test]
    fn blank_line_between_bullets_inserts_a_spacer() {
        // No blank between a/b → tight; the blank before c becomes a spacer
        // paragraph (a non-breaking-space line) that separates the groups.
        let deck = parse("- a\n- b\n\n- c\n  - nested\n").unwrap();
        let src = &deck.slides[0].source;
        assert!(src.contains("- a") && src.contains("- b") && src.contains("- c"));
        assert!(
            src.contains('\u{a0}'),
            "the blank gap becomes a spacer: {src:?}"
        );
        assert!(src.contains("  - nested"), "nested bullets pass through");
    }

    #[test]
    fn more_blank_lines_make_a_bigger_gap() {
        let one = parse("- a\n\n- b\n").unwrap().slides[0]
            .source
            .matches('\u{a0}')
            .count();
        let two = parse("- a\n\n\n- b\n").unwrap().slides[0]
            .source
            .matches('\u{a0}')
            .count();
        assert_eq!(one, 1);
        assert_eq!(two, 2);
    }

    #[test]
    fn tight_bullets_get_no_spacer() {
        let deck = parse("- a\n- b\n- c\n").unwrap();
        assert!(!deck.slides[0].source.contains('\u{a0}'));
    }

    #[test]
    fn grouping_leaves_nested_fences_intact() {
        // Blank lines inside a ```` fence (whose content includes ``` lines)
        // must pass through group_lists untouched — no spacer paragraphs.
        let src = "- a\n\n````md\n```\n\n- not a bullet\n\n```\n````\n";
        let source = &parse(src).unwrap().slides[0].source;
        assert!(source.contains("````md\n```\n\n- not a bullet\n\n```\n````"));
        assert_eq!(source.matches('\u{a0}').count(), 0);
    }

    // --- sizing ---

    #[test]
    fn fence_width_annotation_parsed() {
        let deck = parse("```mermaid {width=60%}\ngraph TD\n```\n").unwrap();
        let block = &deck.slides[0].code_blocks[0];
        assert_eq!(block.width_percent(), Some(60.0));
        assert!(block.highlighted_lines().is_none());
    }

    #[test]
    fn fence_width_coexists_with_line_highlights() {
        let deck = parse("```rust {2,4-5 width=80%}\nx\n```\n").unwrap();
        let block = &deck.slides[0].code_blocks[0];
        assert_eq!(block.width_percent(), Some(80.0));
        let lines = block.highlighted_lines().unwrap();
        assert_eq!(lines.into_iter().collect::<Vec<_>>(), vec![2, 4, 5]);
    }

    #[test]
    fn fence_transparent_flag_parsed() {
        let deck = parse("```mermaid {width=55% transparent}\ngraph LR\n```\n").unwrap();
        let block = &deck.slides[0].code_blocks[0];
        assert!(block.transparent_background());
        assert_eq!(block.width_percent(), Some(55.0));

        let deck = parse("```mermaid {transparent}\ngraph LR\n```\n").unwrap();
        assert!(deck.slides[0].code_blocks[0].transparent_background());

        let deck = parse("```mermaid {width=55%}\ngraph LR\n```\n").unwrap();
        assert!(!deck.slides[0].code_blocks[0].transparent_background());
    }

    #[test]
    fn fence_transparent_coexists_with_line_highlights() {
        // Word flags must not break the numeric highlight parser.
        let deck = parse("```rust {2,4-5 transparent}\nx\n```\n").unwrap();
        let block = &deck.slides[0].code_blocks[0];
        assert!(block.transparent_background());
        let lines = block.highlighted_lines().unwrap();
        assert_eq!(lines.into_iter().collect::<Vec<_>>(), vec![2, 4, 5]);
    }

    #[test]
    fn column_slides_restore_fence_annotations() {
        // Two-column code blocks lost their `{...}` annotations because the
        // column source is the cleaned text; column_slides restores them
        // from the parent, in document order (left fences then right).
        let src = "<!-- layout: TwoColumn -->\n\n\
            ```python {1}\nleft = 1\n```\n\n\
            ***\n\n\
            ```rust {2}\nfn f() {\n    let x = 1;\n}\n```\n";
        let slide = &parse(src).unwrap().slides[0];
        let ((_, left), (_, right)) = slide.column_slides(0).unwrap();

        assert_eq!(left.code_blocks.len(), 1);
        assert_eq!(
            left.code_blocks[0]
                .highlighted_lines()
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(right.code_blocks.len(), 1);
        assert_eq!(
            right.code_blocks[0]
                .highlighted_lines()
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn image_width_attr_becomes_url_fragment() {
        let deck = parse("![logo](assets/logo.png){width=30%}\n").unwrap();
        let source = &deck.slides[0].source;
        assert!(source.contains("![logo](assets/logo.png#preso-img=width:30)"));
        assert!(!source.contains("{width"));
    }

    #[test]
    fn image_framing_flags_become_url_fragment() {
        let deck = parse("![a](x.png){width=40% border shadow}\n").unwrap();
        let source = &deck.slides[0].source;
        assert!(source.contains("![a](x.png#preso-img=width:40+border+shadow)"));

        let deck = parse("![b](y.png){plain}\n").unwrap();
        assert!(
            deck.slides[0]
                .source
                .contains("![b](y.png#preso-img=plain)")
        );

        let deck = parse("![c](z.png){border}\n").unwrap();
        assert!(
            deck.slides[0]
                .source
                .contains("![c](z.png#preso-img=border)")
        );
    }

    #[test]
    fn adjacent_images_become_an_image_row() {
        let deck = parse("![a](a.png)\n![b](b.png){width=40%}\n").unwrap();
        let slide = &deck.slides[0];
        // Lifted into one row marker, the source line replaced.
        assert!(slide.source.contains("![](preso-imagerow:0)"));
        assert!(!slide.source.contains("a.png"));
        assert_eq!(slide.image_rows.len(), 1);
        let row = &slide.image_rows[0];
        assert_eq!(row.images.len(), 2);
        assert_eq!(row.images[0].url, "a.png");
        assert_eq!(row.images[0].alt, "a");
        // Per-image attributes are carried through as the URL fragment.
        assert_eq!(row.images[1].url, "b.png#preso-img=width:40");
    }

    #[test]
    fn a_single_image_is_not_a_row() {
        let deck = parse("![only](one.png)\n").unwrap();
        assert!(deck.slides[0].image_rows.is_empty());
        assert!(deck.slides[0].source.contains("![only](one.png)"));
    }

    #[test]
    fn blank_line_separates_image_rows() {
        // A blank line between images keeps them stacked (two separate
        // single images, neither lifted into a row).
        let deck = parse("![a](a.png)\n\n![b](b.png)\n").unwrap();
        assert!(deck.slides[0].image_rows.is_empty());
        assert!(deck.slides[0].source.contains("![a](a.png)"));
        assert!(deck.slides[0].source.contains("![b](b.png)"));
    }

    #[test]
    fn three_adjacent_images_row_together() {
        let deck = parse("![a](a.png)\n![b](b.png)\n![c](c.png)\n").unwrap();
        assert_eq!(deck.slides[0].image_rows.len(), 1);
        assert_eq!(deck.slides[0].image_rows[0].images.len(), 3);
    }

    #[test]
    fn image_row_fit_flag_carried_in_fragment() {
        // `{fit}` on a row image is recognized and encoded for the renderer.
        let deck = parse("![a](a.png){fit}\n![b](b.png)\n").unwrap();
        let row = &deck.slides[0].image_rows[0];
        assert!(row.images[0].url.contains("#preso-img=fit"));
    }

    #[test]
    fn image_align_attr_becomes_url_fragment() {
        let deck = parse("![a](x.png){align=center}\n").unwrap();
        assert!(
            deck.slides[0]
                .source
                .contains("![a](x.png#preso-img=align:center)")
        );

        // Composes with width and framing flags.
        let deck = parse("![b](y.png){width=50% align=right border}\n").unwrap();
        assert!(
            deck.slides[0]
                .source
                .contains("![b](y.png#preso-img=width:50+align:right+border)")
        );

        // An invalid alignment leaves the whole group literal.
        let deck = parse("![c](z.png){align=middle}\n").unwrap();
        assert!(deck.slides[0].source.contains("{align=middle}"));
    }

    #[test]
    fn malformed_image_width_left_untouched() {
        let deck = parse("![a](x.png){width=banana}\n![b](y.png){width=150%}\n").unwrap();
        let source = &deck.slides[0].source;
        assert!(source.contains("{width=banana}"));
        assert!(source.contains("{width=150%}"));

        // Unknown flags leave the whole group as literal text.
        let deck = parse("![c](z.png){sparkles}\n").unwrap();
        assert!(deck.slides[0].source.contains("{sparkles}"));
    }

    #[test]
    fn image_width_inside_fence_is_literal() {
        let deck = parse("```md\n![a](x.png){width=30%}\n```\n").unwrap();
        assert!(deck.slides[0].source.contains("{width=30%}"));
    }

    #[test]
    fn example_talk_parses() {
        let src = include_str!("../../../docs/example-talk.md");
        let deck = parse(src).unwrap();
        assert_eq!(
            deck.frontmatter.title.as_deref(),
            Some("Preso: Native Markdown Presentations")
        );
        assert_eq!(deck.slides.len(), 15);

        // Full-bleed background slide: `background=` is an image path.
        let bg = &deck.slides[13];
        assert_eq!(
            bg.overrides.background.as_deref(),
            Some("assets/backdrop.png")
        );

        // Slide 1 is the title slide; slide 7 a section header.
        assert_eq!(deck.slides[0].overrides.kind.as_deref(), Some("title"));
        assert_eq!(deck.slides[5].overrides.kind.as_deref(), Some("section"));

        // Slide 2 ("Why Another Tool?") has 3 steps and a step note
        let why = &deck.slides[1];
        assert_eq!(why.step_count(), 3);
        assert!(why.notes.iter().any(|n| n.step == Some(2)));

        // Two-column slides: one-sided heading (left only) then matching
        // headings on both sides.
        let (one_left, one_right) = deck.slides[3].columns_at(0).unwrap();
        assert_eq!(crate::model::leading_heading_level(&one_left), Some(2));
        assert_eq!(crate::model::leading_heading_level(&one_right), None);
        let (both_left, both_right) = deck.slides[4].columns_at(0).unwrap();
        assert_eq!(crate::model::leading_heading_level(&both_left), Some(3));
        assert_eq!(crate::model::leading_heading_level(&both_right), Some(3));

        // The transparent-diagram slides carry the fence flag
        let transparent = &deck.slides[7];
        assert!(transparent.source.contains("Transparent"));
        assert!(transparent.code_blocks[0].transparent_background());
        assert_eq!(transparent.code_blocks[0].width_percent(), Some(55.0));

        // The Graphviz slide carries a sized dot fence
        let dot = &deck.slides[9];
        assert!(dot.source.contains("Graphviz"));
        assert_eq!(dot.code_blocks[0].language.as_deref(), Some("dot"));
        assert_eq!(dot.code_blocks[0].width_percent(), Some(45.0));

        // The "Tables" slide carries an extracted table with 3 columns.
        let tables = &deck.slides[11];
        assert!(tables.source.contains("preso-table:0"));
        assert_eq!(tables.tables[0].headers.len(), 3);

        // The "Edge Cases" slide keeps its in-fence `---`
        let edge = &deck.slides[12];
        assert!(edge.source.contains("Edge Cases"));
        assert!(edge.source.contains("---"));

        // Final slide has a slide-level note
        assert!(!deck.slides[14].notes.is_empty());
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    proptest! {
        /// The parser must never panic, whatever bytes arrive.
        #[test]
        fn never_panics(src in "\\PC*") {
            let _ = super::parse(&src);
        }

        /// Steps are cumulative: each step's source starts with the previous.
        #[test]
        fn steps_are_cumulative(src in "[a-z\\n#`~ <!\\->]{0,300}") {
            if let Ok(deck) = super::parse(&src) {
                for slide in &deck.slides {
                    for w in slide.steps.windows(2) {
                        let prev = w[0].trim_end();
                        prop_assert!(w[1].starts_with(prev) || prev.is_empty());
                    }
                }
            }
        }
    }
}
