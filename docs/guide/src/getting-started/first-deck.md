# Your First Deck

A deck is just a markdown file. Create `talk.md`:

```markdown
# Hello, preso

My first deck

---

## Why preso?

- It's just markdown
- It runs natively
- It exports to PDF
```

Two slides, separated by a line containing only `---`. Open it:

```sh
preso talk.md
```

Two windows appear: the **audience** window (the clean slide) and the
**presenter** view (the slide plus your tools). Press <kbd>→</kbd> or
<kbd>Space</kbd> to advance, <kbd>←</kbd> to go back. Press <kbd>f</kbd> to
fullscreen the focused window, and <kbd>Esc</kbd> to close.

> 💡 **Live reload.** Leave preso running and edit `talk.md` in your editor —
> it reloads the deck every time you save, keeping your place. Author with the
> windows open beside you.

## Add a title slide

The first slide of a talk usually wants a different look. Mark it as a *title*
slide with a comment directive, and give the deck a theme:

```markdown
---
theme: dark
---

<!-- slide: kind=title -->

# Hello, preso

My first deck

---

## Why preso?

- It's just markdown
- It runs natively
```

The block at the very top between `---` lines is **frontmatter** (deck-wide
settings). The `<!-- slide: kind=title -->` line tells the theme to style that
slide as a title. See [Slide Kinds](../writing/slide-kinds.md).

## Reveal one point at a time

Add a `<!-- pause -->` and the bullets after it appear on the next press:

```markdown
## Why preso?

- It's just markdown
<!-- pause -->
- It runs natively
- It exports to PDF
```

More on this in [Reveal Steps](../writing/reveal-steps.md).

## Rehearsing on a laptop

With only one screen, open just the audience window:

```sh
preso talk.md --audience-only
```

That's the essentials. Next, **[Presenting](presenting.md)** covers the
presenter view and every control; or jump into
**[Writing Decks](../writing/structure.md)** for the full authoring toolkit.
