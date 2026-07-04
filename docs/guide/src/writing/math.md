# Math

preso renders LaTeX math.

## Display math

A `$$ … $$` block renders on its own line, following the slide's alignment
(left by default). It can be on one line or span several:

```markdown
$$
x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}
$$
```

![Rendered math](../images/math.png)

Matrices, fractions, sums, Greek, and the usual LaTeX constructs all work:

```markdown
$$
\begin{pmatrix} a & b \\ c & d \end{pmatrix}
\begin{pmatrix} x \\ y \end{pmatrix}
=
\begin{pmatrix} ax + by \\ cx + dy \end{pmatrix}
$$
```

## Inline math

Wrap an expression in single `$` to set it inline with the text:

```markdown
The discriminant $b^2 - 4ac$ decides how many real roots there are.
```

Math is rendered in the theme's text colour, so it sits naturally in body
copy and headings.
