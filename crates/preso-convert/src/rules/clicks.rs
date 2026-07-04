//! Click animations → reveal steps.
//!
//! Slidev reveals content with Vue components/directives; preso uses
//! `<!-- pause -->` markers. We handle the two common block forms:
//!
//! * `<v-clicks>` around a list → a pause before each item after the first;
//! * `<v-click>` around a block → a pause before the block;
//! * `<v-after>` → no pause (same step as the previous).
//!
//! Timing attributes (`at=`, ranges, `.hide`) and the directive-on-element
//! form (`<div v-click>`) can't be expressed in preso and are warned about.

use super::{Rule, SlideCtx};

pub struct Clicks;

impl Rule for Clicks {
    fn apply(&self, ctx: &mut SlideCtx) {
        let mut warnings = Vec::new();
        let mut body = ctx.body.clone();

        body = replace_blocks(&body, "v-clicks", &mut warnings, |inner, attrs, warns| {
            if !attrs.is_empty() {
                warns.push(format!("<v-clicks {attrs}>: options not converted"));
            }
            pause_between_items(inner)
        });
        body = replace_blocks(&body, "v-click", &mut warnings, |inner, attrs, warns| {
            if !attrs.is_empty() {
                warns.push(format!("<v-click {attrs}>: timing not converted"));
            }
            format!("<!-- pause -->\n{}", inner.trim())
        });
        body = replace_blocks(&body, "v-after", &mut warnings, |inner, _attrs, _warns| {
            inner.trim().to_string()
        });

        // Anything left is the directive-on-element form we can't convert.
        if body.contains("v-click") || body.contains("v-after") {
            warnings.push(
                "`v-click`/`v-after` element directives left as-is; add `<!-- pause -->` manually"
                    .to_string(),
            );
        }

        for w in warnings {
            ctx.warn(w);
        }
        ctx.body = body;
    }
}

/// Insert `<!-- pause -->` before each top-level list item after the first.
fn pause_between_items(inner: &str) -> String {
    let mut out = String::new();
    let mut seen_item = false;
    for line in inner.lines() {
        if is_top_level_item(line) {
            if seen_item {
                out.push_str("<!-- pause -->\n");
            }
            seen_item = true;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// A list item with no leading indentation (`- `, `* `, `+ `, `1. `).
fn is_top_level_item(line: &str) -> bool {
    if line.starts_with(' ') || line.starts_with('\t') {
        return false;
    }
    let t = line.trim_start();
    t.starts_with("- ")
        || t.starts_with("* ")
        || t.starts_with("+ ")
        || t.split_once(". ")
            .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Replace every `<tag …>…</tag>` block, passing the inner text and the
/// opening tag's attribute string to `f`. Non-nested; unbalanced tags are
/// left untouched.
fn replace_blocks(
    body: &str,
    tag: &str,
    warnings: &mut Vec<String>,
    mut f: impl FnMut(&str, &str, &mut Vec<String>) -> String,
) -> String {
    let open_prefix = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::new();
    let mut rest = body;
    loop {
        let Some(start) = find_open(rest, &open_prefix) else {
            out.push_str(rest);
            break;
        };
        // Opening tag must end with `>`.
        let Some(gt) = rest[start..].find('>') else {
            out.push_str(rest);
            break;
        };
        let attrs = rest[start + open_prefix.len()..start + gt]
            .trim()
            .to_string();
        let inner_start = start + gt + 1;
        let Some(close_rel) = rest[inner_start..].find(&close) else {
            // No matching close — leave the rest as-is (handled by the
            // leftover warning in `apply`).
            out.push_str(rest);
            break;
        };
        let inner = &rest[inner_start..inner_start + close_rel];
        out.push_str(&rest[..start]);
        out.push_str(&f(inner, &attrs, warnings));
        rest = &rest[inner_start + close_rel + close.len()..];
    }
    out
}

/// Find `<tag` only where it's a real element open (`<tag>`, `<tag ` or
/// `<tag\n`), not a longer name like `<v-clicks` when searching `<v-click`.
fn find_open(body: &str, open_prefix: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = body[from..].find(open_prefix) {
        let at = from + rel;
        let after = body[at + open_prefix.len()..].chars().next();
        if matches!(
            after,
            Some('>') | Some(' ') | Some('\n') | Some('\t') | Some('\r') | None
        ) {
            return Some(at);
        }
        from = at + open_prefix.len();
    }
    None
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
        Clicks.apply(&mut c);
        c
    }

    #[test]
    fn v_clicks_list_becomes_pauses() {
        let c = run("<v-clicks>\n\n- one\n- two\n- three\n\n</v-clicks>\n");
        let pauses = c.body.matches("<!-- pause -->").count();
        assert_eq!(pauses, 2, "two pauses between three items:\n{}", c.body);
        assert!(!c.body.contains("v-clicks"));
    }

    #[test]
    fn v_click_block_gets_one_pause() {
        let c = run("intro\n\n<v-click>\n\n## Revealed\n\n</v-click>\n");
        assert_eq!(c.body.matches("<!-- pause -->").count(), 1);
        assert!(c.body.contains("## Revealed"));
        assert!(!c.body.contains("v-click"));
    }

    #[test]
    fn does_not_match_v_clicks_when_looking_for_v_click() {
        // <v-clicks> must not be torn apart by the <v-click> pass.
        let c = run("<v-clicks>\n- a\n- b\n</v-clicks>\n");
        assert!(!c.body.contains("clicks>"));
        assert_eq!(c.body.matches("<!-- pause -->").count(), 1);
    }
}
