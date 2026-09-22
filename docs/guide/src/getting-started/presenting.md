# Presenting

preso opens two windows:

- The **audience window** — just the slide, nothing else. This is what you put
  on the projector or shared screen.
- The **presenter view** — the current slide plus everything you need to drive
  the talk.

The two stay in sync: navigating in either window moves both.

## The presenter view

The presenter view shows, around the current slide:

- **Next** — a preview of what the next press will reveal. While the current
  slide still has `<!-- pause -->` steps to go, this is the *current* slide at
  its next step (the bullet about to appear); once the slide is fully revealed,
  it's the next slide.
- **Speaker notes** — the notes attached to the current slide (and any
  step-specific notes for the current step). See
  [Speaker Notes](../writing/reveal-steps.md).
- **Timer** — elapsed time. It starts counting as soon as preso opens; press
  <kbd>r</kbd> at the real start of your talk to zero it (it keeps running).
- A **problem banner** — if the deck fails to parse on reload, the presenter
  view shows the error so you can fix it without losing your place.

### Presenter layouts

The presenter view has two layouts:

- **Slide** (the default) — the current slide fills the window, with the notes
  and the next preview in a strip beneath it.
- **Notes** — the speaker notes take the left of the window at full height, in
  type sized to fit (up to 32pt), and the current and next slides stack in a
  column on the right. For talks that lean on their notes, or a presenter
  screen you'll read from a step back.

A deck picks its layout in the frontmatter:

```markdown
---
title: My Talk
presenter: notes
---
```

Press <kbd>n</kbd> — or click the **Layout** label at the right of the status
line — to switch while presenting. The switch lasts for the session and
survives hot reloads, unless you edit the deck's `presenter:` line itself.

In either layout, a slide's step-specific notes follow the reveal: once you're
past a step, its note dims, so the note for the step on screen stands out.

### A countdown

Pass a talk length in minutes and the presenter view adds a countdown next to
the elapsed timer, warning you when five minutes remain:

```sh
preso talk.md --duration 30
```

## Two displays

With a second display connected, preso places itself automatically: the
audience window goes **fullscreen on the secondary** display and the presenter
view is **maximized on the primary**. Plug a display in (or unplug one)
mid-session and preso re-runs that placement. With a single display, both
windows open at their last-used size and position.

To rehearse on a laptop with no second screen, open only the audience window
with `--audience-only`.

## Driving the talk

| Key | Action |
|-----|--------|
| <kbd>→</kbd> <kbd>↓</kbd> <kbd>Space</kbd> <kbd>PageDown</kbd> | Next step / slide |
| <kbd>←</kbd> <kbd>↑</kbd> <kbd>Backspace</kbd> <kbd>PageUp</kbd> | Previous |
| <kbd>Home</kbd> / <kbd>End</kbd> | First / last slide |
| type a number then <kbd>Enter</kbd> | Jump to that slide |
| <kbd>Esc</kbd> | Toggle the slide overview grid |
| <kbd>f</kbd> | Toggle fullscreen (focused window) |
| <kbd>r</kbd> | Reset the timer |
| <kbd>n</kbd> | Switch the [presenter layout](#presenter-layouts) |

The **overview grid** (<kbd>Esc</kbd>) shows every slide as a thumbnail;
click one — or type its number and press <kbd>Enter</kbd> — to jump there.

See [Keyboard Shortcuts](../reference/keyboard.md) for the complete list.

## Annotating the slide

While presenting you can draw attention to the live audience slide:

| Key | Action |
|-----|--------|
| <kbd>l</kbd> | Toggle the laser pointer |
| <kbd>p</kbd> | Toggle pen drawing |
| <kbd>c</kbd> | Clear all annotations |

The laser follows your cursor over the slide; the pen lets you draw freehand.
Both appear on the audience window in real time.

---

That's everything you need to present. From here, **[Writing
Decks](../writing/structure.md)** covers the full authoring feature set, and
**[Theming](../theming/basics.md)** covers how to make a deck your own.
