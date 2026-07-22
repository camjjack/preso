# Exporting (PDF and PowerPoint)

preso can render a deck straight to a document — no window opens, so it
works on a build server or in a script:

```sh
preso talk.md --export-pdf talk.pdf
preso talk.md --export-pptx talk.pptx
```

Both write one page/slide per deck slide (each fully revealed) at high
resolution. Everything that renders on screen renders in the export — code
highlighting, tables, math, Mermaid/Graphviz diagrams, gradients, logos, and
slide numbers — because export uses the same renderer as the live windows.

The `.pptx` is a **picture-per-slide** file: pixel-faithful (your theme
included), but the slides are images, not editable text. It answers "the
organizers want a PowerPoint", not "hand off for editing" — for editable
output, `preso-convert talk.md --to pptx` produces real text boxes and
tables (plain styling), and `--to slidev` targets Slidev; see
[Leaving preso](migrating.md#leaving-preso-exporting-to-slidev-or-powerpoint).

## Modes

| Flag | Result |
|------|--------|
| `--export-pdf <file>` | One page per slide, fully revealed. |
| `--export-pptx <file>` | One PowerPoint slide per slide, fully revealed. |
| `… --export-steps` | One page/slide per **reveal step** — each `<!-- pause -->` and each [click-through](writing/code.md#click-through-highlighting) stage becomes its own page. |
| `--export-pdf <file> --export-2up` | Handout layout: two slides per A4 page (PDF only). |

```sh
# A build-up PDF that mirrors how the talk reveals
preso talk.md --export-pdf steps.pdf --export-steps

# A printable handout
preso talk.md --export-pdf handout.pdf --export-2up
```

The `--theme` flag applies to export too, so you can render the same deck under
different themes:

```sh
preso talk.md --theme light --export-pdf talk-light.pdf
```

> 💡 Image, logo, and background paths resolve relative to the deck file, just
> as when presenting — run the export from anywhere as long as those assets are
> where the deck expects them.

## File size

Each slide is embedded as an image, and preso picks the smaller codec per
slide — lossless for flat/text slides (which stay crisp), JPEG for
photographic ones (in both formats). Two flags trade quality for size:

| Flag | Default | Effect |
|------|---------|--------|
| `--export-width <px>` | `1600` | Pixel width of each slide (height follows 16:9). Halving it roughly quarters the image area. |
| `--export-quality <1-100>` | `70` | JPEG quality. Lower = smaller, with more compression artifacts. |

```sh
# Smaller file for a long deck (≈¼–⅒ the default size)
preso talk.md --export-pdf talk.pdf --export-width 1280 --export-quality 60
```

The defaults aim for a good size/clarity balance; drop both for a big
image-heavy deck, or raise them for print-quality output.
