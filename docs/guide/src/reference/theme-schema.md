# Theme Schema

Every theme key, its type, and its default. Colours are `#rrggbb` (or
`#rrggbbaa`); sizes are [design units](../theming/basics.md#design-units).
Unknown keys are rejected. For prose and examples, see
[Theming](../theming/basics.md).

## Top level

| Key | Type | Default |
|-----|------|---------|
| `name` | string | *required* |
| `code_theme` | string | *required* — a highlighter theme, e.g. `base16-ocean.dark` |

## `[colors]` — all required

| Key | Role |
|-----|------|
| `background` | Slide background (no gradient/image) |
| `text` | Body text, tables, math |
| `heading` | Headings |
| `accent` | Accent; default code-highlight tint |
| `link` | Links |
| `muted` | De-emphasised text; default number/border colour |
| `code_background` | Code panel; default table header fill |

## `[fonts]`

| Key | Type | Default |
|-----|------|---------|
| `body_size` `h1_size` `h2_size` `h3_size` `code_size` | number | *required* |
| `body_family` | string | bundled **Inter** |
| `heading_family` | string | `body_family` |
| `code_family` | string | bundled **JetBrains Mono** |
| `files` | string[] | `[]` (font files, relative to the theme) |

## `[spacing]` — all required

| Key | Type |
|-----|------|
| `slide_padding` | number |
| `paragraph_gap` | number |

## `[slide]`

| Key | Type | Default |
|-----|------|---------|
| `background_image` | string (path) | none |
| `gradient` | table (below) | none |
| `bar` | table (below) | none |
| `bars` | array of bar tables | `[]` |
| `logo` | table (below) | none |
| `align` | `top` \| `center` | `top` |
| `halign` | `left` \| `center` \| `right` | `left` |

**`gradient`** `= { from, to, angle }` — `from`/`to` required; `angle` degrees,
default `180` (`0` = `to` at top).

**`bar`** (and each entry of `bars`):

| Key | Type | Default |
|-----|------|---------|
| `side` | `top`\|`bottom`\|`left`\|`right` | `left` |
| `size` | number | `12` |
| `color` | colour | `#000000` |
| `reserve` | bool | `false` |
| `hidden` | bool | `false` |

**`logo`**:

| Key | Type | Default |
|-----|------|---------|
| `path` | string | *(none)* |
| `width` | number (% of slide width) | `10` |
| `position` | corner | `bottom-right` |
| `opacity` | number 0–1 | `1.0` |
| `padding_x` `padding_y` | number | `24` |
| `border` | border table (see `[image]`) | none |
| `shadow` | shadow (see `[image]`) | none |
| `hidden` | bool | `false` |

Corners: `top-left`, `top-right`, `bottom-left`, `bottom-right`.

## `[heading]`

| Key | Type | Default |
|-----|------|---------|
| `color` | colour | `colors.heading` |
| `h1_color` `h2_color` `h3_color` | colour | `color` |

## `[code_block]`

| Key | Type | Default |
|-----|------|---------|
| `background` | colour | `colors.code_background` |
| `border_radius` | number | `8` |
| `padding` | number | `code_size` |
| `padding_top` `padding_right` `padding_bottom` `padding_left` | number | `padding` |
| `highlight_color` | colour | `accent`, faint |
| `highlight_style` | `background` \| `dim` | `background` |
| `dim_opacity` | number 0–1 | `0.35` |

## `[table]`

| Key | Type | Default |
|-----|------|---------|
| `header_background` | colour | `colors.code_background` |
| `header_color` | colour | `colors.heading` |
| `stripe_background` | colour | none (no striping) |
| `border_color` | colour | `colors.muted`, faint |
| `border_width` | number | `1` |
| `padding` | number | `10` |
| `padding_top` `padding_right` `padding_bottom` `padding_left` | number | `padding` |
| `border_radius` | number | `0` |

## `[image]`

| Key | Type | Default |
|-----|------|---------|
| `border` | `{ color, width, radius }` | none |
| `shadow` | `true` or `{ color, offset, blur }` | none |

**`border`**: `color` default `colors.muted`, `width` default `3`, `radius`
default `8`. **`shadow`**: `offset = [x, y]` default `[0, 8]`, `blur` default
`24`.

## `[quote]`

Styling for blockquote (`> …`) callouts.

| Key | Type | Default |
|-----|------|---------|
| `background` | colour | none |
| `border_color` | colour | `colors.accent` |
| `border_width` | number | `4` |
| `padding` | number | `16` |
| `padding_top` `padding_right` `padding_bottom` `padding_left` | number | `padding` |
| `border_radius` | number | `0` |
| `italic` | bool | `false` |
| `align` | `left` \| `center` \| `right` | `left` |

## `[slide_number]`

Omit the whole section for no slide number.

| Key | Type | Default |
|-----|------|---------|
| `format` | string (`{current}`, `{total}`) | `{current}` |
| `size` | number | `24` |
| `font` | string | body font |
| `position` | corner | `bottom-right` |
| `color` | colour | `colors.muted` |
| `padding_x` `padding_y` | number | `24` |
| `hidden` | bool | `false` |

## `[footnote]`

Styles the `<!-- footnote: … -->` disclaimer line. Optional — the directive,
not this section, decides which slides show one.

| Key | Type | Default |
|-----|------|---------|
| `size` | number | `18` |
| `font` | string | body font |
| `align` | `left` \| `center` \| `right` | `left` |
| `color` | colour | `colors.muted` |
| `padding_x` | number | `60` |
| `padding_y` | number | `36` |
| `hidden` | bool | `false` |

## `[title]` / `[section]` — kind overlays

Partial themes for title/section slides. Any section above can appear nested —
`[title.colors]`, `[title.slide]`, `[section.heading]`, `[title.slide_number]`,
and so on — plus `code_theme`. Set fields win; unset fields inherit from the
base theme. See [Slide-Kind Overlays](../theming/overlays.md).
