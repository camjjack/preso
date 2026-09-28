//! What can be completed at a cursor: whole directives after `<!--`, the
//! values a directive takes, and asset paths. The caller turns a
//! [`Context`] into completion items — listing directories for a
//! [`Context::Path`] is its job, since this module does no I/O.
//!
//! Lines are 0-based; columns are byte offsets within the line.

use crate::lint::{self, RefKind};
use crate::outline::{self, LineKind};
use crate::parser;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Context {
    /// After `<!--`: a whole directive, replacing `replace` (which runs from
    /// the `<!--` through any `-->` already after the cursor).
    Directive { replace: Range<usize> },
    /// One of a fixed set of values.
    Value {
        values: &'static [&'static str],
        replace: Range<usize>,
    },
    /// A `<!-- slide: … -->` key or flag.
    SlideKey { replace: Range<usize> },
    /// A file path. `dir` is the directory part typed so far (`""` or ending
    /// in `/`), relative to the deck; `replace` covers the name after it.
    Path {
        kind: RefKind,
        dir: String,
        replace: Range<usize>,
    },
    /// Inside a diagram fence's `{…}`: its attributes, or — in a zoom stage
    /// — a node label. Which labels exist is only known once `source` is
    /// drawn, which is the caller's job. A label may hold spaces, so it
    /// replaces `label` (back to the stage's last `|` or `,`); an attribute
    /// replaces `attr` (back to the last space).
    DiagramAnnotation {
        language: String,
        source: String,
        attr: Range<usize>,
        label: Range<usize>,
    },
}

/// A directive to offer after `<!--`, in LSP snippet syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snippet {
    pub label: &'static str,
    pub snippet: &'static str,
    pub detail: &'static str,
}

pub const DIRECTIVE_SNIPPETS: &[Snippet] = &[
    Snippet {
        label: "layout: TwoColumn",
        snippet: "<!-- layout: TwoColumn -->",
        detail: "Two columns, split at a `***` line",
    },
    Snippet {
        label: "layout: TwoColumn ratio",
        snippet: "<!-- layout: TwoColumn ${1:2:1} -->",
        detail: "Two columns sized left:right",
    },
    Snippet {
        label: "slide:",
        snippet: "<!-- slide: $1 -->",
        detail: "Per-slide options: kind, align, background, transition, size…",
    },
    Snippet {
        label: "slide: kind=title",
        snippet: "<!-- slide: kind=title -->",
        detail: "Style this slide with the theme's title overlay",
    },
    Snippet {
        label: "slide: kind=section",
        snippet: "<!-- slide: kind=section -->",
        detail: "Style this slide with the theme's section overlay",
    },
    Snippet {
        label: "slide: background=",
        snippet: "<!-- slide: background=$1 -->",
        detail: "A #colour or a full-bleed image",
    },
    Snippet {
        label: "slide: hidden",
        snippet: "<!-- slide: hidden -->",
        detail: "Leave this slide out of the presentation and exports",
    },
    Snippet {
        label: "note:",
        snippet: "<!-- note: $1 -->",
        detail: "Speaker note",
    },
    Snippet {
        label: "note[n]:",
        snippet: "<!-- note[${1:1}]: $2 -->",
        detail: "Speaker note shown from reveal step n",
    },
    Snippet {
        label: "pause",
        snippet: "<!-- pause -->",
        detail: "Reveal what follows on the next step",
    },
    Snippet {
        label: "footnote:",
        snippet: "<!-- footnote: $1 -->",
        detail: "Small print along the bottom of the slide",
    },
    Snippet {
        label: "video:",
        snippet: "<!-- video: $1 -->",
        detail: "A clip to play on this slide",
    },
    Snippet {
        label: "image:",
        snippet: "<!-- image: $1 position=${2:center} -->",
        detail: "A decoration image on a layer behind the content",
    },
    Snippet {
        label: "highlight:",
        snippet: "<!-- highlight: ${1:rect} x=${2:10}% y=${3:10}% w=${4:30}% h=${5:20}% -->",
        detail: "A callout over the next image",
    },
    Snippet {
        label: "zoom[n]:",
        snippet: "<!-- zoom[${1:1}]: ${2:50}%,${3:50}%,${4:2}x -->",
        detail: "Zoom the slide's content onto a point (x%,y%,magnification) from reveal step n",
    },
    Snippet {
        label: "zoom[n]: all",
        snippet: "<!-- zoom[${1:2}]: all -->",
        detail: "Zoom back out to the whole slide from reveal step n",
    },
    Snippet {
        label: "highlight[n]:",
        snippet: "<!-- highlight[${1:1}]: ${2:rect} x=${3:10}% y=${4:10}% w=${5:30}% h=${6:20}% -->",
        detail: "A callout over the next image, shown from reveal step n",
    },
    Snippet {
        label: "table: size=",
        snippet: "<!-- table: size=${1:24} -->",
        detail: "Text size of the next table",
    },
    Snippet {
        label: "include:",
        snippet: "<!-- include: $1 -->",
        detail: "Splice in another markdown file's slides",
    },
];

/// Tokens for an image's `{…}` group.
pub const IMAGE_ATTRS: &[&str] = &[
    "width=25%",
    "width=50%",
    "width=75%",
    "width=100%",
    "align=center",
    "align=right",
    "border",
    "shadow",
    "plain",
    "fit",
];

/// Tokens for a code or diagram fence's `{…}` annotation (line numbers are
/// the author's to type).
pub const FENCE_ATTRS: &[&str] = &[
    "all",
    "zoom",
    "width=50%",
    "width=75%",
    "width=100%",
    "size=24",
    "align=left",
    "align=center",
    "align=right",
    "dim",
    "background",
    "transparent",
];

/// Frontmatter keys, ready to take a value.
pub const FRONTMATTER_KEY_ITEMS: &[&str] = &[
    "title: ",
    "theme: ",
    "transition: ",
    "aspect: ",
    "presenter: ",
];

/// Common `aspect:` ratios.
pub const ASPECTS: &[&str] = &["16:9", "4:3", "16:10"];

/// `<!-- image: … -->` options after the path.
pub const LAYER_IMAGE_KEYS: &[&str] = &[
    "position=",
    "width=",
    "opacity=",
    "padding=",
    "padding-top=",
    "padding-right=",
    "padding-bottom=",
    "padding-left=",
];

/// A highlight's shape: its first word.
pub const HIGHLIGHT_SHAPES: &[&str] = &["rect", "ellipse", "circle"];

/// A highlight's options after the shape.
pub const HIGHLIGHT_WORDS: &[&str] = &[
    "x=",
    "y=",
    "w=",
    "h=",
    "color=",
    "opacity=",
    "stroke=",
    "mode=",
    "spotlight",
    "under",
    "clip",
];

/// A highlight's `mode=` values.
pub const HIGHLIGHT_MODES: &[&str] = &["fill", "spotlight", "under", "behind"];

/// `<!-- zoom[n]: … -->` values: back out, or a point to fill in.
pub const ZOOM_VALUES: &[&str] = &["all", "50%,50%,2x"];

/// `<!-- layout: … -->` completions.
pub const LAYOUT_VALUES: &[&str] = &["TwoColumn", "TwoColumn 2:1", "TwoColumn 1:2", "Content"];

/// A snippet with its tab stops removed, for clients without snippet
/// support: `${1:x}` → `x`, `$1` → ``.
pub fn plain(snippet: &str) -> String {
    let mut out = String::new();
    let mut rest = snippet;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        if let Some(inner) = rest.strip_prefix('{')
            && let Some(close) = inner.find('}')
        {
            let body = &inner[..close];
            out.push_str(body.split_once(':').map_or("", |(_, v)| v));
            rest = &inner[close + 1..];
        } else {
            rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        }
    }
    out.push_str(rest);
    out
}

/// File extensions worth offering for a reference kind.
pub fn extensions(kind: RefKind) -> &'static [&'static str] {
    match kind {
        RefKind::Image | RefKind::LayerImage | RefKind::Background => lint::IMAGE_EXTENSIONS,
        RefKind::Video => lint::VIDEO_EXTENSIONS,
        RefKind::Include => &["md"],
        RefKind::Theme => &["toml"],
    }
}

/// What to complete with the cursor at `col` on `line`, if anything.
pub fn context(source: &str, line: usize, col: usize) -> Option<Context> {
    let text = source.lines().nth(line)?;
    let mut col = col.min(text.len());
    while !text.is_char_boundary(col) {
        col -= 1;
    }
    let prefix = &text[..col];
    if let Some((yaml, _)) = parser::frontmatter_span(source)
        && yaml.contains(&line)
    {
        return frontmatter(prefix, col);
    }
    let (kind, opens_fence) = line_kind(source, line)?;

    // Inside `{…}`: a fence's annotation, or an image's attributes.
    let attrs = |open: usize, values: &'static [&'static str]| {
        let typed = &prefix[open + 1..];
        (!typed.contains('}')).then(|| {
            let token = typed
                .rfind([' ', ','])
                .map_or(open + 1, |i| open + 1 + i + 1);
            Context::Value {
                values,
                replace: token..col,
            }
        })
    };
    if kind == LineKind::Code {
        // The language, typed straight after the fence's backticks.
        let marks = prefix.trim_start();
        let fence_len = marks.chars().take_while(|&c| c == '`' || c == '~').count();
        if opens_fence
            && fence_len >= 3
            && !prefix.contains('{')
            && marks[fence_len..]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric())
        {
            return Some(Context::Value {
                values: lint::DIAGRAM_LANGUAGES,
                replace: col - (marks.len() - fence_len)..col,
            });
        }
        let open = opens_fence.then(|| prefix.find('{')).flatten()?;
        let (_, block) = parser::clean_fence_line(text);
        let language = block.language.unwrap_or_default();
        if !lint::DIAGRAM_LANGUAGES.contains(&language.as_str()) {
            return attrs(open, FENCE_ATTRS);
        }
        let Some(Context::Value { replace: attr, .. }) = attrs(open, FENCE_ATTRS) else {
            return None;
        };
        let typed = &prefix[open + 1..];
        let stage_at = typed
            .rfind(['|', ','])
            .map_or(open + 1, |i| open + 1 + i + 1);
        let label_at =
            stage_at + (prefix[stage_at..].len() - prefix[stage_at..].trim_start().len());
        return Some(Context::DiagramAnnotation {
            language,
            source: fence_body(source, line),
            attr,
            label: label_at..col,
        });
    }
    if kind != LineKind::Text {
        return None;
    }
    if let Some(close) = prefix.rfind("){")
        && prefix[..close].contains("![")
        && let Some(found) = attrs(close + 1, IMAGE_ATTRS)
    {
        return Some(found);
    }

    if let Some(open) = prefix.rfind("![")
        && let Some(close) = prefix[open..].find("](")
    {
        let at = open + close + 2;
        if !prefix[at..].contains(')') {
            let at = at + usize::from(prefix[at..].starts_with('<'));
            return Some(path(RefKind::Image, prefix, at));
        }
    }

    let opener = prefix.find("<!--")?;
    if !prefix[..opener].trim().is_empty() {
        return None;
    }
    let body_at = opener + 4;
    let body = &prefix[body_at..];
    let Some(colon) = body.find(':') else {
        let name_like = body
            .trim_start()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_[]".contains(c));
        if !name_like {
            return None;
        }
        let after = &text[col..];
        let end = if after.trim() == "-->" {
            col + after.trim_end().len()
        } else {
            col
        };
        return Some(Context::Directive {
            replace: opener..end,
        });
    };

    let name = body[..colon].trim();
    let spec_at = body_at + colon + 1;
    let spec = &prefix[spec_at..];
    let value_at = spec_at + (spec.len() - spec.trim_start().len());
    // The last whitespace-separated token, being typed.
    let token_at = spec_at
        + spec
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map_or(spec.len() - spec.trim_start().len(), |(i, c)| {
                i + c.len_utf8()
            });
    let token = &prefix[token_at..];
    // A step-gated name (`zoom[2]`) takes the same values as its base.
    let name = name.split_once('[').map_or(name, |(base, _)| base);
    match name {
        "zoom" => Some(Context::Value {
            values: ZOOM_VALUES,
            replace: value_at..col,
        }),
        "layout" => Some(Context::Value {
            values: LAYOUT_VALUES,
            replace: value_at..col,
        }),
        "video" => Some(path(RefKind::Video, prefix, value_at)),
        "include" => Some(path(RefKind::Include, prefix, value_at)),
        "image" if token_at == value_at => Some(path(RefKind::LayerImage, prefix, value_at)),
        "image" => Some(match token.strip_prefix("position=") {
            Some(_) => Context::Value {
                values: lint::ANCHORS,
                replace: token_at + "position=".len()..col,
            },
            None => Context::Value {
                values: LAYER_IMAGE_KEYS,
                replace: token_at..col,
            },
        }),
        "table" => Some(Context::Value {
            values: &["size="],
            replace: token_at..col,
        }),
        "highlight" => Some(if token_at == value_at {
            Context::Value {
                values: HIGHLIGHT_SHAPES,
                replace: token_at..col,
            }
        } else if token.starts_with("mode=") {
            Context::Value {
                values: HIGHLIGHT_MODES,
                replace: token_at + "mode=".len()..col,
            }
        } else {
            Context::Value {
                values: HIGHLIGHT_WORDS,
                replace: token_at..col,
            }
        }),
        "slide" => match token.split_once('=') {
            None => Some(Context::SlideKey {
                replace: token_at..col,
            }),
            Some(("background", v)) if !v.starts_with('#') => Some(path(
                RefKind::Background,
                prefix,
                token_at + "background=".len(),
            )),
            Some((key, _)) => {
                let (_, values) = lint::SLIDE_KEYS.iter().find(|(k, _)| *k == key)?;
                (!values.is_empty()).then(|| Context::Value {
                    values,
                    replace: token_at + key.len() + 1..col,
                })
            }
        },
        _ => None,
    }
}

/// The lines inside the fence that opens on `open`, up to its close (or
/// the end of the source, if it never closes).
fn fence_body(source: &str, open: usize) -> String {
    let mut fence = crate::fence::Tracker::default();
    let mut body = Vec::new();
    for (i, text) in source.lines().enumerate().skip(open) {
        fence.process(text);
        if i > open {
            if !fence.in_fence() {
                break;
            }
            body.push(text);
        }
    }
    body.join("\n")
}

/// What to complete on a frontmatter line: a key, or the value of one preso
/// reads (a theme by name or `.toml` path).
fn frontmatter(prefix: &str, col: usize) -> Option<Context> {
    let Some((key, value)) = prefix.split_once(':') else {
        let typed = prefix.trim_start();
        return (typed.len() == prefix.len()
            && typed
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .then_some(Context::Value {
            values: FRONTMATTER_KEY_ITEMS,
            replace: 0..col,
        });
    };
    let at = key.len() + 1 + (value.len() - value.trim_start().len());
    let at = at + usize::from(value.trim_start().starts_with(['"', '\'']));
    let typed = &prefix[at.min(prefix.len())..];
    let values: &'static [&'static str] = match key {
        "transition" => lint::TRANSITIONS,
        "presenter" => lint::PRESENTER_LAYOUTS,
        "aspect" => ASPECTS,
        "theme" if typed.contains(['/', '.']) => return Some(path(RefKind::Theme, prefix, at)),
        "theme" => lint::BUILTIN_THEMES,
        _ => return None,
    };
    Some(Context::Value {
        values,
        replace: at.min(col)..col,
    })
}

/// A path context for the text typed from column `at` to the cursor.
fn path(kind: RefKind, prefix: &str, at: usize) -> Context {
    let typed = &prefix[at..];
    let split = typed.rfind('/').map_or(0, |i| i + 1);
    Context::Path {
        kind,
        dir: typed[..split].to_string(),
        replace: at + split..prefix.len(),
    }
}

/// How the slide processor reads `line`, and whether it opens a code
/// fence. `None` for frontmatter and delimiter lines.
pub(crate) fn line_kind(source: &str, line: usize) -> Option<(LineKind, bool)> {
    let body_start = parser::frontmatter_span(source).map_or(0, |(_, start)| start);
    let lines: Vec<&str> = source.lines().collect();
    let segment = parser::segments(source, body_start)
        .into_iter()
        .find(|r| r.contains(&line))?;
    let kinds = outline::classify(&lines[segment.clone()]);
    // A code line opens a fence when no fence is open before it.
    let mut fence = crate::fence::Tracker::default();
    let mut opens = false;
    for (i, kind) in segment.clone().zip(&kinds) {
        if *kind == LineKind::Code {
            let was_open = fence.in_fence();
            fence.process(lines[i]);
            if i == line {
                opens = !was_open;
            }
        }
        if i == line {
            break;
        }
    }
    Some((kinds[line - segment.start], opens))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Complete at the `|` in `line`.
    fn at(line: &str) -> Option<Context> {
        let col = line.find('|').unwrap();
        let src = line.replacen('|', "", 1);
        context(&src, 0, col)
    }

    /// Complete at the `@` in `source` (for lines that use `|` themselves).
    fn at_mark(source: &str) -> Option<Context> {
        let offset = source.find('@').unwrap();
        let src = source.replacen('@', "", 1);
        let line = source[..offset].matches('\n').count();
        let col = offset - source[..offset].rfind('\n').map_or(0, |i| i + 1);
        context(&src, line, col)
    }

    #[test]
    fn frontmatter_keys_and_values_complete() {
        let fm = |line: &str| at_mark(&format!("---\n{line}\n---\n# x\n"));
        assert_eq!(
            fm("tr@"),
            Some(Context::Value {
                values: FRONTMATTER_KEY_ITEMS,
                replace: 0..2
            })
        );
        assert_eq!(
            fm("transition: pa@"),
            Some(Context::Value {
                values: lint::TRANSITIONS,
                replace: 12..14
            })
        );
        assert_eq!(
            fm("presenter: \"n@"),
            Some(Context::Value {
                values: lint::PRESENTER_LAYOUTS,
                replace: 12..13
            })
        );
        assert_eq!(
            fm("theme: d@"),
            Some(Context::Value {
                values: lint::BUILTIN_THEMES,
                replace: 7..8
            })
        );
        assert_eq!(
            fm("theme: themes/c@"),
            Some(Context::Path {
                kind: RefKind::Theme,
                dir: "themes/".into(),
                replace: 14..15
            })
        );
        assert_eq!(fm("title: @"), None);
    }

    #[test]
    fn directive_options_complete() {
        assert_eq!(
            at("<!-- highlight[2]: |"),
            Some(Context::Value {
                values: HIGHLIGHT_SHAPES,
                replace: 19..19
            })
        );
        assert_eq!(
            at("<!-- highlight: rect x=1 |"),
            Some(Context::Value {
                values: HIGHLIGHT_WORDS,
                replace: 25..25
            })
        );
        assert_eq!(
            at("<!-- highlight: rect mode=sp|"),
            Some(Context::Value {
                values: HIGHLIGHT_MODES,
                replace: 26..28
            })
        );
        assert_eq!(
            at("<!-- image: a.png op|"),
            Some(Context::Value {
                values: LAYER_IMAGE_KEYS,
                replace: 18..20
            })
        );
        assert_eq!(
            at("<!-- table: |"),
            Some(Context::Value {
                values: &["size="],
                replace: 12..12
            })
        );
        let labels: Vec<&str> = DIRECTIVE_SNIPPETS.iter().map(|s| s.label).collect();
        assert!(labels.contains(&"highlight[n]:"));
    }

    #[test]
    fn a_fence_language_completes() {
        assert_eq!(
            at_mark("```mer@\ngraph LR\n```\n"),
            Some(Context::Value {
                values: lint::DIAGRAM_LANGUAGES,
                replace: 3..6
            })
        );
        // Not once the language is followed by an annotation or a space.
        assert!(!matches!(
            at_mark("```rust {@}\na\n```\n"),
            Some(Context::Value { values, .. }) if values == lint::DIAGRAM_LANGUAGES
        ));
    }

    #[test]
    fn zoom_directives_complete() {
        let labels: Vec<&str> = DIRECTIVE_SNIPPETS.iter().map(|s| s.label).collect();
        assert!(labels.contains(&"zoom[n]:") && labels.contains(&"zoom[n]: all"));
        // The snippet, filled in with its defaults, is a zoom the parser keeps.
        let snippet = DIRECTIVE_SNIPPETS
            .iter()
            .find(|s| s.label == "zoom[n]:")
            .unwrap();
        let deck = crate::parser::parse(&format!("# x\n{}\n", plain(snippet.snippet))).unwrap();
        assert_eq!(deck.slides[0].zooms.len(), 1);

        assert_eq!(
            at("<!-- zoom[2]: |"),
            Some(Context::Value {
                values: ZOOM_VALUES,
                replace: 14..14
            })
        );
        assert_eq!(
            at("<!-- zoom: a|"),
            Some(Context::Value {
                values: ZOOM_VALUES,
                replace: 11..12
            })
        );
        assert!(FENCE_ATTRS.contains(&"zoom"));
    }

    #[test]
    fn a_diagram_annotation_completes_labels_and_attributes() {
        let src = "```mermaid {all|Read, Parse in@}\ngraph LR\n  a[Parse input]\n```\n";
        let Some(Context::DiagramAnnotation {
            language,
            source,
            attr,
            label,
        }) = at_mark(src)
        else {
            panic!("{:?}", at_mark(src));
        };
        assert_eq!(language, "mermaid");
        assert_eq!(source, "graph LR\n  a[Parse input]");
        let line = src.lines().next().unwrap().replacen('@', "", 1);
        // A label replaces back to the comma, spaces and all; an attribute
        // only the word being typed.
        assert_eq!(&line[label], "Parse in");
        assert_eq!(&line[attr], "in");

        // A code fence still gets plain attributes.
        assert_eq!(
            at_mark("```rust {all|2 zo@}\na\n```\n"),
            Some(Context::Value {
                values: FENCE_ATTRS,
                replace: 15..17
            })
        );
    }

    #[test]
    fn directives_after_an_opener() {
        assert_eq!(at("<!-- |"), Some(Context::Directive { replace: 0..5 }));
        assert_eq!(at("<!-- la|"), Some(Context::Directive { replace: 0..7 }));
        // An auto-closed `-->` after the cursor is replaced too.
        assert_eq!(at("<!-- | -->"), Some(Context::Directive { replace: 0..9 }));
        assert_eq!(at("text <!-- |"), None);
    }

    #[test]
    fn values_after_a_directive_name() {
        assert_eq!(
            at("<!-- layout: Two|"),
            Some(Context::Value {
                values: LAYOUT_VALUES,
                replace: 13..16
            })
        );
        assert_eq!(
            at("<!-- slide: align=center kind=ti| -->"),
            Some(Context::Value {
                values: &["title", "section"],
                replace: 30..32
            })
        );
        assert_eq!(
            at("<!-- slide: align=center k|"),
            Some(Context::SlideKey { replace: 25..26 })
        );
        assert_eq!(
            at("<!-- image: a.png position=|"),
            Some(Context::Value {
                values: lint::ANCHORS,
                replace: 27..27
            })
        );
        assert_eq!(at("<!-- slide: number=|"), None);
    }

    #[test]
    fn paths() {
        assert_eq!(
            at("see ![alt](img/ph|"),
            Some(Context::Path {
                kind: RefKind::Image,
                dir: "img/".into(),
                replace: 15..17
            })
        );
        assert_eq!(
            at("<!-- video: clips/|"),
            Some(Context::Path {
                kind: RefKind::Video,
                dir: "clips/".into(),
                replace: 18..18
            })
        );
        assert_eq!(
            at("<!-- slide: background=b|"),
            Some(Context::Path {
                kind: RefKind::Background,
                dir: "".into(),
                replace: 23..24
            })
        );
        assert_eq!(at("<!-- slide: background=#|"), None);
        assert_eq!(at("![done](a.png) |"), None);
    }

    #[test]
    fn attributes_in_braces() {
        assert_eq!(
            at("![x](a.png){width=50% bo|"),
            Some(Context::Value {
                values: IMAGE_ATTRS,
                replace: 22..24
            })
        );
        assert_eq!(
            at("![x](a.png){|"),
            Some(Context::Value {
                values: IMAGE_ATTRS,
                replace: 12..12
            })
        );
        assert_eq!(at("![x](a.png){border} |"), None);
        let fence = "```rust {2,4 al|\ncode\n```\n";
        let col = fence.find('|').unwrap();
        assert_eq!(
            context(&fence.replacen('|', "", 1), 0, col),
            Some(Context::Value {
                values: FENCE_ATTRS,
                replace: 13..15
            })
        );
        // Inside the code, or on the closing fence: nothing.
        assert_eq!(context("```rust {x}\ncode {\n```\n", 1, 6), None);
        assert_eq!(context("```\ncode\n``` {\n", 2, 5), None);
    }

    #[test]
    fn nothing_inside_code_or_frontmatter() {
        assert_eq!(context("```\n<!-- \n```\n", 1, 5), None);
        assert_eq!(context("---\ntitle: <!-- \n---\n", 1, 16), None);
        assert_eq!(context("# A\n---\n<!-- \n", 1, 0), None); // the delimiter
        assert!(context("# A\n---\n<!-- \n", 2, 5).is_some());
    }

    #[test]
    fn plain_strips_tab_stops() {
        assert_eq!(plain("<!-- note[${1:1}]: $2 -->"), "<!-- note[1]:  -->");
        assert_eq!(
            plain("<!-- layout: TwoColumn ${1:2:1} -->"),
            "<!-- layout: TwoColumn 2:1 -->"
        );
    }

    #[test]
    fn every_snippet_is_a_known_directive() {
        for s in DIRECTIVE_SNIPPETS {
            let text = plain(s.snippet);
            let body = text.trim_start_matches("<!--").trim_start();
            let name = body
                .split(|c: char| c == ':' || c == '[' || c.is_whitespace())
                .next()
                .unwrap();
            assert!(
                lint::DIRECTIVES.contains(&name) || name == "pause",
                "{}",
                s.label
            );
        }
    }

    #[test]
    fn clamps_odd_columns() {
        // Column 6 is inside the two-byte `é`: back off to its start.
        assert_eq!(
            context("<!-- é", 0, 6),
            Some(Context::Directive { replace: 0..5 })
        );
        assert!(context("# A\n", 0, 999).is_none());
        assert!(context("", 3, 0).is_none());
    }
}
