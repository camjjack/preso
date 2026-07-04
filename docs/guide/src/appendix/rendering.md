# Rendering Notes & Limitations

## Renderer

preso draws slides with iced's **wgpu (GPU)** backend by default, falling back
to a CPU software rasterizer (**tiny-skia**).

Both renderers are compiled in. Pass `--software` to force the tiny-skia path if
wgpu misbehaves on your GPU (it also makes [video](../writing/video.md) fall back
to an external player). An explicit `ICED_BACKEND=tiny-skia|wgpu` overrides both.
A binary built without default features is software-only.

PDF export always renders on tiny-skia — deterministic and identical on any
machine, no GPU required.

## Transitions

iced has no general per-widget opacity or transforms, so a transition can't
fade or move live slide content. preso works around this by capturing the
outgoing slide as a bitmap (`window::screenshot`, which works on both
renderers) and animating that image — which *can* be faded and clipped — over
the live incoming slide. Set `transition:` in frontmatter:

- `fade` (alias `dissolve`) — a true cross-dissolve between the two slides.
- `wipe` — a directional reveal (the outgoing slide is clipped away from the
  edge). `slide`, `push`, and `cover` map to this too: genuine sliding motion
  needs transforms the renderer doesn't have, so the closest honest effect is a
  wipe.
- `none` — instant cut.

A single slide can override the deck default for the change *into* it with
`<!-- slide: transition=wipe -->` (or `fade` / `none`).

Transitions animate on the **audience** window; the presenter view switches
instantly so you're never waiting on an effect (so on a single screen, watch
the audience window — or run `--audience-only` — to see them). Reveal steps
within a slide never transition — only slide changes do. The first slide change
after launch has nothing to capture yet, so it cuts.

## Tables

preso renders markdown tables itself (rather than via the markdown widget) so
they can be [themed](../theming/elements.md#tables). This is why tables get
full header/stripe/border styling that the underlying widget couldn't provide.

