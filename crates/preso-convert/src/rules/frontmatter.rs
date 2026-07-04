//! Frontmatter → preso directive rules: `layout`, `class`, `background`,
//! `transition`, and a final pass that warns about anything left over.

use super::{Rule, SlideCtx, as_string};
use serde_norway::Value;

/// Deck-level headmatter keys handled elsewhere (see `lib::deck_frontmatter`);
/// they must not be reported as unconverted slide frontmatter.
pub const DECK_KEYS: &[&str] = &[
    "theme",
    "title",
    "titleTemplate",
    "transition",
    "aspectRatio",
    "canvasWidth",
    "info",
    "author",
    "keywords",
    "highlighter",
    "lineNumbers",
    "colorSchema",
    "fonts",
    "drawings",
    "mdc",
    "css",
    "monaco",
    "download",
    "exportFilename",
    "selectable",
];

/// `layout:` → slide kind / alignment / two-column layout.
pub struct Layout;

impl Rule for Layout {
    fn apply(&self, ctx: &mut SlideCtx) {
        let Some(layout) = ctx.take("layout").as_ref().and_then(as_string) else {
            return;
        };
        match layout.as_str() {
            "default" | "full" => {} // preso's default already fills the slide
            "center" | "middle" => {
                ctx.set_override("align", "center");
                ctx.set_override("halign", "center");
            }
            "cover" | "intro" => {
                ctx.set_override("kind", "title");
                ctx.set_override("align", "center");
            }
            "section" => ctx.set_override("kind", "section"),
            "statement" | "fact" => {
                ctx.set_override("kind", "section");
                ctx.set_override("align", "center");
                ctx.set_override("halign", "center");
            }
            "quote" => {
                ctx.set_override("align", "center");
                ctx.warn("layout 'quote' mapped to centered text; preso has no quote styling");
            }
            "two-cols" => ctx.layout = Some("TwoColumn".to_string()),
            "two-cols-header" => {
                ctx.layout = Some("TwoColumn".to_string());
                ctx.warn(
                    "layout 'two-cols-header': header (::title::) kept inline above the columns",
                );
            }
            "image-left" | "image-right" => image_columns(ctx, &layout),
            "image" => {
                ctx.warn(
                    "layout 'image': preso has no per-slide background image directive yet; \
                     the image is kept inline",
                );
                if let Some(img) = ctx.take("image").as_ref().and_then(as_string) {
                    ctx.body = format!("![]({})\n\n{}", strip_public(&img), ctx.body);
                }
            }
            "none" => ctx.warn("layout 'none' has no preso equivalent; default styling used"),
            other => ctx.warn(format!("unknown layout '{other}' dropped")),
        }
    }
}

/// Build a two-column body with the slide's `image:` on the given side.
fn image_columns(ctx: &mut SlideCtx, layout: &str) {
    let Some(image) = ctx.take("image").as_ref().and_then(as_string) else {
        ctx.warn(format!("{layout} has no `image:`; left as a single column"));
        return;
    };
    ctx.take("backgroundSize");
    ctx.layout = Some("TwoColumn".to_string());
    let img = format!("![]({})", strip_public(&image));
    let body = ctx.body.trim();
    ctx.body = if layout == "image-left" {
        format!("{img}\n\n***\n\n{body}")
    } else {
        format!("{body}\n\n***\n\n{img}")
    };
    ctx.warn(format!("{layout} approximated as a two-column layout"));
}

/// `class:` → horizontal alignment for the handful of UnoCSS text classes we
/// can map; everything else is dropped with a warning.
pub struct Class;

impl Rule for Class {
    fn apply(&self, ctx: &mut SlideCtx) {
        let Some(value) = ctx.take("class") else {
            return;
        };
        let classes: Vec<String> = match value {
            Value::String(s) => s.split_whitespace().map(str::to_string).collect(),
            Value::Sequence(seq) => seq.iter().filter_map(as_string).collect(),
            _ => return,
        };
        let mut unmapped = Vec::new();
        for class in classes {
            match class.as_str() {
                "text-center" => ctx.set_override("halign", "center"),
                "text-right" => ctx.set_override("halign", "right"),
                "text-left" => ctx.set_override("halign", "left"),
                other => unmapped.push(other.to_string()),
            }
        }
        if !unmapped.is_empty() {
            ctx.warn(format!("CSS class(es) dropped: {}", unmapped.join(" ")));
        }
    }
}

/// `background:` → `background=#hex` when it's a colour; image backgrounds
/// warn (preso has no per-slide background-image directive yet).
pub struct Background;

impl Rule for Background {
    fn apply(&self, ctx: &mut SlideCtx) {
        let Some(value) = ctx.take("background").as_ref().and_then(as_string) else {
            return;
        };
        let v = value.trim();
        if v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit()) {
            ctx.set_override("background", v);
        } else {
            ctx.warn(format!(
                "background '{v}' dropped (preso supports only solid `background=#hex` per slide)"
            ));
        }
    }
}

/// `transition:` is deck-level in preso, so a per-slide value is dropped.
pub struct Transition;

impl Rule for Transition {
    fn apply(&self, ctx: &mut SlideCtx) {
        if ctx.take("transition").is_some() {
            ctx.warn("per-slide `transition` dropped (preso transitions are deck-level)");
        }
    }
}

/// Warn about any frontmatter keys no rule consumed.
pub struct LeftoverFrontmatter;

impl Rule for LeftoverFrontmatter {
    fn apply(&self, ctx: &mut SlideCtx) {
        let mut keys: Vec<String> = ctx
            .front
            .keys()
            .filter_map(as_string)
            // Deck-level keys on slide 0 are handled as headmatter.
            .filter(|k| !(ctx.index == 0 && DECK_KEYS.contains(&k.as_str())))
            .collect();
        keys.sort();
        if !keys.is_empty() {
            ctx.warn(format!(
                "unsupported frontmatter dropped: {}",
                keys.join(", ")
            ));
        }
    }
}

/// Strip Slidev's public-dir (`/`) and alias (`@/`) image path prefixes.
pub(crate) fn strip_public(path: &str) -> String {
    path.strip_prefix("@/")
        .or_else(|| path.strip_prefix('/'))
        .unwrap_or(path)
        .to_string()
}
