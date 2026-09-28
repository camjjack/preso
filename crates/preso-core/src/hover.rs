//! What the word under the cursor does: a directive, a `slide:` option, a
//! transition, a highlight or image option, a frontmatter key, a code
//! fence's flag or an image attribute. The text is the guide's reference
//! tables, short enough for a tooltip.
//!
//! Lines are 0-based; columns are byte offsets within the line.

use crate::lint;
use crate::outline::LineKind;
use crate::parser;
use std::ops::Range;

/// A tooltip: markdown, and the span of the word it describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    pub cols: Range<usize>,
    pub text: String,
}

const DIRECTIVES: &[(&str, &str)] = &[
    (
        "layout",
        "`<!-- layout: TwoColumn -->` splits the slide into two columns at a `***` line; \
         `TwoColumn 2:1` sets their widths. `Content` (the default) is one column.",
    ),
    (
        "slide",
        "`<!-- slide: … -->` sets options for this slide: `kind`, `align`, `halign`, \
         `background`, `fit`, `fill`, `size`, `number`, `transition`, and the `hidden` flag.",
    ),
    (
        "note",
        "`<!-- note: … -->` is a speaker note, shown in the presenter view only. \
         `note[n]:` shows from reveal step `n` onward. It may span lines.",
    ),
    ("speaker", "`<!-- speaker: … -->` is a synonym for `note:`."),
    (
        "footnote",
        "`<!-- footnote: … -->` is small print along the bottom of the slide, such as an \
         image credit.",
    ),
    (
        "video",
        "`<!-- video: clip.mp4 -->` makes the slide playable: a ▶ badge shows, and \
         <kbd>v</kbd> plays the clip.",
    ),
    (
        "image",
        "`<!-- image: path … -->` places an image on a layer behind the text: \
         `position=`, `width=NN%`, `opacity=`, `padding=`.",
    ),
    (
        "highlight",
        "`<!-- highlight: rect|ellipse x= y= w= h= … -->` draws a callout over the next \
         image (coordinates in % of it). `spotlight` dims all but the region, `under` draws \
         behind the image, `clip` keeps a wash to its opaque pixels. `highlight[n]:` shows \
         from reveal step `n`.",
    ),
    (
        "table",
        "`<!-- table: size=NN -->` sets the text size of the next table, in design units.",
    ),
    (
        "include",
        "`<!-- include: chapter.md -->` splices in another markdown file's slides.",
    ),
    (
        "zoom",
        "`<!-- zoom[n]: x%,y%,Nx -->` zooms the slide's content from reveal step `n`: the \
         point `x%` across and `y%` down moves to the middle, magnified `N` times. `all` \
         zooms back out. The background and chrome hold still.",
    ),
    (
        "pause",
        "`<!-- pause -->` starts a new reveal step: what follows shows on the next press.",
    ),
    (
        "v-click",
        "`<!-- v-click -->` is a synonym for `<!-- pause -->`.",
    ),
];

const SLIDE_KEYS: &[(&str, &str)] = &[
    (
        "kind",
        "`kind=title|section` styles the slide with the theme's title or section overlay.",
    ),
    ("align", "`align=top|center` places the content vertically."),
    (
        "halign",
        "`halign=left|center|right` places the content horizontally.",
    ),
    (
        "background",
        "`background=` is a `#colour`, or an image path for a full-bleed background (which \
         drops the theme's accent bars).",
    ),
    (
        "fit",
        "`fit=cover|contain|stretch|none` scales a background image. `cover` (the default) \
         crops the overhang.",
    ),
    (
        "fill",
        "`fill=#rrggbb` shows where a `fit=contain` background doesn't reach.",
    ),
    (
        "size",
        "`size=NN` sets this slide's body text size; headings, code and spacing follow in \
         proportion.",
    ),
    (
        "number",
        "`number=N` resets the slide number to `N`; later slides continue from there.",
    ),
    (
        "transition",
        "`transition=` is the transition *into* this slide, overriding the deck's `transition:`. \
         Going back plays it in reverse.",
    ),
    (
        "hidden",
        "`hidden` leaves the slide out of the presentation and exports.",
    ),
];

const FRONTMATTER: &[(&str, &str)] = &[
    (
        "title",
        "`title:` names the deck: the window and PDF title.",
    ),
    (
        "theme",
        "`theme:` is a built-in theme (`dark`, `light`), a theme in your themes folder by name, \
         or a path to a `.toml` theme. `--theme` overrides it.",
    ),
    (
        "transition",
        "`transition:` is the slide transition for the whole deck (default: none). A slide can \
         override it with `<!-- slide: transition=… -->`.",
    ),
    (
        "aspect",
        "`aspect:` is the slide aspect ratio for exports, like `\"16:9\"` or `\"4:3\"`.",
    ),
    (
        "presenter",
        "`presenter:` is the presenter-view layout: `slide` (the default) or `notes` for large \
         speaker notes.",
    ),
];

const FENCE: &[(&str, &str)] = &[
    (
        "zoom",
        "`zoom` makes the click-through stages zoom onto what they select: a code block's lines, \
         or the diagram nodes they name. Start with `all` to open unzoomed.",
    ),
    (
        "transparent",
        "`transparent` drops a diagram's light card, so it sits on the slide background.",
    ),
    (
        "dim",
        "`dim` fades the lines that aren't highlighted, instead of tinting the highlighted ones.",
    ),
    (
        "background",
        "`background` tints the highlighted lines (overriding a theme set to `dim`).",
    ),
    (
        "all",
        "`all` highlights (or zooms onto) nothing in particular: the whole block.",
    ),
    ("none", "`none` highlights nothing: the whole block."),
    (
        "width",
        "`width=NN%` sizes the block to a share of the content width (`100%` for full width).",
    ),
    (
        "size",
        "`size=NN` sets this block's code font size, in design units.",
    ),
    (
        "align",
        "`align=left|center|right` places the block across the slide.",
    ),
    ("mermaid", "A Mermaid diagram, drawn by preso."),
    ("dot", "A Graphviz (DOT) diagram, drawn by preso."),
    ("graphviz", "A Graphviz (DOT) diagram, drawn by preso."),
];

const HIGHLIGHT: &[(&str, &str)] = &[
    ("rect", "`rect` draws a rectangle."),
    ("ellipse", "`ellipse` draws an ellipse filling the box."),
    ("circle", "`circle` is a synonym for `ellipse`."),
    ("x", "`x=` is the left edge, in % of the image width."),
    ("y", "`y=` is the top edge, in % of the image height."),
    ("w", "`w=` is the width, in % of the image width."),
    ("h", "`h=` is the height, in % of the image height."),
    (
        "color",
        "`color=` is a `#colour` or a theme colour name (`accent`, `text`, …).",
    ),
    (
        "opacity",
        "`opacity=` is the fill's opacity, 0 to 1 (default 0.35).",
    ),
    (
        "stroke",
        "`stroke=` is the outline width in design units (0: none).",
    ),
    (
        "mode",
        "`mode=fill|spotlight|under|behind` sets how the shape paints.",
    ),
    (
        "spotlight",
        "`spotlight` dims everything *except* the region.",
    ),
    (
        "under",
        "`under` draws the shape solid *behind* the image, showing through transparency.",
    ),
    ("behind", "`behind` is a synonym for `under`."),
    (
        "clip",
        "`clip` keeps a fill or spotlight wash to the image's opaque pixels.",
    ),
];

const LAYER_IMAGE: &[(&str, &str)] = &[
    (
        "position",
        "`position=` anchors the image: `center`, `top-left`, `top`, `top-right`, `left`, \
         `right`, `bottom-left`, `bottom`, `bottom-right`.",
    ),
    (
        "width",
        "`width=NN%` sizes the image to a share of the slide width.",
    ),
    ("opacity", "`opacity=` fades the image, 0 to 1."),
    (
        "padding",
        "`padding=` insets the image from the slide's edges, in design units.",
    ),
];

const IMAGE_ATTRS: &[(&str, &str)] = &[
    (
        "width",
        "`width=NN%` sizes the image to a share of the content width.",
    ),
    (
        "align",
        "`align=left|center|right` places the image across the slide.",
    ),
    ("border", "`border` frames the image with a thin border."),
    ("shadow", "`shadow` gives the image a drop shadow."),
    (
        "plain",
        "`plain` removes the theme's default border and shadow.",
    ),
    (
        "fit",
        "`fit` packs an image row at the images' own widths, instead of sharing the width \
         equally.",
    ),
];

/// What the word at `col` on `line` does, if it's one preso knows.
pub fn at(source: &str, line: usize, col: usize) -> Option<Hover> {
    let text = source.lines().nth(line)?;
    let (word, cols) = word_at(text, col)?;
    let key = word.split_once('=').map_or(word, |(k, _)| k);
    let hover = |table: &[(&'static str, &'static str)], name: &str| {
        lookup(table, name).map(|doc| Hover {
            cols: cols.clone(),
            text: doc.to_string(),
        })
    };

    if let Some((yaml, _)) = parser::frontmatter_span(source)
        && yaml.contains(&line)
    {
        let (fm_key, _) = text.split_once(':')?;
        return if cols.end <= fm_key.len() {
            hover(FRONTMATTER, fm_key.trim())
        } else if fm_key.trim() == "transition" {
            transition(word.trim_matches(['"', '\'']), cols)
        } else {
            None
        };
    }

    let (kind, opens_fence) = crate::complete::line_kind(source, line)?;
    if kind == LineKind::Code {
        // The opening fence's language or annotation, not the code inside.
        let brace = text.find('{').unwrap_or(text.len());
        if !opens_fence || (cols.start < brace && text[..cols.start].contains(' ')) {
            return None;
        }
        let name = key.trim_start_matches(['`', '~']);
        let at = cols.start + (key.len() - name.len());
        return lookup(FENCE, name).map(|doc| Hover {
            cols: at..cols.end,
            text: doc.to_string(),
        });
    }
    if kind != LineKind::Text {
        return None;
    }

    // An image's `{…}` group.
    for span in crate::style::image_spans(text) {
        if let Some(attrs) = span.attrs
            && attrs.contains(&cols.start)
        {
            return hover(IMAGE_ATTRS, key);
        }
    }

    let trimmed = text.trim_start();
    if !trimmed.starts_with("<!--") {
        return None;
    }
    let body = trimmed[4..].trim_start();
    let name_len = body
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .unwrap_or(body.len());
    let name = &body[..name_len];
    let name_at = text.len() - body.len();
    if cols.start < name_at + name_len {
        return hover(DIRECTIVES, name);
    }
    match name {
        "slide" => match word.split_once('=') {
            Some(("transition", value)) if cols.start + "transition=".len() <= col => {
                transition(value, cols.start + "transition=".len()..cols.end)
            }
            _ => hover(SLIDE_KEYS, key),
        },
        "highlight" => hover(HIGHLIGHT, key),
        "image" => hover(LAYER_IMAGE, key.split('-').next().unwrap_or(key)),
        "table" if key == "size" => Some(Hover {
            cols,
            text: "`size=NN` sets the next table's text size, in design units.".into(),
        }),
        _ => None,
    }
}

/// A transition name's effect.
fn transition(name: &str, cols: Range<usize>) -> Option<Hover> {
    if !lint::TRANSITIONS.contains(&name) {
        return None;
    }
    let text = match name {
        "none" => "No transition: the slide cuts.".to_string(),
        "fade" | "dissolve" => "A cross-dissolve between the two slides.".to_string(),
        "wipe" | "cover" => {
            "A wipe: the outgoing slide is clipped away to reveal the next.".to_string()
        }
        "wipe-content" => "A wipe of the content alone, over a background and design that hold \
                           still."
            .to_string(),
        pan => {
            let way = pan.rsplit_once('-').map_or("left", |(_, d)| d);
            let from = match way {
                "right" => "left",
                "up" => "bottom",
                "down" => "top",
                _ => "right",
            };
            format!(
                "A pan: the content slides {way} as the next slide's slides in from the {from}, \
                 over a background that holds still. Going back pans the other way."
            )
        }
    };
    Some(Hover { cols, text })
}

fn lookup(table: &[(&'static str, &'static str)], name: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == name).map(|(_, doc)| *doc)
}

/// The word at `col`: a run of characters that can make up a name, an
/// option or its value, with its span.
fn word_at(text: &str, col: usize) -> Option<(&str, Range<usize>)> {
    let part = |c: char| c.is_ascii_alphanumeric() || "-_=%.#[]/'\"`~:".contains(c);
    let col = col.min(text.len());
    if !text.is_char_boundary(col) {
        return None;
    }
    let start = text[..col]
        .char_indices()
        .rev()
        .take_while(|(_, c)| part(*c))
        .last()
        .map_or(col, |(i, _)| i);
    let end = col
        + text[col..]
            .char_indices()
            .find(|(_, c)| !part(*c))
            .map_or(text.len() - col, |(i, _)| i);
    let word = &text[start..end];
    // A directive name ends at its colon, a gated one at its `[`.
    let word = word
        .strip_suffix(':')
        .unwrap_or(word)
        .split(['[', ':'])
        .next()
        .filter(|w| !w.is_empty())?;
    Some((word, start..start + word.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hover at the `@` in `source`, returning the described text.
    fn on(source: &str) -> Option<String> {
        let offset = source.find('@').unwrap();
        let src = source.replacen('@', "", 1);
        let line = source[..offset].matches('\n').count();
        let col = offset - source[..offset].rfind('\n').map_or(0, |i| i + 1);
        at(&src, line, col).map(|h| {
            let text = src.lines().nth(line).unwrap();
            format!("{} => {}", &text[h.cols], h.text)
        })
    }

    #[test]
    fn directives_and_their_options() {
        assert!(
            on("<!-- zo@om[1]: 40%,20%,2x -->")
                .unwrap()
                .starts_with("zoom => `<!-- zoom[n]")
        );
        assert!(
            on("<!-- @slide: kind=title -->")
                .unwrap()
                .starts_with("slide =>")
        );
        assert!(
            on("<!-- slide: ki@nd=title -->")
                .unwrap()
                .starts_with("kind=title => `kind=")
        );
        assert!(
            on("<!-- slide: hid@den -->")
                .unwrap()
                .starts_with("hidden =>")
        );
        let pan = on("<!-- slide: transition=pan-u@p -->").unwrap();
        assert!(
            pan.starts_with("pan-up => A pan: the content slides up"),
            "{pan}"
        );
        assert!(
            on("<!-- slide: tran@sition=pan-up -->")
                .unwrap()
                .starts_with("transition=pan-up => `transition=")
        );
        assert!(
            on("<!-- highlight: rect spot@light -->")
                .unwrap()
                .starts_with("spotlight =>")
        );
        assert!(
            on("<!-- image: a.png padding-t@op=4 -->")
                .unwrap()
                .starts_with("padding-top=4 => `padding=")
        );
        assert!(on("<!-- pa@use -->").unwrap().starts_with("pause =>"));
        assert_eq!(on("<!-- TO@DO: later -->"), None);
    }

    #[test]
    fn frontmatter_fences_and_images() {
        assert!(
            on("---\nthe@me: dark\n---\n# x\n")
                .unwrap()
                .starts_with("theme =>")
        );
        assert!(
            on("---\ntransition: sli@de-down\n---\n# x\n")
                .unwrap()
                .starts_with("slide-down => A pan: the content slides down")
        );
        assert_eq!(on("---\ntitle: My t@alk\n---\n# x\n"), None);
        assert!(
            on("```mermaid {all|A zo@om}\nx\n```\n")
                .unwrap()
                .starts_with("zoom =>")
        );
        assert!(
            on("```mer@maid\nx\n```\n")
                .unwrap()
                .starts_with("mermaid =>")
        );
        assert!(
            on("```rust {wid@th=50%}\nx\n```\n")
                .unwrap()
                .starts_with("width=50% =>")
        );
        // Not inside the block.
        assert_eq!(on("```rust\nlet zo@om = 1;\n```\n"), None);
        assert!(on("![](a.png){bor@der}").unwrap().starts_with("border =>"));
        assert_eq!(on("plain bor@der text"), None);
    }
}
