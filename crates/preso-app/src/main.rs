mod app;
mod audience;
mod export;
mod hot_reload;

mod keyboard;
mod media;
mod monitors;
mod overlay;
mod presenter;
mod render;
mod timer;
mod transition;
mod video;
mod window_state;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "preso", version, about = "Native markdown presentations")]
struct Cli {
    /// Path to the markdown presentation file
    file: std::path::PathBuf,

    /// Theme name (dark, light) or path to a theme.toml
    #[arg(short, long)]
    theme: Option<String>,

    /// Audience window only (single window, for rehearsing on a laptop)
    #[arg(long)]
    audience_only: bool,

    /// Force the software (tiny-skia) renderer instead of the default wgpu
    /// backend. Use this if wgpu misbehaves on your GPU; it also disables
    /// embedded video (falling back to an external player) and keeps the
    /// simpler veil-style transitions. No effect in a software-only build
    /// (one compiled without the `gpu` feature).
    #[arg(long)]
    software: bool,

    /// Talk duration in minutes: shows a countdown next to the elapsed
    /// timer, warning when five minutes remain
    #[arg(short, long)]
    duration: Option<u64>,

    /// Export the deck to a PDF (rendered headless, no window) and exit
    #[arg(long, value_name = "OUTPUT")]
    export_pdf: Option<std::path::PathBuf>,

    /// Export the deck to a PowerPoint file (each slide one full-bleed
    /// image, rendered exactly as presented — not editable text) and exit
    #[arg(long, value_name = "OUTPUT", conflicts_with = "export_pdf")]
    export_pptx: Option<std::path::PathBuf>,

    /// With an export: one page/slide per reveal step instead of one fully
    /// revealed page per slide
    #[arg(long)]
    export_steps: bool,

    /// With --export-pdf: handout layout, two slides per A4 page
    #[arg(long = "export-2up", conflicts_with = "export_pptx")]
    export_two_up: bool,

    /// With an export: pixel width of each rendered slide (height follows
    /// 16:9). Lower = smaller file. Default 1600 (slides render at 3840 and are
    /// downscaled to this).
    #[arg(long, value_name = "PX", default_value_t = 1600)]
    export_width: u32,

    /// With an export: JPEG quality 1–100 for the embedded slide images.
    /// Lower = smaller file. Default 70.
    #[arg(long, value_name = "1-100", default_value_t = 70)]
    export_quality: u8,

    /// Enable debug logging
    #[arg(short, long)]
    verbose: bool,
}

/// Read the theme's `[fonts] files` (paths relative to the theme file).
/// A file that can't be read is warned about and skipped.
fn load_theme_fonts(theme: &preso_style::Theme) -> Vec<Vec<u8>> {
    theme
        .fonts
        .files
        .iter()
        .filter_map(|file| {
            let path = theme
                .source_dir
                .as_ref()
                .map_or_else(|| std::path::PathBuf::from(file), |dir| dir.join(file));
            std::fs::read(&path)
                .map_err(
                    |e| tracing::warn!(font = %path.display(), error = %e, "cannot read theme font"),
                )
                .ok()
        })
        .collect()
}

/// Font-availability feedback: family-name resolution is otherwise silent —
/// a typo'd or unmatched `*_family` just falls back to the default with no
/// signal. Warn for any requested family preso didn't load; the full loaded
/// list shows at debug level (`--verbose`).
fn warn_missing_font_families(theme: &preso_style::Theme, theme_fonts: &[Vec<u8>]) {
    let loaded = render::loaded_families(theme_fonts);
    let norm = |s: &str| s.trim().to_lowercase();
    let loaded_norm: std::collections::BTreeSet<String> = loaded.iter().map(|s| norm(s)).collect();
    let missing: Vec<String> = [
        ("body_family", theme.fonts.body_family.as_deref()),
        ("heading_family", theme.fonts.heading_family.as_deref()),
        ("code_family", theme.fonts.code_family.as_deref()),
    ]
    .into_iter()
    .filter_map(|(role, family)| {
        let family = family?;
        (!loaded_norm.contains(&norm(family))).then(|| format!("{role} = {family:?}"))
    })
    .collect();
    let list = loaded.iter().cloned().collect::<Vec<_>>().join(", ");
    if !missing.is_empty() {
        tracing::warn!(
            missing = missing.join(", "),
            loaded = list,
            "theme font families not loaded by preso; a matching installed \
             system font is used if one exists, otherwise the default"
        );
    }
    tracing::debug!(families = list, "loaded font families");
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Renderer selection: default to wgpu. `--software` forces tiny-skia
    // (also the only option in a build compiled without the `gpu` feature).
    // Both backends are linked under `gpu`; ICED_BACKEND set by the user wins.
    #[cfg(feature = "gpu")]
    let use_gpu = !cli.software;
    #[cfg(not(feature = "gpu"))]
    let use_gpu = {
        if !cli.software {
            eprintln!(
                "warning: this build has no GPU renderer (rebuild with the \
                 default features); using the software renderer"
            );
        }
        false
    };
    if std::env::var_os("ICED_BACKEND").is_none() {
        let backend = if use_gpu { "wgpu" } else { "tiny-skia" };
        // SAFETY: called before any other thread exists (start of main).
        unsafe { std::env::set_var("ICED_BACKEND", backend) };
    }

    tracing_subscriber::fmt()
        .with_env_filter(if cli.verbose {
            EnvFilter::new("preso=debug,info")
        } else {
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("preso=info"))
        })
        .init();

    let source = std::fs::read_to_string(&cli.file)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", cli.file.display()))?;
    // Splice in any `<!-- include: … -->` chapter files before anything else
    // sees the deck (theme detection, the app, and PDF export all use this).
    let deck_dir = cli
        .file
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let source = preso_core::include::expand(&source, deck_dir)?;

    // Theme priority: CLI flag > frontmatter > default (dark).
    let theme_name = cli.theme.clone().or_else(|| {
        preso_core::parser::parse(&source)
            .ok()
            .and_then(|d| d.frontmatter.theme)
    });
    let theme = match &theme_name {
        Some(name) => preso_style::load_with_search(name, &app::theme_search_dirs())?,
        None => preso_style::Theme::default(),
    };

    let theme_fonts = load_theme_fonts(&theme);
    warn_missing_font_families(&theme, &theme_fonts);

    // Headless export: render offscreen and exit, no windows at all.
    let export_target = cli
        .export_pdf
        .as_ref()
        .map(|out| {
            (
                out,
                export::Format::Pdf {
                    two_up: cli.export_two_up,
                },
            )
        })
        .or_else(|| {
            cli.export_pptx
                .as_ref()
                .map(|out| (out, export::Format::Pptx))
        });
    if let Some((out, format)) = export_target {
        if std::env::var_os("ICED_TEST_BACKEND").is_none() {
            // SAFETY: before any other thread exists.
            unsafe { std::env::set_var("ICED_TEST_BACKEND", "tiny-skia") };
        }
        // An export run renders only through the Simulator, so the *test*
        // backend is the real one. Mirror it into ICED_BACKEND so backend
        // detection (`overlay::software_renderer`, used by the canvas
        // transform workarounds) tells the truth; no window ever opens here.
        let test_backend =
            std::env::var("ICED_TEST_BACKEND").unwrap_or_else(|_| "tiny-skia".into());
        // SAFETY: before any other thread exists.
        unsafe { std::env::set_var("ICED_BACKEND", test_backend) };
        return export::run(
            &source,
            &cli.file,
            &theme,
            theme_fonts,
            out,
            export::Options {
                steps: cli.export_steps,
                width: cli.export_width,
                quality: f32::from(cli.export_quality.clamp(1, 100)) / 100.0,
                format,
            },
        );
    }

    tracing::info!(file = %cli.file.display(), theme = %theme.name, "starting preso");

    let path = cli.file.clone();
    let theme_override = cli.theme.clone();
    let audience_only = cli.audience_only;
    let duration = cli.duration;
    let default_font = render::body_font(&theme);
    // Whether wgpu is the live backend (honoring an explicit ICED_BACKEND
    // override): embedded video needs the wgpu shader path, so it gates
    // inline playback vs. the external-player fallback.
    let gpu_active = std::env::var("ICED_BACKEND")
        .map(|b| b == "wgpu")
        .unwrap_or(false);
    let mut daemon = iced::daemon(
        move || {
            app::App::new(
                path.clone(),
                source.clone(),
                theme.clone(),
                theme_override.clone(),
                audience_only,
                duration,
                gpu_active,
            )
        },
        app::App::update,
        app::App::view,
    )
    .subscription(app::App::subscription)
    .title(app::App::title)
    .theme(app::App::theme)
    // Bundled fonts: deterministic cross-platform rendering, independent
    // of system-font resolution.
    .font(render::INTER_REGULAR)
    .font(render::INTER_BOLD)
    .font(render::INTER_ITALIC)
    .font(render::INTER_BOLD_ITALIC)
    .font(render::JETBRAINS_MONO);
    for font in theme_fonts {
        daemon = daemon.font(font);
    }
    daemon.default_font(default_font).run()?;

    Ok(())
}
