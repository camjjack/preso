# A Complete Theme

Below is a complete, annotated theme that exercises most of preso's styling —
a gradient background, an accent bar, a logo, a slide number, element styles,
and `[title]` / `[section]` overlays. Use it as a starting point: copy it, point
the `path` values at your own assets, and adjust to taste.

It's the `corporate.toml` shipped in the repo (`docs/themes/`), included here
verbatim so it never drifts from a working theme.

```toml
{{#include ../../../themes/corporate.toml}}
```

To use a theme file, pass it on the command line or name it in frontmatter:

```sh
preso talk.md --theme corporate.toml
```

See [Theme Basics](basics.md#choosing-a-theme) for how themes are resolved, and
the [Theme Schema](../reference/theme-schema.md) for every available key.
