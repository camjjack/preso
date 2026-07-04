# Fonts

The `[fonts]` section sets text sizes and, optionally, font families.

```toml
[fonts]
body_size = 36
h1_size = 80
h2_size = 56
h3_size = 44
code_size = 30
```

## Sizes

The five sizes are **required** and in [design units](basics.md#design-units).
`h4`–`h6` are derived from `h3_size` automatically.

| Key | Applies to |
|-----|-----------|
| `body_size` | Body text and lists |
| `h1_size` | `#` headings |
| `h2_size` | `##` headings |
| `h3_size` | `###` headings (h4–h6 scale down from this) |
| `code_size` | Code blocks |

## Families

By default preso uses its bundled fonts — **Inter** for text/headings and
**JetBrains Mono** for code. Override them per role:

```toml
[fonts]
body_size = 36
h1_size = 80
h2_size = 56
h3_size = 44
code_size = 30
body_family = "Inter"
heading_family = "Poppins"
code_family = "JetBrains Mono"
```

A family name must match what the font declares (its family name, as shown in
Font Book on macOS or `fc-list` on Linux). `heading_family` falls back to
`body_family` if unset.

## Bundling fonts with a theme

To ship a font with the theme rather than relying on it being installed, list
the font files (relative to the theme file) in `files`:

```toml
[fonts]
# …sizes…
heading_family = "Poppins"
files = ["fonts/Poppins-Regular.ttf", "fonts/Poppins-Bold.ttf"]
```

preso loads these at startup so the family is available regardless of what's
installed on the presenting machine. A theme can bundle several weights/styles
of a family; reference it by its family name in `heading_family` etc.

### Bold and italic need their own faces

`**bold**` and `*italic*` only render properly if the family includes those
faces — bold and italic are *not* synthesized. Bundle each style you use:

```toml
files = [
  "fonts/NotoSerif-Regular.ttf",
  "fonts/NotoSerif-Bold.ttf",
  "fonts/NotoSerif-Italic.ttf",
  "fonts/NotoSerif-BoldItalic.ttf",
]
```

> ⚠️ **Variable fonts** (a single file with weight/width axes, e.g.
> `NotoSerif-VariableFont_wdth,wght.ttf`) expose only their *default instance*
> through preso's text engine — usually upright, regular weight. Bold and
> italic then fall back to another font (or render upright). Use the **static**
> face files (the `static/` folder of most Google Fonts downloads) and list
> each style, as above.

## When a font is missing

If a requested family isn't available, preso prints a warning at startup
naming the family, and falls back to a bundled font. Run with `--verbose` to
also list every family preso did load, which is the quickest way to find the
exact name a font expects:

```sh
preso talk.md --theme mytheme.toml --verbose
```
