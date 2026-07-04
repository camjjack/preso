//! Code fence annotations.
//!
//! Static line highlights (`{2,4-6}`) are identical in both formats and pass
//! through untouched. Slidev's click-through highlights (`{2-3|5|all}`) have
//! no preso equivalent, so we flatten them to the static union of every
//! stage. Non-highlight options (`{lines:true}`, `{startLine:N}`,
//! `{maxHeight:…}`) are dropped with a warning.

use super::{Rule, SlideCtx};

pub struct Code;

impl Rule for Code {
    fn apply(&self, ctx: &mut SlideCtx) {
        let mut warnings = Vec::new();
        let mut out = String::new();
        let mut in_fence = false;
        for line in ctx.body.lines() {
            let trimmed = line.trim_start();
            if is_fence_line(trimmed) {
                if !in_fence {
                    in_fence = true;
                    let indent = &line[..line.len() - trimmed.len()];
                    let marker_len = trimmed
                        .chars()
                        .take_while(|&c| c == '`' || c == '~')
                        .count();
                    let marker = &trimmed[..marker_len];
                    let info = &trimmed[marker_len..];
                    out.push_str(indent);
                    out.push_str(marker);
                    out.push_str(&rewrite_info(info, &mut warnings));
                    out.push('\n');
                    continue;
                }
                in_fence = false;
            }
            out.push_str(line);
            out.push('\n');
        }
        for w in warnings {
            ctx.warn(w);
        }
        ctx.body = out.trim_end().to_string();
    }
}

fn is_fence_line(trimmed: &str) -> bool {
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// Rewrite the info string after the fence marker, e.g. `ts {2-3|5|all}`.
fn rewrite_info(info: &str, warns: &mut Vec<String>) -> String {
    let Some(brace) = info.find('{') else {
        return info.to_string();
    };
    let lang = info[..brace].trim();
    let rest = &info[brace..];
    let Some(end) = rest.find('}') else {
        return info.to_string();
    };
    let group = &rest[1..end];
    let trailing = rest[end + 1..].trim();
    if !trailing.is_empty() {
        warns.push(format!("code annotation '{trailing}' dropped"));
    }

    let spec = if group.contains('|') {
        // preso supports click-through stages directly (parallel reveal model).
        Some(group.trim().to_string())
    } else if group.contains(':') {
        warns.push(format!("code option {{{group}}} dropped"));
        None
    } else if is_line_spec(group) {
        Some(group.trim().to_string())
    } else if matches!(group.trim(), "all" | "none" | "*" | "") {
        None
    } else {
        warns.push(format!("code annotation {{{group}}} dropped"));
        None
    };

    match spec {
        Some(s) => format!("{lang} {{{s}}}"),
        None if lang.is_empty() => String::new(),
        None => lang.to_string(),
    }
}

fn is_line_spec(group: &str) -> bool {
    !group.trim().is_empty()
        && group
            .chars()
            .all(|c| c.is_ascii_digit() || c == ',' || c == '-' || c == ' ')
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
        Code.apply(&mut c);
        c
    }

    #[test]
    fn static_highlight_passes_through() {
        let c = run("```rust {2,4-6}\nfn main() {}\n```\n");
        assert!(c.body.contains("```rust {2,4-6}"));
        assert!(c.warnings.is_empty());
    }

    #[test]
    fn dynamic_highlight_passes_through() {
        // preso supports click-through stages, so keep them verbatim.
        let c = run("```ts {2-3|5|all}\ncode\n```\n");
        assert!(c.body.contains("```ts {2-3|5|all}"), "got: {}", c.body);
        assert!(c.warnings.is_empty());
    }

    #[test]
    fn options_are_dropped() {
        let c = run("```ts {lines:true}\ncode\n```\n");
        assert!(c.body.contains("```ts\n"), "got: {}", c.body);
        assert!(c.warnings.iter().any(|w| w.contains("option")));
    }
}
