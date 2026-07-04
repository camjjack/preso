# Diagrams

preso renders [Mermaid](https://mermaid.js.org) and
[Graphviz](https://graphviz.org) diagrams from fenced code blocks — no external
tools, it's all built in.

## Mermaid

````markdown
```mermaid
flowchart LR
    A[Markdown] --> B[preso]
    B --> C[Audience window]
    B --> D[PDF]
```
````

![A Mermaid diagram](../images/diagram-mermaid.png)

## Graphviz

Use a `dot` fence for Graphviz:

````markdown
```dot
digraph {
    rankdir=LR
    Markdown -> preso -> Slides
}
```
````

![A Graphviz diagram](../images/diagram-graphviz.png)

## Sizing and transparent backgrounds

Both accept the same `{…}` annotation as other blocks:

- `{width=60%}` — size the diagram to a percentage of the content width.
- `{transparent}` — drop the diagram's light card and render straight onto the
  slide background. Especially useful on dark or gradient themes.

````markdown
```mermaid {width=70% transparent}
flowchart TD
    Start --> Stop
```
````

The two compose: `{width=60% transparent}` does both.
