# Slide Kinds

Every slide is one of three kinds: **normal** (the default), **title**, or
**section**. The kind selects a styling overlay in the theme, so title and
section slides can look distinct from body slides without per-slide formatting.

Set a slide's kind with the `kind=` directive:

```markdown
<!-- slide: kind=title -->

# preso

Native markdown presentations
```

![A title slide](../images/slide-kind-title.png)

```markdown
<!-- slide: kind=section -->

# Part Two
```

![A section slide](../images/slide-kind-section.png)

| Kind | Use it for |
|------|-----------|
| `title` | The opening slide — title, subtitle, author. |
| `section` | A divider that marks a new part of the talk. |
| *(none)* | Every other slide. |

## How kinds are styled

A theme styles each kind through `[title]` and `[section]` **overlays** — partial
themes layered on top of the base. A title slide might centre its content, use a
larger heading, hide the logo, and drop the slide number; a section slide might
just recolour the heading. Anything an overlay doesn't set falls back to the
base theme.

The built-in `dark` and `light` themes give title and section slides sensible
defaults out of the box. To customise them, see
[Slide-Kind Overlays](../theming/overlays.md).
