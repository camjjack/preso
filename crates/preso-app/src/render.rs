//! Theme → iced conversions and the shared slide markdown renderer.
//!
//! All theme sizes are design units on the 1920×1080 virtual canvas; views
//! pass a `scale` to map them onto real window pixels.

use crate::app::Message;
use crate::media::Media;
use iced::widget::{column, container, markdown, rich_text};
use iced::{Color, Element, Fill, Font, Pixels, border, padding};
use std::cell::Cell;

pub const DESIGN_WIDTH: f32 = 1920.0;
pub const DESIGN_HEIGHT: f32 = 1080.0;

/// Monospace advance width as a fraction of the font size (JetBrains Mono and
/// most code fonts are ~0.6em). Used to estimate a code block's content width
/// since iced offers no measure-before-layout hook.
const MONO_ADVANCE: f32 = 0.6;

/// Bundled fonts, shared by the iced font system (loaded in `main`) and
/// the SVG rasterizer in [`Media`].
pub const INTER_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
pub const INTER_BOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-Bold.ttf");
pub const INTER_ITALIC: &[u8] = include_bytes!("../../../assets/fonts/Inter-Italic.ttf");
pub const INTER_BOLD_ITALIC: &[u8] = include_bytes!("../../../assets/fonts/Inter-BoldItalic.ttf");
pub const JETBRAINS_MONO: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");

/// Bundled body font.
pub const BODY_FONT: Font = Font::with_name("Inter");
/// Bundled monospace font.
pub const MONO_FONT: Font = Font::with_name("JetBrains Mono");

/// `Font::with_name` needs a `'static` name; theme families are dynamic
/// strings, so leak each distinct name once.
pub fn font_named(name: &str) -> Font {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static NAMES: Mutex<Option<HashMap<String, &'static str>>> = Mutex::new(None);

    let mut names = NAMES.lock().expect("font name registry");
    let names = names.get_or_insert_with(HashMap::new);
    let leaked = names
        .entry(name.to_string())
        .or_insert_with(|| Box::leak(name.to_string().into_boxed_str()));
    Font::with_name(leaked)
}

/// Family names a loaded font file declares in its `name` table (both the
/// legacy and the typographic/preferred family). Empty if the bytes don't
/// parse. Lets the app report which families are actually available.
pub fn font_family_names(data: &[u8]) -> Vec<String> {
    let Ok(face) = ttf_parser::Face::parse(data, 0) else {
        return Vec::new();
    };
    let names = face.names();
    let mut out = Vec::new();
    for i in 0..names.len() {
        // name_id 1 = Family, 16 = Typographic (preferred) Family.
        if let Some(name) = names.get(i)
            && (name.name_id == 1 || name.name_id == 16)
            && let Some(s) = name.to_string()
        {
            out.push(s);
        }
    }
    out
}

/// All font families preso loaded this run: the bundled fonts plus the
/// theme's `[fonts] files`. cosmic-text may *also* resolve installed
/// system fonts, which aren't included here.
pub fn loaded_families(theme_fonts: &[Vec<u8>]) -> std::collections::BTreeSet<String> {
    let mut set = std::collections::BTreeSet::new();
    for data in [
        INTER_REGULAR,
        INTER_BOLD,
        INTER_ITALIC,
        INTER_BOLD_ITALIC,
        JETBRAINS_MONO,
    ] {
        set.extend(font_family_names(data));
    }
    for data in theme_fonts {
        set.extend(font_family_names(data));
    }
    set
}

pub fn body_font(theme: &preso_style::Theme) -> Font {
    theme
        .fonts
        .body_family
        .as_deref()
        .map_or(BODY_FONT, font_named)
}

pub fn heading_font(theme: &preso_style::Theme) -> Font {
    theme
        .fonts
        .heading_family
        .as_deref()
        .map_or_else(|| body_font(theme), font_named)
}

pub fn code_font(theme: &preso_style::Theme) -> Font {
    theme
        .fonts
        .code_family
        .as_deref()
        .map_or(MONO_FONT, font_named)
}

/// Resolve `code_theme` against the highlighter's built-in themes
/// (case/whitespace-insensitive); unknown names fall back to Base16Ocean.
pub fn highlight_theme(theme: &preso_style::Theme) -> iced::highlighter::Theme {
    let wanted = theme.code_theme.replace([' ', '-', '_'], "").to_lowercase();
    iced::highlighter::Theme::ALL
        .iter()
        .find(|t| t.to_string().replace([' ', '-', '_'], "").to_lowercase() == wanted)
        .copied()
        .unwrap_or(iced::highlighter::Theme::Base16Ocean)
}

/// Snap a scale factor to a coarse ladder (1/16 steps) so window resizing
/// produces a small, stable set of text sizes instead of a continuum.
pub fn quantize_scale(raw: f32) -> f32 {
    ((raw * 16.0).floor() / 16.0).max(1.0 / 16.0)
}

pub fn color(c: preso_style::Color) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, f32::from(c.a) / 255.0)
}

/// Padding that insets a corner-anchored item from its two anchored edges
/// by `px` (horizontal) and `py` (vertical) — values already scaled.
fn corner_padding(corner: preso_style::Corner, px: f32, py: f32) -> iced::Padding {
    use preso_style::Corner;
    let mut p = iced::Padding::from(0.0);
    match corner {
        Corner::TopLeft => (p.top, p.left) = (py, px),
        Corner::TopRight => (p.top, p.right) = (py, px),
        Corner::BottomLeft => (p.bottom, p.left) = (py, px),
        Corner::BottomRight => (p.bottom, p.right) = (py, px),
    }
    p
}

/// Map a [`preso_core::Anchor`] to iced horizontal/vertical alignment, for
/// placing a layer image within a fill container (uniform padding then insets
/// it from whichever edges it's anchored to).
fn anchor_alignment(
    anchor: preso_core::Anchor,
) -> (iced::alignment::Horizontal, iced::alignment::Vertical) {
    use iced::alignment::{Horizontal as H, Vertical as V};
    use preso_core::Anchor::{
        Bottom, BottomLeft, BottomRight, Center, Left, Right, Top, TopLeft, TopRight,
    };
    match anchor {
        TopLeft => (H::Left, V::Top),
        Top => (H::Center, V::Top),
        TopRight => (H::Right, V::Top),
        Left => (H::Left, V::Center),
        Center => (H::Center, V::Center),
        Right => (H::Right, V::Center),
        BottomLeft => (H::Left, V::Bottom),
        Bottom => (H::Center, V::Bottom),
        BottomRight => (H::Right, V::Bottom),
    }
}

/// Build an iced `Padding` (logical px) from per-side design-unit values
/// `[top, right, bottom, left]` (as resolved by the style sections'
/// `padding_sides`).
fn scaled_padding(sides: [f32; 4], scale: f32) -> iced::Padding {
    iced::Padding {
        top: sides[0] * scale,
        right: sides[1] * scale,
        bottom: sides[2] * scale,
        left: sides[3] * scale,
    }
}

/// iced's default relative text line height; the markdown widget renders
/// headings at this multiple of their font size.
const HEADING_LINE_HEIGHT: f32 = 1.3;

/// Height a leading heading of `level` occupies before the body begins,
/// in design units: its line box plus the paragraph gap the markdown
/// widget inserts after it (see `markdown_settings`).
fn heading_band(theme: &preso_style::Theme, level: u8) -> f32 {
    let f = &theme.fonts;
    let size = match level {
        1 => f.h1_size,
        2 => f.h2_size,
        3 => f.h3_size,
        4 => f.h3_size * 0.85,
        5 => f.h3_size * 0.7,
        _ => f.h3_size * 0.6,
    };
    size * HEADING_LINE_HEIGHT + theme.spacing.paragraph_gap
}

/// Top padding (logical px) for each column of a two-column slide so the
/// columns' body text aligns. A leading heading defines a "header band";
/// the band is sized to the taller of the two headings, and the column
/// with the shorter (or no) heading is pushed down to match. Returns
/// `(left_pad, right_pad)` — at least one is always 0.
///
/// Heading height is estimated from theme font metrics (iced gives no
/// measure-before-layout), so a heading that wraps to two lines aligns
/// approximately; single-line headings — the norm — are exact.
pub fn column_header_pads(
    left: Option<u8>,
    right: Option<u8>,
    theme: &preso_style::Theme,
    scale: f32,
) -> (f32, f32) {
    let band = |level: Option<u8>| level.map(|l| heading_band(theme, l)).unwrap_or(0.0);
    let (l, r) = (band(left), band(right));
    let max = l.max(r);
    ((max - l) * scale, (max - r) * scale)
}

/// Custom iced theme so default widget styling follows the deck theme.
pub fn iced_theme(theme: &preso_style::Theme) -> iced::Theme {
    iced::Theme::custom(
        theme.name.clone(),
        iced::theme::Palette {
            background: color(theme.colors.background),
            text: color(theme.colors.text),
            primary: color(theme.colors.heading),
            ..iced::theme::Palette::DARK
        },
    )
}

/// Markdown widget settings derived from the theme at a given scale.
///
/// Scaled sizes are rounded to whole pixels: fractional text sizes vary
/// continuously during resize and bloat the glyph caches.
pub fn markdown_settings(theme: &preso_style::Theme, scale: f32) -> markdown::Settings {
    let s = |v: f32| Pixels((v * scale).round().max(1.0));
    let f = &theme.fonts;
    markdown::Settings {
        text_size: s(f.body_size),
        h1_size: s(f.h1_size),
        h2_size: s(f.h2_size),
        h3_size: s(f.h3_size),
        h4_size: s(f.h3_size * 0.85),
        h5_size: s(f.h3_size * 0.7),
        h6_size: s(f.h3_size * 0.6),
        code_size: s(f.code_size),
        spacing: s(theme.spacing.paragraph_gap),
        style: markdown::Style {
            font: body_font(theme),
            inline_code_highlight: markdown::Highlight {
                background: color(theme.colors.code_background).into(),
                border: border::rounded((4.0 * scale).round()),
            },
            inline_code_padding: padding::left((4.0 * scale).round()).right((4.0 * scale).round()),
            inline_code_color: color(theme.colors.accent),
            inline_code_font: code_font(theme),
            code_block_font: code_font(theme),
            link_color: color(theme.colors.link),
        },
    }
}

/// Everything a slide render needs beyond the markdown content itself.
#[derive(Clone, Copy)]
pub struct SlideContext<'a> {
    pub media: &'a Media,
    /// Slide whose `code_blocks` annotations apply (per-column sub-slide
    /// for two-column layouts).
    pub code_slide: &'a preso_core::Slide,
    /// Slide whose `math_blocks` the `preso-math:` markers index into.
    pub math_slide: &'a preso_core::Slide,
    pub theme: &'a preso_style::Theme,
    pub scale: f32,
    /// Time since app start, for animated content (GIF frame selection).
    /// `Duration::ZERO` in static contexts (export) for determinism.
    pub animation_time: std::time::Duration,
    /// Horizontal alignment of the slide's content blocks, already
    /// resolved (per-slide override → theme). See [`resolve_halign`].
    pub halign: preso_style::HorizontalAlign,
    /// Current reveal step (0-based). Selects the click-through highlight
    /// stage of multi-stage code blocks (`{2-3|5|all}`) and which
    /// `<!-- highlight[n]: … -->` image callouts are visible.
    pub step: usize,
    /// DPI factor of the window being rendered into, for the tiny-skia
    /// canvas workarounds (see `overlay::compensated_frame`). `1.0` in
    /// offscreen contexts (export).
    pub scale_factor: f32,
    /// Highlight-author mode (`H` on the presenter): stack an interactive
    /// drag canvas over every image so dragging a box publishes a
    /// `preso-hl-draw:` URI (see `HighlightAuthor`). Only ever set for the
    /// presenter's *current* slide, which renders interactively.
    pub authoring: bool,
}

/// Resolve a slide's horizontal alignment: a per-slide `halign=` override
/// wins over the (kind-resolved) theme's `[slide].halign`.
pub fn resolve_halign(
    overrides: &preso_core::SlideOverrides,
    theme: &preso_style::Theme,
) -> preso_style::HorizontalAlign {
    use preso_style::HorizontalAlign as H;
    match overrides.halign.as_deref() {
        Some("center") => H::Center,
        Some("right") => H::Right,
        Some("left") => H::Left,
        _ => theme.slide.halign,
    }
}

impl SlideContext<'_> {
    /// Usable content width in logical pixels at this render scale.
    pub fn content_width(&self) -> f32 {
        (DESIGN_WIDTH - 2.0 * self.theme.spacing.slide_padding) * self.scale
    }
}

/// Markdown viewer wired to the deck theme and media cache.
struct PresoViewer<'a> {
    ctx: SlideContext<'a>,
    /// Document-order ordinal of the next code block, to pair the
    /// rendered block with its fence annotation.
    code_ordinal: Cell<usize>,
}

impl<'a> PresoViewer<'a> {
    /// Render a GFM table with the theme's `[table]` styling: header fill,
    /// per-column alignment, zebra striping, and row separators. preso draws
    /// tables itself because iced's markdown table can't be themed (cells are
    /// inaccessible and only separator colour is stylable).
    fn render_table(
        &self,
        table: &'a preso_core::Table,
        settings: markdown::Settings,
    ) -> Element<'a, markdown::Uri> {
        use iced::FillPortion;
        use iced::alignment::Horizontal;

        let t = &self.ctx.theme.table;
        let pal = &self.ctx.theme.colors;
        let scale = self.ctx.scale;
        let header_bg = color(t.header_background.unwrap_or(pal.code_background));
        let header_fg = color(t.header_color.unwrap_or(pal.heading));
        let stripe = t.stripe_background.map(color);
        let mut border_c = color(t.border_color.unwrap_or(pal.muted));
        if t.border_color.is_none() {
            border_c.a = 0.4; // a quiet default rule when unthemed
        }
        let border_w = t.border_width.unwrap_or(1.0) * scale;
        let pad = scaled_padding(t.padding_sides(10.0), scale);
        let radius = t.border_radius.unwrap_or(0.0) * scale;
        let text_c = color(pal.text);
        let base = body_font(self.ctx.theme);
        let bold = Font {
            weight: iced::font::Weight::Bold,
            ..base
        };
        let mono = code_font(self.ctx.theme);
        let mark = mark_highlight(self.ctx.theme, scale);
        // Per-table `<!-- table: size=NN -->` overrides the body text size.
        let size = table
            .font_size
            .map(|s| Pixels((s * scale).round().max(1.0)))
            .unwrap_or(settings.text_size);
        let ncols = table.headers.len().max(1);

        // Column widths proportional to the widest line in the column (cheap
        // stand-in for content measurement, which iced offers no hook for).
        // `<br>` splits a cell into lines, so measure the longest one.
        let widths: Vec<u16> = (0..ncols)
            .map(|c| {
                let mut w = table.headers.get(c).map_or(1, |h| widest_line(h));
                for r in &table.rows {
                    w = w.max(r.get(c).map_or(0, |s| widest_line(s)));
                }
                w.clamp(1, 1000) as u16
            })
            .collect();

        let row_el = |cells: &[String], fg: Color, font: Font, bg: Option<Color>| {
            let cols = (0..ncols).map(|c| {
                let s = cells.get(c).map(String::as_str).unwrap_or("");
                // `<br>` in a cell becomes a hard line break (iced's text
                // honours `\n`); GFM rows can't hold real newlines.
                let s = normalize_breaks(s);
                let txt = rich_text(inline_spans(&s, font, mono, fg, mark))
                    .on_link_click(|u| u)
                    .size(size);
                let align = match table.aligns.get(c).copied().unwrap_or_default() {
                    preso_core::TableAlign::Left => Horizontal::Left,
                    preso_core::TableAlign::Center => Horizontal::Center,
                    preso_core::TableAlign::Right => Horizontal::Right,
                };
                Element::from(
                    container(txt)
                        .width(FillPortion(widths[c]))
                        .padding(pad)
                        .align_x(align),
                )
            });
            container(iced::widget::row(cols).width(Fill))
                .width(Fill)
                .style(move |_| container::Style {
                    background: bg.map(Into::into),
                    ..container::Style::default()
                })
        };

        let mut items: Vec<Element<'a, markdown::Uri>> = Vec::new();
        items.push(row_el(&table.headers, header_fg, bold, Some(header_bg)).into());
        for (i, row) in table.rows.iter().enumerate() {
            if border_w > 0.0 {
                items.push(
                    container(iced::widget::Space::new().width(Fill).height(border_w))
                        .style(move |_| container::Style {
                            background: Some(border_c.into()),
                            ..container::Style::default()
                        })
                        .into(),
                );
            }
            let bg = (i % 2 == 1).then_some(stripe).flatten();
            items.push(row_el(row, text_c, base, bg).into());
        }

        container(column(items).width(Fill))
            .width(Fill)
            .clip(true)
            .padding(padding::top(settings.spacing.0).bottom(settings.spacing.0))
            .style(move |_| container::Style {
                border: border::rounded(radius),
                ..container::Style::default()
            })
            .into()
    }

    /// Build a framed image element (GIF or raster), sized to its natural
    /// dimensions or `{width=NN%}`, capped at `max_width` (logical px).
    /// `None` if the image can't be loaded — the caller shows alt text.
    fn build_image(
        &self,
        base_url: &str,
        attrs: &ImageAttrs,
        max_width: f32,
    ) -> Option<Element<'a, markdown::Uri>> {
        let target_width = attrs
            .width_pct
            .map(|p| self.ctx.content_width() * p / 100.0);
        let (border, shadow) = attrs.framing(self.ctx.theme);

        let shapes = self.resolve_shapes(attrs);

        // Animated GIFs: pick the frame for the current animation time.
        // `clip` washes need static pixels to composite into, so they don't
        // apply to GIFs; the shapes fall back to the canvas overlay.
        if base_url.to_lowercase().ends_with(".gif")
            && let Some(gif) = self.ctx.media.gif(base_url)
        {
            let frame = gif.frame_at(self.ctx.animation_time).clone();
            let width = target_width
                .unwrap_or(gif.size.width * self.ctx.scale)
                .min(max_width);
            let height = width * gif.size.height / gif.size.width.max(1.0);
            let img = self.overlay_shapes(
                iced::widget::image(frame)
                    .width(width)
                    .height(height)
                    .into(),
                &shapes,
                false,
                width,
                height,
            );
            return Some(framed_image(
                img,
                border,
                shadow,
                self.ctx.scale,
                self.ctx.theme,
            ));
        }

        // A `clip` highlight bakes its wash into the image's opaque pixels
        // (respecting alpha), so the transparent background is untouched.
        let ops: Vec<crate::media::MaskOp> = shapes.iter().filter_map(|s| s.mask_op()).collect();
        let masked = (!ops.is_empty())
            .then(|| self.ctx.media.masked_image(base_url, target_width, &ops))
            .flatten();
        let baked = masked.is_some();
        let (handle, size) = match masked {
            Some(hs) => hs,
            None => self.ctx.media.slide_image(base_url, target_width)?,
        };

        let is_vector = base_url.to_lowercase().ends_with(".svg");
        let radius = border.map(|b| b.radius * self.ctx.scale).unwrap_or(0.0);
        let img = iced::widget::image(handle).border_radius(radius);
        let img: Element<'a, markdown::Uri> = if size == iced::Size::ZERO {
            // Dimensions unknown (unreadable header): best effort — no
            // highlight overlay either, its box needs a known height.
            match target_width {
                Some(width) => img.width(width.min(max_width)),
                None => img.width(max_width),
            }
            .into()
        } else {
            // Vector rasters are already at display size; bitmaps are
            // natural pixels mapped through the canvas scale.
            let natural = if is_vector {
                size
            } else {
                iced::Size::new(size.width * self.ctx.scale, size.height * self.ctx.scale)
            };
            let width = target_width.unwrap_or(natural.width).min(max_width);
            let height = width * natural.height / natural.width.max(1.0);
            self.overlay_shapes(
                img.width(width).height(height).into(),
                &shapes,
                baked,
                width,
                height,
            )
        };
        Some(framed_image(
            img,
            border,
            shadow,
            self.ctx.scale,
            self.ctx.theme,
        ))
    }

    /// The image's `<!-- highlight: … -->` shapes visible at the current
    /// reveal step, resolved against the theme. Empty when the image has no
    /// `hl:` fragment token or no shape is visible yet.
    fn resolve_shapes(&self, attrs: &ImageAttrs) -> Vec<ResolvedHighlight> {
        let Some(group) = attrs
            .highlight
            .and_then(|i| self.ctx.math_slide.highlights.get(i))
        else {
            return Vec::new();
        };
        group
            .iter()
            .filter(|h| h.step.is_none_or(|s| s <= self.ctx.step))
            .map(|h| ResolvedHighlight::new(h, self.ctx.theme, self.ctx.scale))
            .collect()
    }

    /// Stack the highlight canvases around the image: `mode=under` shapes
    /// below it (showing through transparent pixels), the rest above.
    /// `baked` = the image already has its `clip` washes composited in
    /// (via `Media::masked_image`), so those shapes are skipped here; when
    /// `false` (GIF, or a masking fallback) they draw on the canvas
    /// unmasked. Runs before framing, so callouts cover only the image.
    fn overlay_shapes(
        &self,
        img: Element<'a, markdown::Uri>,
        shapes: &[ResolvedHighlight],
        baked: bool,
        width: f32,
        height: f32,
    ) -> Element<'a, markdown::Uri> {
        let under: Vec<ResolvedHighlight> = shapes.iter().filter(|s| s.under).copied().collect();
        // Over-image canvas: everything not drawn below the image and not
        // already baked into its pixels (a `clip` wash when `baked`).
        let over: Vec<ResolvedHighlight> = shapes
            .iter()
            .filter(|s| !(s.under || (baked && s.clip)))
            .copied()
            .collect();
        let authoring = self.ctx.authoring;
        if under.is_empty() && over.is_empty() && !authoring {
            return img;
        }
        let layer = |shapes: Vec<ResolvedHighlight>| -> Element<'a, markdown::Uri> {
            iced::widget::canvas(HighlightOverlay {
                shapes,
                scale_factor: self.ctx.scale_factor,
            })
            .width(width)
            .height(height)
            .into()
        };
        let mut layers: Vec<Element<'a, markdown::Uri>> = Vec::with_capacity(4);
        if !under.is_empty() {
            layers.push(layer(under));
        }
        layers.push(img);
        if !over.is_empty() {
            layers.push(layer(over));
        }
        // Highlight-author mode: an interactive drag canvas on top of every
        // image, so any of them can be boxed to copy a directive.
        if authoring {
            layers.push(
                iced::widget::canvas(HighlightAuthor {
                    accent: color(self.ctx.theme.colors.accent),
                    scale_factor: self.ctx.scale_factor,
                })
                .width(width)
                .height(height)
                .into(),
            );
        }
        iced::widget::stack(layers).into()
    }

    /// Lay a [`preso_core::ImageRow`] out horizontally: each image gets an
    /// equal share of the content width, centred in its cell, with a gap
    /// between. A missing image shows its alt text, muted.
    fn render_image_row(
        &self,
        row: &'a preso_core::ImageRow,
        settings: markdown::Settings,
    ) -> Element<'a, markdown::Uri> {
        let gap = settings.spacing.0;
        let content_w = self.ctx.content_width();
        // `{fit}` on any image packs the row at the images' actual widths
        // instead of sharing the content width equally.
        let fit = row
            .images
            .iter()
            .any(|img| parse_image_fragment(&img.url).1.fit);
        // Each image sizes against the whole content width in `fit` mode, or
        // an equal share otherwise.
        let n = row.images.len().max(1) as f32;
        let cell_width = if fit {
            content_w
        } else {
            ((content_w - gap * (n - 1.0)) / n).max(1.0)
        };

        let cells = row.images.iter().map(move |img| {
            let (base_url, attrs) = parse_image_fragment(&img.url);
            let content: Element<'a, markdown::Uri> = self
                .build_image(base_url, &attrs, cell_width)
                .unwrap_or_else(|| {
                    iced::widget::text(img.alt.clone())
                        .size(settings.text_size)
                        .color(color(self.ctx.theme.colors.muted))
                        .into()
                });
            if fit {
                content // natural width, no equal cell
            } else {
                container(content)
                    .width(Fill)
                    .align_x(iced::alignment::Horizontal::Center)
                    .into()
            }
        });

        // Equal mode fills the width; fit mode shrinks to the images and
        // centres the group.
        let row_widget = iced::widget::row(cells)
            .spacing(gap)
            .align_y(iced::Alignment::Center);
        let row_widget = if fit {
            row_widget
        } else {
            row_widget.width(Fill)
        };
        container(row_widget)
            .width(Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .padding(padding::top(settings.spacing.0).bottom(settings.spacing.0))
            .into()
    }
}

/// Render a table cell's inline markdown (`` `code` `` and `**bold**`) into
/// owned rich-text spans. Everything else passes through as plain text.
/// The `==mark==` background style: the theme's `colors.mark`, else the
/// accent at ~35% alpha so it works on both dark and light themes.
fn mark_highlight(theme: &preso_style::Theme, scale: f32) -> markdown::Highlight {
    let bg = theme.colors.mark.unwrap_or(preso_style::Color {
        a: 90,
        ..theme.colors.accent
    });
    markdown::Highlight {
        background: color(bg).into(),
        border: border::rounded((4.0 * scale).round()),
    }
}

/// Restyle sentinel-tagged spans as text highlights. preso-core rewrites
/// `==marked==` into inline code prefixed with `MARK_SENTINEL` (the only
/// inline construct the markdown widget can background); here the tag is
/// stripped and the span switches back to the inherited font and color,
/// with the mark background in place of the code one. Untagged spans pass
/// through unchanged.
fn restyle_marks(
    spans: &[iced::widget::text::Span<'static, markdown::Uri>],
    mark: markdown::Highlight,
) -> Vec<iced::widget::text::Span<'static, markdown::Uri>> {
    spans
        .iter()
        .map(|span| {
            let Some(stripped) = span.text.strip_prefix(preso_core::parser::MARK_SENTINEL) else {
                return span.clone();
            };
            let mut s = span.clone();
            s.text = stripped.to_string().into();
            s.font = None; // inherit the surrounding text font, not code
            s.color = None; // inherit the surrounding text color
            s.highlight = Some(mark);
            s
        })
        .collect()
}

/// Whether any span carries the `==mark==` sentinel tag.
fn has_marks(spans: &[iced::widget::text::Span<'static, markdown::Uri>]) -> bool {
    spans
        .iter()
        .any(|s| s.text.starts_with(preso_core::parser::MARK_SENTINEL))
}

fn inline_spans(
    text: &str,
    base: Font,
    mono: Font,
    fg: Color,
    mark: markdown::Highlight,
) -> Vec<iced::widget::text::Span<'static, markdown::Uri, Font>> {
    use iced::widget::span;
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(after) = rest.strip_prefix('`')
            && let Some(end) = after.find('`')
        {
            if !plain.is_empty() {
                out.push(span(std::mem::take(&mut plain)).font(base).color(fg));
            }
            out.push(span(after[..end].to_string()).font(mono).color(fg));
            i += 1 + end + 1;
            continue;
        }
        if let Some(after) = rest.strip_prefix("**")
            && let Some(end) = after.find("**")
        {
            if !plain.is_empty() {
                out.push(span(std::mem::take(&mut plain)).font(base).color(fg));
            }
            out.push(span(after[..end].to_string()).font(bold_of(base)).color(fg));
            i += 2 + end + 2;
            continue;
        }
        // `==marked==` cell text renders with the mark background. Table
        // cells keep their raw markdown (they're lifted before the parser's
        // sentinel rewrite), so the marks are parsed here directly.
        if let Some(after) = rest.strip_prefix("==")
            && let Some(end) = after.find("==")
            && !after[..end].is_empty()
            && after[..end].trim() == &after[..end]
        {
            if !plain.is_empty() {
                out.push(span(std::mem::take(&mut plain)).font(base).color(fg));
            }
            out.push(
                span(after[..end].to_string())
                    .font(base)
                    .color(fg)
                    .background(mark.background)
                    .border(mark.border),
            );
            i += 2 + end + 2;
            continue;
        }
        let ch = rest.chars().next().expect("non-empty rest");
        plain.push(ch);
        i += ch.len_utf8();
    }
    if !plain.is_empty() {
        out.push(span(plain).font(base).color(fg));
    }
    if out.is_empty() {
        out.push(span(String::new()).font(base).color(fg));
    }
    out
}

fn bold_of(base: Font) -> Font {
    Font {
        weight: iced::font::Weight::Bold,
        ..base
    }
}

/// Replace `<br>` line-break tags (`<br>`, `<br/>`, `<br />`, any case) with
/// newlines, so a table cell can hold multiple lines — a GFM row can't carry
/// a literal newline, so `<br>` is the usual convention.
fn normalize_breaks(s: &str) -> String {
    if !s.contains(['<', '>']) {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if let Some(len) = br_tag_len(bytes, i) {
            out.push('\n');
            i += len;
        } else {
            let ch = s[i..].chars().next().expect("non-empty");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// If a `<br…>` tag starts at byte `i`, its length in bytes; else `None`.
fn br_tag_len(b: &[u8], i: usize) -> Option<usize> {
    if *b.get(i)? != b'<'
        || !b.get(i + 1)?.eq_ignore_ascii_case(&b'b')
        || !b.get(i + 2)?.eq_ignore_ascii_case(&b'r')
    {
        return None;
    }
    let mut j = i + 3;
    while matches!(b.get(j), Some(b' ' | b'\t')) {
        j += 1;
    }
    if b.get(j) == Some(&b'/') {
        j += 1;
    }
    while matches!(b.get(j), Some(b' ' | b'\t')) {
        j += 1;
    }
    (b.get(j) == Some(&b'>')).then_some(j + 1 - i)
}

/// The longest line of a table cell (after `<br>` → newline), in characters —
/// the column-width heuristic uses it so a multi-line cell isn't over-wide.
fn widest_line(s: &str) -> usize {
    normalize_breaks(s)
        .lines()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
}

impl<'a> markdown::Viewer<'a, markdown::Uri> for PresoViewer<'a> {
    fn on_link_click(uri: markdown::Uri) -> markdown::Uri {
        uri
    }

    fn heading(
        &self,
        settings: markdown::Settings,
        level: &'a markdown::HeadingLevel,
        text: &'a markdown::Text,
        index: usize,
    ) -> Element<'a, markdown::Uri> {
        use markdown::HeadingLevel;

        // v3b: per-level overrides → [heading].color → colors.heading.
        let style = &self.ctx.theme.heading;
        let level_color = match level {
            HeadingLevel::H1 => style.h1_color,
            HeadingLevel::H2 => style.h2_color,
            HeadingLevel::H3 => style.h3_color,
            _ => None,
        };
        let heading_color = color(
            level_color
                .or(style.color)
                .unwrap_or(self.ctx.theme.colors.heading),
        );
        let spans: Vec<_> = restyle_marks(
            &text.spans(settings.style),
            mark_highlight(self.ctx.theme, self.ctx.scale),
        )
        .into_iter()
        .map(|span| span.color(heading_color))
        .collect();

        container(
            rich_text(spans)
                .on_link_click(Self::on_link_click)
                .font(heading_font(self.ctx.theme))
                .size(match level {
                    HeadingLevel::H1 => settings.h1_size,
                    HeadingLevel::H2 => settings.h2_size,
                    HeadingLevel::H3 => settings.h3_size,
                    HeadingLevel::H4 => settings.h4_size,
                    HeadingLevel::H5 => settings.h5_size,
                    HeadingLevel::H6 => settings.h6_size,
                }),
        )
        .padding(padding::top(if index > 0 {
            settings.text_size.0 / 2.0
        } else {
            0.0
        }))
        .into()
    }

    fn paragraph(
        &self,
        settings: markdown::Settings,
        text: &markdown::Text,
    ) -> Element<'a, markdown::Uri> {
        // `==marked==` text arrives as sentinel-tagged inline code (see
        // preso-core's `replace_marks`); restyle those spans as highlights.
        // Lists and quotes render their items through this method too, so
        // marks work everywhere prose flows.
        let spans = text.spans(settings.style);
        if !has_marks(&spans) {
            return markdown::paragraph(settings, text, Self::on_link_click);
        }
        rich_text(restyle_marks(
            &spans,
            mark_highlight(self.ctx.theme, self.ctx.scale),
        ))
        .size(settings.text_size)
        .on_link_click(Self::on_link_click)
        .into()
    }

    fn image(
        &self,
        settings: markdown::Settings,
        url: &'a markdown::Uri,
        _title: &'a str,
        alt: &markdown::Text,
    ) -> Element<'a, markdown::Uri> {
        // Display math markers produced by preso-core.
        if let Some(index) = url.strip_prefix("preso-math:") {
            let block = index
                .parse::<usize>()
                .ok()
                .and_then(|i| self.ctx.math_slide.math_blocks.get(i));
            if let Some(block) = block {
                let font_px = self.ctx.theme.fonts.body_size * self.ctx.scale * 1.15;
                if let Some((handle, size)) =
                    self.ctx
                        .media
                        .math(&block.latex, font_px, self.ctx.theme.colors.text)
                {
                    return container(
                        iced::widget::image(handle)
                            .width(size.width)
                            .height(size.height),
                    )
                    .padding(padding::top(settings.spacing.0).bottom(settings.spacing.0))
                    .into();
                }
                // Render failure: show the LaTeX source, themed as code.
                return container(
                    rich_text([iced::widget::span(block.latex.clone())
                        .font(MONO_FONT)
                        .color(color(self.ctx.theme.colors.accent))])
                    .on_link_click(Self::on_link_click)
                    .size(settings.code_size),
                )
                .padding(settings.spacing.0)
                .into();
            }
        }

        // Table markers produced by preso-core (rendered by preso, themeable).
        if let Some(index) = url.strip_prefix("preso-table:")
            && let Some(table) = index
                .parse::<usize>()
                .ok()
                .and_then(|i| self.ctx.math_slide.tables.get(i))
        {
            return self.render_table(table, settings);
        }

        // Image-row markers produced by preso-core: a run of adjacent images
        // laid out side by side instead of stacked.
        if let Some(index) = url.strip_prefix("preso-imagerow:")
            && let Some(row) = index
                .parse::<usize>()
                .ok()
                .and_then(|i| self.ctx.math_slide.image_rows.get(i))
        {
            return self.render_image_row(row, settings);
        }

        // Regular images, resolved relative to the deck file. Attributes
        // (`{width=NN% align=… border shadow plain}`) arrive as a `#preso-img=`
        // fragment (see preso-core::parser::rewrite_image_attrs).
        let (base_url, attrs) = parse_image_fragment(url);
        if let Some(framed) = self.build_image(base_url, &attrs, self.ctx.content_width()) {
            return place_image(framed, attrs.align, settings.spacing.0);
        }

        // Missing image: show the alt text, muted.
        container(
            rich_text(
                alt.spans(settings.style)
                    .iter()
                    .map(|s| s.clone().color(color(self.ctx.theme.colors.muted)))
                    .collect::<Vec<_>>(),
            )
            .on_link_click(Self::on_link_click)
            .size(settings.text_size),
        )
        .padding(settings.spacing.0)
        .into()
    }

    fn code_block(
        &self,
        settings: markdown::Settings,
        language: Option<&'a str>,
        code: &'a str,
        _lines: &'a [markdown::Text],
    ) -> Element<'a, markdown::Uri> {
        let ordinal = self.code_ordinal.get();
        self.code_ordinal.set(ordinal + 1);

        // Mermaid fences render as diagrams, on a light card so the
        // diagram's dark strokes stay visible on dark themes. A
        // `transparent` flag drops the card (and, for Mermaid, the SVG's
        // own canvas background) so the slide shows through.
        // A `{width=NN%}` fence annotation sizes the diagram relative to
        // the content width; otherwise the intrinsic size is used.
        // On render failure this falls through to plain code rendering.
        let block = self.ctx.code_slide.code_blocks.get(ordinal);
        let target_width = block
            .and_then(preso_core::CodeBlock::width_percent)
            .map(|pct| self.ctx.content_width() * pct / 100.0);
        let transparent = block.is_some_and(preso_core::CodeBlock::transparent_background);
        let diagram = match language {
            Some("mermaid") => {
                self.ctx
                    .media
                    .mermaid(code, self.ctx.scale, target_width, transparent)
            }
            Some("dot" | "graphviz") => self.ctx.media.graphviz(code, self.ctx.scale, target_width),
            _ => None,
        };
        if let Some((handle, size)) = diagram {
            let img = iced::widget::image(handle)
                .width(size.width)
                .height(size.height);
            let framed: Element<'a, markdown::Uri> = if transparent {
                img.into()
            } else {
                container(img)
                    .padding(12.0 * self.ctx.scale)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgb8(0xf4, 0xf4, 0xf6).into()),
                        border: border::rounded(8),
                        ..container::Style::default()
                    })
                    .into()
            };
            return container(framed)
                .padding(padding::top(settings.spacing.0).bottom(settings.spacing.0))
                .into();
        }

        // Click-through highlighting: pick the stage for the current reveal
        // step (clamped to the block's last stage). A static `{2,4-6}` has a
        // single stage, so this is just its line set at every step.
        let code_block = self.ctx.code_slide.code_blocks.get(ordinal);
        let highlighted = code_block.and_then(|cb| {
            let stage = self.ctx.step.min(cb.stage_count().saturating_sub(1));
            cb.highlighted_lines_at(stage)
        });
        let highlight_bg = match self.ctx.theme.code_block.highlight_color {
            Some(c) => color(c),
            None => {
                let accent = self.ctx.theme.colors.accent;
                Color::from_rgba8(accent.r, accent.g, accent.b, 0.16)
            }
        };
        // "Focus mode": fade the non-selected lines instead of tinting the
        // selected ones. Only meaningful when some lines are selected. A
        // per-block `{dim}`/`{background}` flag overrides the theme default.
        let focus = match code_block.and_then(preso_core::CodeBlock::highlight_style) {
            Some("dim") => true,
            Some("background") => false,
            _ => matches!(
                self.ctx.theme.code_block.highlight_style,
                Some(preso_style::HighlightStyle::Dim)
            ),
        };
        let dim_opacity = self.ctx.theme.code_block.dim_opacity.unwrap_or(0.35);

        // The code panel's background, and a default foreground that
        // contrasts with it. Tokens the highlighter leaves uncoloured must
        // not fall back to the *slide's* text colour — on a dark code panel
        // (e.g. a dark `code_theme` over a light slide theme) that renders
        // invisible. Pick light-on-dark or dark-on-light from the panel's
        // luminance instead, so plain tokens always read.
        let code_bg = color(
            self.ctx
                .theme
                .code_block
                .background
                .unwrap_or(self.ctx.theme.colors.code_background),
        );
        let luminance = 0.2126 * code_bg.r + 0.7152 * code_bg.g + 0.0722 * code_bg.b;
        let default_code_color = if luminance < 0.5 {
            Color::from_rgb8(0xd4, 0xd4, 0xd4)
        } else {
            Color::from_rgb8(0x1c, 0x1c, 0x1c)
        };

        // Per-block font size (`{size=NN}`) overrides the theme's code_size,
        // scaled and rounded like the rest of the markdown sizes.
        let code_size = code_block
            .and_then(|cb| cb.font_size())
            .map(|s| Pixels((s * self.ctx.scale).round().max(1.0)))
            .unwrap_or(settings.code_size);

        // Highlight with the theme's `code_theme` (the markdown widget's
        // built-in pass hardcodes its own theme, so we re-highlight here).
        let base_font = code_font(self.ctx.theme);
        let mut stream = iced::highlighter::Stream::new(&iced::highlighter::Settings {
            theme: highlight_theme(self.ctx.theme),
            token: language.unwrap_or("txt").to_string(),
        });

        let rows: Vec<Element<'a, markdown::Uri>> = code
            .lines()
            .enumerate()
            .map(|(i, line)| {
                let is_highlighted = highlighted
                    .as_ref()
                    .is_some_and(|set| set.contains(&(i + 1)));
                // In focus mode, fade every line that isn't selected.
                let dim = focus && highlighted.is_some() && !is_highlighted;
                let mut spans: Vec<_> = stream
                    .highlight_line(line)
                    .map(|(range, highlight)| {
                        // Keep the token weight, but our code family.
                        let font = Font {
                            weight: highlight.font().map(|f| f.weight).unwrap_or_default(),
                            ..base_font
                        };
                        let span = iced::widget::span(line[range].to_string()).font(font);
                        if dim {
                            let mut c = highlight.color().unwrap_or(default_code_color);
                            c.a *= dim_opacity;
                            span.color(c)
                        } else {
                            span.color(highlight.color().unwrap_or(default_code_color))
                        }
                    })
                    .collect();
                stream.commit();
                if spans.is_empty() {
                    // Preserve blank lines' height.
                    spans.push(iced::widget::span(" ".to_string()).font(base_font));
                }

                let row = rich_text(spans)
                    .on_link_click(Self::on_link_click)
                    .size(code_size);
                // Background tint only emphasises selected lines outside
                // focus mode; focus mode dims the rest instead.
                if is_highlighted && !focus {
                    container(row)
                        .width(Fill)
                        .style(move |_| container::Style {
                            background: Some(highlight_bg.into()),
                            ..container::Style::default()
                        })
                        .into()
                } else {
                    container(row).width(Fill).into()
                }
            })
            .collect();

        // v3b: [code_block] overrides with v2 fallbacks. `code_bg` was
        // resolved above (it also drives the default token colour).
        let style = &self.ctx.theme.code_block;
        let radius = style.border_radius.unwrap_or(8.0) * self.ctx.scale;
        // Default padding is the code font size (design units); per-side
        // `[code_block] padding_*` override the uniform `padding`.
        let pad = scaled_padding(
            style.padding_sides(self.ctx.theme.fonts.code_size),
            self.ctx.scale,
        );
        // Panel width: `{width=NN%}` sizes it to that fraction of the content
        // width (`width=100%` = full width); otherwise hug the content. iced
        // gives no measure-before-layout hook, so the content estimate uses
        // the longest line and the monospace advance (~0.6em; a little slack
        // avoids wrapping), capped at the content width. The rows stay `Fill`
        // so a highlighted line's tint still spans the whole panel.
        let content_w = self.ctx.content_width();
        let panel_w = match code_block.and_then(preso_core::CodeBlock::width_percent) {
            Some(pct) => content_w * pct / 100.0,
            None => {
                let longest = code.lines().map(|l| l.chars().count()).max().unwrap_or(0);
                let text_w = (longest as f32 + 0.5) * code_size.0 * MONO_ADVANCE;
                (text_w + pad.left + pad.right).min(content_w)
            }
        };
        let panel = container(column(rows))
            .width(panel_w)
            .padding(pad)
            .style(move |_| container::Style {
                background: Some(code_bg.into()),
                border: border::rounded(radius),
                ..container::Style::default()
            });
        // `{align=center|right}` centres/right-aligns the panel within the
        // content width; the default (left) keeps it where it sits.
        use iced::alignment::Horizontal;
        match code_block.and_then(preso_core::CodeBlock::align) {
            Some("center") => container(panel)
                .width(Fill)
                .align_x(Horizontal::Center)
                .into(),
            Some("right") => container(panel)
                .width(Fill)
                .align_x(Horizontal::Right)
                .into(),
            _ => panel.into(),
        }
    }

    /// Blockquote (`> …`) as a themed callout: a leading accent bar, optional
    /// fill, padding, and placement, from the `[quote]` theme section.
    fn quote(
        &self,
        settings: markdown::Settings,
        contents: &'a [markdown::Item],
    ) -> Element<'a, markdown::Uri> {
        use iced::alignment::Horizontal;

        let q = &self.ctx.theme.quote;
        let scale = self.ctx.scale;
        let bar_color = color(q.border_color.unwrap_or(self.ctx.theme.colors.accent));
        let bar_w = q.border_width.unwrap_or(4.0) * scale;
        let pad = scaled_padding(q.padding_sides(16.0), scale);
        let radius = q.border_radius.unwrap_or(0.0) * scale;
        let bg = q.background.map(color);
        let align = match q.align.unwrap_or(preso_style::HorizontalAlign::Left) {
            preso_style::HorizontalAlign::Left => Horizontal::Left,
            preso_style::HorizontalAlign::Center => Horizontal::Center,
            preso_style::HorizontalAlign::Right => Horizontal::Right,
        };

        // Inner markdown, italicised when the theme asks for it.
        let mut inner = settings;
        if q.italic.unwrap_or(false) {
            inner.style.font = Font {
                style: iced::font::Style::Italic,
                ..inner.style.font
            };
        }
        let body = container(markdown::view_with(contents, inner, self)).padding(pad);
        // A vertical rule (not a Fill container) so the bar matches the
        // quote's height instead of stretching to the whole slide.
        let bar = iced::widget::rule::vertical(bar_w).style(move |_| iced::widget::rule::Style {
            color: bar_color,
            radius: 0.0.into(),
            fill_mode: iced::widget::rule::FillMode::Full,
            snap: true,
        });
        let callout =
            container(iced::widget::row([bar.into(), body.into()]).height(iced::Length::Shrink))
                .style(move |_| container::Style {
                    background: bg.map(Into::into),
                    border: border::rounded(radius),
                    ..container::Style::default()
                });

        container(callout)
            .width(Fill)
            .align_x(align)
            .padding(padding::top(settings.spacing.0).bottom(settings.spacing.0))
            .into()
    }
}

fn slide_view<'a>(
    content: &'a markdown::Content,
    ctx: SlideContext<'a>,
) -> Element<'a, markdown::Uri> {
    use preso_style::HorizontalAlign as H;
    let settings = markdown_settings(ctx.theme, ctx.scale);
    let halign = ctx.halign;
    let viewer = PresoViewer {
        ctx,
        code_ordinal: Cell::new(0),
    };
    // Default (left): iced's built-in column, untouched.
    let h = match halign {
        H::Left => return markdown::view_with(content.items(), settings, &viewer),
        H::Center => iced::alignment::Horizontal::Center,
        H::Right => iced::alignment::Horizontal::Right,
    };
    // Centre/right: wrap each top-level block in a full-width container and
    // align it. The markdown widget's own column is `Shrink`, so alignment
    // only has room once the column and each block fill the slide width.
    // The shared `viewer` keeps its document-order code-block counter across
    // items (and across the recursive `view_with` calls inside lists).
    column(content.items().iter().enumerate().map(|(i, item)| {
        container(markdown::item(&viewer, settings, item, i))
            .width(Fill)
            .align_x(h)
            .into()
    }))
    .spacing(settings.spacing)
    .width(Fill)
    .into()
}

/// Image attributes decoded from a `#preso-img=` URL fragment.
#[derive(Debug, Default, PartialEq)]
struct ImageAttrs {
    width_pct: Option<f32>,
    /// Per-image horizontal alignment; `None` leaves it left (the default).
    align: Option<preso_style::HorizontalAlign>,
    border: bool,
    shadow: bool,
    plain: bool,
    /// In an image row: take the image's actual width instead of an equal
    /// share of the row (any flagged image switches the whole row).
    fit: bool,
    /// Index of the image's highlight group in `Slide::highlights`, from
    /// the `hl:N` token the parser appends for `<!-- highlight: … -->`.
    highlight: Option<usize>,
}

impl ImageAttrs {
    /// Effective framing: theme `[image]` defaults, with `border`/`shadow`
    /// flags forcing one on and `plain` stripping both.
    fn framing(
        &self,
        theme: &preso_style::Theme,
    ) -> (
        Option<preso_style::ImageBorder>,
        Option<preso_style::ImageShadow>,
    ) {
        if self.plain {
            return (None, None);
        }
        let border = if self.border {
            Some(theme.image.border.unwrap_or_default())
        } else {
            theme.image.border
        };
        let shadow = if self.shadow {
            theme
                .image
                .shadow
                .and_then(|s| s.resolve())
                .or(Some(preso_style::ImageShadow::default()))
        } else {
            theme.image.shadow.and_then(|s| s.resolve())
        };
        (border, shadow)
    }
}

/// One image callout resolved for drawing: shape, fractional region, and
/// final colors.
#[derive(Clone, Copy)]
struct ResolvedHighlight {
    ellipse: bool,
    /// `mode=spotlight`: this region is a hole in a scrim dimming the rest
    /// of the image, instead of a fill over the region.
    spotlight: bool,
    /// `mode=under`: drawn on the canvas layered below the image (the
    /// split into layers happens in `PresoViewer::overlay_shapes`).
    under: bool,
    /// `clip`: bake the wash into the image's opaque pixels (via
    /// `Media::masked_image`) rather than drawing it as a canvas rectangle,
    /// so a transparent image's background stays untouched. Fill/spotlight
    /// only; ignored for `under`.
    clip: bool,
    /// Top-left corner and size as fractions of the image.
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fill: Color,
    /// Outline color and width in logical px; `None` = no outline.
    stroke: Option<(Color, f32)>,
}

impl ResolvedHighlight {
    fn new(h: &preso_core::Highlight, theme: &preso_style::Theme, scale: f32) -> Self {
        let palette = &theme.colors;
        // `color=` takes a palette name or `#hex`; anything unparseable
        // falls back to the accent rather than dropping the callout.
        use preso_core::HighlightMode as Mode;
        let base = match h.color.as_deref() {
            // An uncolored spotlight dims with black — the accent (often a
            // light tint) would brighten the surroundings instead.
            None if h.mode == Mode::Spotlight => preso_style::Color::rgb(0, 0, 0),
            None | Some("accent") => palette.accent,
            Some("text") => palette.text,
            Some("heading") => palette.heading,
            Some("link") => palette.link,
            Some("muted") => palette.muted,
            Some(other) => preso_style::Color::parse(other).unwrap_or(palette.accent),
        };
        let base = color(base);
        Self {
            ellipse: matches!(h.shape, preso_core::HighlightShape::Ellipse),
            spotlight: h.mode == Mode::Spotlight,
            under: h.mode == Mode::Under,
            // `clip` only masks fill/spotlight washes; `under` draws its own
            // way (behind the image), so ignore clip there.
            clip: h.clip && h.mode != Mode::Under,
            x: h.x,
            y: h.y,
            w: h.w,
            h: h.h,
            fill: Color {
                a: base.a * h.opacity,
                ..base
            },
            stroke: (h.stroke > 0.0).then(|| (base, (h.stroke * scale).max(1.0))),
        }
    }

    /// This shape as a pixel-composite op for [`Media::masked_image`], or
    /// `None` if it isn't a clipped wash. `clip` washes bake into the image;
    /// they carry no outline (a rect/ellipse outline wouldn't follow the
    /// image's silhouette anyway).
    fn mask_op(&self) -> Option<crate::media::MaskOp> {
        if !self.clip || self.fill.a <= 0.0 {
            return None;
        }
        let to_u8 = |c: f32| (c * 255.0).round() as u8;
        Some(crate::media::MaskOp {
            ellipse: self.ellipse,
            spotlight: self.spotlight,
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
            color: (to_u8(self.fill.r), to_u8(self.fill.g), to_u8(self.fill.b)),
            alpha: self.fill.a,
        })
    }

    /// Trace the region outline into a path builder, in canvas coordinates
    /// (`origin` is the compensated frame offset, `size` the canvas size).
    fn trace(
        &self,
        b: &mut iced::widget::canvas::path::Builder,
        origin: iced::Point,
        size: iced::Size,
    ) {
        let top_left = iced::Point::new(
            origin.x + self.x * size.width,
            origin.y + self.y * size.height,
        );
        let region = iced::Size::new(self.w * size.width, self.h * size.height);
        if self.ellipse {
            b.ellipse(iced::widget::canvas::path::arc::Elliptical {
                center: iced::Point::new(
                    top_left.x + region.width / 2.0,
                    top_left.y + region.height / 2.0,
                ),
                radii: iced::Vector::new(region.width / 2.0, region.height / 2.0),
                rotation: iced::Radians(0.0),
                start_angle: iced::Radians(0.0),
                end_angle: iced::Radians(std::f32::consts::TAU),
            });
        } else {
            b.rectangle(top_left, region);
        }
    }
}

/// Canvas program painting a highlight group over its image. The canvas
/// fills the image's stack, so fractional coordinates just scale by the
/// widget bounds.
struct HighlightOverlay {
    shapes: Vec<ResolvedHighlight>,
    /// Window DPI factor, for `overlay::compensated_frame`.
    scale_factor: f32,
}

impl<M> iced::widget::canvas::Program<M> for HighlightOverlay {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas;
        let (mut frame, origin) =
            crate::overlay::compensated_frame(renderer, bounds, self.scale_factor);
        let size = bounds.size();

        // Spotlight scrim: one even-odd fill covering the whole image with
        // every visible spotlight region punched out, so those regions stay
        // at full fidelity while the rest dims. The first spotlight shape's
        // color/opacity styles the scrim.
        let spots: Vec<&ResolvedHighlight> = self.shapes.iter().filter(|s| s.spotlight).collect();
        if let Some(first) = spots.first().filter(|s| s.fill.a > 0.0) {
            let scrim = canvas::Path::new(|b| {
                b.rectangle(origin, size);
                for s in &spots {
                    s.trace(b, origin, size);
                }
            });
            frame.fill(
                &scrim,
                canvas::Fill {
                    style: canvas::Style::Solid(first.fill),
                    rule: canvas::fill::Rule::EvenOdd,
                },
            );
        }

        for s in &self.shapes {
            let path = canvas::Path::new(|b| s.trace(b, origin, size));
            if !s.spotlight && s.fill.a > 0.0 {
                frame.fill(&path, s.fill);
            }
            if let Some((stroke_color, width)) = s.stroke {
                frame.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_color(stroke_color)
                        .with_width(width),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

/// Highlight-author overlay (`H` mode): an interactive canvas the size of
/// the image. Dragging a box publishes a `preso-hl-draw:x,y,w,h` URI —
/// percentages of the image — which rides the slide's normal link path to
/// [`crate::app::Message::LinkClicked`], where it becomes a
/// `<!-- highlight: rect … -->` directive on the clipboard. Because its
/// bounds equal the image's, the drag is already image-relative.
struct HighlightAuthor {
    accent: iced::Color,
    scale_factor: f32,
}

impl HighlightAuthor {
    /// Cursor → image fraction (0..1 each axis), clamped so a drag that
    /// leaves the image sticks to its edge.
    fn frac(bounds: iced::Rectangle, cursor: iced::mouse::Cursor) -> Option<iced::Point> {
        let p = cursor.position()?;
        Some(iced::Point::new(
            ((p.x - bounds.x) / bounds.width.max(1.0)).clamp(0.0, 1.0),
            ((p.y - bounds.y) / bounds.height.max(1.0)).clamp(0.0, 1.0),
        ))
    }
}

impl iced::widget::canvas::Program<markdown::Uri> for HighlightAuthor {
    /// The in-progress drag as `(start, current)` image fractions, or `None`
    /// when idle.
    type State = Option<(iced::Point, iced::Point)>;

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::widget::canvas::Event,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<iced::widget::canvas::Action<markdown::Uri>> {
        use iced::mouse::{Button, Event as Mouse};
        use iced::widget::canvas::Action;
        match event {
            // Only a press *inside this image* starts a drag, so sibling
            // image canvases don't all react to one press.
            iced::Event::Mouse(Mouse::ButtonPressed(Button::Left)) => {
                let start = cursor.position_in(bounds).map(|p| {
                    iced::Point::new(
                        (p.x / bounds.width.max(1.0)).clamp(0.0, 1.0),
                        (p.y / bounds.height.max(1.0)).clamp(0.0, 1.0),
                    )
                })?;
                *state = Some((start, start));
                Some(Action::request_redraw().and_capture())
            }
            iced::Event::Mouse(Mouse::CursorMoved { .. }) => {
                let (start, _) = (*state)?;
                let current = Self::frac(bounds, cursor).unwrap_or(start);
                *state = Some((start, current));
                Some(Action::request_redraw().and_capture())
            }
            iced::Event::Mouse(Mouse::ButtonReleased(Button::Left)) => {
                let (start, _) = (*state)?;
                let end = Self::frac(bounds, cursor).unwrap_or(start);
                *state = None;
                let (uri, redraw_only) = author_uri(start, end);
                match uri {
                    Some(uri) if !redraw_only => Some(Action::publish(uri).and_capture()),
                    // A click or a hair-thin slip: nothing to copy, just clear.
                    _ => Some(Action::request_redraw().and_capture()),
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas;
        let (mut frame, origin) =
            crate::overlay::compensated_frame(renderer, bounds, self.scale_factor);
        if let Some((a, b)) = state {
            let x = a.x.min(b.x) * bounds.width;
            let y = a.y.min(b.y) * bounds.height;
            let w = (a.x - b.x).abs() * bounds.width;
            let h = (a.y - b.y).abs() * bounds.height;
            let rect = canvas::Path::rectangle(
                iced::Point::new(origin.x + x, origin.y + y),
                iced::Size::new(w, h),
            );
            frame.fill(
                &rect,
                iced::Color {
                    a: 0.22,
                    ..self.accent
                },
            );
            frame.stroke(
                &rect,
                canvas::Stroke::default()
                    .with_color(self.accent)
                    .with_width(2.0),
            );
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        iced::mouse::Interaction::Crosshair
    }
}

/// Two image-fraction corners → a `preso-hl-draw:` URI. Returns
/// `(uri, redraw_only)`: `redraw_only` is `true` for a drag too small to be
/// a deliberate box (a click), so the caller just clears the rubber band.
/// Coordinates are emitted as whole percentages of the image.
fn author_uri(start: iced::Point, end: iced::Point) -> (Option<markdown::Uri>, bool) {
    let x = start.x.min(end.x);
    let y = start.y.min(end.y);
    let w = (start.x - end.x).abs();
    let h = (start.y - end.y).abs();
    if w < 0.01 || h < 0.01 {
        return (None, true);
    }
    let pct = |f: f32| (f * 100.0).round() as i32;
    (
        Some(format!(
            "preso-hl-draw:{},{},{},{}",
            pct(x),
            pct(y),
            pct(w),
            pct(h)
        )),
        false,
    )
}

/// Parse a `preso-hl-draw:x,y,w,h` URI (whole percentages) into the
/// `<!-- highlight: rect … -->` directive it should produce, or `None` if
/// it isn't one. Shared by the click handler and its test.
pub fn author_directive(uri: &str) -> Option<String> {
    let body = uri.strip_prefix("preso-hl-draw:")?;
    let mut it = body.split(',').map(|n| n.trim().parse::<i32>().ok());
    let (x, y, w, h) = (it.next()??, it.next()??, it.next()??, it.next()??);
    if it.next().is_some() {
        return None;
    }
    Some(format!(
        "<!-- highlight: rect x={x}% y={y}% w={w}% h={h}% -->"
    ))
}

/// Split a markdown image URL into the real path and its decoded attrs
/// (`x.png#preso-img=width:30+border` → `x.png` + width/border).
fn parse_image_fragment(url: &str) -> (&str, ImageAttrs) {
    let Some((base, encoded)) = url.split_once("#preso-img=") else {
        return (url, ImageAttrs::default());
    };
    let mut attrs = ImageAttrs::default();
    for token in encoded.split('+') {
        match token {
            "border" => attrs.border = true,
            "shadow" => attrs.shadow = true,
            "plain" => attrs.plain = true,
            "fit" => attrs.fit = true,
            _ => {
                if let Some(pct) = token.strip_prefix("width:") {
                    attrs.width_pct = pct.parse().ok();
                } else if let Some(index) = token.strip_prefix("hl:") {
                    attrs.highlight = index.parse().ok();
                } else if let Some(a) = token.strip_prefix("align:") {
                    use preso_style::HorizontalAlign as H;
                    attrs.align = match a {
                        "left" => Some(H::Left),
                        "center" => Some(H::Center),
                        "right" => Some(H::Right),
                        _ => None,
                    };
                }
            }
        }
    }
    (base, attrs)
}

/// Place a (framed) image into its content block: vertical spacing above and
/// below, plus optional horizontal alignment. `align` of `None`/`Left` keeps
/// the image at its natural left position; `Center`/`Right` make the block
/// fill the content width so the image can shift within it.
fn place_image<'a>(
    content: impl Into<Element<'a, markdown::Uri>>,
    align: Option<preso_style::HorizontalAlign>,
    spacing: f32,
) -> Element<'a, markdown::Uri> {
    use preso_style::HorizontalAlign as H;
    let mut block = container(content).padding(padding::top(spacing).bottom(spacing));
    block = match align {
        Some(H::Center) => block
            .width(Fill)
            .align_x(iced::alignment::Horizontal::Center),
        Some(H::Right) => block
            .width(Fill)
            .align_x(iced::alignment::Horizontal::Right),
        _ => block,
    };
    block.into()
}

/// Wrap an image in its border and/or drop shadow (no-op without either).
/// Sizes are design units mapped through `scale`.
fn framed_image<'a, M: 'a>(
    img: impl Into<Element<'a, M>>,
    border: Option<preso_style::ImageBorder>,
    shadow: Option<preso_style::ImageShadow>,
    scale: f32,
    theme: &preso_style::Theme,
) -> Element<'a, M> {
    if border.is_none() && shadow.is_none() {
        return img.into();
    }
    let border_width = border.map(|b| (b.width * scale).max(1.0)).unwrap_or(0.0);
    let iced_border = match border {
        Some(b) => iced::Border {
            color: color(b.color.unwrap_or(theme.colors.muted)),
            width: border_width,
            radius: ((b.radius + b.width) * scale).into(),
        },
        None => iced::Border::default(),
    };
    let iced_shadow = match shadow {
        Some(s) => iced::Shadow {
            color: color(s.color),
            offset: iced::Vector::new(s.offset[0] * scale, s.offset[1] * scale),
            blur_radius: s.blur * scale,
        },
        None => iced::Shadow::default(),
    };
    container(img)
        // Keep the border ring outside the image instead of over it.
        .padding(border_width)
        .style(move |_| container::Style {
            border: iced_border,
            shadow: iced_shadow,
            ..container::Style::default()
        })
        .into()
}

/// Per-call options for [`slide_surface`].
pub struct SurfaceOptions {
    /// Design-space → logical-pixel scale.
    pub scale: f32,
    /// Canvas size in logical pixels.
    pub size: iced::Size,
    /// (current, total), 1-based, for the theme's `[slide_number]` stamp.
    pub number: Option<(usize, usize)>,
    /// Slide footnote text (`<!-- footnote: … -->`), styled by `[footnote]`
    /// and drawn along the bottom edge. `None` = no footnote on this slide.
    pub footnote: Option<String>,
    /// Decoration images (`<!-- image: … -->`) drawn below the content.
    pub layer_images: Vec<preso_core::LayerImage>,
    /// The slide has a `<!-- video: … -->` clip: draw a centered play/pause
    /// badge as the affordance. Playback is inline under the `video` feature
    /// (wgpu) or an external player otherwise.
    pub video: bool,
    /// The clip is currently playing: the badge shows ⏸ (running) instead of
    /// ▶ (paused/idle). Drives the presenter's video feedback — the audience
    /// hides the badge entirely while the clip plays over the slide.
    pub video_playing: bool,
    /// Dither gradient backgrounds (screen). PDF export turns this off:
    /// dither noise defeats JPEG compression (~14x larger pages), and
    /// iced's plain gradient is what exports always shipped.
    pub dither: bool,
}

/// The full slide visual: themed background (solid or gradient), optional
/// accent bar and logo watermark, then the content with the theme's
/// alignment and padding. Shared by the audience window and PDF export.
pub fn slide_surface<'a>(
    body: Element<'a, Message>,
    media: &'a Media,
    theme: &'a preso_style::Theme,
    overrides: &preso_core::SlideOverrides,
    options: SurfaceOptions,
) -> Element<'a, Message> {
    use iced::widget::stack;

    let SurfaceOptions {
        scale,
        size,
        number,
        footnote,
        layer_images,
        video,
        video_playing,
        dither,
    } = options;
    let (width, height) = (size.width, size.height);

    let style = &theme.slide;

    // Effective accent bars: the single `bar` plus any extra `bars`, minus
    // hidden ones. Drives both content reservation and drawing.
    let bars: Vec<&preso_style::AccentBar> = style
        .bar
        .iter()
        .chain(style.bars.iter())
        .filter(|b| !b.hidden)
        .collect();

    // 1. Background (content renders on top, so a background image doubles
    //    as a "text over photo" slide).
    let mut layers: Vec<Element<'a, Message>> =
        vec![background_layer(media, theme, overrides, size, dither)];

    // Space carved out by `reserve` bars: content and positioned layer images
    // both keep clear of it (chrome — bars, logo, number — may still overlap).
    let mut reserved = iced::Padding::from(0.0);
    for b in bars.iter().filter(|b| b.reserve) {
        let t = b.size * scale;
        match b.side {
            preso_style::Side::Top => reserved.top += t,
            preso_style::Side::Bottom => reserved.bottom += t,
            preso_style::Side::Left => reserved.left += t,
            preso_style::Side::Right => reserved.right += t,
        }
    }

    // 1.5. Decoration images (`<!-- image: … -->`): on a layer above the
    //      background but below the content, so overlapping text stays on
    //      top. They sit inside any reserved-bar area, so `position=right`
    //      clears a reserved right bar.
    layers.extend(
        layer_images
            .iter()
            .filter_map(|li| layer_image_element(media, li, reserved, width, scale)),
    );

    // 2. The slide content itself. Per-slide `align=` wins over the theme.
    //    Content sits directly above the background so the transition veil
    //    (added next) covers only the things that change between slides;
    //    the accent bar, logo, and slide number are layered *afterwards*,
    //    above the veil, so persistent chrome doesn't flash on each change.
    // Uniform slide padding plus the reserved-bar insets computed above.
    let sp = theme.spacing.slide_padding * scale;
    let pad = iced::Padding {
        top: reserved.top + sp,
        right: reserved.right + sp,
        bottom: reserved.bottom + sp,
        left: reserved.left + sp,
    };
    let content = container(body).padding(pad).width(Fill).height(Fill);
    let align = match overrides.align.as_deref() {
        Some("center") => preso_style::VerticalAlign::Center,
        Some("top") => preso_style::VerticalAlign::Top,
        _ => style.align,
    };
    let content = match align {
        preso_style::VerticalAlign::Top => content,
        preso_style::VerticalAlign::Center => content.align_y(iced::alignment::Vertical::Center),
    };
    layers.push(content.into());

    // 2.5. Video affordance: a centered play/pause badge for
    //      `<!-- video: … -->` slides. It sits above the content, so it
    //      dissolves with the slide.
    if video {
        layers.push(video_badge_layer(scale, video_playing));
    }

    // 3. (Slide transitions are no longer a veil inside the surface; the
    //    audience window overlays the captured outgoing frame instead — see
    //    `transition.rs` and `audience.rs`.)

    // 4. Accent bars along their edges (`hidden` already filtered out).
    layers.extend(bars.iter().map(|bar| bar_layer(bar, scale)));

    // 5. Logo watermark in a corner (`hidden` lets a kind overlay drop it).
    if let Some(logo) = style
        .logo
        .as_ref()
        .filter(|l| !l.hidden && !l.path.is_empty())
        && let Some(layer) = logo_layer(media, logo, theme, width, scale)
    {
        layers.push(layer);
    }

    // 6. Slide number stamp, on top so content can't cover it
    //    (`hidden` lets a kind overlay drop it).
    if let (Some(number_style), Some(counts)) =
        (theme.slide_number.as_ref().filter(|n| !n.hidden), number)
    {
        layers.push(number_layer(number_style, theme, counts, scale));
    }

    // 7. Footnote (`<!-- footnote: … -->`), styled by `[footnote]`.
    //    `hidden` (via a kind overlay) drops it.
    if let Some(text_str) = footnote.filter(|s| !s.trim().is_empty())
        && !theme.footnote.hidden
    {
        layers.push(footnote_layer(theme, text_str, scale));
    }

    container(stack(layers)).width(width).height(height).into()
}

/// Layer 1: the slide background. Precedence: per-slide `background=`
/// override (a `#hex` color, else an image path) → theme `background_image`
/// → theme gradient → flat theme color.
fn background_layer<'a>(
    media: &Media,
    theme: &preso_style::Theme,
    overrides: &preso_core::SlideOverrides,
    size: iced::Size,
    dither: bool,
) -> Element<'a, Message> {
    use iced::widget::space;

    let style = &theme.slide;
    let solid = |bg: iced::Background| -> Element<'a, Message> {
        container(space())
            .width(Fill)
            .height(Fill)
            .style(move |_| container::Style {
                background: Some(bg),
                ..container::Style::default()
            })
            .into()
    };
    let cover = |handle: iced::widget::image::Handle| -> Element<'a, Message> {
        iced::widget::image(handle)
            .width(Fill)
            .height(Fill)
            .content_fit(iced::ContentFit::Cover)
            .into()
    };

    let override_str = overrides.background.as_deref();
    let override_color = override_str.and_then(preso_style::Color::parse);
    // A non-color override value is treated as an image path (deck-relative).
    let override_image = match (override_str, override_color) {
        (Some(s), None) if !s.is_empty() => Some(s),
        _ => None,
    };
    let bg_image = override_image.or(style.background_image.as_deref());

    if let Some(c) = override_color {
        solid(color(c).into())
    } else if let Some(path) = bg_image {
        // Cover-fit over the whole canvas; fall back to the flat theme
        // color if the file can't load (missing/unsupported).
        match media.slide_image(path, Some(size.width)) {
            Some((handle, _)) => cover(handle),
            None => solid(color(theme.colors.background).into()),
        }
    } else if let Some(g) = style.gradient {
        // On screen, render the gradient pre-dithered (Media::gradient):
        // iced's own gradients quantize straight to 8-bit, which bands on
        // slow dark ramps. Export keeps iced's gradient (dither defeats
        // JPEG compression).
        match dither.then(|| media.gradient(g, size)) {
            Some(Some((handle, _))) => iced::widget::image(handle)
                .width(Fill)
                .height(Fill)
                .content_fit(iced::ContentFit::Fill)
                .into(),
            _ => solid(
                iced::Gradient::Linear(
                    iced::gradient::Linear::new(iced::Radians(g.angle.to_radians()))
                        .add_stop(0.0, color(g.from))
                        .add_stop(1.0, color(g.to)),
                )
                .into(),
            ),
        }
    } else {
        solid(color(theme.colors.background).into())
    }
}

/// Layer 1.5: one decoration image (`<!-- image: … -->`), positioned and
/// sized like the logo but authored per slide. `None` if its file can't
/// load. `reserved` insets it clear of any `reserve` accent bars.
fn layer_image_element<'a>(
    media: &Media,
    li: &preso_core::LayerImage,
    reserved: iced::Padding,
    canvas_width: f32,
    scale: f32,
) -> Option<Element<'a, Message>> {
    let target = li.width.map(|w| canvas_width * w / 100.0);
    let (handle, size) = media.slide_image(&li.path, target)?;
    let w = target
        .unwrap_or(size.width * scale)
        .min(canvas_width)
        .max(1.0);
    let h = if size.width > 0.0 {
        w * size.height / size.width.max(1.0)
    } else {
        w
    };
    let (hx, vy) = anchor_alignment(li.position);
    let positioned = container(
        iced::widget::image(handle)
            .width(w)
            .height(h)
            .opacity(li.opacity),
    )
    .width(Fill)
    .height(Fill)
    .padding(iced::Padding {
        top: reserved.top + li.padding[0] * scale,
        right: reserved.right + li.padding[1] * scale,
        bottom: reserved.bottom + li.padding[2] * scale,
        left: reserved.left + li.padding[3] * scale,
    })
    .align_x(hx)
    .align_y(vy);
    Some(positioned.into())
}

/// Layer 2.5: the centered play/pause badge for `<!-- video: … -->` slides —
/// ⏸ while the clip runs, ▶ when paused or idle. Playback itself is handled
/// by the `video` module (inline under wgpu with the `video` feature, else an
/// external player).
fn video_badge_layer<'a>(scale: f32, playing: bool) -> Element<'a, Message> {
    use iced::alignment::{Horizontal, Vertical};

    let diameter = 130.0 * scale;
    // Both glyphs are solid Geometric-Shapes characters (not the emoji-styled
    // U+23F8 ⏸, which renders in a box on macOS), so pause matches play's
    // weight: two black vertical bars vs. the black triangle.
    let glyph = if playing { "▮▮" } else { "▶" };
    let badge = container(
        iced::widget::text(glyph)
            .size(56.0 * scale)
            .color(iced::Color::WHITE),
    )
    .width(diameter)
    .height(diameter)
    .align_x(Horizontal::Center)
    .align_y(Vertical::Center)
    .style(move |_| container::Style {
        background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()),
        border: iced::Border {
            radius: (diameter / 2.0).into(),
            ..iced::Border::default()
        },
        ..container::Style::default()
    });
    container(badge)
        .width(Fill)
        .height(Fill)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .into()
}

/// Layer 4: one accent bar along its edge.
fn bar_layer<'a>(bar: &preso_style::AccentBar, scale: f32) -> Element<'a, Message> {
    use iced::alignment::{Horizontal, Vertical};
    use iced::widget::space;
    use preso_style::Side;

    let thickness = bar.size * scale;
    let bar_color = color(bar.color);
    let rect = container(space()).style(move |_| container::Style {
        background: Some(bar_color.into()),
        ..container::Style::default()
    });
    let rect = match bar.side {
        Side::Top | Side::Bottom => rect.width(Fill).height(thickness),
        Side::Left | Side::Right => rect.width(thickness).height(Fill),
    };
    let positioned = container(rect).width(Fill).height(Fill);
    match bar.side {
        Side::Top => positioned.align_y(Vertical::Top),
        Side::Bottom => positioned.align_y(Vertical::Bottom),
        Side::Left => positioned.align_x(Horizontal::Left),
        Side::Right => positioned.align_x(Horizontal::Right),
    }
    .into()
}

/// Layer 5: the corner logo watermark. `None` if its image can't load.
fn logo_layer<'a>(
    media: &Media,
    logo: &preso_style::Logo,
    theme: &preso_style::Theme,
    canvas_width: f32,
    scale: f32,
) -> Option<Element<'a, Message>> {
    use iced::widget::image;

    let target = canvas_width * logo.width / 100.0;
    let (handle, size) = media.slide_image(&logo.path, Some(target))?;
    let radius = logo.border.map(|b| b.radius * scale).unwrap_or(0.0);
    // Always honor the requested width; derive height from the
    // image's aspect ratio when known.
    let img = if size == iced::Size::ZERO {
        image(handle)
            .width(target)
            .opacity(logo.opacity)
            .border_radius(radius)
    } else {
        image(handle)
            .width(target)
            .height(target * size.height / size.width.max(1.0))
            .opacity(logo.opacity)
            .border_radius(radius)
    };
    let img: Element<'a, Message> = framed_image(
        img,
        logo.border,
        logo.shadow.and_then(|s| s.resolve()),
        scale,
        theme,
    );
    let pad = corner_padding(
        logo.position,
        logo.padding_x * scale,
        logo.padding_y * scale,
    );
    Some(align_corner(container(img).padding(pad), logo.position))
}

/// Layer 6: the slide-number stamp in its corner.
fn number_layer<'a>(
    style: &preso_style::SlideNumber,
    theme: &preso_style::Theme,
    (current, total): (usize, usize),
    scale: f32,
) -> Element<'a, Message> {
    use iced::widget::text;

    let label = style
        .format
        .replace("{current}", &current.to_string())
        .replace("{total}", &total.to_string());
    let stamp = text(label)
        .size(style.size * scale)
        .font(
            style
                .font
                .as_deref()
                .map(font_named)
                .unwrap_or_else(|| body_font(theme)),
        )
        .color(color(style.color.unwrap_or(theme.colors.muted)));
    // Anchor the number's *center* at (padding_x, padding_y) from the
    // corner, not its edge: a box of twice the padding, anchored to the
    // corner, has its centre exactly there, and the number is centred
    // within it. So the position holds as the digit count changes
    // ("9" → "10") instead of one edge staying put and the rest drifting.
    let px = style.padding_x * scale;
    let py = style.padding_y * scale;
    let stamp = container(stamp).center_x(px * 2.0).center_y(py * 2.0);
    align_corner(stamp, style.position)
}

/// Layer 7: the footnote — a small disclaimer line along the bottom edge.
fn footnote_layer<'a>(
    theme: &preso_style::Theme,
    text_str: String,
    scale: f32,
) -> Element<'a, Message> {
    use iced::alignment::{Horizontal, Vertical};
    use iced::widget::text;
    use preso_style::HorizontalAlign as H;

    let style = &theme.footnote;
    let label = text(text_str)
        .size(style.size * scale)
        .font(
            style
                .font
                .as_deref()
                .map(font_named)
                .unwrap_or_else(|| body_font(theme)),
        )
        .color(color(style.color.unwrap_or(theme.colors.muted)));
    let halign = match style.align {
        H::Left => Horizontal::Left,
        H::Center => Horizontal::Center,
        H::Right => Horizontal::Right,
    };
    // The line fills the width (so alignment has room and long credits
    // wrap), inset from the sides and bottom, anchored to the bottom edge.
    let px = style.padding_x * scale;
    let line = container(label).width(Fill).align_x(halign).padding(
        iced::Padding::default()
            .left(px)
            .right(px)
            .bottom(style.padding_y * scale),
    );
    container(line)
        .width(Fill)
        .height(Fill)
        .align_y(Vertical::Bottom)
        .into()
}

/// A full-canvas container whose child is anchored to `corner`. Shared by
/// the logo and slide-number chrome.
fn align_corner<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    corner: preso_style::Corner,
) -> Element<'a, M> {
    use iced::alignment::{Horizontal, Vertical};
    use preso_style::Corner;

    let positioned = container(content).width(Fill).height(Fill);
    match corner {
        Corner::TopLeft => positioned.align_y(Vertical::Top).align_x(Horizontal::Left),
        Corner::TopRight => positioned.align_y(Vertical::Top).align_x(Horizontal::Right),
        Corner::BottomLeft => positioned
            .align_y(Vertical::Bottom)
            .align_x(Horizontal::Left),
        Corner::BottomRight => positioned
            .align_y(Vertical::Bottom)
            .align_x(Horizontal::Right),
    }
    .into()
}

/// Render slide markdown, emitting link clicks (presenter window).
pub fn slide<'a>(content: &'a markdown::Content, ctx: SlideContext<'a>) -> Element<'a, Message> {
    slide_view(content, ctx).map(Message::LinkClicked)
}

/// Render slide markdown with links styled but inert (audience window,
/// spec §6.3: the audience surface never responds to clicks).
pub fn slide_inert<'a>(
    content: &'a markdown::Content,
    ctx: SlideContext<'a>,
) -> Element<'a, Message> {
    slide_view(content, ctx).map(|_| Message::Noop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_drag_normalizes_and_formats() {
        use iced::Point;
        // A left-to-right, top-to-bottom drag.
        let fwd = author_uri(Point::new(0.12, 0.20), Point::new(0.27, 0.47));
        assert_eq!(fwd, (Some("preso-hl-draw:12,20,15,27".into()), false));
        // Dragging the other way gives the same top-left corner and size.
        let rev = author_uri(Point::new(0.27, 0.47), Point::new(0.12, 0.20));
        assert_eq!(rev.0, fwd.0);
        // …and the URI round-trips into the directive the click handler copies.
        assert_eq!(
            author_directive(&fwd.0.unwrap()).unwrap(),
            "<!-- highlight: rect x=12% y=20% w=15% h=27% -->"
        );
    }

    #[test]
    fn author_drag_ignores_clicks_and_bad_uris() {
        use iced::Point;
        // A near-zero-area drag (a click) copies nothing.
        assert_eq!(
            author_uri(Point::new(0.5, 0.5), Point::new(0.505, 0.5)),
            (None, true)
        );
        // Non-author and malformed URIs don't yield a directive.
        assert!(author_directive("https://example.com").is_none());
        assert!(author_directive("preso-hl-draw:1,2,3").is_none());
        assert!(author_directive("preso-hl-draw:1,2,3,4,5").is_none());
        assert!(author_directive("preso-hl-draw:a,b,c,d").is_none());
    }

    #[test]
    fn bundled_font_family_names() {
        // The check that powers the "font not loaded" warning must read
        // the real family names from the bundled files.
        assert!(
            font_family_names(INTER_REGULAR)
                .iter()
                .any(|f| f == "Inter")
        );
        assert!(
            font_family_names(JETBRAINS_MONO)
                .iter()
                .any(|f| f == "JetBrains Mono")
        );
        assert!(font_family_names(b"not a font").is_empty());

        let loaded = loaded_families(&[]);
        assert!(loaded.contains("Inter"));
        assert!(loaded.contains("JetBrains Mono"));
    }

    #[test]
    fn br_tags_become_newlines_in_cells() {
        assert_eq!(normalize_breaks("a<br>b"), "a\nb");
        assert_eq!(normalize_breaks("a<br/>b<br />c"), "a\nb\nc");
        assert_eq!(normalize_breaks("A<BR>b"), "A\nb"); // case-insensitive
        assert_eq!(normalize_breaks("plain text"), "plain text");
        assert_eq!(normalize_breaks("a < b > c"), "a < b > c"); // not a <br> tag
        // Width heuristic measures the longest line, not the whole string.
        assert_eq!(widest_line("short<br>much longer line"), 16);
    }

    #[test]
    fn image_fragment_decodes_align_and_friends() {
        use preso_style::HorizontalAlign as H;
        let (base, attrs) = parse_image_fragment("x.png#preso-img=width:50+align:right+border");
        assert_eq!(base, "x.png");
        assert_eq!(attrs.width_pct, Some(50.0));
        assert_eq!(attrs.align, Some(H::Right));
        assert!(attrs.border);

        let (_, attrs) = parse_image_fragment("y.png#preso-img=align:center");
        assert_eq!(attrs.align, Some(H::Center));

        // The row-layout `fit` flag.
        let (_, attrs) = parse_image_fragment("z.png#preso-img=width:20+fit");
        assert!(attrs.fit);
        assert_eq!(attrs.width_pct, Some(20.0));

        // No fragment → no attributes, alignment left as default.
        let (base, attrs) = parse_image_fragment("z.png");
        assert_eq!(base, "z.png");
        assert_eq!(attrs.align, None);
    }
}
