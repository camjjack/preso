# Element Styles

Four optional sections style specific content. Each falls back to the
[palette](colors.md) when unset, so you only set what you want to change. Sizes
are [design units](basics.md#design-units).

## Headings

`[heading]` overrides heading colour, optionally per level:

```toml
[heading]
color = "#ffffff"      # all headings
h1_color = "#89b4fa"   # …or per level, overriding `color`
h2_color = "#f5c2e7"
h3_color = "#a6e3a1"
```

A level's colour resolves as: its `hN_color` → `color` → `colors.heading`.

## Code blocks

`[code_block]` styles the code panel and how highlighted lines are shown:

```toml
[code_block]
background = "#11161f"      # falls back to colors.code_background
border_radius = 8
padding = 24
highlight_color = "#56b6c2" # tint behind highlighted lines
highlight_style = "background"
dim_opacity = 0.35
```

| Key | Meaning |
|-----|---------|
| `background` | Panel colour; falls back to `colors.code_background`. |
| `border_radius` | Corner radius. |
| `padding` | Inner padding. |
| `highlight_color` | Tint behind `{2,4-6}` lines (default: accent, faint). |
| `highlight_style` | `background` (tint the selected lines, default) or `dim` (fade everything else — "focus mode"). |
| `dim_opacity` | Opacity of the non-selected lines in `dim` mode (default `0.35`). |

See [Code Blocks](../writing/code.md) for the deck-side line/click-through
annotations these style.

## Tables

`[table]` styles GFM tables — header, striping, borders:

```toml
[table]
header_background = "#1e293b"
header_color = "#f8fafc"
stripe_background = "#0f1b2e"   # zebra striping on alternate rows
border_color = "#334155"
border_width = 1
padding = 12
border_radius = 6
```

All fields are optional and fall back to palette-derived defaults. See
[Tables](../writing/text-tables.md#tables) for the markdown side.

## Images

`[image]` sets default framing for content images (and the logo). Per-image
markdown flags (`{border shadow plain}`) override these.

```toml
[image]
border = { color = "#334155", width = 2, radius = 8 }
shadow = true
```

- **`border`** — `{ color, width, radius }`; `color` falls back to
  `colors.muted`.
- **`shadow`** — `true` for the default drop shadow, or
  `{ color, offset = [x, y], blur }` to tune it.

See [Images & Backgrounds](../writing/images.md) for the deck-side syntax.

## Quotes

`[quote]` turns a markdown blockquote (`> …`) into a styled callout — a way to
make one part of a slide stand out.

```toml
[quote]
background    = "#172230"   # fill (optional)
border_color  = "#56b6c2"   # leading accent bar; default colors.accent
border_width  = 6           # bar thickness
padding       = 24
border_radius = 8
italic        = true        # render the quote text italic
align         = "center"    # left (default) | center | right — place the callout
```

| Key | Meaning |
|-----|---------|
| `background` | Fill behind the quote (default: none). |
| `border_color` | Leading accent bar; defaults to `colors.accent`. |
| `border_width` | Bar thickness, design units (default 4). |
| `padding` | Inner padding (default 16). |
| `border_radius` | Corner radius (default 0). |
| `italic` | Italicise the quote text (default false). |
| `align` | `left` / `center` / `right` — where the callout sits (default left). |

See [Quotes](../writing/text-tables.md#quotes) for the deck-side syntax.
