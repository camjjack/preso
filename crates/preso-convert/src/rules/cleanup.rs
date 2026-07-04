//! Final body pass: remove things preso (a native renderer, not a browser)
//! can't render, and warn so the author knows what was lost. We strip
//! `<style>` blocks outright and flag — but don't mangle — Vue components and
//! UnoCSS, which need manual attention.

use super::{Rule, SlideCtx};

pub struct Cleanup;

impl Rule for Cleanup {
    fn apply(&self, ctx: &mut SlideCtx) {
        let mut body = ctx.body.clone();

        if let Some(stripped) = strip_style_blocks(&body) {
            body = stripped;
            ctx.warn("`<style>` block removed (preso has no per-slide CSS)");
        }

        // Detect, but don't remove, component-like tags: `<PascalCase …>`.
        if has_component_tag(&body) {
            ctx.warn(
                "possible Vue component(s) left in the body; preso renders plain markdown only",
            );
        }

        ctx.body = collapse_blank_lines(&body);
    }
}

/// Remove every `<style …>…</style>` block. Returns `None` if there were none.
fn strip_style_blocks(body: &str) -> Option<String> {
    if !body.contains("<style") {
        return None;
    }
    let mut out = String::new();
    let mut rest = body;
    let mut removed = false;
    while let Some(start) = rest.find("<style") {
        if let Some(end_rel) = rest[start..].find("</style>") {
            out.push_str(&rest[..start]);
            rest = &rest[start + end_rel + "</style>".len()..];
            removed = true;
        } else {
            break;
        }
    }
    out.push_str(rest);
    removed.then_some(out)
}

/// A self-closing or opening tag whose name starts with an uppercase letter —
/// the shape of a Vue/Slidev component (`<Tweet/>`, `<Youtube .../>`).
fn has_component_tag(body: &str) -> bool {
    let mut in_fence = false;
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let bytes = line.as_bytes();
        for i in 0..bytes.len().saturating_sub(1) {
            if bytes[i] == b'<' && bytes[i + 1].is_ascii_uppercase() {
                return true;
            }
        }
    }
    false
}

/// Collapse runs of 3+ blank lines (left by removed blocks) down to one.
fn collapse_blank_lines(body: &str) -> String {
    let mut out = String::new();
    let mut blanks = 0;
    for line in body.lines() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks >= 2 {
                continue;
            }
        } else {
            blanks = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_norway::Mapping;

    fn run(body: &str) -> SlideCtx {
        let mut c = SlideCtx {
            index: 0,
            front: Mapping::new(),
            overrides: vec![],
            layout: None,
            body: body.to_string(),
            notes: vec![],
            warnings: vec![],
        };
        Cleanup.apply(&mut c);
        c
    }

    #[test]
    fn removes_style_block() {
        let c = run("# Hi\n\n<style>\nh1 { color: red }\n</style>\n");
        assert!(!c.body.contains("color"));
        assert!(c.warnings.iter().any(|w| w.contains("style")));
    }

    #[test]
    fn flags_component() {
        let c = run("# Hi\n\n<Tweet id=\"123\" />\n");
        assert!(c.warnings.iter().any(|w| w.contains("component")));
    }
}
