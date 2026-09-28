# Reveal Steps & Speaker Notes

## Reveal steps

Split a slide into steps with `<!-- pause -->` on its own line. Each press
reveals the next chunk; content is cumulative.

```markdown
## Why preso?

- It's just markdown
<!-- pause -->
- It runs natively
<!-- pause -->
- It exports to PDF
```

That slide takes three presses to reveal fully. `<!-- v-click -->` is accepted
as a synonym for `<!-- pause -->` if you're coming from Slidev.

Reveal steps appear instantly — except a zoom, whose camera eases to its new
place. Moving between slides runs the deck's
[transition](../appendix/rendering.md#transitions), if one is set
(`transition:` in frontmatter; the default is `none`).

> 💡 A multi-stage [code highlight](code.md#click-through-highlighting) also
> advances on each press, in step with your pauses.

## Zooming the slide

`<!-- zoom[n]: x%,y%,Nx -->` zooms the whole slide's content from step `n`: the
point `x%` across and `y%` down the slide moves to the middle, magnified `N`
times. `<!-- zoom[n]: all -->` zooms back out. Like
[`highlight[n]`](images.md#stepping-through-highlights), each one adds its step
to the slide:

```markdown
## Architecture

![](architecture.png)

<!-- zoom[1]: 25%,40%,2x -->
<!-- zoom[2]: 75%,60%,3x -->
<!-- zoom[3]: all -->
```

Only the content zooms — the background, accent bars, logo and slide number
hold still. A zoom that would show past the slide's edge is held inside it, so
`10%,10%,2x` shows the top-left quarter. `<!-- zoom: … -->` without a step
applies from the start. It's the escape hatch for anything a
[code](code.md#zooming-onto-lines) or [diagram](diagrams.md#zooming-onto-nodes)
zoom can't name — an image, a table, a corner of the slide.

## Speaker notes

Attach notes to a slide with `<!-- note: … -->`. They appear in the presenter
view only, never on the audience window.

```markdown
## Quarterly results

- Revenue up 24%

<!-- note: Pause here for the revenue question; the chart is on the next slide. -->
```

A note can span multiple lines — everything up to the closing `-->` is the
note:

```markdown
<!-- note:
Remember to:
- thank the team
- tease the roadmap
-->
```

`<!-- speaker: … -->` is an accepted synonym for `<!-- note: … -->`.

### Step-specific notes

Number a note to show it only from a given reveal step onward.
`<!-- note[n]: … -->` appears once step `n` is reached (steps count from the
first `<!-- pause -->`, so `note[1]` shows after the first pause):

```markdown
- It's just markdown
<!-- pause -->
- It runs natively

<!-- note: Opening line. -->
<!-- note[1]: Now mention the native rendering. -->
```

Once the talk moves past a step, the presenter view dims that step's note, so
the one for the step on screen stands out. Notes without a step number stay
at full strength throughout.

Notes and `pause` markers are stripped from the rendered slide; they never show
on the audience window.
