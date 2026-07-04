# Colors

The `[colors]` section is the theme's palette. All seven colours are required,
written as `#rrggbb` (or `#rrggbbaa` for alpha):

```toml
[colors]
background = "#1e1e2e"
text = "#cdd6f4"
heading = "#89b4fa"
accent = "#f5c2e7"
link = "#74c7ec"
muted = "#6c7086"
code_background = "#11111b"
```

| Colour | Used for |
|--------|----------|
| `background` | The slide background (when there's no gradient or image). |
| `text` | Body text, list items, table cells, math. |
| `heading` | Headings (`#`, `##`, `###`), unless overridden per level. |
| `accent` | Accent details — the default code-highlight tint, and a fallback for bars. |
| `link` | Hyperlinks. |
| `muted` | De-emphasised text — the default slide-number colour, table borders, image borders. |
| `code_background` | The panel behind code blocks (and the default table header fill). |

These are the *defaults* for each role. Many can be overridden in their own
section — heading colours per level in [`[heading]`](elements.md#headings),
the code-highlight tint in [`[code_block]`](elements.md#code-blocks), table and
image colours in [Element Styles](elements.md) — but the palette is what they
fall back to, so a coherent palette gets you most of the way.

> 💡 Syntax-highlighting colours inside code blocks come from `code_theme`
> (e.g. `base16-ocean.dark`), not the palette. See [Fonts](fonts.md) and the
> [Code Blocks](elements.md#code-blocks) styling.
