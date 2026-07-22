# Migrating to preso

`preso-convert` turns a [Slidev](https://sli.dev) deck or a PowerPoint
(`.pptx`) file into preso markdown. It ships alongside `preso` and picks the
importer from the input's extension.

```sh
preso-convert slides.md -o talk.md      # write to a file
preso-convert slides.md                 # …or to stdout
preso-convert slides.md -o talk.md --force   # overwrite an existing file
preso-convert deck.pptx -o talk.md      # PowerPoint → preso
```

Anything that can't be represented in preso is reported as a warning on stderr
(silence with `--quiet`); the conversion always produces output, so you can
convert, skim the warnings, and clean up by hand.

## From Slidev

### What converts

| Slidev | preso |
|--------|-------|
| `---` slide separators | `---` |
| headmatter `title` / `transition` / `aspectRatio` | frontmatter `title` / `transition` / `aspect` |
| `layout: center` | `align=center halign=center` |
| `layout: cover` / `intro` | `kind=title` |
| `layout: section` | `kind=section` |
| `layout: statement` / `fact` | `kind=section`, centered |
| `layout: two-cols` + `::right::` | `<!-- layout: TwoColumn -->` + `***` |
| `layout: image-left` / `image-right` | a two-column layout with the image |
| `class: text-center` / `-right` / `-left` | `halign=…` |
| `background: #hex` | `background=#hex` |
| `<v-clicks>` / `<v-click>` | `<!-- pause -->` reveal steps |
| trailing `<!-- … -->` | `<!-- note: … -->` speaker notes |
| code `{2,4-6}` and `{2-3\|5\|all}` | unchanged (preso supports both) |
| `![](/img.png)`, `@/img.png` | `![](img.png)` (public/alias prefix stripped) |
| `<img src width>` | `![](src){width=…}` |

### What doesn't

These are browser-only and have no place in a native renderer, so they're
**stripped with a warning**: Vue components, UnoCSS classes, `<style>` blocks,
Monaco/twoslash, drawings, and iframes.

A few mappings are approximate and worth checking afterwards:

- A Slidev **theme** is an npm package; only `dark`/`light` carry over, anything
  else falls back to the default theme (with a warning) — pick or write a preso
  theme instead.
- **Transitions** map to preso's set: `fade`/`fade-out` → fade, directional
  `slide-*` → a [wipe](appendix/rendering.md#transitions); anything else falls
  back to fade (with a warning).
- **Image layouts** are approximated as two-column slides.

## From PowerPoint

A `.pptx` is a zip of XML. The importer reads the slides in presentation
order and pulls out their content — text, tables, images, and speaker notes.
PowerPoint is a freeform visual canvas, so expect to do layout work by hand
afterwards; text-heavy decks come across well.

### What converts

| PowerPoint | preso |
|------------|-------|
| slide order (from the presentation, not file names) | deck order |
| slide size | frontmatter `aspect` (e.g. `16:9`) |
| title placeholder | `## ` heading |
| Title Slide layout | `kind=title` with a `# ` heading |
| Section Header layout | `kind=section` with a `# ` heading |
| Two Content layout | `<!-- layout: TwoColumn -->` split at `***` |
| a hidden slide (PowerPoint's *Hide Slide*) | `<!-- slide: hidden -->` |
| subtitle placeholder | a paragraph under the title |
| body text, indented by outline level | nested `- ` bullets |
| **bold** / *italic* runs | `**bold**` / `*italic*` |
| tables | GitHub-flavoured markdown tables |
| images | extracted to disk and referenced with `![](…)` |
| speaker notes | `<!-- note: … -->` |
| markdown punctuation in text (`C#`, `a_b`, `*`) | escaped, so it renders verbatim |

A slide's **kind** and **two-column** split come from its PowerPoint *layout*,
so they're as reliable as the original deck's use of layouts. For a
two-column slide the title becomes a shared header band above the columns.
**Emphasis** is only emitted where a run *contrasts* with the rest of its
paragraph — a paragraph that's uniformly bold (often just the base style) is
left plain. A slide you'd marked **Hide Slide** in PowerPoint converts with a
`<!-- slide: hidden -->` directive: its content stays in the markdown but it
won't appear in the presentation or PDF (delete the directive to bring it
back).

**Images** are written into a `<output>.assets/` folder next to the markdown
(so `talk.md` → `talk.assets/`) and linked from the deck. This only happens
when you pass `-o`; writing to stdout leaves nowhere to put them, so they're
reported as warnings instead. Because PowerPoint positions everything
absolutely, extracted images are placed *after* each slide's text rather than
where they sat on the slide — move them where you want them.

### What doesn't

**Charts, SmartArt, embedded objects, and the WordArt look** of text are not
converted — each is reported as a per-slide warning so you know which slides
need attention. **Vector images** (EMF/WMF/SVG, common for pasted diagrams)
are skipped too, since preso renders raster images only. Footer, date, and
slide-number placeholders are dropped (preso draws its own). A slide with no
convertible content at all converts to an empty slide and is dropped on load;
the warning tells you which.

Positioning, fonts, colours, transitions, and animations don't carry over —
style the result with a preso [theme](theming/basics.md) instead.

## After converting

Open the result and review the warnings:

```sh
preso-convert slides.md -o talk.md
preso talk.md
```

The converter is built as an extensible rule pipeline; see its
[README](https://github.com/camjjack/preso/blob/main/crates/preso-convert/README.md)
to add mappings.

## Leaving preso (exporting to Slidev or PowerPoint)

Migration works in reverse too, so adopting preso isn't a one-way door:

```sh
preso-convert talk.md --to slidev -o slides.md
```

`<!-- include: … -->` chapters are assembled first, so the whole deck
exports. Content carries over cleanly — code line-highlights (`{2,4-6}`,
`{1|2|3}`), math, and Mermaid use the same syntax in Slidev; slide kinds
become `layout: cover` / `layout: section`, two-column slides become
`layout: two-cols` with `::right::`, reveal steps become `<v-click>`
blocks, and speaker notes become Slidev's trailing-comment notes.
preso-only styling (themes, image framing, footnotes, layer decorations)
has no Slidev equivalent and is dropped — each drop is reported as a
warning, like the import direction.

For an **editable PowerPoint** (text boxes, real tables, notes pages —
for people who will edit the deck in PowerPoint):

```sh
preso-convert talk.md --to pptx -o talk.pptx
```

Math and diagrams embed as images; themes don't translate (plain slides);
and since PowerPoint has no flow layout, block positions are estimated —
expect to nudge things. [Image highlights](writing/images.md#highlighting-parts-of-an-image)
map to real editable DrawingML shapes over the picture — a translucent
`fill` box/ellipse, an `under` patch behind the picture, or a `spotlight`
scrim with the picked regions cut out — so you can restyle or reposition
them in PowerPoint. (Two caveats: a `clip` wash can't follow the image's
alpha as a vector shape, and a spotlight over an `ellipse` region is cut as
a rectangle; both are reported.) For a pixel-faithful (non-editable) `.pptx`
of the deck exactly as presented, use
[`preso --export-pptx`](exporting.md) instead.
