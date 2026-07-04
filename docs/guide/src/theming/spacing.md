# Spacing

The `[spacing]` section controls the slide margin and the gap between blocks,
both in [design units](basics.md#design-units):

```toml
[spacing]
slide_padding = 60
paragraph_gap = 20
```

| Key | Effect |
|-----|--------|
| `slide_padding` | The margin between slide content and the slide edge, on all sides. |
| `paragraph_gap` | Vertical space between blocks — paragraphs, lists, headings, code, and the gap after a heading. |

`slide_padding` is the content inset; chrome like the [logo and slide
number](chrome.md) has its own corner padding and isn't affected by it. An
[accent bar with `reserve = true`](chrome.md#accent-bars) adds to the padding on
its edge so content stays clear of the bar.

> 💡 Larger `slide_padding` gives a more spacious, minimal look; smaller values
> fit more content. `paragraph_gap` is the main lever for how "airy" body
> slides feel.
