//! Problems the parser forgives but an author wants to hear about, and every
//! file a deck points at.
//!
//! The parser is deliberately lenient — a malformed directive is dropped, an
//! unknown `slide:` key ignored, a misspelt directive is just a comment —
//! because a deck on stage must never fail. [`check`] reports those same
//! lines while the deck is being written. Validation calls the parser's own
//! directive parsers wherever they exist, so a finding here means the
//! renderer really does ignore or reinterpret the line.
//!
//! [`Analysis::references`] lists each asset path with its location (for
//! existence checks, links and — later — bundling). Resolving paths is left
//! to the caller: this module does no I/O.
//!
//! Line numbers are 0-based; columns are byte offsets within the line.

use crate::error::ParseError;
use crate::outline::{self, LineKind};
use crate::parser;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Hint,
}

/// One problem, with where it is and, when there's an obvious fix, the
/// text to replace `cols` with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub cols: Range<usize>,
    pub severity: Severity,
    /// A stable identifier (`unknown-directive`, `layout-ratio`, …).
    pub code: &'static str,
    pub message: String,
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// `![alt](path)`, anywhere in a line.
    Image,
    /// `<!-- video: path -->`
    Video,
    /// `<!-- image: path … -->`
    LayerImage,
    /// `<!-- slide: background=path -->` (a `#colour` is not a reference).
    Background,
    /// `<!-- include: path -->`
    Include,
    /// Frontmatter `theme: path.toml`, or a theme name (not a built-in)
    /// looked up in the user's theme folder.
    Theme,
}

/// A path the deck points at, as written (fragment and query stripped),
/// relative to the deck file's directory unless absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub kind: RefKind,
    pub path: String,
    pub line: usize,
    /// The path as it appears in the line.
    pub cols: Range<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Analysis {
    pub findings: Vec<Finding>,
    pub references: Vec<Reference>,
    /// Diagram fences whose zoom stages name nodes. Whether a label matches
    /// a node is only known once the diagram is drawn, which this crate
    /// doesn't do — so they're handed on for a caller that can.
    pub diagrams: Vec<DiagramZoom>,
}

/// A Mermaid or Graphviz fence that zooms onto nodes by label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramZoom {
    /// `mermaid`, `dot` or `graphviz`, as written.
    pub language: String,
    /// The diagram source: the lines between the fences.
    pub source: String,
    /// The opening fence line.
    pub line: usize,
    /// Every label its stages name, with where it is in that line.
    pub labels: Vec<(String, Range<usize>)>,
}

/// Fence languages that draw a diagram rather than show code.
pub const DIAGRAM_LANGUAGES: &[&str] = &["mermaid", "dot", "graphviz"];

/// Frontmatter keys preso reads (see `model::Frontmatter`).
pub const FRONTMATTER_KEYS: &[&str] = &["title", "theme", "transition", "aspect", "presenter"];

/// Built-in theme names (`preso_style::registry::builtin`).
pub const BUILTIN_THEMES: &[&str] = &["dark", "light"];

/// Frontmatter `presenter:` layouts (the app's `presenter::Layout`).
pub const PRESENTER_LAYOUTS: &[&str] = &["slide", "notes"];

/// Every directive name the parser acts on.
pub const DIRECTIVES: &[&str] = &[
    "layout",
    "slide",
    "note",
    "speaker",
    "footnote",
    "video",
    "image",
    "highlight",
    "table",
    "include",
    "zoom",
];

/// `<!-- slide: … -->` keys, and the values each accepts (empty: any).
pub const SLIDE_KEYS: &[(&str, &[&str])] = &[
    ("kind", &["title", "section"]),
    ("align", &["top", "center"]),
    ("halign", &["left", "center", "right"]),
    ("background", &[]),
    ("fit", &["cover", "contain", "stretch", "none"]),
    ("fill", &[]),
    ("transition", TRANSITIONS),
    ("number", &[]),
    ("size", &[]),
];

/// Bare `<!-- slide: … -->` flags.
pub const SLIDE_FLAGS: &[&str] = &["hidden"];

/// Transition names the app maps to an effect (`transition::Kind`).
pub const TRANSITIONS: &[&str] = &[
    "fade",
    "dissolve",
    "wipe",
    "cover",
    "wipe-content",
    "pan",
    "pan-left",
    "pan-right",
    "pan-up",
    "pan-down",
    "slide",
    "slide-left",
    "slide-right",
    "slide-up",
    "slide-down",
    "push",
    "push-left",
    "push-right",
    "push-up",
    "push-down",
    "none",
];

/// `<!-- image: … position=… -->` anchors.
pub const ANCHORS: &[&str] = &[
    "center",
    "top-left",
    "top",
    "top-right",
    "left",
    "right",
    "bottom-left",
    "bottom",
    "bottom-right",
];

/// Image formats preso decodes (`image` crate codecs + resvg).
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg"];

/// Containers the video players handle.
pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "m4v", "webm", "mkv"];

/// Lint `source` and collect its references.
pub fn check(source: &str) -> Analysis {
    let mut out = Analysis::default();
    let lines: Vec<&str> = source.lines().collect();
    let body_start = match parser::frontmatter_span(source) {
        Some((yaml, body_start)) => {
            check_frontmatter(source, yaml, &mut out);
            body_start
        }
        None => 0,
    };

    for range in parser::segments(source, body_start) {
        let segment = &lines[range.clone()];
        let kinds = outline::classify(segment);
        // The last layout directive wins, as in the parser.
        let mut two_column: Option<usize> = None;
        let mut has_split = false;
        // The fence being read: its opening line and the lines inside.
        let mut fence = crate::fence::Tracker::default();
        let mut open: Option<(usize, Vec<&str>)> = None;
        // A `$$` block being read: its opening line, and whether it closed.
        let mut math: Option<(usize, bool)> = None;
        for (i, (line, kind)) in segment.iter().zip(kinds.iter().copied()).enumerate() {
            let n = range.start + i;
            if kind == LineKind::Math {
                let trimmed = line.trim();
                match &mut math {
                    Some((_, closed)) if !*closed => *closed = trimmed == "$$",
                    _ => {
                        // A one-line `$$ x $$` opens and closes at once.
                        let one_line = trimmed.len() > 4 && trimmed.ends_with("$$");
                        math = Some((n, one_line));
                    }
                }
                continue;
            }
            if let Some((at, false)) = math.take() {
                unclosed_math(lines[at], at, &mut out.findings);
            }
            if kind == LineKind::Code {
                let n = range.start + i;
                let was_open = fence.in_fence();
                fence.process(line);
                if !was_open {
                    open = Some((n, Vec::new()));
                } else if !fence.in_fence() {
                    if let Some((at, body)) = open.take() {
                        check_fence(lines[at], at, &body, &mut out);
                    }
                } else if let Some((_, body)) = &mut open {
                    body.push(line);
                }
                continue;
            }
            if kind != LineKind::Text {
                continue;
            }
            let n = range.start + i;
            let trimmed = line.trim();
            if trimmed == "***" {
                has_split = true;
            }
            if trimmed.starts_with("<!--") {
                if let Some(layout) = check_comment(line, n, &mut out) {
                    two_column = layout.then_some(n);
                }
            } else {
                scan_images(line, n, &mut out.references);
                check_image_attrs(line, n, &mut out.findings);
            }
        }
        // A fence left open runs to the end of the slide; so does math.
        if let Some((at, body)) = open.take() {
            check_fence(lines[at], at, &body, &mut out);
        }
        if let Some((at, false)) = math.take() {
            unclosed_math(lines[at], at, &mut out.findings);
        }
        check_highlights_attach(segment, range.start, &kinds, &mut out.findings);
        if let Some(n) = two_column
            && !has_split
        {
            out.findings.push(Finding {
                line: n,
                cols: trimmed_cols(lines[n]),
                severity: Severity::Warning,
                code: "two-column-no-split",
                message: "no `***` line splits this slide, so it renders as one column \
                          (columns split at `***`; a `---` starts a new slide)"
                    .into(),
                fix: None,
            });
        }
    }
    out
}

fn check_frontmatter(source: &str, yaml: Range<usize>, out: &mut Analysis) {
    let e = match parser::extract_frontmatter(source) {
        Ok(_) => return check_frontmatter_values(source, yaml, out),
        Err(ParseError::Frontmatter(e)) => e,
    };
    let line = e.location().map_or(yaml.start, |l| {
        (yaml.start + l.line().saturating_sub(1)).min(yaml.end.saturating_sub(1))
    });
    let text = source.lines().nth(line).unwrap_or("");
    out.findings.push(Finding {
        line,
        cols: 0..text.len(),
        severity: Severity::Error,
        code: "frontmatter",
        message: format!("frontmatter isn't valid YAML, so the deck won't open: {e}"),
        fix: None,
    });
}

/// A `$$` block that never closes takes the rest of its slide as math.
fn unclosed_math(line: &str, n: usize, findings: &mut Vec<Finding>) {
    findings.push(Finding {
        line: n,
        cols: trimmed_cols(line),
        severity: Severity::Warning,
        code: "unclosed-math",
        message: "this `$$` is never closed, so everything after it on the slide is set as \
                  math — end the block with a `$$` line"
            .into(),
        fix: None,
    });
}

/// A highlight draws over the next image on its slide; flag those left with
/// none after them. Which images count is the parser's business (an image
/// row, an image beside text, a list item…), so this asks the parser itself
/// how many it attached: the rest are the last ones written.
fn check_highlights_attach(
    segment: &[&str],
    start: usize,
    kinds: &[LineKind],
    findings: &mut Vec<Finding>,
) {
    let written: Vec<usize> = segment
        .iter()
        .zip(kinds)
        .enumerate()
        .filter(|(_, (line, kind))| {
            **kind == LineKind::Text
                && parser::highlight_directive(line.trim())
                    .and_then(|(step, spec)| parser::parse_highlight(step, spec))
                    .is_some()
        })
        .map(|(i, _)| start + i)
        .collect();
    if written.is_empty() {
        return;
    }
    let attached: usize = parser::process_slide(&segment.join("\n"), 0)
        .highlights
        .iter()
        .map(Vec::len)
        .sum();
    for &n in written.iter().skip(attached) {
        findings.push(Finding {
            line: n,
            cols: trimmed_cols(segment[n - start]),
            severity: Severity::Warning,
            code: "highlight-no-image",
            message: "no image follows this highlight on the slide, so it isn't drawn — a \
                      highlight marks up the next image"
                .into(),
            fix: None,
        });
    }
}

/// Check the values of the frontmatter keys preso reads, and flag a key that
/// looks like a misspelling of one. Only top-level `key: value` lines are
/// read: preso's keys are all plain scalars.
fn check_frontmatter_values(source: &str, yaml: Range<usize>, out: &mut Analysis) {
    let lines: Vec<&str> = source.lines().collect();
    for n in yaml {
        let Some(line) = lines.get(n) else { break };
        let Some((key, raw)) = line.split_once(':') else {
            continue;
        };
        if key.is_empty() || key.starts_with([' ', '\t', '-', '#']) {
            continue;
        }
        let key = key.trim_end();
        // The value, with its quotes and any trailing comment dropped.
        let raw_at = key.len() + 1;
        let trimmed = raw.trim_start();
        let at = raw_at + (raw.len() - trimmed.len());
        let unquoted = trimmed.strip_prefix(['"', '\'']).map_or(trimmed, |v| {
            v.trim_end().strip_suffix(['"', '\'']).unwrap_or(v)
        });
        let value = unquoted.split(" #").next().unwrap_or("").trim_end();
        let at = at + usize::from(unquoted.len() != trimmed.len());
        let cols = at..at + value.len();
        let warn = |code, message: String, fix: Option<String>| Finding {
            line: n,
            cols: cols.clone(),
            severity: Severity::Warning,
            code,
            message,
            fix,
        };
        if !FRONTMATTER_KEYS.contains(&key) {
            // Other tools' keys are welcome; only a near miss is worth a word.
            if let Some(near) = suggest(key, FRONTMATTER_KEYS) {
                out.findings.push(Finding {
                    line: n,
                    cols: 0..key.len(),
                    severity: Severity::Hint,
                    code: "unknown-key",
                    message: format!(
                        "preso doesn't read `{key}` — did you mean `{near}`? (other tools' keys \
                         are fine to leave)"
                    ),
                    fix: Some(near.to_string()),
                });
            }
            continue;
        }
        if value.is_empty() {
            continue;
        }
        match key {
            "transition" if !TRANSITIONS.contains(&value) => out.findings.push(warn(
                "bad-value",
                format!("unknown transition `{value}`, so slides cut without one"),
                suggest(value, TRANSITIONS).map(str::to_string),
            )),
            // Slidev's `presenter: true|false` is fine (preso ignores it).
            "presenter"
                if !PRESENTER_LAYOUTS.contains(&value)
                    && !matches!(value, "true" | "false" | "yes" | "no") =>
            {
                out.findings.push(warn(
                    "bad-value",
                    format!(
                        "`presenter` takes `slide` or `notes`; `{value}` opens the `slide` layout"
                    ),
                    suggest(value, PRESENTER_LAYOUTS).map(str::to_string),
                ))
            }
            "aspect" if parse_aspect(value).is_none() => out.findings.push(warn(
                "bad-value",
                format!(
                    "`aspect` takes a `width:height` ratio like `16:9` or `4:3`; with \
                     `{value}`, exports use 16:9"
                ),
                None,
            )),
            "theme" if !BUILTIN_THEMES.contains(&value) => {
                push_ref(RefKind::Theme, value, n, at, &mut out.references);
            }
            _ => {}
        }
    }
}

/// A `width:height` aspect ratio of positive numbers.
fn parse_aspect(value: &str) -> Option<(f64, f64)> {
    let (w, h) = value.split_once(':')?;
    let (w, h): (f64, f64) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w > 0.0 && h > 0.0).then_some((w, h))
}

/// Check a comment line. Returns `Some(is_two_column)` for a layout
/// directive, so the caller can check the slide has a split.
fn check_comment(line: &str, n: usize, out: &mut Analysis) -> Option<bool> {
    let trimmed = line.trim();
    let lead = line.len() - line.trim_start().len();
    let whole = lead..lead + trimmed.len();
    let body = &trimmed[4..];
    let name_at = lead + 4 + (body.len() - body.trim_start().len());
    let body = body.trim_start();
    let name_len = body
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .unwrap_or(body.len());
    let name = &body[..name_len];
    let after = &body[name_len..];
    let closed = trimmed.ends_with("-->");
    let finding = |cols: Range<usize>, code, message: String, fix: Option<String>| Finding {
        line: n,
        cols,
        severity: Severity::Warning,
        code,
        message,
        fix,
    };

    // A step-gated name: `note[2]:`, `highlight[1]:`.
    let (name_is_directive, gated) = if after.starts_with(':') {
        (true, false)
    } else if after.starts_with('[') && after.contains("]:") {
        (true, true)
    } else {
        (false, false)
    };

    if !name_is_directive {
        // Bare markers must be written exactly.
        let inner = body.trim_end_matches("-->").trim();
        for marker in ["pause", "v-click"] {
            let canonical = format!("<!-- {marker} -->");
            if inner == marker && trimmed != canonical {
                out.findings.push(finding(
                    whole.clone(),
                    "marker-spelling",
                    format!("write this exactly as `{canonical}`; as written it's ignored"),
                    Some(canonical),
                ));
            }
        }
        return None;
    }

    let name_cols = name_at..name_at + name_len;
    if !DIRECTIVES.contains(&name) {
        if let Some(suggestion) = suggest(name, DIRECTIVES) {
            let mut f = finding(
                name_cols,
                "unknown-directive",
                format!(
                    "`{name}` isn't a preso directive, so this is an ordinary comment — did you mean `{suggestion}`?"
                ),
                Some(suggestion.to_string()),
            );
            // `<!-- Note: … -->` is as likely a private comment as a typo.
            if matches!(suggestion, "note" | "speaker") {
                f.severity = Severity::Hint;
            }
            out.findings.push(f);
        }
        return None;
    }
    if name == "note" && gated {
        let step = after[1..after.find(']').unwrap_or(1)].trim();
        if step.parse::<usize>().is_err() {
            let at = name_at + name_len;
            out.findings.push(finding(
                at..at + after.find(']').map_or(0, |i| i + 1),
                "note-step",
                format!(
                    "a note's step is a reveal step number, like `note[1]:`; with `[{step}]` \
                     this is an ordinary comment and the note never shows"
                ),
                None,
            ));
        }
    }
    if matches!(name, "note" | "speaker") || (gated && !matches!(name, "highlight" | "zoom")) {
        return None; // Notes may span lines; other gated names aren't directives.
    }
    if !closed {
        out.findings.push(finding(
            name_cols,
            "unclosed-directive",
            format!(
                "a `{name}:` directive must close with `-->` on the same line; this one is ignored"
            ),
            None,
        ));
        return None;
    }

    // Where a slice of `line` (a directive's spec) starts within it.
    let spec_of = |spec: &str| spec.as_ptr() as usize - line.as_ptr() as usize;
    match name {
        "layout" => {
            let spec = parser::directive(trimmed, "layout")?;
            return Some(check_layout(spec, spec_of(spec), n, out));
        }
        "slide" => {
            let spec = parser::directive(trimmed, "slide")?;
            check_slide(spec, spec_of(spec), n, out);
        }
        "video" => {
            let spec = parser::directive(trimmed, "video")?;
            let path = spec.trim();
            if path.is_empty() {
                out.findings.push(finding(
                    whole,
                    "missing-path",
                    "`video:` needs a clip path".into(),
                    None,
                ));
            } else {
                let at = spec_of(spec) + (spec.len() - spec.trim_start().len());
                push_ref(RefKind::Video, path, n, at, &mut out.references);
            }
        }
        "image" => {
            let spec = parser::directive(trimmed, "image")?;
            if parser::parse_layer_image(spec).is_none() {
                out.findings.push(finding(
                    whole,
                    "missing-path",
                    "`image:` needs an image path".into(),
                    None,
                ));
            } else {
                check_layer_image(spec, spec_of(spec), n, out);
            }
        }
        "highlight" => {
            let (step, spec) = parser::highlight_directive(trimmed)?;
            if parser::parse_highlight(step, spec).is_none() {
                out.findings.push(finding(
                    whole,
                    "highlight-dropped",
                    "this highlight is dropped: it needs a shape (`rect`, `ellipse` or `circle`), \
                     positions within 0–100%, and a `w=` and `h=` above 0"
                        .into(),
                    None,
                ));
            }
        }
        "zoom" => {
            let (_, spec) = parser::zoom_directive(trimmed)?;
            if parser::parse_zoom(spec).is_none() {
                out.findings.push(finding(
                    whole,
                    "zoom-dropped",
                    "this zoom is dropped: write the point to zoom onto and how far, as \
                     `x%,y%,Nx` — the point within 0–100% of the slide, the magnification \
                     1 or more — or `all` to zoom back out"
                        .into(),
                    None,
                ));
            }
        }
        "table" => {
            let spec = parser::directive(trimmed, "table")?;
            for (token, cols) in tokens(spec, spec_of(spec)) {
                match token.split_once('=') {
                    Some(("size", v)) if positive(v) => {}
                    Some(("size", _)) => out.findings.push(finding(
                        cols,
                        "bad-value",
                        "`size` must be a number above 0".into(),
                        None,
                    )),
                    _ => out.findings.push(finding(
                        cols,
                        "unknown-key",
                        format!("`{token}` isn't a table option (`size=NN` is)"),
                        None,
                    )),
                }
            }
        }
        "include" => {
            let path = trimmed
                .split_once(':')
                .map(|(_, rest)| rest.trim_end_matches("-->").trim())
                .unwrap_or("");
            // `include` is matched more strictly than every other directive.
            if !trimmed.starts_with("<!-- include:") {
                out.findings.push(finding(
                    whole.clone(),
                    "include-spelling",
                    "an include only works written as `<!-- include: path -->` — as written it's ignored"
                        .into(),
                    Some(format!("<!-- include: {path} -->")),
                ));
            }
            if path.is_empty() {
                out.findings.push(finding(
                    whole,
                    "missing-path",
                    "`include:` needs a markdown file path".into(),
                    None,
                ));
            } else {
                push_ref(
                    RefKind::Include,
                    path,
                    n,
                    spec_of(path),
                    &mut out.references,
                );
            }
        }
        "footnote" => {
            let text = parser::directive(trimmed, "footnote")?;
            if text.trim().is_empty() {
                out.findings.push(finding(
                    whole,
                    "missing-text",
                    "this footnote is empty, so it's ignored".into(),
                    None,
                ));
            }
        }
        _ => {}
    }
    None
}

/// Check a fenced block's `{…}` annotation against its contents: a `zoom`
/// with nothing to zoom onto, and stage lines past the block's end. A
/// diagram's zoom labels are collected for the caller to match.
fn check_fence(opener: &str, n: usize, body: &[&str], out: &mut Analysis) {
    let (_, block) = parser::clean_fence_line(opener);
    let (Some(annotation), Some(brace)) = (block.annotation.as_deref(), opener.find('{')) else {
        return;
    };
    let cols = brace..brace + annotation.len();
    let diagram = block
        .language
        .as_deref()
        .is_some_and(|l| DIAGRAM_LANGUAGES.contains(&l));
    let stages = 0..block.stage_count();
    let warn = |cols: Range<usize>, code, message: String| Finding {
        line: n,
        cols,
        severity: Severity::Warning,
        code,
        message,
        fix: None,
    };

    if block.zooms() {
        let targets = stages.clone().any(|s| {
            if diagram {
                block.zoom_labels_at(s).is_some()
            } else {
                block.zoom_lines_at(s).is_some()
            }
        });
        if !targets {
            let zoom_word = annotation
                .split(|c: char| c.is_whitespace() || matches!(c, '{' | '}' | '|' | ','))
                .find(|w| *w == "zoom")
                .map(|w| {
                    let at = brace + (w.as_ptr() as usize - annotation.as_ptr() as usize);
                    at..at + w.len()
                })
                .unwrap_or(cols.clone());
            let example = if diagram {
                "`{all|Parse zoom}` zooms onto the node labelled Parse"
            } else {
                "`{all|3-4 zoom}` zooms onto lines 3–4"
            };
            out.findings.push(warn(
                zoom_word,
                "zoom-nothing",
                format!(
                    "`zoom` zooms onto each `|` stage's {}, but no stage names any, so \
                     nothing zooms — e.g. {example}",
                    if diagram { "nodes" } else { "lines" }
                ),
            ));
        }
        if diagram {
            let mut labels: Vec<(String, Range<usize>)> = Vec::new();
            for s in stages.clone() {
                for (label, span) in block.zoom_label_spans_at(s).unwrap_or_default() {
                    let span = brace + span.start..brace + span.end;
                    if !labels.iter().any(|(_, r)| *r == span) {
                        labels.push((label, span));
                    }
                }
            }
            if !labels.is_empty() {
                out.diagrams.push(DiagramZoom {
                    language: block.language.clone().unwrap_or_default(),
                    source: body.join("\n"),
                    line: n,
                    labels,
                });
            }
        }
    }

    check_fence_tokens(annotation, brace, diagram, n, &mut out.findings);

    if !diagram
        && let Some(past) = stages
            .filter_map(|s| block.highlighted_lines_at(s))
            .filter_map(|lines| lines.last().copied())
            .filter(|&last| last > body.len())
            .max()
    {
        let what = if block.zooms() {
            "highlighted or zoomed onto"
        } else {
            "highlighted"
        };
        out.findings.push(warn(
            cols,
            "line-out-of-range",
            format!(
                "this block has {} line{}, so line {past} is never {what}",
                body.len(),
                if body.len() == 1 { "" } else { "s" }
            ),
        ));
    }
}

/// Bare words a fence annotation understands besides line numbers.
pub const FENCE_FLAGS: &[&str] = &["zoom", "transparent", "dim", "background", "all", "none"];

/// `key=` options a fence annotation understands.
pub const FENCE_KEYS: &[&str] = &["width", "size", "align"];

/// Flag the tokens of a fence's `{…}` the parser ignores, and options with
/// values it can't use. A diagram's bare words are node labels for its zoom
/// stages, so only its flags and `key=value` options are checked.
fn check_fence_tokens(
    annotation: &str,
    brace: usize,
    diagram: bool,
    n: usize,
    findings: &mut Vec<Finding>,
) {
    let is_lines = |t: &str| {
        t.split('-').count() <= 2
            && t.split('-')
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    };
    let tokens = annotation
        .split(|c: char| c.is_whitespace() || matches!(c, '{' | '}' | '|' | ','))
        .filter(|t| !t.is_empty());
    for token in tokens {
        let at = brace + (token.as_ptr() as usize - annotation.as_ptr() as usize);
        let cols = at..at + token.len();
        let problem = match token.split_once('=') {
            Some(("width", v)) => {
                let pct = v.strip_suffix('%').and_then(|p| p.parse::<f32>().ok());
                match (pct, v.parse::<f32>()) {
                    (Some(p), _) if p > 0.0 && p <= 100.0 => None,
                    (_, Ok(p)) if p > 0.0 && p <= 100.0 => Some((
                        format!(
                            "`width` is a percentage: `width={v}%`; without the `%` it's ignored"
                        ),
                        Some(format!("width={v}%")),
                    )),
                    _ => Some((
                        format!(
                            "`width` takes a percentage from 1 to 100%, like `width=60%`; `{v}` is ignored"
                        ),
                        None,
                    )),
                }
            }
            Some(("size", v)) if !positive(v) => Some((
                format!("`size` takes a code font size above 0, like `size=22`; `{v}` is ignored"),
                None,
            )),
            Some(("align", v)) if !matches!(v, "left" | "center" | "right") => Some((
                format!("`align` takes `left`, `center` or `right`; `{v}` is ignored"),
                suggest(v, &["left", "center", "right"]).map(|a| format!("align={a}")),
            )),
            Some((key, v)) if !FENCE_KEYS.contains(&key) => Some((
                format!(
                    "`{key}` isn't a code block option (`width=`, `size=` and `align=` are), so it's ignored"
                ),
                suggest(key, FENCE_KEYS).map(|k| format!("{k}={v}")),
            )),
            Some(_) => None,
            None if FENCE_FLAGS.contains(&token) || is_lines(token) || diagram => None,
            None => Some((
                format!(
                    "`{token}` means nothing here, so it's ignored — a code block takes line \
                     numbers and stages (`2,4-6`, `1|3|all`), `zoom`, `dim`, `background`, \
                     `width=`, `size=` and `align=`"
                ),
                suggest(token, FENCE_FLAGS).map(str::to_string),
            )),
        };
        if let Some((message, fix)) = problem {
            findings.push(Finding {
                line: n,
                cols,
                severity: Severity::Warning,
                code: "fence-option",
                message,
                fix,
            });
        }
    }
}

/// Returns whether the layout is `TwoColumn`.
fn check_layout(spec: &str, at: usize, n: usize, out: &mut Analysis) -> bool {
    let mut toks = tokens(spec, at);
    let Some((first, cols)) = toks.next() else {
        out.findings.push(Finding {
            line: n,
            cols: at..at + spec.len(),
            severity: Severity::Warning,
            code: "unknown-layout",
            message: "empty layout: this slide renders as `Content`".into(),
            fix: None,
        });
        return false;
    };
    match first {
        "TwoColumn" => {
            if let Some((ratio, cols)) = toks.next()
                && parser::parse_ratio(ratio).is_none()
            {
                out.findings.push(Finding {
                    line: n,
                    cols,
                    severity: Severity::Warning,
                    code: "layout-ratio",
                    message: format!(
                        "`{ratio}` isn't a `left:right` ratio of whole numbers above 0, so the columns split 1:1"
                    ),
                    fix: None,
                });
            }
            true
        }
        "Content" => false,
        other => {
            let fix = suggest(other, &["TwoColumn", "Content"]).map(str::to_string);
            out.findings.push(Finding {
                line: n,
                cols,
                severity: Severity::Warning,
                code: "unknown-layout",
                message: format!(
                    "unknown layout `{other}` (layouts are `TwoColumn` and `Content`, case-sensitive): this slide renders as `Content`"
                ),
                fix,
            });
            false
        }
    }
}

fn check_slide(spec: &str, at: usize, n: usize, out: &mut Analysis) {
    let keys: Vec<&str> = SLIDE_KEYS.iter().map(|(k, _)| *k).collect();
    let warn = |cols, code, message: String, fix: Option<String>| Finding {
        line: n,
        cols,
        severity: Severity::Warning,
        code,
        message,
        fix,
    };
    for (token, cols) in tokens(spec, at) {
        let Some((key, value)) = token.split_once('=') else {
            if SLIDE_FLAGS.contains(&token) {
                continue;
            }
            let message = if keys.contains(&token) {
                format!("`{token}` needs a value: `{token}=…`")
            } else {
                format!("`{token}` isn't a slide option, so it's ignored")
            };
            let fix = suggest(token, SLIDE_FLAGS).map(str::to_string);
            out.findings.push(warn(cols, "unknown-key", message, fix));
            continue;
        };
        let value_cols = cols.start + key.len() + 1..cols.end;
        let Some((_, allowed)) = SLIDE_KEYS.iter().find(|(k, _)| *k == key) else {
            let fix = suggest(key, &keys).map(|k| format!("{k}={value}"));
            out.findings.push(warn(
                cols,
                "unknown-key",
                format!("`{key}` isn't a slide option, so it's ignored"),
                fix,
            ));
            continue;
        };
        let bad = match key {
            "background" if value.starts_with('#') => {
                (!is_hex_colour(value)).then(|| "`#rgb`, `#rrggbb` or `#rrggbbaa`".to_string())
            }
            "background" if value.is_empty() => Some("a `#colour` or an image path".to_string()),
            "background" => {
                push_ref(
                    RefKind::Background,
                    value,
                    n,
                    value_cols.start,
                    &mut out.references,
                );
                None
            }
            "fill" => (!is_hex_colour(value))
                .then(|| "a `#rgb`, `#rrggbb` or `#rrggbbaa` colour".to_string()),
            "number" => value
                .parse::<usize>()
                .is_err()
                .then(|| "a whole number".to_string()),
            "size" => (!positive(value)).then(|| "a number above 0".to_string()),
            // The renderer also takes `fit` for `contain`, `fill` for `stretch`.
            "fit" if matches!(value, "fit" | "fill") => None,
            _ if !allowed.is_empty() && !allowed.contains(&value) => Some(
                allowed
                    .iter()
                    .map(|v| format!("`{v}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            _ => None,
        };
        if let Some(expected) = bad {
            let fix = suggest(value, allowed).map(str::to_string);
            out.findings.push(warn(
                value_cols,
                "bad-value",
                format!("`{key}` takes {expected}; `{value}` is ignored"),
                fix,
            ));
        }
    }
}

fn check_layer_image(spec: &str, at: usize, n: usize, out: &mut Analysis) {
    const KEYS: &[&str] = &[
        "position",
        "width",
        "opacity",
        "padding",
        "padding-top",
        "padding-right",
        "padding-bottom",
        "padding-left",
    ];
    let mut toks = tokens(spec, at);
    if let Some((path, cols)) = toks.next() {
        push_ref(
            RefKind::LayerImage,
            path,
            n,
            cols.start,
            &mut out.references,
        );
    }
    for (token, cols) in toks {
        let (key, value) = token.split_once('=').unwrap_or((token, ""));
        let problem = if !KEYS.contains(&key) {
            Some((
                "unknown-key",
                format!("`{key}` isn't an image option, so it's ignored"),
                suggest(key, KEYS).map(|k| format!("{k}={value}")),
            ))
        } else if key == "position" && !ANCHORS.contains(&value) {
            Some((
                "bad-value",
                format!("unknown position `{value}`: the image is centred"),
                suggest(value, ANCHORS).map(|a| format!("position={a}")),
            ))
        } else if key == "width"
            && !value
                .trim_end_matches('%')
                .parse::<f32>()
                .is_ok_and(|w| w > 0.0)
        {
            Some((
                "bad-value",
                format!(
                    "`width` takes a percentage of the slide width above 0, like `width=40%`; \
                     `{value}` is ignored, so the image keeps its own size"
                ),
                None,
            ))
        } else if key == "opacity" && value.parse::<f32>().is_err() {
            Some((
                "bad-value",
                format!("`opacity` takes a number from 0 to 1; `{value}` is ignored"),
                None,
            ))
        } else if key.starts_with("padding") && value.parse::<f32>().is_err() {
            Some((
                "bad-value",
                format!("`{key}` takes a number (design units); `{value}` is ignored"),
                None,
            ))
        } else {
            None
        };
        if let Some((code, message, fix)) = problem {
            out.findings.push(Finding {
                line: n,
                cols,
                severity: Severity::Warning,
                code,
                message,
                fix,
            });
        }
    }
}

/// Collect `![alt](path)` references in a markdown line, skipping inline
/// code spans.
fn scan_images(line: &str, n: usize, refs: &mut Vec<Reference>) {
    let code = code_spans(line);
    let mut from = 0;
    while let Some(found) = line[from..].find("![") {
        let start = from + found;
        from = start + 2;
        if code.iter().any(|c| c.contains(&start)) {
            continue;
        }
        let Some(close) = line[from..].find("](") else {
            return;
        };
        let mut at = from + close + 2;
        let rest = &line[at..];
        let path = if let Some(inner) = rest.strip_prefix('<') {
            at += 1;
            &inner[..inner.find('>').unwrap_or(inner.len())]
        } else {
            let end = rest
                .find(|c: char| c == ')' || c.is_whitespace())
                .unwrap_or(rest.len());
            &rest[..end]
        };
        push_ref(RefKind::Image, path, n, at, refs);
        from = at + path.len();
    }
}

/// An image's `{…}` group with any token the parser doesn't know is left as
/// literal text — the image loses all its styling and the braces show on
/// the slide. Flag it, with a fix when every bad token has an obvious
/// correction.
fn check_image_attrs(line: &str, n: usize, findings: &mut Vec<Finding>) {
    for span in crate::style::image_spans(line) {
        let Some(attrs) = span.attrs else { continue };
        let spec = &line[attrs.clone()];
        if spec.trim().is_empty() || parser::encode_image_attrs(spec).is_some() {
            continue;
        }
        let tokens: Vec<&str> = spec.split_whitespace().collect();
        let bad: Vec<&str> = tokens
            .iter()
            .copied()
            .filter(|t| parser::encode_image_attrs(t).is_none())
            .collect();
        let fixed: Option<Vec<String>> = tokens
            .iter()
            .map(|t| {
                if parser::encode_image_attrs(t).is_some() {
                    Some(t.to_string())
                } else {
                    fix_image_attr(t)
                }
            })
            .collect();
        findings.push(Finding {
            line: n,
            cols: attrs.start - 1..attrs.end + 1,
            severity: Severity::Warning,
            code: "image-attrs",
            message: format!(
                "these image attributes are ignored, and the braces show on the slide: {} \
                 (image attributes are `width=NN%`, `align=left|center|right`, `border`, \
                 `shadow`, `plain` and `fit`)",
                bad.iter()
                    .map(|t| format!("`{t}` isn't one"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            fix: fixed.map(|t| format!("{{{}}}", t.join(" "))),
        });
    }
}

/// The attribute `token` was most likely meant to be.
fn fix_image_attr(token: &str) -> Option<String> {
    match token.split_once('=') {
        Some((key, value)) => match suggest(key, &["width", "align"])? {
            "width" => {
                let pct = value.trim_end_matches('%');
                pct.parse::<f32>()
                    .ok()
                    .filter(|p| *p > 0.0 && *p <= 100.0)
                    .map(|_| format!("width={pct}%"))
            }
            _ => suggest(value, &["left", "center", "right"]).map(|v| format!("align={v}")),
        },
        None => suggest(token, &["border", "shadow", "plain", "fit"]).map(str::to_string),
    }
}

/// Record a reference unless it's a URL or empty. `at` is where `raw`
/// starts in the line; a `#fragment` or `?query` is dropped from the path.
fn push_ref(kind: RefKind, raw: &str, n: usize, at: usize, refs: &mut Vec<Reference>) {
    let is_url = raw.contains("://") || raw.starts_with("data:") || raw.starts_with("mailto:");
    let path = raw.split(['#', '?']).next().unwrap_or("");
    if is_url || path.is_empty() {
        return;
    }
    refs.push(Reference {
        kind,
        path: path.to_string(),
        line: n,
        cols: at..at + path.len(),
    });
}

/// Byte ranges of inline code spans (`` `…` ``, matched by run length).
pub(crate) fn code_spans(line: &str) -> Vec<Range<usize>> {
    let bytes = line.as_bytes();
    let run_at = |i: usize| bytes[i..].iter().take_while(|&&b| b == b'`').count();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let len = run_at(i);
        let mut j = i + len;
        let mut closed = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let l = run_at(j);
                if l == len {
                    closed = Some(j + l);
                    break;
                }
                j += l;
            } else {
                j += 1;
            }
        }
        match closed {
            Some(end) => {
                spans.push(i..end);
                i = end;
            }
            None => i += len,
        }
    }
    spans
}

/// Whitespace-separated tokens with their columns, given the spec starts at
/// column `at`.
fn tokens(spec: &str, at: usize) -> impl Iterator<Item = (&str, Range<usize>)> {
    spec.split_whitespace().map(move |t| {
        let offset = t.as_ptr() as usize - spec.as_ptr() as usize;
        (t, at + offset..at + offset + t.len())
    })
}

fn trimmed_cols(line: &str) -> Range<usize> {
    let lead = line.len() - line.trim_start().len();
    lead..lead + line.trim().len()
}

fn positive(v: &str) -> bool {
    v.parse::<f32>().is_ok_and(|n| n > 0.0)
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa` — what `preso_style::Color::parse`
/// accepts.
pub fn is_hex_colour(v: &str) -> bool {
    v.strip_prefix('#').is_some_and(|hex| {
        matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// The candidate `word` most plausibly meant: an exact case-insensitive
/// match, else the nearest within two edits (one for short words).
pub(crate) fn suggest<'a>(word: &str, candidates: &[&'a str]) -> Option<&'a str> {
    if word.is_empty() {
        return None;
    }
    let lower = word.to_ascii_lowercase();
    if let Some(c) = candidates.iter().find(|c| c.to_ascii_lowercase() == lower) {
        return Some(c);
    }
    let limit = if word.len() <= 4 { 1 } else { 2 };
    candidates
        .iter()
        .map(|c| (distance(&lower, &c.to_ascii_lowercase()), *c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

/// Levenshtein distance.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(source: &str) -> Vec<&'static str> {
        check(source).findings.iter().map(|f| f.code).collect()
    }

    fn only(source: &str) -> Finding {
        let findings = check(source).findings;
        assert_eq!(findings.len(), 1, "{findings:#?}");
        findings.into_iter().next().unwrap()
    }

    // --- The lists above mirror the parser; these keep them in step. ---

    #[test]
    fn every_listed_directive_is_one_the_parser_acts_on() {
        for name in DIRECTIVES {
            if *name == "include" {
                // Expanded before parsing, by `include::expand`.
                let expanded = crate::include::expand(
                    "<!-- include: none.md -->\n",
                    std::path::Path::new("/nonexistent"),
                );
                assert!(expanded.is_err(), "include is read");
                continue;
            }
            // A directive is consumed; an ordinary comment is left in the slide.
            let src = format!("# x\n<!-- {name}: a -->\n");
            let slide = &parser::parse(&src).unwrap().slides[0];
            assert!(
                !slide.source.contains("<!--"),
                "{name} is left as a comment"
            );
        }
        let src = "# x\n<!-- nonsense: a -->\n";
        assert!(
            parser::parse(src).unwrap().slides[0]
                .source
                .contains("<!--")
        );
    }

    #[test]
    fn every_listed_slide_option_changes_the_slide() {
        let plain = parser::parse("# x\n").unwrap().slides[0].overrides.clone();
        for (key, values) in SLIDE_KEYS {
            let sample = match *key {
                "background" | "fill" => "#123456",
                "number" | "size" => "7",
                _ => values[0],
            };
            let src = format!("# x\n<!-- slide: {key}={sample} -->\n");
            let set = &parser::parse(&src).unwrap().slides[0].overrides;
            assert_ne!(*set, plain, "{key}={sample}");
        }
        for flag in SLIDE_FLAGS {
            // `hidden` drops the slide.
            let src = format!("# x\n<!-- slide: {flag} -->\n\n---\n\n# y\n");
            assert_eq!(parser::parse(&src).unwrap().slides.len(), 1, "{flag}");
        }
    }

    #[test]
    fn every_listed_anchor_places_the_image_differently() {
        let positions: std::collections::HashSet<String> = ANCHORS
            .iter()
            .map(|a| {
                let image = parser::parse_layer_image(&format!("x.png position={a}")).unwrap();
                format!("{:?}", image.position)
            })
            .collect();
        assert_eq!(positions.len(), ANCHORS.len());
    }

    #[test]
    fn every_offered_attribute_is_one_the_parser_reads() {
        for attr in crate::complete::IMAGE_ATTRS {
            assert!(parser::encode_image_attrs(attr).is_some(), "image {attr}");
        }
        for attr in crate::complete::FENCE_ATTRS.iter().chain(FENCE_FLAGS) {
            let src = format!("```rust {{{attr}}}\na\n```\n");
            assert!(
                check(&src)
                    .findings
                    .iter()
                    .all(|f| f.code != "fence-option"),
                "fence {attr}"
            );
        }
        let spec = |extra: &str| format!("rect x=1 y=1 w=5 h=5 {extra}");
        for word in crate::complete::HIGHLIGHT_WORDS {
            let token = match *word {
                "color=" => "color=#fff".to_string(),
                "mode=" => "mode=fill".to_string(),
                w if w.ends_with('=') => format!("{w}1"),
                w => w.to_string(),
            };
            assert!(
                parser::parse_highlight(None, &spec(&token)).is_some(),
                "highlight {token}"
            );
        }
        for mode in crate::complete::HIGHLIGHT_MODES {
            assert!(
                parser::parse_highlight(None, &spec(&format!("mode={mode}"))).is_some(),
                "{mode}"
            );
        }
        for shape in crate::complete::HIGHLIGHT_SHAPES {
            assert!(parser::parse_highlight(None, &format!("{shape} x=1 y=1 w=5 h=5")).is_some());
        }
        for key in crate::complete::LAYER_IMAGE_KEYS {
            let src = format!("# x\n<!-- image: a.png {key}1 -->\n");
            assert!(
                check(&src).findings.iter().all(|f| f.code != "unknown-key"),
                "image {key}"
            );
        }
    }

    #[test]
    fn frontmatter_values_are_checked() {
        let f = only("---\ntransition: wipee\n---\n# x\n");
        assert_eq!((f.code, f.fix.as_deref()), ("bad-value", Some("wipe")));
        assert_eq!((f.line, f.cols.clone()), (1, 12..17));
        let f = only("---\npresenter: \"note\"\n---\n# x\n");
        assert_eq!((f.code, f.fix.as_deref()), ("bad-value", Some("notes")));
        // Pointing inside the quotes.
        assert_eq!(f.cols, 12..16);
        assert_eq!(only("---\naspect: wide\n---\n# x\n").code, "bad-value");
        // A near miss of a key gets a hint; other tools' keys are left alone.
        let f = only("---\ntheem: dark\n---\n# x\n");
        assert_eq!(
            (f.code, f.severity, f.fix.as_deref()),
            ("unknown-key", Severity::Hint, Some("theme"))
        );
        let fine = "---\ntitle: T\ntheme: dark\ntransition: pan-up\naspect: \"4:3\"\n\
                    presenter: false\nlayout: cover\nhighlighter: shiki\n---\n# x\n";
        assert_eq!(check(fine).findings, vec![]);
    }

    #[test]
    fn a_theme_path_is_a_reference() {
        let refs = check("---\ntheme: themes/corp.toml # house style\n---\n# x\n").references;
        let [r] = refs.as_slice() else {
            panic!("{refs:?}")
        };
        assert_eq!(
            (r.kind, r.path.as_str(), r.line, r.cols.clone()),
            (RefKind::Theme, "themes/corp.toml", 1, 7..23)
        );
        // A built-in is not a file.
        assert!(check("---\ntheme: light\n---\n# x\n").references.is_empty());
    }

    #[test]
    fn directives_the_parser_drops_quietly() {
        let f = only("# x\n<!-- note[x]: hi -->\n");
        assert_eq!((f.code, f.cols.clone()), ("note-step", 9..12));
        assert!(codes("# x\n<!-- note[2]: hi -->\n").is_empty());
        assert_eq!(only("# x\n<!-- footnote:   -->\n").code, "missing-text");
        let image = check("# x\n<!-- image: a.png width=wide opacity=half padding=lots -->\n");
        let bad: Vec<_> = image.findings.iter().map(|f| f.code).collect();
        assert_eq!(bad, ["bad-value"; 3]);
        assert!(codes("# x\n<!-- image: a.png width=40% opacity=0.5 padding=12 -->\n").is_empty());
    }

    #[test]
    fn a_highlight_with_no_image_after_it_is_flagged() {
        let f = only("# x\n![](a.png)\n<!-- highlight: rect x=1 y=1 w=5 h=5 -->\n");
        assert_eq!((f.code, f.line), ("highlight-no-image", 2));
        // Only the leftovers: the first attaches to the image after it.
        let src = "# x\n<!-- highlight: rect x=1 y=1 w=5 h=5 -->\n![](a.png)\n\
                   <!-- highlight[1]: ellipse x=1 y=1 w=5 h=5 -->\n";
        assert_eq!((only(src).line), 3);
        // Image rows and images beside text count as the parser counts them.
        for ok in [
            "<!-- highlight: rect x=1 y=1 w=5 h=5 -->\n![](a.png)\n![](b.png)\n",
            "<!-- highlight: rect x=1 y=1 w=5 h=5 -->\n![](icon.png) A label\n",
        ] {
            assert!(codes(&format!("# x\n{ok}")).is_empty(), "{ok}");
        }
    }

    #[test]
    fn an_unclosed_math_block_is_flagged() {
        let f = only("# x\n$$\nx^2\n\nMore text\n\n---\n\n# y\n");
        assert_eq!((f.code, f.line), ("unclosed-math", 1));
        assert!(codes("# x\n$$\nx^2\n$$\n\nMore\n").is_empty());
        assert!(codes("# x\n$$ x^2 $$\nMore\n").is_empty());
    }

    #[test]
    fn fence_options_the_parser_ignores_are_flagged() {
        let found =
            check("```rust {sise=20 dimm width=50 align=middle 2,4-6}\na\nb\nc\nd\ne\nf\n```\n");
        let fixes: Vec<_> = found
            .findings
            .iter()
            .map(|f| (f.code, f.fix.as_deref()))
            .collect();
        assert_eq!(
            fixes,
            [
                ("fence-option", Some("size=20")),
                ("fence-option", Some("dim")),
                ("fence-option", Some("width=50%")),
                ("fence-option", None),
            ]
        );
        assert_eq!(codes("```rust {width=150%}\na\n```\n"), ["fence-option"]);
        // Everything the parser reads is fine, and a diagram's words are labels.
        assert!(codes("```rust {all|2,4-5|6 zoom dim size=22 width=80% align=center}\na\nb\nc\nd\ne\nf\n```\n").is_empty());
        assert!(
            check("```mermaid {all|Parse input zoom transparent}\ngraph LR\n```\n")
                .findings
                .is_empty()
        );
    }

    #[test]
    fn a_zoom_the_parser_drops_is_flagged() {
        // What the parser keeps, lint leaves alone.
        for ok in [
            "<!-- zoom[1]: 40%,20%,2x -->",
            "<!-- zoom[2]: all -->",
            "<!-- zoom: 75 60 3 -->",
        ] {
            assert!(codes(&format!("# x\n{ok}\n")).is_empty(), "{ok}");
        }
        for bad in [
            "<!-- zoom[1]: 140%,20%,2x -->",
            "<!-- zoom[1]: 40%,20% -->",
            "<!-- zoom: banana -->",
            "<!-- zoom[1]: 50%,50%,0.5x -->",
        ] {
            let src = format!("# x\n{bad}\n");
            // Exactly the lines the parser drops.
            let slide = &parser::parse(&src).unwrap().slides[0];
            assert!(slide.zooms.is_empty(), "{bad}");
            assert_eq!(only(&src).code, "zoom-dropped", "{bad}");
        }
        // A misspelling is caught now that `zoom` is a directive.
        let f = only("# x\n<!-- zom[1]: 40%,20%,2x -->\n");
        assert_eq!(
            (f.code, f.fix.as_deref()),
            ("unknown-directive", Some("zoom"))
        );
    }

    #[test]
    fn a_zoom_flag_with_nothing_to_zoom_onto_is_flagged() {
        for src in [
            "```rust {zoom}\na\n```\n",
            "```rust {all zoom}\na\nb\n```\n",
            "```mermaid {zoom}\ngraph LR\n  a\n```\n",
        ] {
            let f = only(src);
            assert_eq!(f.code, "zoom-nothing", "{src}");
            // Pointing at the `zoom` word itself.
            let line = src.lines().next().unwrap();
            assert_eq!(&line[f.cols.clone()], "zoom", "{src}");
        }
        // Stages to zoom onto: fine, as is a block that doesn't zoom.
        assert!(codes("```rust {all|2 zoom}\na\nb\n```\n").is_empty());
        assert!(codes("```rust {2}\na\nb\n```\n").is_empty());
    }

    #[test]
    fn stage_lines_past_the_end_of_the_block_are_flagged() {
        let f = only("```rust {all|2|5 zoom}\na\nb\nc\n```\n");
        assert_eq!(f.code, "line-out-of-range");
        assert!(
            f.message.contains("3 lines") && f.message.contains("line 5"),
            "{}",
            f.message
        );
        assert!(f.message.contains("zoomed onto"), "{}", f.message);
        // Plain highlights too, and a fence left open runs to the slide's end.
        assert_eq!(codes("```rust {9}\na\n```\n"), ["line-out-of-range"]);
        assert_eq!(codes("# x\n```rust {3}\na\nb\n"), ["line-out-of-range"]);
        assert!(codes("# x\n```rust {2}\na\nb\n").is_empty());
    }

    #[test]
    fn diagram_zoom_labels_are_handed_on_with_their_place() {
        let src = "# x\n```mermaid {all|Layout|Read, Parse input zoom}\ngraph LR\n  a[Read]\n```\n";
        let analysis = check(src);
        assert!(analysis.findings.is_empty(), "{:?}", analysis.findings);
        let [diagram] = analysis.diagrams.as_slice() else {
            panic!("{:?}", analysis.diagrams);
        };
        assert_eq!(diagram.language, "mermaid");
        assert_eq!(diagram.line, 1);
        assert_eq!(diagram.source, "graph LR\n  a[Read]");
        let line = src.lines().nth(1).unwrap();
        let named: Vec<(&str, &str)> = diagram
            .labels
            .iter()
            .map(|(label, cols)| (label.as_str(), &line[cols.clone()]))
            .collect();
        assert_eq!(
            named,
            [
                ("Layout", "Layout"),
                ("Read", "Read"),
                ("Parse input", "Parse input")
            ]
        );
        // A diagram that doesn't zoom has nothing to hand on.
        assert!(
            check("```mermaid {Layout}\ngraph LR\n```\n")
                .diagrams
                .is_empty()
        );
    }

    #[test]
    fn the_example_talk_lints_clean() {
        // It uses every kind of zoom, the pan transitions and most
        // directives: a finding here is a false alarm.
        let src = include_str!("../../../docs/example-talk.md");
        let analysis = check(src);
        assert_eq!(analysis.findings, vec![]);
        assert_eq!(
            analysis.diagrams.len(),
            1,
            "the zooming architecture diagram"
        );
    }

    #[test]
    fn a_clean_deck_has_no_findings() {
        let src = "---\ntitle: T\n---\n\n<!-- layout: TwoColumn 2:1 -->\n# A\n\nleft\n\n***\n\n\
                   right\n<!-- slide: kind=title align=center background=#112233 hidden -->\n\
                   <!-- note: hi -->\n<!-- pause -->\n<!-- highlight[1]: rect x=1 y=2 w=3 h=4 -->\n\
                   ![x](a.png)\n<!-- table: size=24 -->\n<!-- image: logo.png position=top-right -->\n\
                   <!-- TODO: an ordinary comment -->\n";
        assert_eq!(check(src).findings, vec![]);
    }

    #[test]
    fn misspelt_directive_suggests_the_real_one() {
        let f = only("<!-- layuot: TwoColumn -->\n");
        assert_eq!(f.code, "unknown-directive");
        assert_eq!(f.cols, 5..11);
        assert_eq!(f.fix.as_deref(), Some("layout"));
        // `Note:` is a hint: it may be a private comment.
        assert_eq!(only("<!-- Notes: remember -->\n").severity, Severity::Hint);
        assert_eq!(codes("<!-- nonsense: x -->\n"), Vec::<&str>::new());
    }

    #[test]
    fn layout_problems() {
        let f = only("<!-- layout: twocolumn -->\n");
        assert_eq!(
            (f.code, f.fix.as_deref()),
            ("unknown-layout", Some("TwoColumn"))
        );
        assert_eq!(f.cols, 13..22);
        let f = only("<!-- layout: TwoColumn 60-40 -->\na\n***\nb\n");
        assert_eq!((f.code, f.cols), ("layout-ratio", 23..28));
        assert_eq!(
            codes("<!-- layout: TwoColumn -->\nall one column\n"),
            vec!["two-column-no-split"]
        );
        // A `***` inside a fence doesn't split.
        assert_eq!(
            codes("<!-- layout: TwoColumn -->\n```\n***\n```\n"),
            vec!["two-column-no-split"]
        );
    }

    #[test]
    fn slide_option_problems() {
        let f = only("<!-- slide: knd=title -->\n");
        assert_eq!(
            (f.code, f.fix.as_deref()),
            ("unknown-key", Some("kind=title"))
        );
        let f = only("<!-- slide: kind=Title -->\n");
        assert_eq!((f.code, f.fix.as_deref()), ("bad-value", Some("title")));
        assert_eq!(f.cols, 17..22);
        assert_eq!(
            codes("<!-- slide: background=#12345 -->\n"),
            vec!["bad-value"]
        );
        assert_eq!(
            codes("<!-- slide: size=0 number=x -->\n"),
            vec!["bad-value", "bad-value"]
        );
        assert_eq!(codes("<!-- slide: hiden -->\n"), vec!["unknown-key"]);
        assert_eq!(codes("<!-- slide: kind -->\n"), vec!["unknown-key"]);
        assert_eq!(
            codes("<!-- slide: fit=fit transition=push -->\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn dropped_highlights_and_layer_images() {
        assert_eq!(
            codes("<!-- highlight: rect x=10 y=10 -->\n"),
            vec!["highlight-dropped"]
        );
        assert_eq!(
            codes("<!-- highlight: star w=1 h=1 -->\n"),
            vec!["highlight-dropped"]
        );
        assert_eq!(codes("<!-- image: -->\n"), vec!["missing-path"]);
        let f = only("<!-- image: a.png position=middle -->\n");
        assert_eq!(f.code, "bad-value");
    }

    #[test]
    fn directives_must_close_on_their_line() {
        let f = only("<!-- slide: kind=title\n-->\n");
        assert_eq!(f.code, "unclosed-directive");
        // …but notes may span lines.
        assert_eq!(codes("<!-- note: one\ntwo -->\n"), Vec::<&str>::new());
    }

    #[test]
    fn marker_and_include_spellings() {
        let f = only("<!--pause-->\n");
        assert_eq!(
            (f.code, f.fix.as_deref()),
            ("marker-spelling", Some("<!-- pause -->"))
        );
        let f = only("<!--include: ch1.md-->\n");
        assert_eq!(
            (f.code, f.fix.as_deref()),
            ("include-spelling", Some("<!-- include: ch1.md -->"))
        );
    }

    #[test]
    fn nothing_is_checked_inside_fences_notes_or_math() {
        let src = "```\n<!-- layuot: x -->\n![](missing.png)\n```\n\
                   <!-- note: see\n<!-- slide: bogus -->\n-->\n$$\n<!-- layout: nope -->\n$$\n";
        let a = check(src);
        assert_eq!(a.findings, vec![]);
        assert_eq!(a.references, vec![]);
    }

    #[test]
    fn frontmatter_errors_point_into_the_yaml() {
        let f = only("---\ntitle: [unclosed\n---\n# A\n");
        assert_eq!(f.code, "frontmatter");
        assert_eq!(f.severity, Severity::Error);
        assert!((1..2).contains(&f.line), "{f:?}");
    }

    #[test]
    fn references_with_their_columns() {
        let src = "Text ![a](img/x.png#preso-img=width:30) and ![b](<my pic.jpg>)\n\
                   `![not](code.png)` ![c](https://example.com/x.png)\n\
                   <!-- video: clip.mp4 -->\n<!-- image: logo.svg position=top -->\n\
                   <!-- slide: background=bg.jpg -->\n<!-- include: ch/one.md -->\n";
        let refs = check(src).references;
        let got: Vec<_> = refs
            .iter()
            .map(|r| (r.kind, r.path.as_str(), r.line))
            .collect();
        assert_eq!(
            got,
            vec![
                (RefKind::Image, "img/x.png", 0),
                (RefKind::Image, "my pic.jpg", 0),
                (RefKind::Video, "clip.mp4", 2),
                (RefKind::LayerImage, "logo.svg", 3),
                (RefKind::Background, "bg.jpg", 4),
                (RefKind::Include, "ch/one.md", 5),
            ]
        );
        let line0 = src.lines().next().unwrap();
        assert_eq!(&line0[refs[0].cols.clone()], "img/x.png");
        assert_eq!(&line0[refs[1].cols.clone()], "my pic.jpg");
        let line4 = src.lines().nth(4).unwrap();
        assert_eq!(&line4[refs[4].cols.clone()], "bg.jpg");
    }

    #[test]
    fn ignored_image_attributes() {
        let f = only("![x](a.png){width=50% boarder}\n");
        assert_eq!(f.code, "image-attrs");
        assert_eq!(f.cols, 11..30);
        assert_eq!(f.fix.as_deref(), Some("{width=50% border}"));
        // A missing `%`, and an alignment typo.
        assert_eq!(
            only("![x](a.png){width=50 align=centre}\n").fix.as_deref(),
            Some("{width=50% align=center}")
        );
        // No fix when a token has no obvious correction.
        assert_eq!(only("![x](a.png){sparkly}\n").fix, None);
        // Valid and empty groups are fine.
        assert_eq!(
            codes("![x](a.png){width=40% align=center shadow}\n"),
            Vec::<&str>::new()
        );
        assert_eq!(codes("![x](a.png){}\n"), Vec::<&str>::new());
    }

    #[test]
    fn suggestions_stay_close() {
        assert_eq!(suggest("layuot", DIRECTIVES), Some("layout"));
        assert_eq!(suggest("TODO", DIRECTIVES), None);
        assert_eq!(suggest("nte", DIRECTIVES), Some("note"));
        assert_eq!(suggest("x", DIRECTIVES), None);
    }

    #[test]
    fn hex_colours() {
        assert!(is_hex_colour("#abc") && is_hex_colour("#aabbcc") && is_hex_colour("#aabbccdd"));
        assert!(!is_hex_colour("#abcd") && !is_hex_colour("#ggg") && !is_hex_colour("red"));
    }
}
