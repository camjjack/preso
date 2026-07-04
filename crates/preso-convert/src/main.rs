//! `preso-convert` — turn a Slidev deck or PowerPoint file into preso markdown.

use anyhow::Context as _;
use clap::Parser;
use std::io::Write as _;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "preso-convert",
    version,
    about = "Convert a Slidev deck or PowerPoint (.pptx) file to preso markdown"
)]
struct Cli {
    /// Slidev markdown or PowerPoint (.pptx) file to convert
    input: PathBuf,

    /// Output file (defaults to stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Overwrite the output file if it already exists
    #[arg(short, long)]
    force: bool,

    /// Suppress conversion warnings on stderr
    #[arg(short, long)]
    quiet: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Dispatch on extension: `.pptx` is a binary zip read by the importer;
    // anything else is treated as Slidev markdown.
    let is_pptx = cli
        .input
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pptx"));

    let result = if is_pptx {
        // Images are extracted into `<output-stem>.assets/` beside the output;
        // with no output file (stdout) there's nowhere to put them.
        let assets = cli.output.as_deref().and_then(assets_dir);
        preso_convert::convert_pptx(&cli.input, assets.as_deref())?
    } else {
        let source = std::fs::read_to_string(&cli.input)
            .with_context(|| format!("cannot read {}", cli.input.display()))?;
        preso_convert::convert(&source)
    };

    if !cli.quiet && !result.warnings.is_empty() {
        eprintln!("{} conversion warning(s):", result.warnings.len());
        for warning in &result.warnings {
            eprintln!("  - {warning}");
        }
    }

    match &cli.output {
        Some(path) => {
            anyhow::ensure!(
                cli.force || !path.exists(),
                "{} already exists (use --force to overwrite)",
                path.display()
            );
            std::fs::write(path, &result.output)
                .with_context(|| format!("cannot write {}", path.display()))?;
            write_media(path, &result.media)?;
            if !cli.quiet {
                eprintln!("wrote {}", path.display());
                if !result.media.is_empty() {
                    eprintln!("wrote {} asset(s)", result.media.len());
                }
            }
        }
        None => {
            std::io::stdout()
                .write_all(result.output.as_bytes())
                .context("write to stdout")?;
        }
    }
    Ok(())
}

/// The assets directory name for an output file: `talk.md` → `talk.assets`.
fn assets_dir(output: &std::path::Path) -> Option<String> {
    output.file_stem()?.to_str().map(|s| format!("{s}.assets"))
}

/// Write extracted media, each path resolved relative to the output file.
fn write_media(output: &std::path::Path, media: &[(String, Vec<u8>)]) -> anyhow::Result<()> {
    let base = output.parent().unwrap_or(std::path::Path::new("."));
    for (rel, bytes) in media {
        let path = base.join(rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        std::fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}
