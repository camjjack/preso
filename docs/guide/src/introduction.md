# preso

**preso** is a native markdown presentation app. You write your talk in markdown and present it in a dual-window setup — a clean
**audience** window plus a **presenter** view with notes, a timer, and a
preview of what's coming next — or export it to PDF.

No browser, no Electron, no network — slides render on the GPU, with a software
rasterizer as a fallback, and PDF export renders identically on any machine.

```markdown
---
title: My Talk
theme: dark
---

# My Talk

A subtitle

---

## First point

- Built from plain markdown
- Presented natively
```

That's a complete two-slide deck. Open it with `preso my-talk.md` and you're
presenting.

## What you get

- **Plain-markdown decks** — slides separated by `---`, with YAML frontmatter.
- **Dual-window presenting** — a clean audience window and a presenter view
  (current slide, what's next, speaker notes, elapsed + countdown timers).
- **Themes** — TOML themes controlling colours, fonts, gradients, accent bars,
  logos, and slide numbers, with two built in (`dark`, `light`).
- **Rich content** — syntax-highlighted code (with line and click-through
  highlighting), tables, LaTeX math, and Mermaid / Graphviz diagrams.
- **Layouts & alignment** — two-column slides, title/section slide kinds, and
  vertical/horizontal content alignment.
- **Reveal steps** — build a slide up one piece at a time.
- **Annotation** — a laser pointer and pen drawing over the live slide.
- **PDF export** — one page per slide, one per reveal step, or a handout.

## How this guide is organised

- **[Getting Started](getting-started/installation.md)** — install preso, write
  your first deck, and learn to present.
- **[Writing Decks](writing/structure.md)** — every authoring feature, one page
  at a time.
- **[Theming](theming/basics.md)** — make decks look the way you want.
- **[Going Further](exporting.md)** — PDF export and migrating from Slidev or PowerPoint.
- **[Reference](reference/cli.md)** — the command line, keyboard shortcuts,
  deck directives, and the full theme schema.

New here? Start with **[Installation](getting-started/installation.md)**.
