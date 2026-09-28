// Launches preso-lsp for markdown files, and exposes each of its layout
// code actions as a command so it can be bound to a key.
//
// The server does all the work; see docs/decisions/0009-language-server.md.

const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

// Command id → the code-action kind the server gives that action
// (`preso_core::edit::Action::kind`).
const ACTIONS = {
  "preso.twoColumns": "refactor.preso.layout.twoColumns",
  "preso.singleColumn": "refactor.preso.layout.singleColumn",
  "preso.titleSlide": "refactor.preso.kind.title",
  "preso.sectionSlide": "refactor.preso.kind.section",
  "preso.normalSlide": "refactor.preso.kind.normal",
  "preso.insertSlideAfter": "refactor.preso.slide.insertAfter",
  "preso.duplicateSlide": "refactor.preso.slide.duplicate",
  "preso.moveSlideUp": "refactor.preso.slide.moveUp",
  "preso.moveSlideDown": "refactor.preso.slide.moveDown",
  "preso.hideSlide": "refactor.preso.slide.hide",
  "preso.unhideSlide": "refactor.preso.slide.unhide",
  // The server offers the newest unused files first, so "apply first" is
  // "insert the file I just saved".
  "preso.insertImage": "refactor.preso.insert.image",
  "preso.backgroundImage": "refactor.preso.insert.background",
  "preso.insertVideo": "refactor.preso.insert.video",
};

let client;

async function activate(context) {
  const command = vscode.workspace.getConfiguration("preso").get("server.path") || "preso-lsp";
  client = new LanguageClient(
    "preso",
    "preso",
    { command, transport: TransportKind.stdio },
    {
      documentSelector: [
        { scheme: "file", language: "markdown" },
        { scheme: "untitled", language: "markdown" },
      ],
    },
  );

  for (const [id, kind] of Object.entries(ACTIONS)) {
    context.subscriptions.push(
      vscode.commands.registerCommand(id, () =>
        vscode.commands.executeCommand("editor.action.codeAction", { kind, apply: "first" }),
      ),
    );
  }

  try {
    await client.start();
  } catch (err) {
    const choice = await vscode.window.showErrorMessage(
      `preso: couldn't start \`${command}\` (${err.message}). It comes with preso ` +
        "(Homebrew, the release downloads and the .deb all include it); if it's " +
        "installed somewhere not on your PATH, set `preso.server.path`.",
      "Installation guide",
    );
    if (choice) {
      vscode.env.openExternal(
        vscode.Uri.parse("https://camjjack.github.io/preso/getting-started/editor-support.html"),
      );
    }
  }
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
