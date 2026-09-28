//! preso-diagram: LaTeX math (RaTeX) and Mermaid diagrams rendered to
//! SVG, plus SVG rasterization with a configurable font set.
//!

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiagramError {
    #[error("math parse error: {0}")]
    MathParse(String),

    #[error("mermaid render error: {0}")]
    Mermaid(String),

    #[error("graphviz parse error: {0}")]
    Graphviz(String),

    #[error("SVG could not be parsed for rasterization: {0}")]
    Svg(String),

    #[error("rasterization produced an empty image")]
    EmptyRaster,
}

/// A rasterized image: straight RGBA, 8 bits per channel.
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A region of an SVG's canvas as fractions (`0.0`–`1.0`) of its size:
/// `x`/`y` the top-left corner. See [`Renderer::label_region`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// How far an SVG's content spills past the canvas it declares, per side, in
/// user units — see [`Renderer::svg_overflow`]. All zero when it fits.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Overflow {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Overflow {
    /// Whether any side spills enough to be worth reporting. Content that
    /// merely *touches* the edge — a border stroke drawn on the boundary,
    /// which is normal — must not count, so this ignores anything under half
    /// a user unit.
    pub fn is_clipped(&self) -> bool {
        const EPSILON: f32 = 0.5;
        self.left.max(self.top).max(self.right).max(self.bottom) > EPSILON
    }

    /// The spilled sides, largest first, as `(side, units)` — for reporting.
    pub fn sides(&self) -> Vec<(&'static str, f32)> {
        let mut sides: Vec<(&'static str, f32)> = [
            ("right", self.right),
            ("bottom", self.bottom),
            ("left", self.left),
            ("top", self.top),
        ]
        .into_iter()
        .filter(|(_, units)| *units > 0.5)
        .collect();
        sides.sort_by(|a, b| b.1.total_cmp(&a.1));
        sides
    }
}

/// CSS generic families. A list ending in one of these already has a
/// fallback usvg can honour (they map to the configured sans/mono faces), so
/// it needs nothing added.
const GENERIC_FAMILIES: [&str; 6] = [
    "sans-serif",
    "serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
];

/// Every `font-family` value in an SVG, as `(byte range of the value, the
/// value)` — covering both the attribute form (`font-family="Arial"`) and
/// the CSS form (`style="font-family:Arial"`, or a `<style>` rule).
fn font_family_values(svg: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut found = Vec::new();
    let bytes = svg.as_bytes();
    let mut at = 0;
    while let Some(hit) = svg[at..].find("font-family") {
        let start = at + hit;
        at = start + "font-family".len();
        // Skip the whitespace and the `=` or `:` that introduces the value.
        let mut i = at;
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let Some(&sep) = bytes.get(i) else { break };
        if sep != b'=' && sep != b':' {
            continue; // `font-family-something`, or a false positive
        }
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        // Attribute values are quoted; CSS ones run to `;`, `}` or the
        // closing quote of the `style` attribute they sit in.
        let (value_start, terminators): (usize, &[u8]) = match bytes.get(i) {
            Some(b'"') => (i + 1, b"\""),
            Some(b'\'') => (i + 1, b"'"),
            _ => (i, b";}\"'"),
        };
        let end = svg[value_start..]
            .find(|c: char| terminators.contains(&(c as u8)))
            .map_or(svg.len(), |o| value_start + o);
        found.push((value_start..end, svg[value_start..end].trim()));
        at = end;
    }
    found
}

/// Whether any family in a `font-family` list is one this renderer can
/// actually set type in — a generic keyword, or a face in the database.
fn list_resolves(list: &str, available: &std::collections::HashSet<String>) -> bool {
    list.split(',').any(|name| {
        let name = name.trim().trim_matches(['"', '\'']).to_lowercase();
        GENERIC_FAMILIES.contains(&name.as_str()) || available.contains(&name)
    })
}

/// Renders math and Mermaid sources to SVG and rasterizes SVG to RGBA.
pub struct Renderer {
    options: resvg::usvg::Options<'static>,
    /// Every family name in `options.fontdb`, lowercased — the set a
    /// `font-family` is checked against. Built once here because the
    /// database is fixed after construction and the check runs on every
    /// parse, which is two to four times per image rendered.
    available_families: std::collections::HashSet<String>,
}

impl Renderer {
    /// `fonts`: font file contents (TTF) made available to SVG `<text>`
    /// rasterization, used as the default sans-serif/monospace families
    /// alongside the system fonts.
    pub fn new(fonts: &[&[u8]], sans_serif: &str, monospace: &str) -> Self {
        Self::with_fonts(true, fonts, sans_serif, monospace)
    }

    fn with_fonts(system: bool, fonts: &[&[u8]], sans_serif: &str, monospace: &str) -> Self {
        use resvg::usvg::fontdb::{Family, Query};

        let mut options = resvg::usvg::Options::default();
        let db = options.fontdb_mut();
        if system {
            db.load_system_fonts();
        }
        for font in fonts {
            db.load_font_data(font.to_vec());
        }
        db.set_sans_serif_family(sans_serif);
        db.set_monospace_family(monospace);
        // A family an SVG names that isn't installed falls back to its
        // generic family, and text with no font at all is dropped. Graphviz
        // asks for `Times, serif`, and fontdb's `serif` is Times New Roman,
        // which most Linux machines lack — so its labels would vanish. Where
        // there's no serif font, `serif` means the sans-serif we ship.
        let serif = Query {
            families: &[Family::Serif],
            ..Query::default()
        };
        if db.query(&serif).is_none() {
            db.set_serif_family(sans_serif);
        }
        let available_families = options
            .fontdb
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.to_lowercase()))
            .collect();
        Self {
            options,
            available_families,
        }
    }

    /// LaTeX → standalone SVG with embedded glyph outlines.
    /// `color` is RGB — RaTeX has no alpha channel, so an RGBA parameter
    /// here would silently drop it. `font_size` is in output user units
    /// (pixels).
    pub fn math_svg(
        &self,
        latex: &str,
        display: bool,
        color: (u8, u8, u8),
        font_size: f32,
    ) -> Result<String, DiagramError> {
        let style = if display {
            ratex_types::math_style::MathStyle::Display
        } else {
            ratex_types::math_style::MathStyle::Text
        };
        let color = ratex_types::color::Color::parse(&format!(
            "#{:02x}{:02x}{:02x}",
            color.0, color.1, color.2
        ))
        .unwrap_or(ratex_types::color::Color::BLACK);

        let layout_opts = ratex_layout::LayoutOptions::default()
            .with_style(style)
            .with_color(color);
        let svg_opts = ratex_svg::SvgOptions {
            font_size: f64::from(font_size),
            padding: f64::from(font_size) * 0.1,
            stroke_width: 1.5,
            embed_glyphs: true,
            font_dir: String::new(),
        };

        let ast = ratex_parser::parser::parse(latex)
            .map_err(|e| DiagramError::MathParse(e.to_string()))?;
        let layout_box = ratex_layout::layout(&ast, &layout_opts);
        let display_list = ratex_layout::to_display_list(&layout_box);
        Ok(ratex_svg::render_to_svg(&display_list, &svg_opts))
    }

    /// Mermaid source → SVG. With `transparent`, the canvas-background
    /// rect renders as `none` so the slide shows through (node fills are
    /// untouched).
    pub fn mermaid_svg(&self, source: &str, transparent: bool) -> Result<String, DiagramError> {
        let mut options = mermaid_rs_renderer::RenderOptions::default();
        if transparent {
            options.theme.background = "none".to_owned();
        }
        mermaid_rs_renderer::render_with_options(source, options)
            .map_err(|e| DiagramError::Mermaid(e.to_string()))
    }

    /// Graphviz DOT source → SVG, via the pure-Rust `layout` engine.
    pub fn graphviz_svg(&self, source: &str) -> Result<String, DiagramError> {
        let graph = layout::gv::DotParser::new(source)
            .process()
            .map_err(DiagramError::Graphviz)?;
        let mut builder = layout::gv::GraphBuilder::new();
        builder.visit_graph(&graph);
        let mut visual = builder.get();
        let mut svg = layout::backends::svg::SVGWriter::new();
        visual.do_it(false, false, false, &mut svg);
        Ok(svg.finalize())
    }

    /// The font families an SVG asks for that this machine hasn't got, and
    /// that name no generic to fall back on — the ones whose text would be
    /// dropped without [`Self::with_font_fallback`]. Lowercased, deduped.
    pub fn missing_font_families(&self, svg: &str) -> Vec<String> {
        let available = &self.available_families;
        let mut missing: Vec<String> = font_family_values(svg)
            .into_iter()
            .filter(|(_, list)| !list_resolves(list, available))
            .map(|(_, list)| list.trim_matches(['"', '\'']).to_string())
            .collect();
        missing.sort();
        missing.dedup();
        missing
    }

    /// Give every unresolvable `font-family` a generic to fall back on.
    ///
    /// usvg drops a text element outright when it can't match the family:
    /// its `font_family` option is the default for text that names *no*
    /// family, not a fallback for one that fails. So an SVG asking for
    /// `Arial` renders every label on a machine that has Arial and none at
    /// all on one that doesn't — a deck that builds locally and comes out of
    /// CI with empty boxes. Appending a generic keyword (which maps to the
    /// configured sans or monospace face) makes the text render in a
    /// substitute instead, as a browser would.
    ///
    /// Only lists that resolve to nothing are touched, so an SVG naming a
    /// font that *is* installed is left exactly as written.
    fn with_font_fallback<'s>(&self, svg: &'s str) -> std::borrow::Cow<'s, str> {
        let available = &self.available_families;
        let patches: Vec<(std::ops::Range<usize>, &str)> = font_family_values(svg)
            .into_iter()
            .filter(|(_, list)| !list_resolves(list, available))
            .map(|(range, list)| {
                // A missing monospace face wants the monospace substitute;
                // the name is all there is to go on.
                let generic = if list.to_lowercase().contains("mono") {
                    "monospace"
                } else {
                    "sans-serif"
                };
                (range, generic)
            })
            .collect();
        if patches.is_empty() {
            return std::borrow::Cow::Borrowed(svg);
        }
        let mut out = String::with_capacity(svg.len() + patches.len() * 12);
        let mut copied = 0;
        for (range, generic) in patches {
            out.push_str(&svg[copied..range.end]);
            out.push_str(", ");
            out.push_str(generic);
            copied = range.end;
        }
        out.push_str(&svg[copied..]);
        std::borrow::Cow::Owned(out)
    }

    /// Parse an SVG the way this renderer draws it: with a generic appended
    /// to any font family the machine can't resolve.
    fn parse(&self, svg: &str) -> Result<resvg::usvg::Tree, DiagramError> {
        resvg::usvg::Tree::from_str(&self.with_font_fallback(svg), &self.options)
            .map_err(|e| DiagramError::Svg(e.to_string()))
    }

    /// Intrinsic size of an SVG in user units, without rasterizing.
    pub fn svg_size(&self, svg: &str) -> Result<(f32, f32), DiagramError> {
        let size = self.parse(svg)?.size();
        Ok((size.width(), size.height()))
    }

    /// How far an SVG's drawn content falls outside the canvas the file
    /// declares, per side, in user units — measured *after* fonts are
    /// substituted, which is what usually causes it.
    ///
    /// A file that hard-codes `width`/`height` with no `viewBox` (what
    /// PowerPoint exports) has a canvas measured for the exact fonts it was
    /// authored with. Substitute a wider face for one this machine hasn't
    /// got and the text overruns that canvas, and the canvas clips it — the
    /// author sees an edge sliced off and no reason why, at every size,
    /// since the clipping happens inside the SVG's own coordinate space.
    pub fn svg_overflow(&self, svg: &str) -> Result<Overflow, DiagramError> {
        let tree = self.parse(svg)?;
        let size = tree.size();
        // The *layer* box, so content a clip path already trims doesn't read
        // as overflow.
        let bbox = tree.root().abs_layer_bounding_box();
        Ok(Overflow {
            left: (-bbox.left()).max(0.0),
            top: (-bbox.top()).max(0.0),
            right: (bbox.right() - size.width()).max(0.0),
            bottom: (bbox.bottom() - size.height()).max(0.0),
        })
    }

    /// Where the nodes labelled `labels` sit in a diagram, for zooming onto
    /// them: the union of each label's node shape, as fractions of the canvas.
    ///
    /// Neither the Mermaid nor the Graphviz backend gives its nodes ids, so
    /// nodes are found by their label text (matched case-insensitively with
    /// whitespace collapsed; exactly if any label matches so, else as a
    /// substring). A label's node is the smallest shape that encloses the
    /// text — which also finds a subgraph by its title. A label without an
    /// enclosing shape of its own (an edge label, say) contributes just its
    /// text. `Ok(None)` when no label is found — including when no font can
    /// shape the labels, since usvg drops text it can't lay out.
    pub fn label_region(
        &self,
        svg: &str,
        labels: &[String],
    ) -> Result<Option<Region>, DiagramError> {
        // The same parse as rasterizing, font fallback and all, so a label
        // that renders is a label that's found.
        let tree = self.parse(svg)?;
        let size = tree.size();
        let canvas_area = size.width() * size.height();

        let mut texts: Vec<(Label, resvg::usvg::Rect)> = Vec::new();
        let mut shapes: Vec<resvg::usvg::Rect> = Vec::new();
        collect_nodes(tree.root(), &mut texts, &mut shapes);

        let mut union: Option<(f32, f32, f32, f32)> = None;
        for label in labels {
            let wanted = normalize(label);
            let exact: Vec<_> = texts.iter().filter(|(t, _)| t.matched == wanted).collect();
            let matched = if exact.is_empty() {
                texts
                    .iter()
                    .filter(|(t, _)| t.matched.contains(&wanted))
                    .collect()
            } else {
                exact
            };
            for (_, text) in matched {
                // The node shape: the smallest enclosing one, unless that's
                // most of the canvas (the background, or a cluster the label
                // merely sits inside).
                let node = shapes
                    .iter()
                    .filter(|s| encloses(s, text))
                    .min_by(|a, b| area(a).total_cmp(&area(b)))
                    .filter(|s| area(s) < canvas_area * 0.5)
                    .unwrap_or(text);
                let (l, t, r, b) = (node.left(), node.top(), node.right(), node.bottom());
                union = Some(match union {
                    None => (l, t, r, b),
                    Some((ul, ut, ur, ub)) => (ul.min(l), ut.min(t), ur.max(r), ub.max(b)),
                });
            }
        }
        Ok(union.map(|(l, t, r, b)| {
            let (l, t) = (l.max(0.0), t.max(0.0));
            let (r, b) = (r.min(size.width()), b.min(size.height()));
            Region {
                x: l / size.width(),
                y: t / size.height(),
                w: ((r - l) / size.width()).max(0.0),
                h: ((b - t) / size.height()).max(0.0),
            }
        }))
    }

    /// The text of every label a diagram draws, in drawing order and without
    /// repeats — the names [`Self::label_region`] can find a node by.
    pub fn labels(&self, svg: &str) -> Result<Vec<String>, DiagramError> {
        let tree = self.parse(svg)?;
        let (mut texts, mut shapes) = (Vec::new(), Vec::new());
        collect_nodes(tree.root(), &mut texts, &mut shapes);
        let mut labels: Vec<String> = Vec::new();
        for (text, _) in texts {
            let label = text.raw.split_whitespace().collect::<Vec<_>>().join(" ");
            if !label.is_empty() && !labels.contains(&label) {
                labels.push(label);
            }
        }
        Ok(labels)
    }

    /// Rasterize an SVG at the given scale factor.
    pub fn rasterize(&self, svg: &str, scale: f32) -> Result<Raster, DiagramError> {
        let tree = self.parse(svg)?;
        let size = tree.size();
        let width = (size.width() * scale).ceil() as u32;
        let height = (size.height() * scale).ceil() as u32;
        if width == 0 || height == 0 {
            return Err(DiagramError::EmptyRaster);
        }
        let mut pixmap =
            resvg::tiny_skia::Pixmap::new(width, height).ok_or(DiagramError::EmptyRaster)?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        Ok(Raster {
            width,
            height,
            rgba: pixmap.take(),
        })
    }
}

/// Walk a usvg tree gathering each text node's (normalized) content and box,
/// and the box of every path — the candidate node shapes.
fn collect_nodes(
    group: &resvg::usvg::Group,
    texts: &mut Vec<(Label, resvg::usvg::Rect)>,
    shapes: &mut Vec<resvg::usvg::Rect>,
) {
    for node in group.children() {
        match node {
            resvg::usvg::Node::Group(g) => collect_nodes(g, texts, shapes),
            resvg::usvg::Node::Path(p) => shapes.push(p.abs_bounding_box()),
            resvg::usvg::Node::Text(t) => {
                let content: String = t
                    .chunks()
                    .iter()
                    .map(|c| c.text())
                    .collect::<Vec<_>>()
                    .join(" ");
                texts.push((
                    Label {
                        matched: normalize(&content),
                        raw: content,
                    },
                    t.abs_bounding_box(),
                ));
            }
            resvg::usvg::Node::Image(_) => {}
        }
    }
}

/// A text node's content: as drawn, and normalized for matching.
struct Label {
    raw: String,
    matched: String,
}

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn area(r: &resvg::usvg::Rect) -> f32 {
    r.width() * r.height()
}

/// Whether `outer` contains `inner` (with a hair of slack for rounding) and
/// is strictly bigger — a label's own underline or text path isn't its node.
fn encloses(outer: &resvg::usvg::Rect, inner: &resvg::usvg::Rect) -> bool {
    const SLACK: f32 = 0.5;
    outer.left() <= inner.left() + SLACK
        && outer.top() <= inner.top() + SLACK
        && outer.right() >= inner.right() - SLACK
        && outer.bottom() >= inner.bottom() - SLACK
        && area(outer) > area(inner) * 1.2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> Renderer {
        Renderer::new(&[], "Helvetica", "Menlo")
    }

    /// A face handed to the renderer outright, so a test asserting that a
    /// family *resolves* doesn't depend on what the machine has installed.
    /// The file the app embeds (see `preso-app::render::INTER_REGULAR`).
    const INTER_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

    /// A renderer with only the font the app ships — no system fonts, as on
    /// a bare CI runner. Label lookup needs a font that can shape the labels
    /// (usvg drops text it can't lay out), so this is the case that counts.
    fn app_renderer() -> Renderer {
        Renderer::with_fonts(false, &[INTER_REGULAR], "Inter", "Inter")
    }

    #[test]
    fn label_region_finds_the_node_around_a_label() {
        let r = app_renderer();
        let svg = r
            .mermaid_svg("graph LR\n  a[Parse] --> b[Layout] --> c[Paint]\n", false)
            .unwrap();
        let labels = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let parse = r.label_region(&svg, &labels(&["Parse"])).unwrap().unwrap();
        let layout = r.label_region(&svg, &labels(&["layout"])).unwrap().unwrap();
        let paint = r.label_region(&svg, &labels(&["Paint"])).unwrap().unwrap();
        // Left to right, each a node-sized piece of the canvas.
        assert!(
            parse.x < layout.x && layout.x < paint.x,
            "{parse:?} {layout:?} {paint:?}"
        );
        for region in [parse, layout, paint] {
            assert!(region.w > 0.05 && region.w < 0.5, "{region:?}");
            assert!(region.h > 0.1, "{region:?}");
        }
        // Several labels zoom onto their union.
        let both = r
            .label_region(&svg, &labels(&["Parse", "Paint"]))
            .unwrap()
            .unwrap();
        assert!(both.x <= parse.x && both.x + both.w >= paint.x + paint.w - 1e-4);
        // An unknown label finds nothing.
        assert_eq!(r.label_region(&svg, &labels(&["Nope"])).unwrap(), None);
    }

    #[test]
    fn graphviz_text_renders_without_a_serif_font() {
        // Graphviz asks for `Times, serif`; with neither installed, its
        // labels used to be dropped. Ink in the middle of the node's shape,
        // well clear of its outline, is the label.
        let r = app_renderer();
        let svg = r.graphviz_svg("digraph { parse }").unwrap();
        let tree = resvg::usvg::Tree::from_str(&svg, &r.options).unwrap();
        let (mut texts, mut shapes) = (Vec::new(), Vec::new());
        collect_nodes(tree.root(), &mut texts, &mut shapes);
        let node = shapes
            .iter()
            .max_by(|a, b| area(a).total_cmp(&area(b)))
            .unwrap();

        let scale = 2.0;
        let raster = r.rasterize(&svg, scale).unwrap();
        let w = raster.width as usize;
        let px = |v: f32| (v * scale) as usize;
        let (cx, cy) = (
            node.left() + node.width() / 2.0,
            node.top() + node.height() / 2.0,
        );
        let (hw, hh) = (node.width() * 0.3, node.height() * 0.3);
        let ink = (px(cy - hh)..px(cy + hh))
            .flat_map(|y| (px(cx - hw)..px(cx + hw)).map(move |x| (y * w + x) * 4))
            .filter(|&i| raster.rgba[i + 3] > 128 && raster.rgba[i] < 100)
            .count();
        assert!(ink > 20, "label ink {ink}");
    }

    #[test]
    fn labels_are_listed_as_drawn() {
        let r = app_renderer();
        let svg = r
            .mermaid_svg(
                "graph LR\n  a[Parse input] --> b[Layout]\n  a --> b\n",
                false,
            )
            .unwrap();
        assert_eq!(r.labels(&svg).unwrap(), ["Parse input", "Layout"]);
    }

    #[test]
    fn label_region_works_on_graphviz() {
        let r = app_renderer();
        let svg = r
            .graphviz_svg("digraph { parse -> layout -> paint }")
            .unwrap();
        let region = r
            .label_region(&svg, &["layout".to_string()])
            .unwrap()
            .expect("graphviz node found");
        assert!(
            region.w > 0.0 && region.w < 0.9 && region.h < 0.5,
            "{region:?}"
        );
    }

    #[test]
    fn math_renders_and_rasterizes() {
        let r = renderer();
        let svg = r
            .math_svg(
                r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}",
                true,
                (255, 255, 255),
                40.0,
            )
            .unwrap();
        assert!(svg.contains("<svg"));
        let raster = r.rasterize(&svg, 2.0).unwrap();
        assert!(raster.width > 100 && raster.height > 40);
        // Some non-transparent pixels must exist
        assert!(raster.rgba.chunks(4).any(|p| p[3] > 0));
    }

    #[test]
    fn invalid_math_is_an_error_not_a_panic() {
        let r = renderer();
        assert!(
            r.math_svg(r"\frac{unclosed", true, (0, 0, 0), 40.0)
                .is_err()
        );
    }

    #[test]
    fn mermaid_renders_and_rasterizes() {
        let r = renderer();
        let svg = r
            .mermaid_svg("graph TD\n    A[Start] --> B[End]\n", false)
            .unwrap();
        assert!(svg.contains("<svg"));
        let raster = r.rasterize(&svg, 1.0).unwrap();
        assert!(raster.width > 50 && raster.height > 50);
    }

    #[test]
    fn transparent_mermaid_has_no_canvas_background() {
        let r = renderer();
        let source = "graph TD\n    A[Start] --> B[End]\n";
        let opaque = r.mermaid_svg(source, false).unwrap();
        let transparent = r.mermaid_svg(source, true).unwrap();
        // The corner pixel sits on the canvas, outside any node.
        let corner_alpha = |svg: &str| r.rasterize(svg, 1.0).unwrap().rgba[3];
        assert_eq!(corner_alpha(&opaque), 0xff);
        assert_eq!(corner_alpha(&transparent), 0);
    }

    #[test]
    fn graphviz_renders_and_rasterizes() {
        let r = renderer();
        let svg = r
            .graphviz_svg("digraph { a -> b; b -> c; a -> c; }")
            .unwrap();
        assert!(svg.contains("<svg"));
        let raster = r.rasterize(&svg, 1.0).unwrap();
        assert!(raster.width > 50 && raster.height > 50);
    }

    #[test]
    fn invalid_graphviz_is_an_error() {
        let r = renderer();
        assert!(r.graphviz_svg("this is not dot {{{{").is_err());
    }

    #[test]
    fn an_unresolvable_font_family_gains_a_generic() {
        let r = renderer();
        // usvg drops text whose family it can't match, so a file naming only
        // a font this machine lacks would render as empty boxes. It gets a
        // generic to fall back on instead.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="20">
                       <text x="0" y="15" font-family="NoSuchFaceXYZ">hi</text></svg>"#;
        let patched = r.with_font_fallback(svg);
        assert!(
            patched.contains(r#"font-family="NoSuchFaceXYZ, sans-serif""#),
            "{patched}"
        );
        assert_eq!(r.missing_font_families(svg), vec!["NoSuchFaceXYZ"]);

        // A family that resolves is left exactly as written — no rewrite,
        // and nothing to report. The face is one this renderer is *given*:
        // naming whatever the machine happens to have would assert about the
        // machine rather than the code, and pass on a Mac while failing on a
        // bare CI runner.
        let embedded = Renderer::new(&[INTER_REGULAR], "Inter", "Menlo");
        let installed = r#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="20">
                             <text font-family="Inter">hi</text></svg>"#;
        assert!(matches!(
            embedded.with_font_fallback(installed),
            std::borrow::Cow::Borrowed(_)
        ));
        assert!(embedded.missing_font_families(installed).is_empty());

        // So is a list that already ends in a generic — that is exactly the
        // fallback this adds.
        let listed = r#"<text font-family="Calibri, sans-serif">hi</text>"#;
        assert!(matches!(
            r.with_font_fallback(listed),
            std::borrow::Cow::Borrowed(_)
        ));

        // The CSS form is rewritten too, in a `style` attribute or a rule.
        let styled = r#"<text style="font-family:NoSuchFaceXYZ;fill:red">hi</text>"#;
        assert!(
            r.with_font_fallback(styled)
                .contains("font-family:NoSuchFaceXYZ, sans-serif;fill:red"),
            "{}",
            r.with_font_fallback(styled)
        );

        // A missing monospace face falls back to the monospace substitute,
        // since its name is the only clue to what it was for. Invented, like
        // the name above: a real one (Consolas Mono) would be asserting that
        // this machine lacks it.
        let mono = r#"<text font-family="NoSuchFaceMonoXYZ">x</text>"#;
        assert!(
            r.with_font_fallback(mono)
                .contains("NoSuchFaceMonoXYZ, monospace"),
            "{}",
            r.with_font_fallback(mono)
        );
    }

    #[test]
    fn overflow_is_measured_against_the_declared_canvas() {
        let r = renderer();
        // A rect running 20 units past a 100-wide canvas: the file's own
        // canvas clips it, and no amount of resizing the image will help.
        let over = r
            .svg_overflow(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">
                     <rect x="10" y="10" width="110" height="20" fill="black"/>
                   </svg>"#,
            )
            .unwrap();
        assert!(over.is_clipped());
        assert_eq!(over.sides(), vec![("right", 20.0)]);
        assert_eq!(over.left, 0.0);

        // Content that fits raises nothing.
        let fits = r
            .svg_overflow(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">
                     <rect x="10" y="10" width="50" height="20" fill="black"/>
                   </svg>"#,
            )
            .unwrap();
        assert!(!fits.is_clipped());
        assert_eq!(fits.sides(), vec![]);

        // A border stroked *on* the boundary is normal and must stay quiet —
        // half its width sits outside, which is why the epsilon exists.
        let border = r
            .svg_overflow(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">
                     <rect x="0.5" y="0.5" width="99" height="49" fill="none"
                           stroke="black" stroke-width="1"/>
                   </svg>"#,
            )
            .unwrap();
        assert!(!border.is_clipped(), "{border:?}");

        // Spilling off the top-left counts too, and sides come back largest
        // first.
        let corner = r
            .svg_overflow(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">
                     <rect x="-30" y="-5" width="40" height="20" fill="black"/>
                   </svg>"#,
            )
            .unwrap();
        assert_eq!(corner.sides(), vec![("left", 30.0), ("top", 5.0)]);
    }

    #[test]
    fn garbage_mermaid_does_not_panic() {
        // mermaid-rs-renderer is lenient: garbage may render as a trivial
        // diagram rather than erroring. We only require it never panics.
        let r = renderer();
        let _ = r.mermaid_svg("not a diagram at all $$$", false);
        let _ = r.mermaid_svg("", false);
    }
}
