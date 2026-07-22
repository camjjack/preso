# preso-convert

Convert a [Slidev](https://sli.dev) deck or a PowerPoint (`.pptx`) file into
preso markdown — or export a preso deck back out to Slidev or editable
PowerPoint. The importer is
chosen from the input's extension; `--to` selects an exporter instead.

```sh
preso-convert talk.md -o talk.preso.md     # write to a file
preso-convert talk.md                       # or to stdout
preso-convert talk.md -o out.md --force     # overwrite existing
preso-convert deck.pptx -o talk.md          # PowerPoint → preso
preso-convert talk.md --to slidev -o slides.md   # preso → Slidev
preso-convert talk.md --to pptx -o talk.pptx      # preso → editable PowerPoint
```

Anything that can't be represented in the target format is reported as a
warning on stderr (silence with `--quiet`); the conversion always produces
output. The Slidev exporter assembles `<!-- include: … -->` chapters first,
passes content (code line-highlights, math, Mermaid share Slidev's syntax)
through unchanged, and maps directives: slide kinds → `layout: cover` /
`section`, TwoColumn + `***` → `two-cols` + `::right::`, reveal steps →
`<v-click>`, notes → Slidev's trailing-comment notes. preso-only styling
(themes, image framing, footnotes, layer decorations) is dropped with
warnings.

`--to pptx` produces *editable* PowerPoint (contrast `preso --export-pptx`,
which is pixel-faithful pictures): headings/paragraphs/bullets become text
runs, tables become PowerPoint tables, notes become notes pages, images
embed, and math/Mermaid/Graphviz render to embedded PNGs. Slides are plain
(themes don't translate), and PowerPoint has no flow layout, so block
positions are estimated — expect to nudge things after a handoff.

## From Slidev: what converts

| Slidev | preso | Notes |
|--------|-------|-------|
| `---` slide separators | `---` | unchanged |
| deck headmatter `title` | `title:` | |
| deck `theme` | `theme:` | only `dark`/`light`; npm themes warn |
| deck `transition` | `transition:` | preso renders all as a fade |
| deck `aspectRatio: 16/9` | `aspect: "16:9"` | |
| `layout: center` | `align=center halign=center` | |
| `layout: cover`/`intro` | `kind=title` | |
| `layout: section` | `kind=section` | |
| `layout: statement`/`fact` | `kind=section` + centered | |
| `layout: two-cols` + `::right::` | `<!-- layout: TwoColumn -->` + `***` | |
| `layout: image-left`/`image-right` | TwoColumn with the image column | approximated |
| `class: text-center`/`-right`/`-left` | `halign=…` | other classes dropped |
| `background: #hex` | `background=#hex` | image backgrounds warn |
| `<v-clicks>` around a list | `<!-- pause -->` between items | |
| `<v-click>` block | `<!-- pause -->` before block | timing attrs warn |
| trailing `<!-- … -->` | `<!-- note: … -->` | speaker notes |
| code `{2,4-6}` | `{2,4-6}` | unchanged |
| code `{2-3\|5\|all}` | `{2-3\|5\|all}` | click-through stages pass through (preso supports them) |
| `![](/img.png)`, `@/img.png` | `![](img.png)` | public/alias prefix stripped |
| `<img src width>` | `![](src){width=…}` | |

Not converted (warned and stripped/left): Vue components, UnoCSS classes,
`<style>` blocks, Monaco/twoslash, drawings, iframes — these are browser-only
and have no place in a native renderer.

## From PowerPoint: what converts

A `.pptx` is a zip of Office Open XML. The importer reads `presentation.xml`
for slide order (resolved through the package relationships — the `slideN.xml`
numbers do *not* match presentation order) and slide size, then extracts each
slide's text, tables, images, and notes, using its `slideLayout` to infer the
slide's kind and column structure.

| PowerPoint | preso | Notes |
|------------|-------|-------|
| slide order via `<p:sldIdLst>` | deck order | not file-name order |
| `<p:sldSz>` | `aspect: "W:H"` | reduced ratio |
| layout `type="title"` (or `ctrTitle` ph) | `kind=title` + `# ` heading | |
| layout `type="secHead"` | `kind=section` + `# ` heading | |
| layout `type="two…"` | `<!-- layout: TwoColumn -->` + `***` | by placeholder `idx` |
| `<p:sld show="0">` (Hide Slide) | `<!-- slide: hidden -->` | kept in source, dropped on load |
| `title` placeholder | `## ` heading | |
| `subTitle` placeholder | paragraph under the title | |
| body paragraphs, `<a:pPr lvl>` | nested `- ` bullets | indent per level |
| `<a:rPr b/i>` runs | `**bold**` / `*italic*` | contrast-only (see below) |
| `<a:tbl>` in a `<p:graphicFrame>` | GFM table | first row is the header |
| `<p:pic>` (via `r:embed` → rels) | extracted file + `![](…)` | see below |
| linked notes slide body | `<!-- note: … -->` | |
| markdown punctuation in runs | escaped | renders verbatim |
| `sldNum`/`ftr`/`dt` placeholders | dropped | preso draws its own |

**Emphasis** is emitted only where a run's formatting *contrasts* with the
rest of its paragraph; a uniformly bold/italic paragraph is the base style,
not inline emphasis, so it's left plain. Adjacent same-format runs (PowerPoint
splits words across runs freely) are coalesced first.

**Images** are written to `<output-stem>.assets/` beside the output file and
linked from the deck (so `-o talk.md` → `talk.assets/imageN.png`). With no
`-o` (stdout) there's nowhere to write them, so they're reported as warnings
instead. Because PowerPoint positions shapes absolutely, extracted images are
appended after each slide's text rather than placed where they sat.

Not converted (reported as per-slide warnings): charts, SmartArt, and
embedded objects (other `<p:graphicFrame>` content), vector images
(EMF/WMF/SVG — preso renders raster images only), plus positioning, fonts,
colours, transitions, and animations. A slide with no convertible content
becomes an empty slide (dropped on load, with a warning).

## Architecture / extending

The pipeline is a list of [`Rule`]s, each owning one concern. A `Rule`
mutates a shared `SlideCtx` (the unconsumed frontmatter, the body, the
directives/notes to emit, and a warnings list). To support a new mapping —
whether a new Slidev feature or a new preso capability it can target — add a
rule and register it in `default_rules()`:

```rust
pub struct MyRule;
impl Rule for MyRule {
    fn apply(&self, ctx: &mut SlideCtx) {
        // consume a frontmatter key with ctx.take("foo"),
        // rewrite ctx.body, push ctx.set_override(...) / ctx.layout,
        // and ctx.warn(...) for anything lossy.
    }
}
```

Frontmatter rules run first (emitting directives), then body rules, then the
leftover-frontmatter warning. Each rule has focused unit tests alongside it.
