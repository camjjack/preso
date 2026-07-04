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

/// Renders math and Mermaid sources to SVG and rasterizes SVG to RGBA.
pub struct Renderer {
    options: resvg::usvg::Options<'static>,
}

impl Renderer {
    /// `fonts`: font file contents (TTF) made available to SVG `<text>`
    /// rasterization, used as the default sans-serif/monospace families
    /// alongside the system fonts.
    pub fn new(fonts: &[&[u8]], sans_serif: &str, monospace: &str) -> Self {
        let mut options = resvg::usvg::Options::default();
        let db = options.fontdb_mut();
        db.load_system_fonts();
        for font in fonts {
            db.load_font_data(font.to_vec());
        }
        db.set_sans_serif_family(sans_serif);
        db.set_monospace_family(monospace);
        Self { options }
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

    /// Intrinsic size of an SVG in user units, without rasterizing.
    pub fn svg_size(&self, svg: &str) -> Result<(f32, f32), DiagramError> {
        let tree = resvg::usvg::Tree::from_str(svg, &self.options)
            .map_err(|e| DiagramError::Svg(e.to_string()))?;
        let size = tree.size();
        Ok((size.width(), size.height()))
    }

    /// Rasterize an SVG at the given scale factor.
    pub fn rasterize(&self, svg: &str, scale: f32) -> Result<Raster, DiagramError> {
        let tree = resvg::usvg::Tree::from_str(svg, &self.options)
            .map_err(|e| DiagramError::Svg(e.to_string()))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> Renderer {
        Renderer::new(&[], "Helvetica", "Menlo")
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
    fn garbage_mermaid_does_not_panic() {
        // mermaid-rs-renderer is lenient: garbage may render as a trivial
        // diagram rather than erroring. We only require it never panics.
        let r = renderer();
        let _ = r.mermaid_svg("not a diagram at all $$$", false);
        let _ = r.mermaid_svg("", false);
    }
}
