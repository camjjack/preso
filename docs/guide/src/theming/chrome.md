# Slide Chrome

The `[slide]` section styles the slide surface and the persistent decoration
around your content: backgrounds, accent bars, and a logo. The slide number is
a sibling `[slide_number]` section. All sizes are
[design units](basics.md#design-units).

## Backgrounds

By precedence, the slide background is: a per-slide `background=` override → the
theme's background image → a gradient → the flat `colors.background`.

### Gradient

```toml
[slide]
gradient = { from = "#0f2027", to = "#2c5364", angle = 160 }
```

`angle` is in degrees: `0` puts `to` at the top, `180` (the default) at the
bottom.

### Background image

A cover-fit image behind every slide (path relative to the theme file). Content
renders on top:

```toml
[slide]
background_image = "backdrop.png"
```

For a single slide instead of the whole deck, use the per-slide
`<!-- slide: background=… -->` directive (see
[Images & Backgrounds](../writing/images.md#full-bleed-backgrounds)).

## Accent bars

A solid band along one edge:

```toml
[slide]
bar = { side = "bottom", size = 24, color = "#1f6feb" }
```

| Key | Meaning |
|-----|---------|
| `side` | `top`, `bottom`, `left`, or `right`. |
| `size` | Thickness, design units. |
| `color` | `#rrggbb`. |
| `reserve` | If `true`, slide content is kept clear of the bar; chrome (logo, number) still draws over it. Default `false`. |
| `hidden` | Used in a kind overlay to remove an inherited bar (see below). |

### Multiple bars

For more than one band, use `bars` (a list). A `[title.slide]` /
`[section.slide]` overlay that sets `bars` **replaces** the inherited single
`bar`, so title slides can show two bands while normal slides keep one:

```toml
[slide]
bar = { side = "bottom", size = 16, color = "#1f6feb", reserve = true }

[title.slide]
bars = [
  { side = "top",    size = 16, color = "#1f6feb" },
  { side = "bottom", size = 16, color = "#56b6c2" },
]
```

## Logo

A watermark image in a corner, drawn on every slide:

```toml
[slide]
logo = { path = "logo.png", width = 8, position = "bottom-right", opacity = 0.4 }
```

| Key | Meaning |
|-----|---------|
| `path` | Image path, relative to the theme file. |
| `width` | Width as a percentage of the slide width. |
| `position` | `top-left`, `top-right`, `bottom-left`, `bottom-right`. |
| `opacity` | `0.0`–`1.0`. |
| `padding_x` / `padding_y` | Inset from the corner, design units (default 24). |
| `border` / `shadow` | Frame the logo like a content image (see [Element Styles](elements.md#images)). |
| `hidden` | Used in a kind overlay to drop the logo for that kind. |

## Slide number

A `[slide_number]` section adds a page-number stamp to every slide. Omit the
section and there's no number.

```toml
[slide_number]
format = "{current} / {total}"
size = 24
position = "bottom-left"
color = "#94a3b8"
padding_x = 40
padding_y = 28
```

| Key | Meaning |
|-----|---------|
| `format` | Template; `{current}` and `{total}` expand. |
| `size` | Text size, design units. |
| `font` | Family name; falls back to the body font. |
| `position` | Corner, as for the logo. |
| `color` | Falls back to `colors.muted`. |
| `padding_x` / `padding_y` | Offset from the corner, design units (default 24). |
| `hidden` | Used in a kind overlay to drop the number (e.g. on title slides). |

The number is anchored by its **center**, so it stays put as the digit count
grows ("9" → "10") rather than one edge staying pinned. Because the padding is
in design units like the bars, you can size it to sit just clear of a bar at
any window size.

Numbering counts the slides preso actually shows (hidden slides don't count).
To **re-sync** with another deck — say one that numbered its hidden slides —
put `<!-- slide: number=N -->` on a slide to set its number to `N`; the slides
after it continue from there. `{total}` becomes the highest number reached.

## Footnote

A `[footnote]` section styles the small disclaimer line a slide shows when it
carries a `<!-- footnote: … -->` directive — handy for image credits or a
Creative Commons attribution. Unlike the slide number, the *directive* decides
which slides show one; this section only styles it (and works with no section
at all).

```toml
[footnote]
size = 18
align = "left"        # left | center | right
color = "#94a3b8"
padding_x = 60        # inset from the sides (match slide_padding to line up)
padding_y = 36        # inset from the bottom
```

| Key | Meaning |
|-----|---------|
| `size` | Text size, design units (default 18). |
| `font` | Family name; falls back to the body font. |
| `align` | Horizontal placement along the bottom (default `left`). |
| `color` | Falls back to `colors.muted`. |
| `padding_x` / `padding_y` | Inset from the sides and bottom, design units. |
| `hidden` | Used in a kind overlay to drop the footnote (e.g. on title slides). |

See [Writing Decks → Images](../writing/images.md#crediting-images) for the
directive itself.

## Alignment

`[slide]` also holds the deck-wide content alignment defaults, `align`
(`top`/`center`) and `halign` (`left`/`center`/`right`); per-slide
[directives](../writing/alignment.md) override them.

```toml
[slide]
align = "top"
halign = "left"
```

All of these chrome settings can be overridden per slide kind — see
[Slide-Kind Overlays](overlays.md).
