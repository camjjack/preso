# Alignment

Control where a slide's content sits — vertically and horizontally — with the
`align` and `halign` directives. Both apply to the whole slide (heading, body,
bullets together).

```markdown
<!-- slide: align=center halign=center -->

# A centered statement
```

| Directive | Values | Default |
|-----------|--------|---------|
| `align` | `top`, `center` | `top` |
| `halign` | `left`, `center`, `right` | `left` |

- **`align`** sets the *vertical* position: `top` (content starts at the top) or
  `center` (content is centered in the slide).
- **`halign`** sets the *horizontal* alignment of every content block.

```markdown
<!-- slide: halign=right -->

## Right-aligned

- Bullets
- pushed to the right edge
```

## Per-slide vs theme-wide

The directives above set alignment for one slide. To make it the default for the
whole deck — or just for title/section slides — set `align` / `halign` in the
theme's `[slide]` section (or a `[title.slide]` / `[section.slide]` overlay). A
per-slide directive always wins over the theme. See
[Slide Chrome](../theming/chrome.md) and
[Slide-Kind Overlays](../theming/overlays.md).

> 📝 Centre and right alignment align each top-level block to the full slide
> width. A multi-line paragraph becomes a centered block of left-aligned lines,
> which reads well for titles and short bullets.
