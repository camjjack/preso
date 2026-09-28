# preso for VS Code

Language support for [preso](https://github.com/camjjack/preso) markdown
decks:

- a slide outline, so you can jump to a slide by its title
- problems reported as you type, for directives preso would ignore
- completion for directives, their values, and image, video and include paths
- slide and layout actions (two columns, title and section slides, insert,
  duplicate, move and hide slides), each also a bindable command

The work is done by `preso-lsp`, the language server that comes with preso:
`brew install camjjack/preso/preso`, the release downloads and the `.deb`
all install it next to `preso`. The extension finds it on your `PATH`, or set
**preso: Server Path**.

See [*Editor Support*](https://camjjack.github.io/preso/getting-started/editor-support.html) in the preso guide for setup and key
bindings.
