# Theme Basics

A theme is a TOML file describing colours, fonts, spacing, and slide chrome.
Two are built in — `dark` and `light` — and you can write your own.

## Choosing a theme

In the deck's frontmatter:

```markdown
---
theme: dark
---
```

Or on the command line, which overrides the frontmatter:

```sh
preso talk.md --theme light
preso talk.md --theme ./themes/corporate.toml
```

A theme is resolved in this order:

1. A **built-in** name — `dark` or `light`.
2. A file `<name>.toml` in a **theme search directory**
   (`~/.config/preso/themes/` on Linux/macOS, the platform config dir
   elsewhere) — so `theme: corporate` finds `…/preso/themes/corporate.toml`.
3. A **path** to a `.toml` file.

If none matches, preso reports an error.

## A minimal theme

A theme needs a name, a `code_theme` for syntax highlighting, and the
`[colors]`, `[fonts]`, and `[spacing]` sections. This is the built-in `dark`
theme in full:

```toml
name = "dark"
code_theme = "base16-ocean.dark"

[colors]
background = "#1e1e2e"
text = "#cdd6f4"
heading = "#89b4fa"
accent = "#f5c2e7"
link = "#74c7ec"
muted = "#6c7086"
code_background = "#11111b"

[fonts]
body_size = 36
h1_size = 80
h2_size = 56
h3_size = 44
code_size = 30

[spacing]
slide_padding = 60
paragraph_gap = 20
```

Everything else — backgrounds, bars, logos, per-element styling, slide-kind
overlays — is optional and layered on top.

## Design units

All sizes in a theme (font sizes, padding, bar thickness, …) are **design
units** on a fixed 1920×1080 virtual canvas. preso scales them to the actual
window, so a deck looks identical at any size and the relationships between
elements (a 24-unit bar, a number padded 40 units in) hold everywhere.

## The sections

| Section | Controls | Page |
|---------|----------|------|
| `[colors]` | The palette | [Colors](colors.md) |
| `[fonts]` | Families, sizes, bundled fonts | [Fonts](fonts.md) |
| `[spacing]` | Padding and gaps | [Spacing](spacing.md) |
| `[slide]` | Backgrounds, bars, logo, alignment | [Slide Chrome](chrome.md) |
| `[slide_number]` | The page-number stamp | [Slide Chrome](chrome.md#slide-number) |
| `[heading]` `[code_block]` `[table]` `[image]` `[quote]` | Per-element styling | [Element Styles](elements.md) |
| `[title]` `[section]` | Per-kind overlays | [Slide-Kind Overlays](overlays.md) |

The [Theme Schema](../reference/theme-schema.md) reference lists every key and
default. For a fully worked example, see [A Complete Theme](complete-theme.md).

> 💡 Unknown keys are rejected — a typo in a theme is an error, not a silently
> ignored setting, so mistakes surface immediately.

## Live editing

Themes hot-reload. Leave preso running and edit the theme `.toml` (or the
deck's frontmatter `theme:`) — the deck restyles on save, keeping your place.
This works whether the theme came from the frontmatter or `--theme`, so you can
iterate on a theme with the windows open beside you.

One caveat: **new font files** added to `[fonts] files` need a restart, because
fonts are registered once at startup. Changing colours, spacing, bars, the
logo, element styles, and switching to an already-loaded font family all take
effect live.
