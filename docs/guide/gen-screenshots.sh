#!/usr/bin/env bash
# Regenerate the user-guide screenshots from real decks, so every image is an
# actual preso render. macOS only (render-page.swift uses PDFKit via `swift`).
#
#   docs/guide/gen-screenshots.sh
#
# Screenshots come from docs/example-talk.md, whose slide order is pinned by
# the `example_talk_parses` test — so the page numbers below stay valid.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
OUT="docs/guide/src/images"
WIDTH=1280
mkdir -p "$OUT"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Rendering example deck to PDF…"
cargo run -q --bin preso -- docs/example-talk.md --export-pdf "$TMP/example.pdf"

shot() { # <page-1based> <out-name>
  swift docs/guide/render-page.swift "$TMP/example.pdf" "$1" "$OUT/$2" "$WIDTH"
  echo "  $OUT/$2  (page $1)"
}

echo "Extracting screenshots…"
shot 1  slide-kind-title.png
shot 6  slide-kind-section.png
shot 5  two-columns.png
shot 3  code-highlight.png
shot 12 table.png
shot 8  diagram-mermaid.png
shot 10 diagram-graphviz.png
shot 11 math.png
shot 14 background.png
echo "Done."
