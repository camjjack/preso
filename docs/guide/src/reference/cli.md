# Command Line

```text
preso <FILE> [OPTIONS]
```

`<FILE>` is the deck — a markdown file. With no options, preso opens the
audience and presenter windows.

| Flag | Description |
|------|-------------|
| `-t`, `--theme <NAME\|PATH>` | Theme to use: a built-in (`dark`, `light`), a name found in the theme search dir, or a path to a `.toml` file. Overrides the deck's frontmatter `theme`. |
| `--audience-only` | Open only the audience window (rehearsing on a single screen). |
| `-d`, `--duration <MINUTES>` | Talk length; adds a countdown to the presenter view, warning when five minutes remain. |
| `--export-pdf <OUTPUT>` | Render the deck to a PDF and exit — no window opens. |
| `--export-steps` | With `--export-pdf`: one page per reveal step instead of one fully-revealed page per slide. |
| `--export-2up` | With `--export-pdf`: handout layout, two slides per A4 page. |
| `--export-width <PX>` | With `--export-pdf`: pixel width of each slide image (height follows 16:9); lower = smaller file. Default `1600`. |
| `--export-quality <1-100>` | With `--export-pdf`: JPEG quality of the slide images; lower = smaller file. Default `70`. |
| `--software` | Force the software (tiny-skia) renderer instead of the default wgpu backend. Use if wgpu misbehaves on your GPU; also disables embedded video (falls back to an external player). No effect in a software-only build. See [Rendering Notes](../appendix/rendering.md). |
| `-v`, `--verbose` | Enable debug logging; also lists every font family preso loaded. |
| `--version` | Print the version. |
| `-h`, `--help` | Print help. |

## Examples

```sh
# Present with a theme
preso talk.md --theme dark

# Rehearse on a laptop with a 30-minute countdown
preso talk.md --audience-only --duration 30

# Export a PDF, one page per slide
preso talk.md --export-pdf talk.pdf

# Export a step-by-step PDF and a 2-up handout
preso talk.md --export-pdf steps.pdf --export-steps
preso talk.md --export-pdf handout.pdf --export-2up

# Diagnose a missing theme font
preso talk.md --theme mytheme.toml --verbose
```

See also [Exporting to PDF](../exporting.md) and
[Keyboard Shortcuts](keyboard.md).
