//! Image handling: normalise Slidev's public-dir (`/img.png`) and alias
//! (`@/img.png`) paths to plain relative paths, and convert a simple `<img>`
//! tag with a `width` into preso's `![](src){width=…}` form. Anything fancier
//! in an `<img>` tag is warned about and left for the author.

use super::frontmatter::strip_public;
use super::{Rule, SlideCtx};

pub struct Images;

impl Rule for Images {
    fn apply(&self, ctx: &mut SlideCtx) {
        let mut warnings = Vec::new();
        let body = normalise_markdown_paths(&ctx.body, &mut warnings);
        let body = convert_img_tags(&body, &mut warnings);
        for w in warnings {
            ctx.warn(w);
        }
        ctx.body = body;
    }
}

/// Strip `/` and `@/` prefixes from `![alt](path)` URLs outside code fences.
fn normalise_markdown_paths(body: &str, warnings: &mut Vec<String>) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    let mut relocated = false;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if in_fence {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let mut rewritten = String::new();
        let mut rest = line;
        while let Some(idx) = rest.find("](") {
            let url_start = idx + 2;
            let Some(close_rel) = rest[url_start..].find(')') else {
                break;
            };
            let url = &rest[url_start..url_start + close_rel];
            let stripped = strip_public(url);
            if stripped != url {
                relocated = true;
            }
            rewritten.push_str(&rest[..url_start]);
            rewritten.push_str(&stripped);
            rewritten.push(')');
            rest = &rest[url_start + close_rel + 1..];
        }
        rewritten.push_str(rest);
        out.push_str(&rewritten);
        out.push('\n');
    }
    if relocated {
        warnings.push(
            "image paths had Slidev's `/` (public) or `@/` prefix stripped; place the files \
             relative to the deck"
                .to_string(),
        );
    }
    out.trim_end().to_string()
}

/// Convert `<img src="x" width="40%">` to `![](x){width=40%}`; warn on
/// `<img>` tags we can't reduce to that.
fn convert_img_tags(body: &str, warnings: &mut Vec<String>) -> String {
    if !body.contains("<img") {
        return body.to_string();
    }
    let mut out = String::new();
    let mut rest = body;
    while let Some(start) = rest.find("<img") {
        out.push_str(&rest[..start]);
        let Some(end_rel) = rest[start..].find('>') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let tag = &rest[start..start + end_rel + 1];
        match (attr(tag, "src"), attr(tag, "width")) {
            (Some(src), Some(width)) => {
                out.push_str(&format!("![]({}){{width={}}}", strip_public(&src), width));
            }
            (Some(src), None) => out.push_str(&format!("![]({})", strip_public(&src))),
            _ => {
                warnings.push("`<img>` tag left as-is (no `src`)".to_string());
                out.push_str(tag);
            }
        }
        rest = &rest[start + end_rel + 1..];
    }
    out.push_str(rest);
    out
}

/// Extract a double- or single-quoted HTML attribute value.
fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=");
    let at = tag.find(&key)? + key.len();
    let bytes = tag.as_bytes();
    let quote = *bytes.get(at)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let value_start = at + 1;
    let close_rel = tag[value_start..].find(quote as char)?;
    Some(tag[value_start..value_start + close_rel].to_string())
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
        Images.apply(&mut c);
        c
    }

    #[test]
    fn strips_public_prefix() {
        let c = run("![cat](/cat.png)\n");
        assert!(c.body.contains("![cat](cat.png)"), "got: {}", c.body);
    }

    #[test]
    fn converts_img_tag_with_width() {
        let c = run("<img src=\"/d.png\" width=\"40%\">\n");
        assert!(c.body.contains("![](d.png){width=40%}"), "got: {}", c.body);
    }
}
