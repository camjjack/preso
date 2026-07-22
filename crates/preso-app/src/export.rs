//! Headless document export (PDF or bitmap PowerPoint): render every page
//! offscreen with iced_test's Simulator at a fixed 1920×1080 logical
//! canvas, then hand the captured frames to `preso-export`'s assembler for
//! the chosen format. Snapshots come back at 2× scale (3840×2160 ≈ 288 DPI
//! on a 13.33in page) regardless of the user's screen — no window is
//! opened at all.

use crate::media::Media;
use crate::render::{self, SlideContext};
use anyhow::Context as _;
use iced::Element;
use iced::widget::{container, markdown, row};
use std::path::Path;

const CANVAS_WIDTH: f32 = 1920.0;
const CANVAS_HEIGHT: f32 = 1080.0;

/// Which document the captured pages are assembled into.
pub enum Format {
    Pdf {
        two_up: bool,
    },
    /// PowerPoint, one full-bleed picture per slide — pixel-faithful,
    /// not editable text.
    Pptx,
}

/// Export tuning (mostly file size). `width` downscales each rendered
/// slide (iced_test always renders at 3840×2160); `quality` is the embedded
/// JPEG quality (0..1) where JPEG wins the per-slide codec choice.
pub struct Options {
    pub steps: bool,
    pub width: u32,
    pub quality: f32,
    pub format: Format,
}

/// A two-column slide split: (markdown, sub-slide metadata) per side.
type ColumnPair = (
    (markdown::Content, preso_core::Slide),
    (markdown::Content, preso_core::Slide),
);

pub fn run(
    source: &str,
    deck_path: &Path,
    theme: &preso_style::Theme,
    theme_fonts: Vec<Vec<u8>>,
    out: &Path,
    options: Options,
) -> anyhow::Result<()> {
    let Options {
        steps,
        width,
        quality,
        format,
    } = options;
    let parsed = preso_core::parser::parse(source).context("parse deck")?;
    let media = Media::new(deck_path);
    let iced_theme = render::iced_theme(theme);

    let mut fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
        render::INTER_REGULAR.into(),
        render::INTER_BOLD.into(),
        render::INTER_ITALIC.into(),
        render::INTER_BOLD_ITALIC.into(),
        render::JETBRAINS_MONO.into(),
    ];
    fonts.extend(theme_fonts.into_iter().map(std::borrow::Cow::from));
    let settings = iced::Settings {
        fonts,
        default_font: render::body_font(theme),
        ..iced::Settings::default()
    };

    let tmp = std::env::temp_dir().join(format!("preso-export-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;

    let mut pages: Vec<preso_export::Page> = Vec::new();
    let total = preso_core::display_total(&parsed.slides);
    // Kind themes, resolved once (same selection as App::slide_theme).
    let title_theme = theme.title.apply(theme);
    let section_theme = theme.section.apply(theme);
    let slide_theme = |slide: &preso_core::Slide| match slide.overrides.kind.as_deref() {
        Some("title") => &title_theme,
        Some("section") => &section_theme,
        _ => theme,
    };
    for (slide_index, slide) in parsed.slides.iter().enumerate() {
        let step_range = if steps {
            0..slide.step_count()
        } else {
            slide.step_count() - 1..slide.step_count()
        };
        for step in step_range {
            let content = markdown::Content::parse(slide.step_source(step));
            let columns = slide
                .column_slides(step)
                .map(|((ls, lslide), (rs, rslide))| {
                    (
                        (markdown::Content::parse(&ls), lslide),
                        (markdown::Content::parse(&rs), rslide),
                    )
                });

            let element = page_element(
                &content,
                columns.as_ref(),
                slide,
                &media,
                slide_theme(slide),
                (
                    preso_core::display_number(&parsed.slides, slide_index),
                    total,
                ),
                step,
            );
            let mut simulator = iced_test::simulator::Simulator::with_size(
                settings.clone(),
                iced::Size::new(CANVAS_WIDTH, CANVAS_HEIGHT),
                element,
            );
            let snapshot = simulator
                .snapshot(&iced_theme)
                .map_err(|e| anyhow::anyhow!("render page {}: {e:?}", pages.len() + 1))?;
            pages.push(page_from_snapshot(&snapshot, &tmp, pages.len(), width)?);
            eprint!("\rrendered page {}", pages.len());
        }
    }
    eprintln!();

    let title = parsed
        .frontmatter
        .title
        .clone()
        .unwrap_or_else(|| deck_path.display().to_string());
    match format {
        Format::Pdf { two_up } => {
            let layout = if two_up {
                preso_export::Layout::TwoUp
            } else {
                preso_export::Layout::Slides
            };
            preso_export::write_pdf(&title, &pages, out, layout, quality).context("write PDF")?;
        }
        Format::Pptx => {
            preso_export::write_pptx(&title, &pages, out, quality).context("write PPTX")?;
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    println!("exported {} page(s) to {}", pages.len(), out.display());
    Ok(())
}

/// The audience-canvas element at design resolution (scale 1.0).
fn page_element<'a>(
    content: &'a markdown::Content,
    columns: Option<&'a ColumnPair>,
    slide: &'a preso_core::Slide,
    media: &'a Media,
    theme: &'a preso_style::Theme,
    number: (usize, usize),
    step: usize,
) -> Element<'a, crate::app::Message> {
    let render_one = |content, code_slide| {
        render::slide_inert(
            content,
            SlideContext {
                media,
                code_slide,
                math_slide: slide,
                theme,
                scale: 1.0,
                animation_time: std::time::Duration::ZERO,
                halign: render::resolve_halign(&slide.overrides, theme),
                step,
                // iced_test's Simulator always snapshots at 2×, and the
                // tiny-skia offset bug scales with that factor (see
                // `overlay::compensated_frame`).
                scale_factor: 2.0,
                authoring: false,
            },
        )
    };
    let body: Element<'a, crate::app::Message> = match columns {
        Some(((left_md, left_slide), (right_md, right_slide))) => {
            // Match the on-screen header-band alignment (scale 1.0 here).
            let (lp, rp) = render::column_header_pads(
                left_slide.leading_heading_level(),
                right_slide.leading_heading_level(),
                theme,
                1.0,
            );
            let (lw, rw) = slide.layout.column_portions().unwrap_or((1, 1));
            row![
                container(render_one(left_md, left_slide))
                    .width(iced::FillPortion(lw))
                    .padding(iced::padding::top(lp)),
                container(render_one(right_md, right_slide))
                    .width(iced::FillPortion(rw))
                    .padding(iced::padding::top(rp)),
            ]
            .spacing(40.0)
            .into()
        }
        None => render_one(content, slide),
    };
    render::slide_surface(
        body,
        media,
        theme,
        &slide.overrides,
        render::SurfaceOptions {
            scale: 1.0,
            size: iced::Size::new(CANVAS_WIDTH, CANVAS_HEIGHT),
            number: Some(number),
            footnote: slide.footnote.clone(),
            layer_images: slide.layer_images.clone(),
            video: slide.video.is_some(),
            video_playing: false,
            // Dither noise defeats the JPEG page compression (~14x
            // larger files); pages keep iced's plain gradient.
            dither: false,
        },
    )
}

/// iced_test only exposes snapshot pixels through its PNG side-channel:
/// `matches_image` writes the PNG when the file is missing. Write it to a
/// scratch path and decode it back.
fn page_from_snapshot(
    snapshot: &iced_test::simulator::Snapshot,
    dir: &Path,
    index: usize,
    target_width: u32,
) -> anyhow::Result<preso_export::Page> {
    let stub = dir.join(format!("page-{index}.png"));
    let created = snapshot
        .matches_image(&stub)
        .map_err(|e| anyhow::anyhow!("write snapshot: {e:?}"))?;
    anyhow::ensure!(created, "snapshot collided with an existing file");

    // matches_image appends the renderer name to the file stem.
    let produced = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("page-{index}-")) && n.ends_with(".png"))
        })
        .context("snapshot PNG not found")?;

    let mut image = image::open(&produced)
        .with_context(|| format!("decode {}", produced.display()))?
        .to_rgba8();
    // iced_test always renders at 2× (3840×2160); downscale to the requested
    // width to shrink the PDF (Triangle is fast and clean for downscaling).
    if target_width > 0 && image.width() > target_width {
        let target_height = (image.height() * target_width).div_ceil(image.width());
        image = image::imageops::resize(
            &image,
            target_width,
            target_height,
            image::imageops::FilterType::Triangle,
        );
    }
    tracing::debug!(file = %produced.display(), w = image.width(), h = image.height(), "page snapshot");
    Ok(preso_export::Page {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}
