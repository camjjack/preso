//! preso-lsp: a language server for preso decks — slide outline, folding,
//! diagnostics, completions, document links, and layout code actions.
//! See docs/decisions/0009-language-server.md.
//!
//! Speaks LSP over stdio. Editors launch it; it is not run by hand.

mod convert;
mod diagrams;
mod features;
mod roots;
mod server;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "preso-lsp", version, about = "Language server for preso decks")]
struct Cli {
    /// Accepted for clients that pass it; stdio is the only transport.
    #[arg(long)]
    stdio: bool,
}

fn main() -> anyhow::Result<()> {
    let _ = Cli::parse();
    // stdout carries the protocol, so logs go to stderr, where editors
    // surface them in their language-server output panel.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_env("PRESO_LSP_LOG").unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let (connection, io_threads) = lsp_server::Connection::stdio();
    server::run(&connection)?;
    drop(connection);
    io_threads.join()?;
    Ok(())
}
