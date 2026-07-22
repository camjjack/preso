# Deck Directive Cheat Sheet

Everything you can write in a deck, at a glance. Directives are HTML comments on
their own line; they're invisible in other markdown viewers.

## Structure

| Syntax | Meaning |
|--------|---------|
| `---` | Slide separator (own line, outside code fences). |
| frontmatter | First `---`…`---` block: `title`, `theme`, `transition`, `aspect`. |
| `<!-- include: path.md -->` | Splice in another markdown file ([split a deck across files](../writing/structure.md#splitting-a-deck-across-files)). |

## Per-slide

| Directive | Meaning |
|-----------|---------|
| `<!-- slide: kind=title \| section -->` | [Slide kind](../writing/slide-kinds.md). |
| `<!-- slide: align=top \| center -->` | [Vertical alignment](../writing/alignment.md). |
| `<!-- slide: halign=left \| center \| right -->` | [Horizontal alignment](../writing/alignment.md). |
| `<!-- slide: background=#rrggbb -->` | Solid per-slide background colour. |
| `<!-- slide: background=path.jpg -->` | [Full-bleed background image](../writing/images.md#full-bleed-backgrounds). |
| `<!-- image: path … -->` | [Positioned image](../writing/images.md#positioned-images-behind-the-text) on a layer behind the text (`position`/`width`/`opacity`/`padding`). |
| `<!-- highlight: rect\|ellipse x= y= w= h= … -->` | [Highlight a region](../writing/images.md#highlighting-parts-of-an-image) of the next image (`color`/`opacity`/`stroke`; coordinates in % of the image). |
| `<!-- highlight: … spotlight -->` | [Spotlight mode](../writing/images.md#spotlight-mode): dim everything *except* the region (also `mode=spotlight`). |
| `<!-- highlight: … under -->` | [Under mode](../writing/images.md#under-mode-transparent-images): solid shape *behind* the image — shows through transparent pixels (also `mode=under`/`behind`). |
| `<!-- highlight: … clip -->` | [Clip to the image](../writing/images.md#clipping-to-a-transparent-image): confine a fill/spotlight wash to the image's opaque pixels, sparing the transparent background. |
| `<!-- slide: hidden -->` | Drop the slide — it appears in neither the presentation nor the PDF. |
| `<!-- slide: number=N -->` | Reset the slide number to `N`; later slides continue from there. |
| `<!-- slide: transition=fade\|wipe\|none -->` | [Transition](../appendix/rendering.md#transitions) for the change *into* this slide, overriding the deck default. |
| `<!-- layout: TwoColumn -->` | [Two columns](../writing/two-columns.md), split at `***`. |
| `<!-- layout: TwoColumn 2:1 -->` | Two columns with a left:right ratio. |

Multiple keys can share one `slide` directive:
`<!-- slide: kind=title halign=center -->`. `hidden` is a bare flag and can
sit alongside the others (`<!-- slide: kind=section hidden -->`).

## Reveal & notes

| Directive | Meaning |
|-----------|---------|
| `<!-- pause -->` | Start a new [reveal step](../writing/reveal-steps.md). |
| `<!-- highlight[n]: … -->` | [Image highlight](../writing/images.md#stepping-through-highlights) shown from reveal step `n` onward (mints steps by itself). |
| `<!-- v-click -->` | Synonym for `<!-- pause -->`. |
| `<!-- note: … -->` | [Speaker note](../writing/reveal-steps.md#speaker-notes) (may span lines). |
| `<!-- note[n]: … -->` | Note shown from reveal step `n` onward. |
| `<!-- speaker: … -->` | Synonym for `<!-- note: -->`. |
| `<!-- footnote: … -->` | [Footnote/credit line](../writing/images.md#crediting-images) along the slide's bottom. |
| `<!-- table: size=NN -->` | [Cell font size](../writing/text-tables.md#shrinking-a-table-to-fit) for the next table, design units. |
| `<!-- video: clip.mp4 -->` | Mark the slide [playable](../writing/video.md): a ▶ badge shows, <kbd>v</kbd> launches an external fullscreen player. |
| `***` | Column separator inside a `TwoColumn` slide. |

## Fenced-block annotations

After the language on a code/diagram fence, in `{…}`:

| Annotation | Applies to | Meaning |
|------------|-----------|---------|
| `{2,4-6}` | code | [Highlight](../writing/code.md#line-highlighting) lines 2 and 4–6. |
| `{2\|5\|all}` | code | [Click-through](../writing/code.md#click-through-highlighting) stages; `all`/`none` clear. |
| `{size=NN}` | code | [Code font size](../writing/code.md#font-size) for this block, design units. |
| `{dim}` / `{background}` | code | [Override the highlight style](../writing/code.md#focus-mode) for this block. |
| `{align=left\|center\|right}` | code | [Horizontal placement](../writing/code.md#panel-width-and-alignment) of the block. |
| `{width=NN%}` | diagrams, code, images | Size to a percentage of the content width (`width=100%` = full width for a code block). |
| `{align=left\|center\|right}` | images | [Horizontal position](../writing/images.md#sizing-and-framing) (default `left`). |
| `{transparent}` | diagrams | [Drop the light card](../writing/diagrams.md#sizing-and-transparent-backgrounds). |

## Inline

| Syntax | Meaning |
|--------|---------|
| `![alt](path){width=NN% align=center border shadow plain fit}` | [Image](../writing/images.md) with sizing/alignment/framing (`fit` packs an image row at the images' actual widths instead of sharing the width equally). |
| `==text==` | [Highlighted text](../writing/text-tables.md#highlighted-text): a marker background behind the words (themable via `colors.mark`). |
| `$$ … $$` | [Display math](../writing/math.md). |
| `$ … $` | Inline math. |
| `` ```mermaid `` / `` ```dot `` | [Mermaid / Graphviz diagrams](../writing/diagrams.md). |
