# Slide-Kind Overlays

[Title and section slides](../writing/slide-kinds.md) are styled by `[title]`
and `[section]` **overlays** — partial themes layered on top of the base. An
overlay can set any of the same sections (`colors`, `fonts`, `slide`,
`heading`, …), and **anything it doesn't set is inherited** from the base
theme.

```toml
# Base theme applies to all slides…
[slide]
bar = { side = "right", size = 24, color = "#f9a8d4" }

[fonts]
h1_size = 80
# …

# …and title slides override just these bits:
[title.fonts]
h1_size = 110

[title.slide]
align = "center"
halign = "center"
logo = { hidden = true }

[title.slide_number]
hidden = true

[section.heading]
color = "#f9a8d4"
```

Here, title slides get a bigger `h1`, centered content, no logo, and no slide
number — but keep the base colours, body font, and everything else. Section
slides only recolour the heading.

## How merging works

- Sub-sections merge **field by field**: `[title.colors] heading = "#fff"`
  changes only the heading colour; the rest of the palette is inherited.
- `gradient`, `background_image`, `bar`, and `logo` replace the base value when
  set. To **remove** an inherited bar or logo for a kind, set
  `{ hidden = true }`:

  ```toml
  [title.slide]
  logo = { hidden = true }
  bar = { hidden = true }
  ```

- An overlay that sets `bars` (the [multiple-bars](chrome.md#accent-bars) list)
  replaces the inherited single `bar`.

The nesting works for every section, so `[title.code_block]`, `[section.table]`,
`[title.spacing]`, and so on are all valid.

## Per-slide vs overlay

Overlays are how a *kind* looks across the deck. To restyle a single slide, use
its [per-slide directives](../writing/structure.md#per-slide-directives) — those
always win over both the overlay and the base theme.
