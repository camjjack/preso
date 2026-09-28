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
use iced::widget::markdown;
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
        // One page standing for the whole slide renders it fully revealed,
        // which is what a `<!-- pause -->` build wants — but a click-through
        // code walk (`{2-3|5}`) would land on whatever line the walk finished
        // on, so the page arrives mid-explanation with an arbitrary line
        // emphasised. Those blocks start where the audience first sees them
        // instead. Exporting the steps themselves keeps the walk, since each
        // stage gets its own page.
        let code_stage = (!steps).then_some(0);
        for step in step_range {
            let content = markdown::Content::parse(slide.step_source(step));
            let columns = slide.column_slides(step).map(crate::app::columns_of);

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
                BuildPoint { step, code_stage },
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

/// Which point of a slide's build a page draws: the reveal step, plus the
/// click-through code stage when the page forces one rather than following
/// that step (see [`SlideContext::code_stage`]).
#[derive(Clone, Copy)]
struct BuildPoint {
    step: usize,
    code_stage: Option<usize>,
}

/// The audience-canvas element at design resolution (scale 1.0).
fn page_element<'a>(
    content: &'a markdown::Content,
    columns: Option<&'a crate::app::Columns>,
    slide: &'a preso_core::Slide,
    media: &'a Media,
    theme: &'a preso_style::Theme,
    number: (usize, usize),
    at: BuildPoint,
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
                text_scale: render::resolve_text_scale(&slide.overrides, theme),
                step: at.step,
                code_stage: at.code_stage,
                // iced_test's Simulator always snapshots at 2×, and the
                // tiny-skia offset bug scales with that factor (see
                // `overlay::compensated_frame`).
                scale_factor: 2.0,
                authoring: false,
                zoom_from: None,
            },
        )
    };
    let body: Element<'a, crate::app::Message> = match columns {
        // Same layout as on screen, at design scale (1.0 here).
        Some(columns) => columns.body(
            render_one,
            slide.layout.column_portions().unwrap_or((1, 1)),
            theme,
            1.0,
        ),
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
            // A page standing for the whole slide starts its zoom walk where
            // its code walk starts (see `SlideContext::code_stage`).
            zoom: render::SurfaceZoom::at(slide, if at.code_stage.is_some() { 0 } else { at.step }),
        },
    )
}

/// Test support: render `element` offscreen at `size` (logical) and decode
/// it to a page, downscaled to `width` pixels wide (`0` keeps the Simulator's
/// native 2×). The workspace's `.cargo/config.toml` points the Simulator at
/// the software renderer, as an export run does.
#[cfg(test)]
pub(crate) fn offscreen<'a, Message>(
    element: impl Into<Element<'a, Message>>,
    size: iced::Size,
    settings: iced::Settings,
    theme: &iced::Theme,
    width: u32,
) -> preso_export::Page {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    let mut simulator = iced_test::simulator::Simulator::with_size(settings, size, element);
    let snapshot = simulator.snapshot(theme).expect("snapshot");
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("preso-test-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let page = page_from_snapshot(&snapshot, &dir, 0, width).expect("decode");
    let _ = std::fs::remove_dir_all(&dir);
    page
}

/// Test support: a sampler over `page`'s pixels at (x, y) as fractions of
/// its size.
#[cfg(test)]
pub(crate) fn sampler(page: preso_export::Page) -> impl Fn(f32, f32) -> [u8; 4] {
    move |x, y| {
        let px = ((page.width as f32 * x) as usize).min(page.width as usize - 1);
        let py = ((page.height as f32 * y) as usize).min(page.height as usize - 1);
        let i = (py * page.width as usize + px) * 4;
        page.rgba[i..i + 4].try_into().unwrap()
    }
}

/// iced_test only exposes snapshot pixels through its PNG side-channel:
/// `matches_image` writes the PNG when the file is missing. Write it to a
/// scratch path and decode it back.
pub(crate) fn page_from_snapshot(
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Render the first slide of `source` at reveal `step`, as a page
    /// downscaled to 960px wide.
    fn render_step(source: &str, step: usize) -> preso_export::Page {
        let parsed = preso_core::parser::parse(source).unwrap();
        let slide = &parsed.slides[0];
        let media = Media::new(Path::new("deck.md"));
        let theme = preso_style::Theme::default();
        let content = markdown::Content::parse(slide.step_source(step));
        let element = page_element(
            &content,
            None,
            slide,
            &media,
            &theme,
            (1, 1),
            BuildPoint {
                step,
                code_stage: None,
            },
        );
        let settings = iced::Settings {
            fonts: vec![
                render::INTER_REGULAR.into(),
                render::INTER_BOLD.into(),
                render::JETBRAINS_MONO.into(),
            ],
            default_font: render::body_font(&theme),
            ..iced::Settings::default()
        };
        offscreen(
            element,
            iced::Size::new(CANVAS_WIDTH, CANVAS_HEIGHT),
            settings,
            &render::iced_theme(&theme),
            960,
        )
    }

    /// Pixels that aren't one of the page's two commonest colours (the slide
    /// and panel backgrounds): roughly, how much ink is on it. Bigger text
    /// means more.
    fn ink(page: &preso_export::Page) -> usize {
        let mut counts = std::collections::HashMap::<[u8; 4], usize>::new();
        for px in page.rgba.as_chunks::<4>().0 {
            *counts.entry(*px).or_default() += 1;
        }
        let mut by_count: Vec<usize> = counts.into_values().collect();
        by_count.sort_unstable_by(|a, b| b.cmp(a));
        by_count.iter().skip(2).sum()
    }

    #[test]
    fn a_code_zoom_stage_magnifies_its_lines() {
        let src = "```rust {all|2 zoom}\nfn a() {}\nlet the_line_to_zoom_onto = 1;\nfn c() {}\nfn d() {}\n```\n";
        let whole = render_step(src, 0);
        let zoomed = render_step(src, 1);
        assert_ne!(whole.rgba, zoomed.rgba, "the zoom stage changes the page");
        // One line at ~2× carries more ink than four at 1×.
        assert!(
            ink(&zoomed) > ink(&whole),
            "{} vs {}",
            ink(&zoomed),
            ink(&whole)
        );
    }

    #[test]
    fn a_slide_zoom_magnifies_the_content() {
        let src = "# A heading to zoom onto\n\nSome body text below it.\n\n<!-- zoom[1]: 25%,15%,2x -->\n";
        let whole = render_step(src, 0);
        let zoomed = render_step(src, 1);
        assert!(
            ink(&zoomed) > ink(&whole),
            "{} vs {}",
            ink(&zoomed),
            ink(&whole)
        );
    }

    /// Whether a page has any of a Mermaid diagram's node-outline blue-grey
    /// on it — i.e. the diagram drew, not just the card behind it.
    fn has_diagram_strokes(page: &preso_export::Page) -> bool {
        page.rgba.as_chunks::<4>().0.iter().any(|p| {
            let (r, g, b) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
            (90..=180).contains(&r) && b - r > 15 && (g - r).abs() < 40
        })
    }

    #[test]
    fn tests_render_like_an_export() {
        // Under wgpu, a raster this size (over its 2 MiB synchronous-upload
        // limit) is missing from a one-shot snapshot; the software renderer
        // an export uses draws it. Guards the `.cargo/config.toml` setting.
        let src = "```mermaid {width=100%}\ngraph LR\n  a[Parse] --> b[Layout] --> c[Paint]\n```\n";
        assert!(has_diagram_strokes(&render_step(src, 0)));
    }

    #[test]
    fn a_diagram_zoom_stage_changes_the_view() {
        let src =
            "```mermaid {all|Layout zoom}\ngraph LR\n  a[Parse] --> b[Layout] --> c[Paint]\n```\n";
        let whole = render_step(src, 0);
        let zoomed = render_step(src, 1);
        assert_ne!(whole.rgba, zoomed.rgba);
        // Zoomed, it's drawn from a deeper raster — which must still draw.
        assert!(has_diagram_strokes(&whole) && has_diagram_strokes(&zoomed));
    }
}
