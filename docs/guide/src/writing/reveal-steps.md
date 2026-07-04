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

Reveal steps never animate — each press appears instantly. Only moving between
slides runs the deck's [transition](../appendix/rendering.md#transitions), if one
is set (`transition:` in frontmatter; the default is `none`).

> 💡 A multi-stage [code highlight](code.md#click-through-highlighting) also
> advances on each press, in step with your pauses.

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

Notes and `pause` markers are stripped from the rendered slide; they never show
on the audience window.
