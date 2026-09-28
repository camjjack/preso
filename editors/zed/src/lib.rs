//! Launches `preso-lsp` for Markdown files. The server does all the work;
//! see docs/decisions/0009-language-server.md in the preso repository.

use zed_extension_api::{self as zed, LanguageServerId, Result, settings::LspSettings};

struct Preso;

impl zed::Extension for Preso {
    fn new() -> Self {
        Preso
    }

    fn language_server_command(
        &mut self,
        id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        // `"lsp": { "preso-lsp": { "binary": { "path": … } } }` in settings
        // wins; otherwise find it on PATH.
        let binary = LspSettings::for_worktree(id.as_ref(), worktree)
            .ok()
            .and_then(|s| s.binary);
        let configured = binary.as_ref().and_then(|b| b.path.clone());
        let command = configured
            .or_else(|| worktree.which("preso-lsp"))
            .ok_or_else(|| {
                "preso-lsp isn't on PATH. It comes with preso (Homebrew, the \
                 release downloads and the .deb all include it); see \
                 https://camjjack.github.io/preso/getting-started/editor-support.html — or set lsp.preso-lsp.binary.path"
                    .to_string()
            })?;
        Ok(zed::Command {
            command,
            args: binary.and_then(|b| b.arguments).unwrap_or_default(),
            env: Vec::new(),
        })
    }
}

zed::register_extension!(Preso);
