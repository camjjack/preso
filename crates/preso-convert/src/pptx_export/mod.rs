//! Editable-text PowerPoint export (`preso-convert --to pptx`).
//!
//! Unlike `preso --export-pptx` (picture-per-slide, pixel-faithful), this
//! produces *editable* slides: headings, paragraphs, and bullets become
//! text runs, tables become PowerPoint tables, notes become notes pages,
//! and images embed as pictures. Math, Mermaid, and Graphviz have no
//! editable form, so they render to PNG via `preso-diagram`. Themes are
//! deliberately not translated (plain slides); PowerPoint has no flow
//! layout, so blocks are stacked with estimated heights and text boxes
//! auto-shrink on overflow — expect to nudge things after a handoff.
//! Reveals export fully revealed. Anything unrepresentable is dropped
//! with a warning, like every other conversion direction.

pub(crate) mod blocks;
mod xml;

use anyhow::Context as _;
use blocks::{Block, Run};
use std::io::Write as _;
use std::path::Path;

/// The produced file plus non-fatal notices.
pub struct PptxExport {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

const EMU_PER_PT: f64 = 12_700.0;
const EMU_PER_PX: i64 = 9_525; // 96 dpi
const SLIDE_H: i64 = 6_858_000; // 7.5in
const MARGIN: i64 = 457_200; // 0.5in
const GAP: i64 = 137_160; // 0.15in between stacked blocks

// Font sizes in points (a denser, document-like scale — this is a handoff
// artifact, not the presented deck).
const H_SIZES: [f32; 6] = [30.0, 26.0, 22.0, 20.0, 18.0, 16.0];
const BODY_PT: f32 = 16.0;
const CODE_PT: f32 = 12.0;
const TABLE_PT: f32 = 13.0;
const FOOTNOTE_PT: f32 = 10.0;

/// Convert preso `source` (includes already expanded) into an editable
/// `.pptx`. `deck_dir` resolves relative image paths.
pub fn export(source: &str, deck_dir: &Path) -> anyhow::Result<PptxExport> {
    let parsed = preso_core::parser::parse(source).context("parse deck")?;

    let slide_w = match parsed.frontmatter.aspect.as_deref() {
        Some(a) => {
            let (w, h) = a
                .split_once(':')
                .and_then(|(w, h)| {
                    Some((w.trim().parse::<f64>().ok()?, h.trim().parse::<f64>().ok()?))
                })
                .unwrap_or((16.0, 9.0));
            ((SLIDE_H as f64) * w / h).round() as i64
        }
        None => 12_192_000,
    };

    let mut ctx = Ctx {
        renderer: preso_diagram::Renderer::new(&[], "Helvetica", "Menlo"),
        deck_dir,
        media: Vec::new(),
        warnings: Vec::new(),
    };

    // Build every slide part first (they allocate media as they go).
    let mut slides: Vec<SlideParts> = Vec::new();
    for (i, slide) in parsed.slides.iter().enumerate() {
        slides.push(build_slide(&mut ctx, slide, i + 1, slide_w));
    }
    if parsed.slides.iter().any(|s| s.step_count() > 1) {
        ctx.warnings
            .push("reveal steps are exported fully revealed".into());
    }

    let title = parsed
        .frontmatter
        .title
        .clone()
        .unwrap_or_else(|| "preso deck".into());
    let bytes = assemble(&title, slide_w, &slides, &ctx.media)?;
    Ok(PptxExport {
        bytes,
        warnings: ctx.warnings,
    })
}

struct Ctx<'a> {
    renderer: preso_diagram::Renderer,
    deck_dir: &'a Path,
    /// Embedded media in order: `(bytes, extension)` → `ppt/media/imageN.ext`.
    media: Vec<(Vec<u8>, &'static str)>,
    warnings: Vec<String>,
}

struct SlideParts {
    xml: String,
    rels: Vec<(usize, &'static str, String, bool)>,
    notes: Option<String>,
}

/// Per-slide shape/relationship allocator.
struct Build {
    shapes: String,
    rels: Vec<(usize, &'static str, String, bool)>,
    next_rid: usize,
    next_id: usize,
    /// Document-order ordinal of the next code fence, pairing it with
    /// `Slide::code_blocks` (annotations like `{width=NN%}`).
    code_ordinal: usize,
}

impl Build {
    fn new() -> Self {
        Build {
            // rId1 is the slideLayout; media/links/notes follow.
            rels: vec![(
                1,
                "slideLayout",
                "../slideLayouts/slideLayout1.xml".into(),
                false,
            )],
            shapes: String::new(),
            next_rid: 2,
            next_id: 2,
            code_ordinal: 0,
        }
    }
    fn rid(&mut self, rtype: &'static str, target: String, external: bool) -> usize {
        let rid = self.next_rid;
        self.next_rid += 1;
        self.rels.push((rid, rtype, target, external));
        rid
    }
    fn id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

fn build_slide(ctx: &mut Ctx, slide: &preso_core::Slide, n: usize, slide_w: i64) -> SlideParts {
    let mut build = Build::new();
    let content_w = slide_w - 2 * MARGIN;
    let last = slide.step_count().saturating_sub(1);

    if let Some((left_src, right_src)) = slide.columns_at(last) {
        // Two columns, side by side; each column stacks independently.
        let (lp, rp) = slide.layout.column_portions().unwrap_or((1, 1));
        let total = i64::from(lp) + i64::from(rp);
        let lw = (content_w - GAP) * i64::from(lp) / total;
        let rw = content_w - GAP - lw;
        layout_blocks(
            ctx,
            &mut build,
            slide,
            n,
            &blocks::parse_blocks(&left_src),
            MARGIN,
            lw,
            MARGIN,
        );
        layout_blocks(
            ctx,
            &mut build,
            slide,
            n,
            &blocks::parse_blocks(&right_src),
            MARGIN + lw + GAP,
            rw,
            MARGIN,
        );
    } else {
        let parsed = blocks::parse_blocks(slide.step_source(last));
        layout_blocks(
            ctx, &mut build, slide, n, &parsed, MARGIN, content_w, MARGIN,
        );
    }

    // Footnote: a small muted line pinned to the bottom.
    if let Some(footnote) = &slide.footnote {
        let id = build.id();
        let para = format!(
            r#"<a:p><a:r><a:rPr lang="en-US" sz="{}" dirty="0"><a:solidFill><a:srgbClr val="888888"/></a:solidFill></a:rPr><a:t>{}</a:t></a:r></a:p>"#,
            (FOOTNOTE_PT * 100.0) as u32,
            xml::xml_escape(footnote)
        );
        let h = pt_h(FOOTNOTE_PT, 1);
        build.shapes.push_str(&xml::text_shape(
            id,
            "Footnote",
            (MARGIN, SLIDE_H - MARGIN / 2 - h, content_w, h),
            &para,
        ));
    }
    if slide.video.is_some() {
        ctx.warnings.push(format!(
            "slide {n}: video has no editable-pptx form; dropped"
        ));
    }
    if !slide.layer_images.is_empty() {
        ctx.warnings.push(format!(
            "slide {n}: layer decoration image(s) dropped (theme-adjacent styling)"
        ));
    }
    let notes: Vec<String> = slide.notes_at(last).map(|note| note.text.clone()).collect();
    SlideParts {
        xml: xml::slide_part(&build.shapes),
        rels: build.rels,
        notes: (!notes.is_empty()).then(|| notes.join("\n\n")),
    }
}

/// Stack `blocks` top-to-bottom in the column `[x, x+w]`, starting at `y`.
#[allow(clippy::too_many_arguments)]
fn layout_blocks(
    ctx: &mut Ctx,
    build: &mut Build,
    slide: &preso_core::Slide,
    n: usize,
    parsed: &[Block],
    x: i64,
    w: i64,
    mut y: i64,
) {
    let mut overflowed = false;
    for block in parsed {
        let h = emit_block(ctx, build, slide, n, block, x, y, w);
        y += h + GAP;
        if y > SLIDE_H - MARGIN && !overflowed {
            overflowed = true;
            ctx.warnings.push(format!(
                "slide {n}: content likely overflows; adjust positions in PowerPoint"
            ));
        }
    }
}

/// Emit one block at `(x, y)` within width `w`; returns its estimated height.
#[allow(clippy::too_many_arguments)]
fn emit_block(
    ctx: &mut Ctx,
    build: &mut Build,
    slide: &preso_core::Slide,
    n: usize,
    block: &Block,
    x: i64,
    y: i64,
    w: i64,
) -> i64 {
    match block {
        Block::Heading(level, runs) => {
            let pt = H_SIZES[usize::from(*level - 1).min(5)];
            let bold: Vec<Run> = runs
                .iter()
                .cloned()
                .map(|mut r| {
                    r.bold = true;
                    r
                })
                .collect();
            let para = paragraph(build, &bold, pt, None);
            let h = text_height(&bold, pt, w, 0);
            let id = build.id();
            build
                .shapes
                .push_str(&xml::text_shape(id, "Heading", (x, y, w, h), &para));
            h
        }
        Block::Paragraph(runs) => {
            let para = paragraph(build, runs, BODY_PT, None);
            let h = text_height(runs, BODY_PT, w, 0);
            let id = build.id();
            build
                .shapes
                .push_str(&xml::text_shape(id, "Text", (x, y, w, h), &para));
            h
        }
        Block::List(items) => {
            let mut paras = String::new();
            let mut h = 0;
            for item in items {
                paras.push_str(&paragraph(
                    build,
                    &item.runs,
                    BODY_PT,
                    Some(xml::list_ppr(item.level, item.ordered)),
                ));
                h += text_height(&item.runs, BODY_PT, w, i64::from(item.level) * 342_900);
            }
            let id = build.id();
            build
                .shapes
                .push_str(&xml::text_shape(id, "List", (x, y, w, h), &paras));
            h
        }
        Block::Quote(lines) => {
            let mut paras = String::new();
            let mut h = 0;
            for runs in lines {
                let italic: Vec<Run> = runs
                    .iter()
                    .cloned()
                    .map(|mut r| {
                        r.italic = true;
                        r
                    })
                    .collect();
                paras.push_str(&paragraph(
                    build,
                    &italic,
                    BODY_PT,
                    Some(r#"<a:pPr marL="228600"><a:buNone/></a:pPr>"#.into()),
                ));
                h += text_height(runs, BODY_PT, w, 228_600);
            }
            let id = build.id();
            build
                .shapes
                .push_str(&xml::text_shape(id, "Quote", (x, y, w, h), &paras));
            h
        }
        Block::Code(lang, lines) => {
            let ordinal = build.code_ordinal;
            build.code_ordinal += 1;
            // Diagram fences have no editable form: render to PNG like the
            // live app does, honoring a `{width=NN%}` fence annotation.
            if let Some(l) = lang.as_deref()
                && matches!(l, "mermaid" | "dot" | "graphviz")
            {
                let width_pct = slide
                    .code_blocks
                    .get(ordinal)
                    .and_then(preso_core::CodeBlock::width_percent);
                let source = lines.join("\n");
                match diagram_png(ctx, l, &source) {
                    Some((media_n, ext, px_w, px_h)) => {
                        return emit_picture(
                            build, media_n, ext, px_w, px_h, width_pct, x, y, w, true,
                        );
                    }
                    None => ctx.warnings.push(format!(
                        "a {l} diagram failed to render; emitted as code text"
                    )),
                }
            }
            let paras: String = lines
                .iter()
                .map(|line| {
                    let run = Run {
                        text: if line.is_empty() {
                            " ".into()
                        } else {
                            line.clone()
                        },
                        bold: false,
                        italic: false,
                        code: true,
                        link: None,
                    };
                    format!(
                        "<a:p>{}</a:p>",
                        xml::run_xml(&run, (CODE_PT * 100.0) as u32, None)
                    )
                })
                .collect();
            let h = pt_h(CODE_PT, lines.len().max(1)) + 182_880; // + inset padding
            let id = build.id();
            build
                .shapes
                .push_str(&xml::code_shape(id, (x, y, w, h), &paras));
            h
        }
        Block::Table(idx) => match slide.tables.get(*idx) {
            Some(table) => emit_table(build, table, x, y, w),
            None => 0,
        },
        Block::Math(idx) => match slide.math_blocks.get(*idx) {
            Some(math) => match math_png(ctx, &math.latex) {
                Some((media_n, ext, px_w, px_h)) => {
                    emit_picture(build, media_n, ext, px_w, px_h, None, x, y, w, true)
                }
                None => {
                    ctx.warnings.push(format!(
                        "math {:?} failed to render; emitted as code",
                        math.latex
                    ));
                    emit_block(
                        ctx,
                        build,
                        slide,
                        n,
                        &Block::Code(None, vec![math.latex.clone()]),
                        x,
                        y,
                        w,
                    )
                }
            },
            None => 0,
        },
        Block::Image { url, alt } => {
            let (path, width_pct, centered, highlight) = image_target(url);
            match load_image(ctx, path) {
                Some((media_n, ext, px_w, px_h)) => emit_image(
                    ctx, build, slide, n, media_n, ext, px_w, px_h, width_pct, x, y, w, centered,
                    highlight,
                ),
                None => {
                    ctx.warnings
                        .push(format!("image {path:?} not embeddable; alt text used"));
                    emit_block(
                        ctx,
                        build,
                        slide,
                        n,
                        &Block::Paragraph(blocks::parse_inlines(alt)),
                        x,
                        y,
                        w,
                    )
                }
            }
        }
        Block::ImageRow(idx) => match slide.image_rows.get(*idx) {
            Some(row) => {
                let count = row.images.len().max(1) as i64;
                let cell_w = (w - GAP * (count - 1)) / count;
                let mut max_h = 0;
                for (i, image) in row.images.iter().enumerate() {
                    let (path, width_pct, _, highlight) = image_target(&image.url);
                    if let Some((media_n, ext, px_w, px_h)) = load_image(ctx, path) {
                        let h = emit_image(
                            ctx,
                            build,
                            slide,
                            n,
                            media_n,
                            ext,
                            px_w,
                            px_h,
                            width_pct,
                            x + (cell_w + GAP) * i as i64,
                            y,
                            cell_w,
                            true,
                            highlight,
                        );
                        max_h = max_h.max(h);
                    } else {
                        ctx.warnings
                            .push(format!("image {path:?} not embeddable; skipped from row"));
                    }
                }
                max_h
            }
            None => 0,
        },
    }
}

/// Height of one paragraph of `runs` at `pt`, wrapped to `w` minus `indent`.
fn text_height(runs: &[Run], pt: f32, w: i64, indent: i64) -> i64 {
    let chars: usize = runs.iter().map(|r| r.text.chars().count()).sum();
    let char_w = f64::from(pt) * 0.48 * EMU_PER_PT;
    let per_line = (((w - indent) as f64) / char_w).max(8.0) as usize;
    pt_h(pt, chars.div_ceil(per_line).max(1))
}

/// Height of `lines` lines at `pt` with line spacing.
fn pt_h(pt: f32, lines: usize) -> i64 {
    (lines as f64 * f64::from(pt) * 1.4 * EMU_PER_PT) as i64 + 91_440
}

/// One `<a:p>` from runs, allocating hyperlink rels as needed.
fn paragraph(build: &mut Build, runs: &[Run], pt: f32, ppr: Option<String>) -> String {
    let mut p = String::from("<a:p>");
    if let Some(ppr) = ppr {
        p.push_str(&ppr);
    }
    for run in runs {
        let link_rid = run
            .link
            .as_ref()
            .map(|url| build.rid("hyperlink", url.clone(), true));
        p.push_str(&xml::run_xml(run, (pt * 100.0) as u32, link_rid));
    }
    p.push_str("</a:p>");
    p
}

fn emit_table(build: &mut Build, table: &preso_core::Table, x: i64, y: i64, w: i64) -> i64 {
    let ncols = table.headers.len().max(1) as i64;
    let col_w = w / ncols;
    let row_h = (f64::from(TABLE_PT) * 2.0 * EMU_PER_PT) as i64;
    let mut cells: Vec<Vec<String>> = Vec::new();
    let header: Vec<String> = table
        .headers
        .iter()
        .map(|cell| {
            let bold: Vec<Run> = blocks::parse_inlines(cell)
                .into_iter()
                .map(|mut r| {
                    r.bold = true;
                    r
                })
                .collect();
            paragraph(build, &bold, TABLE_PT, None)
        })
        .collect();
    cells.push(header);
    for row in &table.rows {
        cells.push(
            (0..table.headers.len().max(1))
                .map(|c| {
                    let text = row.get(c).map(String::as_str).unwrap_or("");
                    paragraph(build, &blocks::parse_inlines(text), TABLE_PT, None)
                })
                .collect(),
        );
    }
    let h = row_h * cells.len() as i64;
    let id = build.id();
    build.shapes.push_str(&xml::table_frame(
        id,
        (x, y, w, h),
        &vec![col_w; ncols as usize],
        row_h,
        &cells,
    ));
    h
}

/// Scale a picture into the column; returns its placed rect (EMU).
#[allow(clippy::too_many_arguments)]
fn placed_rect(
    px_w: u32,
    px_h: u32,
    width_pct: Option<f32>,
    x: i64,
    y: i64,
    w: i64,
    centered: bool,
) -> (i64, i64, i64, i64) {
    let natural_w = i64::from(px_w) * EMU_PER_PX;
    let natural_h = i64::from(px_h) * EMU_PER_PX;
    let mut pic_w = match width_pct {
        Some(pct) => ((w as f64) * f64::from(pct) / 100.0) as i64,
        None => natural_w.min(w),
    };
    let max_h = SLIDE_H * 55 / 100;
    let mut pic_h = pic_w * natural_h / natural_w.max(1);
    if pic_h > max_h {
        pic_h = max_h;
        pic_w = pic_h * natural_w / natural_h.max(1);
    }
    let px = if centered { x + (w - pic_w) / 2 } else { x };
    (px, y, pic_w, pic_h)
}

fn push_picture(build: &mut Build, media_n: usize, ext: &'static str, rect: (i64, i64, i64, i64)) {
    let rid = build.rid("image", format!("../media/image{media_n}.{ext}"), false);
    let id = build.id();
    build.shapes.push_str(&xml::picture_shape(id, rid, rect));
}

/// Place a picture scaled into the column; returns its height.
#[allow(clippy::too_many_arguments)]
fn emit_picture(
    build: &mut Build,
    media_n: usize,
    ext: &'static str,
    px_w: u32,
    px_h: u32,
    width_pct: Option<f32>,
    x: i64,
    y: i64,
    w: i64,
    centered: bool,
) -> i64 {
    let rect = placed_rect(px_w, px_h, width_pct, x, y, w, centered);
    push_picture(build, media_n, ext, rect);
    rect.3
}

/// Place a content picture and, if it carries a highlight group, draw its
/// callouts as native DrawingML shapes: `under` shapes below the picture,
/// `fill`/`spotlight` washes above it. Returns the picture height.
#[allow(clippy::too_many_arguments)]
fn emit_image(
    ctx: &mut Ctx,
    build: &mut Build,
    slide: &preso_core::Slide,
    n: usize,
    media_n: usize,
    ext: &'static str,
    px_w: u32,
    px_h: u32,
    width_pct: Option<f32>,
    x: i64,
    y: i64,
    w: i64,
    centered: bool,
    highlight: Option<usize>,
) -> i64 {
    let rect = placed_rect(px_w, px_h, width_pct, x, y, w, centered);
    let group = highlight.and_then(|i| slide.highlights.get(i));
    match group {
        Some(group) if !group.is_empty() => {
            let (behind, front) = highlight_shapes(ctx, build, n, group, rect);
            build.shapes.push_str(&behind);
            push_picture(build, media_n, ext, rect);
            build.shapes.push_str(&front);
        }
        _ => push_picture(build, media_n, ext, rect),
    }
    rect.3
}

/// Build the DrawingML for one image's highlight group: `(behind, front)`
/// XML to bracket the picture. `under` shapes go behind; `fill` shapes and
/// a single holed `spotlight` scrim go in front. Step-gating is ignored —
/// this export is fully revealed, like the rest of it.
fn highlight_shapes(
    ctx: &mut Ctx,
    build: &mut Build,
    n: usize,
    group: &[preso_core::Highlight],
    rect: (i64, i64, i64, i64),
) -> (String, String) {
    use preso_core::HighlightMode as Mode;
    let (px, py, pw, ph) = rect;
    // A region's fractional box → an EMU rect on the placed picture.
    let region = |h: &preso_core::Highlight| -> (i64, i64, i64, i64) {
        (
            px + (f64::from(h.x) * pw as f64) as i64,
            py + (f64::from(h.y) * ph as f64) as i64,
            (f64::from(h.w) * pw as f64) as i64,
            (f64::from(h.h) * ph as f64) as i64,
        )
    };

    let mut behind = String::new();
    let mut front = String::new();
    let mut spots: Vec<&preso_core::Highlight> = Vec::new();
    let mut clipped = false;

    for h in group {
        clipped |= h.clip && h.mode != Mode::Under;
        match h.mode {
            Mode::Spotlight => spots.push(h),
            Mode::Under | Mode::Fill => {
                let ellipse = h.shape == preso_core::HighlightShape::Ellipse;
                let hex = resolve_hex(h.color.as_deref(), false);
                let fill = fill_element(&hex, h.opacity);
                let line = stroke_element(h.stroke, &hex);
                let id = build.id();
                let name = if h.mode == Mode::Under {
                    "Highlight (under)"
                } else {
                    "Highlight"
                };
                let shape = xml::highlight_shape(id, name, ellipse, region(h), &fill, &line);
                if h.mode == Mode::Under {
                    behind.push_str(&shape);
                } else {
                    front.push_str(&shape);
                }
            }
        }
    }

    if let Some(first) = spots.first() {
        // One scrim, styled by the first spotlight, with a rectangular hole
        // per region (an ellipse region degrades to a rectangular cutout).
        let hex = resolve_hex(first.color.as_deref(), true);
        let fill = fill_element(&hex, first.opacity);
        let holes: Vec<(i64, i64, i64, i64)> = spots
            .iter()
            .map(|h| {
                let (rx, ry, rw, rh) = region(h);
                (rx - px, ry - py, rw, rh)
            })
            .collect();
        let id = build.id();
        let scrim = xml::spotlight_scrim(id, rect, &holes, &fill);
        // Draw the scrim under any fill shapes that share the image.
        front.insert_str(0, &scrim);
    }

    if clipped {
        ctx.warnings.push(format!(
            "slide {n}: highlight `clip` can't be an editable shape; the wash isn't confined to the image's opaque pixels"
        ));
    }
    (behind, front)
}

/// A `<a:solidFill>` (or `<a:noFill/>` when fully transparent) for a
/// highlight, `hex` being 6 uppercase hex digits and `opacity` in `0.0`–`1.0`.
fn fill_element(hex: &str, opacity: f32) -> String {
    if opacity <= 0.0 {
        return "<a:noFill/>".into();
    }
    let alpha = (opacity.clamp(0.0, 1.0) * 100_000.0).round() as i64;
    format!(
        r#"<a:solidFill><a:srgbClr val="{hex}"><a:alpha val="{alpha}"/></a:srgbClr></a:solidFill>"#
    )
}

/// A solid `<a:ln>` outline `stroke` design units wide, or empty for none.
fn stroke_element(stroke: f32, hex: &str) -> String {
    if stroke <= 0.0 {
        return String::new();
    }
    let w = (f64::from(stroke) * EMU_PER_PX as f64).round() as i64;
    format!(r#"<a:ln w="{w}"><a:solidFill><a:srgbClr val="{hex}"/></a:solidFill></a:ln>"#)
}

/// Resolve a highlight `color=` (a `#hex` or theme palette name) to 6 hex
/// digits. Themes aren't translated here, so palette names map to fixed
/// defaults that mirror the live renderer's fallbacks.
fn resolve_hex(color: Option<&str>, spotlight: bool) -> String {
    match color {
        None if spotlight => "000000".into(),
        None | Some("accent") => "4472C4".into(),
        Some("text") => "1F1F1F".into(),
        Some("heading") => "000000".into(),
        Some("link") => "0563C1".into(),
        Some("muted") => "888888".into(),
        Some(other) => parse_hex(other).unwrap_or_else(|| "4472C4".into()),
    }
}

/// `#rgb` / `#rrggbb` → 6 uppercase hex digits (no `#`).
fn parse_hex(s: &str) -> Option<String> {
    let h = s.strip_prefix('#')?;
    let full = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => h.to_string(),
        _ => return None,
    };
    full.chars()
        .all(|c| c.is_ascii_hexdigit())
        .then(|| full.to_ascii_uppercase())
}

/// `x.png#preso-img=width:40+align:center+hl:0` →
/// (path, width %, centered?, highlight-group index).
fn image_target(url: &str) -> (&str, Option<f32>, bool, Option<usize>) {
    match url.split_once("#preso-img=") {
        Some((path, spec)) => {
            let mut width = None;
            let mut centered = false;
            let mut highlight = None;
            for token in spec.split('+') {
                if let Some(v) = token.strip_prefix("width:") {
                    width = v.parse().ok();
                } else if token == "align:center" {
                    centered = true;
                } else if let Some(v) = token.strip_prefix("hl:") {
                    highlight = v.parse().ok();
                }
            }
            (path, width, centered, highlight)
        }
        None => (url, None, false, None),
    }
}

/// Load (and if needed rasterize) an image file into `ctx.media`;
/// returns `(media number, px width, px height)`.
fn load_image(ctx: &mut Ctx, rel: &str) -> Option<(usize, &'static str, u32, u32)> {
    let path = ctx.deck_dir.join(rel);
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext == "svg" {
        let svg = std::fs::read_to_string(&path).ok()?;
        let raster = ctx.renderer.rasterize(&svg, 2.0).ok()?;
        let png = raster_png(&raster)?;
        return Some(push_media(ctx, png, "png", raster.width, raster.height));
    }
    let bytes = std::fs::read(&path).ok()?;
    let (w, h) = image::image_dimensions(&path).ok()?;
    let ext = match ext.as_str() {
        "png" => "png",
        "jpg" | "jpeg" => "jpeg",
        "gif" => "gif",
        _ => return None,
    };
    Some(push_media(ctx, bytes, ext, w, h))
}

/// Render a Mermaid or Graphviz fence to PNG media.
fn diagram_png(ctx: &mut Ctx, lang: &str, source: &str) -> Option<(usize, &'static str, u32, u32)> {
    let svg = match lang {
        "mermaid" => ctx.renderer.mermaid_svg(source, false).ok()?,
        _ => ctx.renderer.graphviz_svg(source).ok()?,
    };
    let raster = ctx.renderer.rasterize(&svg, 2.0).ok()?;
    let png = raster_png(&raster)?;
    Some(push_media(ctx, png, "png", raster.width, raster.height))
}

/// Render display math to PNG media.
fn math_png(ctx: &mut Ctx, latex: &str) -> Option<(usize, &'static str, u32, u32)> {
    let svg = ctx.renderer.math_svg(latex, true, (0, 0, 0), 28.0).ok()?;
    let raster = ctx.renderer.rasterize(&svg, 2.0).ok()?;
    let png = raster_png(&raster)?;
    Some(push_media(ctx, png, "png", raster.width, raster.height))
}

fn push_media(
    ctx: &mut Ctx,
    bytes: Vec<u8>,
    ext: &'static str,
    w: u32,
    h: u32,
) -> (usize, &'static str, u32, u32) {
    ctx.media.push((bytes, ext));
    (ctx.media.len(), ext, w, h)
}

fn raster_png(raster: &preso_diagram::Raster) -> Option<Vec<u8>> {
    use image::ImageEncoder;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &raster.rgba,
            raster.width,
            raster.height,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
    Some(png)
}

/// Assemble the OOXML package.
fn assemble(
    title: &str,
    slide_w: i64,
    slides: &[SlideParts],
    media: &[(Vec<u8>, &'static str)],
) -> anyhow::Result<Vec<u8>> {
    use zip::write::SimpleFileOptions;
    let xml_opts =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let media_opts =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let put =
        |zip: &mut zip::ZipWriter<std::io::Cursor<Vec<u8>>>, name: &str, bytes: &[u8], opts| {
            zip.start_file(name, opts)
                .and_then(|()| zip.write_all(bytes).map_err(Into::into))
                .with_context(|| format!("write {name}"))
        };

    // [Content_Types].xml
    let mut types = String::from(
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>"#,
    );
    for ext in ["png", "jpeg", "gif"] {
        if media.iter().any(|(_, e)| e == &ext) {
            types.push_str(&format!(
                r#"<Default Extension="{ext}" ContentType="image/{ext}"/>"#
            ));
        }
    }
    types.push_str(r#"<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/notesMasters/notesMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/ppt/theme/theme2.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#);
    for (i, parts) in slides.iter().enumerate() {
        let n = i + 1;
        types.push_str(&format!(
            r#"<Override PartName="/ppt/slides/slide{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#
        ));
        if parts.notes.is_some() {
            types.push_str(&format!(
                r#"<Override PartName="/ppt/notesSlides/notesSlide{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"/>"#
            ));
        }
    }
    let content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">{types}</Types>"#
    );

    put(
        &mut zip,
        "[Content_Types].xml",
        content_types.as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "_rels/.rels",
        xml::PACKAGE_RELS.as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "docProps/core.xml",
        xml::core_props(title).as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "docProps/app.xml",
        xml::APP_PROPS.as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/presentation.xml",
        xml::presentation_part(slides.len(), slide_w, SLIDE_H).as_bytes(),
        xml_opts,
    )?;
    let mut pres_rels = vec![
        (
            1,
            "slideMaster",
            "slideMasters/slideMaster1.xml".to_string(),
            false,
        ),
        (
            2,
            "notesMaster",
            "notesMasters/notesMaster1.xml".to_string(),
            false,
        ),
    ];
    for i in 0..slides.len() {
        pres_rels.push((i + 3, "slide", format!("slides/slide{}.xml", i + 1), false));
    }
    put(
        &mut zip,
        "ppt/_rels/presentation.xml.rels",
        xml::rels_part(&pres_rels).as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/slideMasters/slideMaster1.xml",
        xml::slide_master().as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        xml::MASTER_RELS.as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/notesMasters/notesMaster1.xml",
        xml::notes_master().as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/notesMasters/_rels/notesMaster1.xml.rels",
        xml::NOTES_MASTER_RELS.as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/slideLayouts/slideLayout1.xml",
        xml::slide_layout().as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        xml::LAYOUT_RELS.as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/theme/theme1.xml",
        xml::theme("preso").as_bytes(),
        xml_opts,
    )?;
    put(
        &mut zip,
        "ppt/theme/theme2.xml",
        xml::theme("preso notes").as_bytes(),
        xml_opts,
    )?;

    for (i, parts) in slides.iter().enumerate() {
        let n = i + 1;
        put(
            &mut zip,
            &format!("ppt/slides/slide{n}.xml"),
            parts.xml.as_bytes(),
            xml_opts,
        )?;
        let mut rels = parts.rels.clone();
        if parts.notes.is_some() {
            let rid = rels.iter().map(|(r, ..)| *r).max().unwrap_or(1) + 1;
            rels.push((
                rid,
                "notesSlide",
                format!("../notesSlides/notesSlide{n}.xml"),
                false,
            ));
        }
        put(
            &mut zip,
            &format!("ppt/slides/_rels/slide{n}.xml.rels"),
            xml::rels_part(&rels).as_bytes(),
            xml_opts,
        )?;
        if let Some(notes) = &parts.notes {
            put(
                &mut zip,
                &format!("ppt/notesSlides/notesSlide{n}.xml"),
                xml::notes_slide_part(notes).as_bytes(),
                xml_opts,
            )?;
            let notes_rels = vec![
                (
                    1,
                    "notesMaster",
                    "../notesMasters/notesMaster1.xml".to_string(),
                    false,
                ),
                (2, "slide", format!("../slides/slide{n}.xml"), false),
            ];
            put(
                &mut zip,
                &format!("ppt/notesSlides/_rels/notesSlide{n}.xml.rels"),
                xml::rels_part(&notes_rels).as_bytes(),
                xml_opts,
            )?;
        }
    }
    for (i, (bytes, ext)) in media.iter().enumerate() {
        put(
            &mut zip,
            &format!("ppt/media/image{}.{ext}", i + 1),
            bytes,
            media_opts,
        )?;
    }

    Ok(zip.finish().context("finish zip")?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn export_deck(src: &str) -> PptxExport {
        export(src, Path::new(".")).unwrap()
    }

    fn part(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> String {
        use std::io::Read;
        let mut s = String::new();
        zip.by_name(name)
            .unwrap_or_else(|_| panic!("missing part {name}"))
            .read_to_string(&mut s)
            .unwrap();
        s
    }

    fn archive(bytes: &[u8]) -> zip::ZipArchive<std::io::Cursor<&[u8]>> {
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap()
    }

    #[test]
    fn text_slides_produce_editable_runs() {
        let src = "---\ntitle: T\n---\n\n# Hello\n\nSome **bold** and `code` text.\n\n- one\n- two\n\n<!-- note: say hi -->\n";
        let e = export_deck(src);
        let mut zip = archive(&e.bytes);
        let slide = part(&mut zip, "ppt/slides/slide1.xml");
        assert!(slide.contains("<a:t>Hello</a:t>"));
        assert!(slide.contains(r#"b="1"/><a:t>bold</a:t>"#) || slide.contains(r#"b="1">"#));
        assert!(slide.contains(xml::MONO_FACE));
        assert!(slide.contains("buChar"), "bullet list present");
        let notes = part(&mut zip, "ppt/notesSlides/notesSlide1.xml");
        assert!(notes.contains("say hi"));
        // Both masters and both themes exist.
        for p in [
            "ppt/slideMasters/slideMaster1.xml",
            "ppt/notesMasters/notesMaster1.xml",
            "ppt/theme/theme1.xml",
            "ppt/theme/theme2.xml",
        ] {
            part(&mut zip, p);
        }
    }

    #[test]
    fn tables_math_and_links_map() {
        let src = "| A | B |\n|---|---|\n| 1 | 2 |\n\n$$\nE = mc^2\n$$\n\nsee [docs](https://example.com)\n";
        let e = export_deck(src);
        let mut zip = archive(&e.bytes);
        let slide = part(&mut zip, "ppt/slides/slide1.xml");
        assert!(slide.contains("<a:tbl>"));
        assert!(slide.contains("<a:t>A</a:t>"));
        assert!(slide.contains("<p:pic>"), "math becomes an embedded image");
        let rels = part(&mut zip, "ppt/slides/_rels/slide1.xml.rels");
        assert!(rels.contains("https://example.com"));
        assert!(rels.contains(r#"TargetMode="External""#));
        assert!(rels.contains("../media/image1.png"));
        assert!(zip.by_name("ppt/media/image1.png").is_ok());
    }

    #[test]
    fn diagram_fences_become_embedded_pictures() {
        let src = "```mermaid {width=60%}\ngraph TD\n    A[Start] --> B[End]\n```\n\n```dot\ndigraph { a -> b; }\n```\n\n```rust\nfn main() {}\n```\n";
        let e = export_deck(src);
        let mut zip = archive(&e.bytes);
        let slide = part(&mut zip, "ppt/slides/slide1.xml");
        // Two diagrams → two pictures; the rust fence stays editable text.
        assert_eq!(slide.matches("<p:pic>").count(), 2, "{slide}");
        assert!(slide.contains("fn main"), "code stays text");
        assert!(zip.by_name("ppt/media/image1.png").is_ok());
        assert!(zip.by_name("ppt/media/image2.png").is_ok());
        // Both diagrams rendered — no fallback-to-code warnings. (An
        // overflow warning is fine: three stacked blocks genuinely don't
        // fit, and saying so is the layout heuristic's job.)
        assert!(
            !e.warnings.iter().any(|w| w.contains("failed to render")),
            "{:?}",
            e.warnings
        );
    }

    #[test]
    fn round_trips_through_the_importer() {
        let src = "---\ntitle: RT\n---\n\n# First\n\nbody text here\n\n<!-- note: remember -->\n\n---\n\n## Second\n\n| H1 | H2 |\n|----|----|\n| a | b |\n";
        let e = export_deck(src);
        let dir = std::env::temp_dir().join(format!("preso-eptx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rt.pptx");
        std::fs::write(&path, &e.bytes).unwrap();

        let back = crate::convert_pptx(&path, None).unwrap();
        assert!(back.output.contains("First"));
        assert!(back.output.contains("body text here"));
        assert!(
            back.output.contains("remember"),
            "notes survive: {}",
            back.output
        );
        assert!(back.output.contains("H1") && back.output.contains("b"));
        let deck = preso_core::parser::parse(&back.output).unwrap();
        assert_eq!(deck.slides.len(), 2);
    }

    fn image_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("preso-hl-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 40, 40, 255]))
            .save(dir.join("p.png"))
            .unwrap();
        dir
    }

    #[test]
    fn image_highlights_become_native_shapes() {
        let dir = image_dir("shapes");
        let src = "<!-- highlight: ellipse x=10% y=20% w=30% h=40% color=#ff0000 opacity=0.5 stroke=2 -->\n\
                   <!-- highlight: rect x=50% y=50% w=20% h=20% spotlight -->\n\
                   <!-- highlight: rect x=5% y=5% w=10% h=10% under color=#0f0 -->\n\
                   ![p](p.png)\n";
        let e = export(src, &dir).unwrap();
        assert!(
            !e.warnings.iter().any(|w| w.contains("highlight")),
            "highlights are mapped, not dropped: {:?}",
            e.warnings
        );
        let mut zip = archive(&e.bytes);
        let slide = part(&mut zip, "ppt/slides/slide1.xml");

        // Fill ellipse with a translucent red, plus an outline.
        assert!(slide.contains(r#"prst="ellipse""#), "{slide}");
        assert!(slide.contains(r#"<a:srgbClr val="FF0000"><a:alpha val="50000"/>"#));
        assert!(slide.contains("<a:ln w="), "stroke outline present");
        // Spotlight → a holed scrim, black by default.
        assert!(slide.contains("<a:custGeom>"), "spotlight scrim");
        assert!(slide.contains(r#"<a:srgbClr val="000000"><a:alpha"#));
        // Under shape uses the expanded #0f0 and sits *behind* the picture.
        let under = slide.find(r#"val="00FF00""#).expect("under fill");
        let pic = slide.find("<p:pic>").expect("picture");
        let scrim = slide.find("<a:custGeom>").expect("scrim");
        assert!(under < pic, "under shape draws before the picture");
        assert!(scrim > pic, "spotlight scrim draws after the picture");
    }

    #[test]
    fn clip_highlight_warns_it_cant_be_a_shape() {
        let dir = image_dir("clip");
        let src = "<!-- highlight: rect x=10% y=10% w=20% h=20% spotlight clip -->\n![p](p.png)\n";
        let e = export(src, &dir).unwrap();
        assert!(
            e.warnings.iter().any(|w| w.contains("clip")),
            "{:?}",
            e.warnings
        );
        // The shape is still emitted (an approximation), just not confined.
        let mut zip = archive(&e.bytes);
        assert!(part(&mut zip, "ppt/slides/slide1.xml").contains("<a:custGeom>"));
    }

    #[test]
    fn warnings_cover_the_dropped_features() {
        let src = "# S\n\n<!-- video: v.mp4 -->\n<!-- image: deco.png -->\n\n![missing](nope.png)\n\ncontent\n<!-- pause -->\nmore\n";
        let e = export_deck(src);
        let joined = e.warnings.join("\n");
        assert!(joined.contains("video"), "{joined}");
        assert!(joined.contains("layer decoration"), "{joined}");
        assert!(joined.contains("nope.png"), "{joined}");
        assert!(joined.contains("fully revealed"), "{joined}");
    }
}
