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

iced has no per-widget opacity, so a transition can't fade live slide content.
For `fade` and `wipe`, preso captures the outgoing slide as a bitmap
(`window::screenshot`, which works on both renderers) and animates that image
— which *can* be faded and clipped — over the live incoming slide. The
`wipe-content` and `pan` transitions instead draw both slides live, clipped
or moved by the renderer. Set `transition:` in frontmatter:

- `fade` (alias `dissolve`) — a true cross-dissolve between the two slides.
- `wipe` — a directional reveal (the outgoing slide is clipped away from the
  edge). `cover` maps to this too.
- `wipe-content` — the same reveal, but only for what's *on* the slide: the
  background, accent bars and logo hold still while the text, images and
  footnote wipe across them, and the slide number switches at once. This one
  needs no screenshot — both slides' content is drawn live, each clipped to
  its side of the edge — so it runs from the very first slide change. Where
  the design itself changes between the two slides (into or out of a
  `kind=title` slide, or one with its own `background=`), there's no shared
  design to hold, and the whole slide wipes instead.
- `pan` — a camera gliding along a strip of slides laid side by side: the
  outgoing content slides off to the left as the next slide's slides in from
  the right, while the background, accent bars and logo stay put. Like
  `wipe-content` it's drawn live, runs from the first slide change, and moves
  the whole slide where the design changes. `pan-left` is the same; `pan-right`,
  `pan-up` and `pan-down` move the content the other ways (the name is the way
  it goes, as in Slidev). `slide` and `push` are pans too, and Slidev's
  `slide-left` / `slide-right` / `slide-up` / `slide-down` work as they are.
- `none` — instant cut.

A single slide can override the deck default for the change *into* it with
`<!-- slide: transition=wipe -->` (or any of the names above).

A transition belongs to the boundary between two slides. Moving on plays the
incoming slide's transition; going back plays the one the slide you're leaving
came in with, in reverse — so a slide that pans in from the right pans back out
to the right, and a `pan-up` retraces itself downwards.

Transitions animate on the **audience** window; the presenter view switches
instantly so you're never waiting on an effect (so on a single screen, watch
the audience window — or run `--audience-only` — to see them). Reveal steps
within a slide never transition — only slide changes do (a
[zoom](../writing/reveal-steps.md#zooming-the-slide) eases its camera instead). For `fade` and `wipe`,
the first slide change after launch has nothing to capture yet, so it cuts.

## Tables

preso renders markdown tables itself (rather than via the markdown widget) so
they can be [themed](../theming/elements.md#tables). This is why tables get
full header/stripe/border styling that the underlying widget couldn't provide.

