//! Export a preso deck to Slidev markdown — the mirror of the Slidev
//! importer. Both formats are markdown split on `---`, so content passes
//! through untouched; the work is mapping preso's directive vocabulary to
//! Slidev's frontmatter and components. Anything unrepresentable is
//! dropped with a warning rather than failing, the same philosophy as the
//! import pipeline. Run `preso_core::include::expand` first so the whole
//! assembled deck is exported.

use crate::{Conversion, yaml_scalar};
use preso_core::fence;
use preso_core::parser::{NoteOpen, directive, highlight_directive, parse_note_open, raw_deck};

/// Convert preso `source` (includes already expanded) to Slidev markdown.
pub fn export(source: &str) -> anyhow::Result<Conversion> {
    let (front, slides) = raw_deck(source)?;
    let mut warnings = Vec::new();

    // Deck headmatter.
    let mut head: Vec<String> = Vec::new();
    if let Some(title) = &front.title {
        head.push(format!("title: {}", yaml_scalar(title)));
    }
    match front.theme.as_deref() {
        // The built-in themes carry only a color scheme worth of meaning.
        Some(scheme @ ("dark" | "light")) => head.push(format!("colorSchema: {scheme}")),
        Some(other) => warnings.push(format!(
            "deck: preso theme {other:?} has no Slidev equivalent (Slidev themes are npm packages); dropped"
        )),
        None => {}
    }
    if let Some(t) = front.transition.as_deref() {
        push_transition(t, &mut head, "deck", &mut warnings);
    }
    if let Some((w, h)) = front.aspect.as_deref().and_then(|a| a.split_once(':')) {
        head.push(format!("aspectRatio: \"{}/{}\"", w.trim(), h.trim()));
    }

    // Slides. Each yields per-slide frontmatter lines plus its body
    // (already carrying the trailing Slidev note comment).
    let rendered: Vec<(Vec<String>, String)> = slides
        .iter()
        .enumerate()
        .filter_map(|(i, slide)| export_slide(slide, i + 1, &mut warnings))
        .collect();

    // Assemble. Slide 1's frontmatter merges into the deck headmatter
    // (Slidev treats the leading block as both).
    let mut out = String::new();
    let first_fm = rendered.first().map(|(fm, _)| fm.as_slice()).unwrap_or(&[]);
    let headmatter: Vec<&String> = head.iter().chain(first_fm).collect();
    if !headmatter.is_empty() {
        out.push_str("---\n");
        for line in headmatter {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("---\n\n");
    }
    for (i, (fm, body)) in rendered.iter().enumerate() {
        if i > 0 {
            out.push_str("\n---\n");
            if !fm.is_empty() {
                for line in fm {
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str("---\n");
            }
            out.push('\n');
        }
        out.push_str(body.trim_end());
        out.push('\n');
    }

    Ok(Conversion {
        output: out,
        warnings,
        media: Vec::new(),
    })
}

/// Map a preso transition name onto Slidev's set, warning when lossy.
/// `none` maps to absence (Slidev's default is no transition).
fn push_transition(value: &str, fm: &mut Vec<String>, scope: &str, warnings: &mut Vec<String>) {
    let mapped = match value {
        "fade" | "dissolve" => Some("fade"),
        // preso renders all motion names as a wipe; slide-left is the
        // closest Slidev effect.
        "wipe" | "slide" | "push" | "cover" => Some("slide-left"),
        "none" => None,
        other => {
            warnings.push(format!("{scope}: unknown transition '{other}'; dropped"));
            return;
        }
    };
    match mapped {
        Some(m) => {
            if m != value {
                warnings.push(format!(
                    "{scope}: transition '{value}' approximated as '{m}'"
                ));
            }
            fm.push(format!("transition: {m}"));
        }
        None => {
            if scope != "deck" {
                warnings.push(format!(
                    "{scope}: transition=none override dropped (Slidev has no per-slide 'none')"
                ));
            }
        }
    }
}

/// Export one slide: `(frontmatter lines, body)`, or `None` for a hidden
/// slide (dropped, matching preso semantics).
fn export_slide(src: &str, n: usize, warnings: &mut Vec<String>) -> Option<(Vec<String>, String)> {
    let mut w: Vec<String> = Vec::new();

    let mut fence = fence::Tracker::default();
    // Content between pause markers; chunk 0 is always visible, later
    // chunks each become a <v-click> block (same cumulative reveal).
    let mut chunks: Vec<String> = vec![String::new()];
    let mut notes: Vec<(Option<usize>, String)> = Vec::new();
    let mut open_note: Option<(Option<usize>, String)> = None;
    let mut kind: Option<String> = None;
    let mut align_center = false;
    let mut halign: Option<String> = None;
    let mut background: Option<String> = None;
    let mut transition: Option<String> = None;
    let mut hidden = false;
    let mut two_col = false;
    let mut col_split_done = false;
    let mut videos: Vec<String> = Vec::new();

    for line in src.lines() {
        // Fenced code is literal; only the opening line needs its
        // annotation sanitized for Slidev.
        let was_in = fence.in_fence();
        if fence.process(line) {
            if was_in {
                push_line(&mut chunks, line);
            } else {
                let (cleaned, mut fw) = sanitize_fence(line);
                w.append(&mut fw);
                push_line(&mut chunks, &cleaned);
            }
            continue;
        }

        // Continuation of a multi-line note comment.
        if let Some((step, mut text)) = open_note.take() {
            match line.find("-->") {
                Some(end) => {
                    text.push(' ');
                    text.push_str(line[..end].trim());
                    notes.push((step, text.trim().to_string()));
                }
                None => {
                    text.push(' ');
                    text.push_str(line.trim());
                    open_note = Some((step, text));
                }
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
                NoteOpen::Complete(step, text) => notes.push((step, text)),
                NoteOpen::Continued(step, text) => open_note = Some((step, text)),
            }
            continue;
        }
        if let Some(spec) = directive(trimmed, "layout") {
            let mut parts = spec.split_whitespace();
            if parts.next() == Some("TwoColumn") {
                two_col = true;
                if let Some(ratio) = parts.next()
                    && ratio != "1:1"
                {
                    w.push(format!(
                        "TwoColumn ratio '{ratio}' has no two-cols equivalent; columns render 1:1"
                    ));
                }
            }
            continue;
        }
        if let Some(spec) = directive(trimmed, "slide") {
            for token in spec.split_whitespace() {
                match token.split_once('=') {
                    Some(("kind", v)) => kind = Some(v.to_string()),
                    Some(("align", "center")) => align_center = true,
                    Some(("align", _)) => {}
                    Some(("halign", v)) => halign = Some(v.to_string()),
                    Some(("background", v)) => background = Some(v.to_string()),
                    Some(("transition", v)) => transition = Some(v.to_string()),
                    Some(("number", v)) => w.push(format!(
                        "slide-number reset 'number={v}' has no Slidev equivalent; dropped"
                    )),
                    Some(_) => {}
                    None => {
                        if token == "hidden" {
                            hidden = true;
                        }
                    }
                }
            }
            continue;
        }
        if let Some(text) = directive(trimmed, "footnote") {
            w.push(format!(
                "footnote {:?} has no Slidev equivalent; dropped",
                text.trim()
            ));
            continue;
        }
        if let Some(spec) = directive(trimmed, "video") {
            let path = spec.trim();
            if !path.is_empty() {
                videos.push(path.to_string());
            }
            continue;
        }
        if directive(trimmed, "image").is_some() {
            w.push("layer decoration (<!-- image: … -->) has no Slidev equivalent; dropped".into());
            continue;
        }
        if highlight_directive(trimmed).is_some() {
            w.push(
                "image highlight (<!-- highlight: … -->) has no Slidev equivalent; dropped".into(),
            );
            continue;
        }
        if directive(trimmed, "table").is_some() {
            w.push("table size hint has no Slidev equivalent; dropped".into());
            continue;
        }
        if two_col && !col_split_done && trimmed == "***" {
            push_line(&mut chunks, "\n::right::\n");
            col_split_done = true;
            continue;
        }
        let (rewritten, mut iw) = rewrite_image_line(line);
        w.append(&mut iw);
        push_line(&mut chunks, &rewritten);
    }
    if let Some((step, text)) = open_note {
        notes.push((step, text.trim().to_string()));
    }

    if hidden {
        // Hidden slides never render or export in preso; other warnings
        // about a dropped slide would be noise.
        warnings.push(format!("slide {n}: hidden slide dropped"));
        return None;
    }

    // Per-slide frontmatter.
    let mut fm: Vec<String> = Vec::new();
    if two_col {
        fm.push("layout: two-cols".into());
        if let Some(k) = &kind {
            w.push(format!(
                "kind={k} styling not applied (two-cols layout wins)"
            ));
        }
    } else {
        match kind.as_deref() {
            Some("title") => fm.push("layout: cover".into()),
            Some("section") => fm.push("layout: section".into()),
            Some(other) => w.push(format!("unknown slide kind '{other}'; dropped")),
            None => {
                if align_center {
                    fm.push("layout: center".into());
                }
            }
        }
    }
    match halign.as_deref() {
        Some("center") => fm.push("class: text-center".into()),
        Some("right") => fm.push("class: text-right".into()),
        _ => {}
    }
    if let Some(bg) = &background {
        // Slidev's cover layout takes a background; elsewhere there is
        // no per-slide background to map onto.
        if fm.iter().any(|l| l == "layout: cover") {
            fm.push(format!("background: {}", yaml_scalar(bg)));
        } else {
            w.push(format!(
                "background override {bg:?} only maps to the cover layout; dropped"
            ));
        }
    }
    if let Some(t) = &transition {
        push_transition(t, &mut fm, "transition override", &mut w);
    }
    if notes.iter().any(|(step, _)| step.is_some()) {
        w.push("step-scoped notes flattened into the slide note".into());
    }

    warnings.extend(w.into_iter().map(|m| format!("slide {n}: {m}")));

    // Body: chunk 0 plain, later chunks in <v-click> (cumulative reveal,
    // like preso's pause).
    let mut body = String::new();
    let mut first = true;
    for chunk in &chunks {
        let content = chunk.trim_matches('\n');
        if content.trim().is_empty() {
            continue;
        }
        if first {
            body.push_str(content);
            body.push('\n');
            first = false;
        } else {
            body.push_str("\n<v-click>\n\n");
            body.push_str(content);
            body.push_str("\n\n</v-click>\n");
        }
    }
    for v in &videos {
        body.push_str(&format!(
            "\n<video controls src=\"{}\"></video>\n",
            html_attr(v)
        ));
    }
    // Slidev presenter notes: the trailing HTML comment of the slide.
    if !notes.is_empty() {
        let joined = notes
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        body.push_str(&format!("\n<!--\n{joined}\n-->\n"));
    }

    Some((fm, body))
}

fn push_line(chunks: &mut [String], line: &str) {
    let buf = chunks.last_mut().expect("chunks never empty");
    buf.push_str(line);
    buf.push('\n');
}

/// Clean a fence-opening line for Slidev. Line-highlight annotations pass
/// through — Slidev uses the same `{2,4-6}` / `{1|2|3}` / `all` syntax —
/// while preso-only tokens (`size=`, `width=`, `align=`, `dim`,
/// `transparent`, …) are dropped with a warning.
fn sanitize_fence(line: &str) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    let indent_len = line.len() - line.trim_start_matches(' ').len();
    let (indent, rest) = line.split_at(indent_len);
    let fence_ch = rest.chars().next().expect("caller verified fence");
    let fence_len = rest.chars().take_while(|&c| c == fence_ch).count();
    let (fence, info) = rest.split_at(fence_len);

    let info = info.trim();
    let mut parts = info.splitn(2, char::is_whitespace);
    let language = parts.next().unwrap_or("");
    let annotation = parts.next().map(str::trim).unwrap_or("");

    if matches!(language, "dot" | "graphviz") {
        warnings.push("Slidev has no Graphviz support; the block stays plain code".into());
    }

    let inner = annotation
        .strip_prefix('{')
        .and_then(|a| a.strip_suffix('}'));
    let cleaned_annotation = inner.map(|inner| {
        let mut dropped: Vec<&str> = Vec::new();
        let stages: Vec<String> = inner
            .split('|')
            .map(|stage| {
                stage
                    .split([',', ' '])
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .filter(|t| {
                        let keep = *t == "all"
                            || t.chars()
                                .all(|c| c.is_ascii_digit() || c == '-' || c == ',');
                        if !keep {
                            dropped.push(t);
                        }
                        keep
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect();
        if !dropped.is_empty() {
            warnings.push(format!(
                "code-fence option(s) {} have no Slidev equivalent; dropped",
                dropped
                    .iter()
                    .map(|t| format!("'{t}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let joined = stages.join("|");
        // All stages empty (e.g. `{transparent}`) → no annotation at all.
        if joined.chars().all(|c| c == '|') {
            String::new()
        } else {
            format!("{{{joined}}}")
        }
    });

    let out = match cleaned_annotation {
        Some(a) if !a.is_empty() => format!("{indent}{fence}{language} {a}"),
        _ if language.is_empty() => format!("{indent}{fence}"),
        _ => format!("{indent}{fence}{language}"),
    };
    (out, warnings)
}

/// Rewrite a whole-line `![alt](url){attrs}` image into HTML Slidev can
/// size (`{width=NN%}` isn't markdown). Lines that aren't a single image
/// with an attribute group pass through untouched.
fn rewrite_image_line(line: &str) -> (String, Vec<String>) {
    let t = line.trim();
    let is_single_image =
        t.starts_with("![") && t.matches("](").count() == 1 && t.ends_with('}') && t.contains("){");
    if !is_single_image {
        return (line.to_string(), Vec::new());
    }

    let Some(close_alt) = t.find("](") else {
        return (line.to_string(), Vec::new());
    };
    let alt = &t[2..close_alt];
    let after = &t[close_alt + 2..];
    let Some(url_end) = after.find("){") else {
        return (line.to_string(), Vec::new());
    };
    let url = &after[..url_end];
    let attrs = &after[url_end + 2..after.len() - 1];

    let mut warnings = Vec::new();
    let mut width: Option<&str> = None;
    let mut align: Option<&str> = None;
    for token in attrs.split_whitespace() {
        if let Some(v) = token.strip_prefix("width=") {
            width = v.strip_suffix('%');
        } else if let Some(v) = token.strip_prefix("align=") {
            align = Some(v);
        } else if matches!(token, "border" | "shadow") {
            warnings.push(format!(
                "image framing '{token}' is preso theme styling; dropped"
            ));
        } else if matches!(token, "plain" | "fit") {
            // No theme framing / row layout in Slidev — nothing to drop.
        } else {
            // Mirror the preso parser: an unrecognized token leaves the
            // whole group as literal text.
            warnings.push(format!(
                "image attribute group left as literal text (unrecognized '{token}')"
            ));
            return (line.to_string(), warnings);
        }
    }

    if width.is_none() && align.is_none() {
        return (format!("![{alt}]({url})"), warnings);
    }
    let mut style = String::new();
    if let Some(w) = width {
        style.push_str(&format!("width: {w}%;"));
    }
    match align {
        Some("center") => style.push_str(" display: block; margin: 0 auto;"),
        Some("right") => style.push_str(" display: block; margin-left: auto;"),
        _ => {}
    }
    (
        format!(
            "<img src=\"{}\" alt=\"{}\" style=\"{}\">",
            html_attr(url),
            html_attr(alt),
            style.trim()
        ),
        warnings,
    )
}

fn html_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_title_becomes_cover_in_headmatter() {
        let src = "---\ntitle: T\ntheme: dark\n---\n\n<!-- slide: kind=title -->\n# Big\n";
        let out = export(src).unwrap().output;
        let head = out.split("---").nth(1).unwrap();
        assert!(head.contains("title: T"));
        assert!(head.contains("colorSchema: dark"));
        assert!(head.contains("layout: cover"));
        assert!(out.contains("# Big"));
    }

    #[test]
    fn section_slide_gets_its_own_frontmatter_block() {
        let src = "# One\n\n---\n\n<!-- slide: kind=section -->\n# Part 2\n";
        let out = export(src).unwrap().output;
        assert!(out.contains("---\nlayout: section\n---"), "got:\n{out}");
    }

    #[test]
    fn two_column_maps_to_two_cols_and_right_marker() {
        let src = "<!-- layout: TwoColumn 2:1 -->\n\nleft\n\n***\n\nright\n";
        let r = export(src).unwrap();
        assert!(r.output.contains("layout: two-cols"));
        assert!(r.output.contains("::right::"));
        assert!(!r.output.contains("***"));
        assert!(r.warnings.iter().any(|w| w.contains("ratio '2:1'")));
    }

    #[test]
    fn pauses_become_cumulative_v_clicks() {
        let src = "- one\n<!-- pause -->\n- two\n<!-- pause -->\n- three\n";
        let out = export(src).unwrap().output;
        assert_eq!(out.matches("<v-click>").count(), 2);
        assert_eq!(out.matches("</v-click>").count(), 2);
        let one = out.find("- one").unwrap();
        let first_click = out.find("<v-click>").unwrap();
        assert!(one < first_click, "chunk 0 stays outside v-click");
    }

    #[test]
    fn notes_merge_into_trailing_comment() {
        let src = "# H\n<!-- note: first -->\n<!-- pause -->\n<!-- note[1]: second\nline -->\n";
        let r = export(src).unwrap();
        assert!(r.output.contains("<!--\nfirst\n\nsecond line\n-->"));
        assert!(r.warnings.iter().any(|w| w.contains("step-scoped")));
    }

    #[test]
    fn line_highlights_pass_through_extras_dropped() {
        let src = "```rust {2,4-6 size=18}\nfn main() {}\n```\n\n```rust {1|2|all}\nx\n```\n";
        let r = export(src).unwrap();
        assert!(r.output.contains("```rust {2,4-6}"));
        assert!(r.output.contains("```rust {1|2|all}"));
        assert!(r.warnings.iter().any(|w| w.contains("'size=18'")));
        // A flags-only annotation disappears entirely.
        let r2 = export("```mermaid {width=60% transparent}\ngraph TD\n```\n").unwrap();
        assert!(r2.output.contains("```mermaid\n"));
    }

    #[test]
    fn image_width_becomes_html_img() {
        let out = export("![logo](a.png){width=30%}\n").unwrap().output;
        assert!(out.contains(r#"<img src="a.png" alt="logo" style="width: 30%;">"#));
        // Plain images pass through untouched.
        let plain = export("![x](y.png)\n").unwrap().output;
        assert!(plain.contains("![x](y.png)"));
    }

    #[test]
    fn video_becomes_html5_video() {
        let out = export("# Demo\n\n<!-- video: clips/d.mp4 -->\n")
            .unwrap()
            .output;
        assert!(out.contains(r#"<video controls src="clips/d.mp4"></video>"#));
    }

    #[test]
    fn hidden_slides_are_dropped_with_warning() {
        let src = "# One\n\n---\n\n<!-- slide: hidden -->\n# Secret\n\n---\n\n# Three\n";
        let r = export(src).unwrap();
        assert!(!r.output.contains("Secret"));
        assert!(r.warnings.iter().any(|w| w.contains("hidden slide")));
    }

    #[test]
    fn transitions_map_and_warn_when_lossy() {
        let src =
            "---\ntransition: wipe\n---\n\n# A\n\n---\n\n<!-- slide: transition=fade -->\n# B\n";
        let r = export(src).unwrap();
        assert!(r.output.contains("transition: slide-left"));
        assert!(r.output.contains("transition: fade"));
        assert!(r.warnings.iter().any(|w| w.contains("approximated")));
    }

    #[test]
    fn directives_inside_fences_stay_literal() {
        let src = "```md\n<!-- pause -->\n<!-- slide: hidden -->\n***\n```\n";
        let r = export(src).unwrap();
        assert!(r.output.contains("<!-- pause -->"));
        assert!(r.output.contains("<!-- slide: hidden -->"));
        assert!(r.output.contains("***"));
        assert!(!r.output.contains("<v-click>"));
    }

    #[test]
    fn math_and_mermaid_pass_through() {
        let src = "$$\nE = mc^2\n$$\n\n```mermaid\ngraph TD\n```\n\ninline $x^2$ too\n";
        let out = export(src).unwrap().output;
        assert!(out.contains("$$\nE = mc^2\n$$"));
        assert!(out.contains("```mermaid"));
        assert!(out.contains("$x^2$"));
    }

    #[test]
    fn example_talk_exports_and_round_trips() {
        let src = include_str!("../../../docs/example-talk.md");
        let r = export(src).unwrap();
        assert!(r.output.contains("layout: cover"));
        assert!(r.output.contains("layout: two-cols"));
        assert!(r.output.contains("::right::"));
        assert!(r.output.contains("<v-click>"));

        // Round trip: importing the export lands back on preso constructs.
        let back = crate::convert(&r.output);
        assert!(back.output.contains("kind=title") || back.output.contains("kind=section"));
        assert!(back.output.contains("<!-- layout: TwoColumn -->"));
    }
}
