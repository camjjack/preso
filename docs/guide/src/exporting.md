# Exporting to PDF

preso can render a deck straight to PDF — no window opens, so it works on a
build server or in a script:

```sh
preso talk.md --export-pdf talk.pdf
```

That writes one page per slide (each fully revealed) at high resolution.
Everything that renders on screen renders in the PDF — code highlighting,
tables, math, Mermaid/Graphviz diagrams, gradients, logos, and slide numbers —
because export uses the same renderer as the live windows.

## Modes

| Flag | Result |
|------|--------|
| `--export-pdf <file>` | One page per slide, fully revealed. |
| `--export-pdf <file> --export-steps` | One page per **reveal step** — each `<!-- pause -->` and each [click-through](writing/code.md#click-through-highlighting) stage becomes its own page. |
| `--export-pdf <file> --export-2up` | Handout layout: two slides per A4 page. |

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
photographic ones. Two flags trade quality for size:

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
