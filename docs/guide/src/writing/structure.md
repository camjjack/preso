# Deck Structure & Frontmatter

A deck is one markdown file: optional **frontmatter** at the top, then
**slides** separated by `---`.

```markdown
---
title: My Talk
theme: dark
transition: fade
aspect: "16:9"
---

# First slide

---

## Second slide
```

## Slide separators

A line containing only `---` (three or more dashes) starts a new slide. It must
be on its own line and outside a code fence — a `---` inside a fenced code block
is left alone, so you can show markdown that contains `---` without splitting
the slide.

## Frontmatter

The block between the first pair of `---` lines is YAML **frontmatter** — deck-
wide settings. All fields are optional:

| Field | Meaning |
|-------|---------|
| `title` | Window/PDF title. |
| `theme` | A built-in theme (`dark`, `light`) or a path to a `.toml` theme. See [Theme Basics](../theming/basics.md). |
| `transition` | Slide transition: `fade` (cross-dissolve), `wipe` (directional reveal), or `none`. `dissolve` is an alias of `fade`; `slide`/`push`/`cover` map to `wipe`. See [Rendering Notes](../appendix/rendering.md#transitions). |
| `aspect` | Slide aspect ratio, e.g. `"16:9"` or `"4:3"`. |

Unknown keys are ignored, so a deck written for another tool still loads.

> 💡 The `--theme` command-line flag overrides the frontmatter `theme`, which is
> handy for trying a deck under different themes without editing it.

## Per-slide directives

Individual slides are configured with HTML-comment directives — invisible in
any other markdown viewer, so a preso deck still reads cleanly on GitHub:

```markdown
<!-- slide: kind=title halign=center -->

# A centered title slide
```

The directives are introduced in the pages that follow:

- `<!-- slide: … -->` — per-slide [kind](slide-kinds.md),
  [alignment](alignment.md), and background.
- `<!-- layout: TwoColumn -->` — [two-column layouts](two-columns.md).
- `<!-- pause -->` — [reveal steps](reveal-steps.md).
- `<!-- note: … -->` — [speaker notes](reveal-steps.md#speaker-notes).

A full list is in the [Directive Cheat Sheet](../reference/directives.md).

## Hiding a slide

Mark a slide `hidden` to drop it from the deck — it shows up in neither the
presentation nor the exported PDF, and doesn't count toward the slide total:

```markdown
<!-- slide: hidden -->

# Draft I'm not ready to show
```

The slide stays in the file (so it's easy to bring back — just remove the
flag), but is removed as the deck is parsed. `hidden` is a bare flag and can
share a directive with other keys, e.g. `<!-- slide: kind=section hidden -->`.

## Resetting the slide number

Slide numbers count only the slides preso shows, so a deck with hidden slides
ends up numbered differently from one that counted them. To realign with such
a deck, set an explicit number on a slide and let the rest follow:

```markdown
<!-- slide: number=50 -->

# This slide is "50"
```

The counter jumps to `50` here and continues (`51`, `52`, …); `{total}`
becomes the highest number reached. Use it wherever the two decks drift apart.

## Splitting a deck across files

A long talk can be split into chapter files and pulled together by a master
deck with `<!-- include: path -->` (the path is relative to the file doing the
including):

```markdown
---
title: My Talk
theme: dark
---

<!-- slide: kind=title -->
# My Talk

---

<!-- include: chapters/intro.md -->

---

<!-- include: chapters/deep-dive.md -->
```

Each chapter file is a normal deck fragment — its own slides separated by `---`.
The include directive is replaced by the file's contents before parsing, so a
chapter contributes however many slides it contains. Separate consecutive
includes with `---`, just as you would any slides.

Notes:

- Includes can nest (a chapter can include sub-chapters); paths resolve relative
  to each file. Circular includes are detected and reported.
- A chapter file may keep its **own frontmatter** so it previews standalone with
  `preso chapters/intro.md` — that leading frontmatter is dropped when included,
  so only the master deck's frontmatter (theme, title, …) applies.
- Editing a chapter file [hot-reloads](../getting-started/first-deck.md) the deck
  just like editing the master.

