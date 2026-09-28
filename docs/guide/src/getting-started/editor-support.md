# Editor Support

preso decks are plain markdown, so any editor works. `preso-lsp`, a language
server, makes your editor preso-aware:

- **Slide outline.** Each slide is listed by its first heading, with the
  slide number, kind and layout beside it. It drives the editor's Outline
  panel, breadcrumbs and *Go to Symbol* (<kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>O</kbd>),
  so you can jump to a slide by its title. Each slide also folds.
- **Problems as you type.** preso's parser forgives mistakes on purpose, so a
  deck never fails on stage. The cost is that a misspelt directive gets
  quietly ignored. The server reports the same mistakes while you write:
  - `<!-- notes: … -->` for `note:`, and a `note[x]:` whose step isn't a
    number
  - `layout: twocolumn`
  - a `TwoColumn` slide with no `***` to split it
  - unknown `slide:` options or values
  - highlights, images and zooms the parser would drop, and a highlight
    with no image after it to draw on
  - code block options the parser ignores (`{sise=20}`, `width=50` without
    the `%`)
  - a code block or diagram with `zoom` but no stages to zoom onto, or
    highlight stages naming lines past the end of the block
  - a diagram zoom stage naming a node the diagram doesn't have (the server
    draws the diagram to find out)
  - a `$$` math block that's never closed, which would swallow the rest of
    the slide
  - a frontmatter YAML error, and `transition:`, `presenter:` or `aspect:`
    values preso can't use
  - images, videos, includes and themes that don't exist

  Where the fix is obvious, a quick fix offers it.
- **Completion.** Type `<!--` to get whole directives. It completes the
  options and values of `slide:`, `highlight:`, `image:`, `table:` and
  `zoom:`, the frontmatter's keys and values, and a code block's language and
  `{…}` options. Inside `![](…)`, `video:`, `image:`, `include:`,
  `background=` and `theme:` it completes file paths, offering only formats
  preso can show. In a diagram's `{…}`, it completes the diagram's node
  labels for zoom stages.
- **Hover.** Rest the pointer on a directive, an option, a transition name, a
  frontmatter key or a code block flag to see what it does.
- **Clickable paths.** Image, video, include and theme paths open the file.
- **Slide and layout actions.** These change slide structure with one command
  (see [below](#slide-and-layout-actions)).

The server only ever edits your editor's buffer. The change shows up unsaved
and is undone with the editor's own undo. It never writes the deck file
itself.

## Installing the server

The server, `preso-lsp`, comes with preso. However you
[installed preso](installation.md), you have it:

| Installed with | Where `preso-lsp` is |
|---|---|
| Homebrew (`preso` or `preso-video`) | Next to `preso`, on your `PATH` |
| A release archive (`.tar.gz` / `.zip`) | In the archive next to `preso`; put both on your `PATH` |
| The `.deb` | `/usr/bin/preso-lsp` |
| Nix | In the same package as `preso` |
| From source | `cargo install --path crates/preso-lsp` |

Check it with `preso-lsp --version`. The editor extensions look for it on
your `PATH`; each has a setting for a binary elsewhere (below). It's the same
version as preso, so upgrading preso upgrades the server too.

## VS Code

Download `preso-vscode-<version>.vsix` from the
[Releases page](https://github.com/camjjack/preso/releases) and install it:

```sh
code --install-extension preso-vscode-*.vsix
```

Or, in VS Code, open the Extensions view, choose **… → Install from VSIX…**,
and pick the file. Use the extension from the same release as your preso.

It starts for every markdown file. If `preso-lsp` isn't on your `PATH`, set
**preso: Server Path** (`preso.server.path`). If the server can't be started,
the extension says so, with a link back to this page.

To build the extension yourself instead, from a preso checkout:

```sh
cd editors/vscode
npm ci
npx @vscode/vsce package --skip-license   # preso is dual-licensed; see LICENSE-*
code --install-extension preso-*.vsix
```

## Zed

The extension isn't in Zed's extension registry yet, so it installs from a
checkout of the preso repository, where it lives in `editors/zed`. Open the
command palette and run **zed: install dev extension**, then choose that
directory. Zed compiles the extension itself, which needs Rust installed
through `rustup`.

If `preso-lsp` isn't on your `PATH`, point Zed at it in `settings.json`:

```json
{ "lsp": { "preso-lsp": { "binary": { "path": "/path/to/preso-lsp" } } } }
```

If your settings pin Markdown's language servers, add `preso-lsp` to the
list. A list there replaces Zed's default of running every server, so
without it the server never starts:

```json
{ "languages": { "Markdown": { "language_servers": ["codebook", "preso-lsp"] } } }
```

## Other editors

Any editor with an LSP client works. Run `preso-lsp` for markdown files; it
talks over stdio. For Neovim 0.11 and later:

```lua
vim.lsp.config('preso', { cmd = { 'preso-lsp' }, filetypes = { 'markdown' } })
vim.lsp.enable('preso')
```

For Helix, in `languages.toml`, add `preso` to markdown's servers:

```toml
[language-server.preso]
command = "preso-lsp"

[[language]]
name = "markdown"
language-servers = ["preso"]
```

## Slide and layout actions

With the cursor in a slide, open the editor's code actions:
<kbd>Cmd</kbd>+<kbd>.</kbd> in VS Code and Zed, `vim.lsp.buf.code_action()`
in Neovim. Layout changes are listed as **Layout: …** and slide operations as
**Slide: …**:

| Action | Kind | What it writes |
|---|---|---|
| Layout: two columns | `refactor.preso.layout.twoColumns` | `<!-- layout: TwoColumn -->` and a `***` split (see below) |
| Layout: single column | `refactor.preso.layout.singleColumn` | Removes both |
| Layout: title / section / normal slide | `refactor.preso.kind.title` / `.section` / `.normal` | Sets or clears `kind=` in `<!-- slide: … -->` |
| Insert image: *file* | `refactor.preso.insert.image` | `![alt](file)` as a paragraph below the one the cursor is in |
| Background image: *file* | `refactor.preso.insert.background` | `background=file` in `<!-- slide: … -->`: a full-bleed image |
| Insert video / Replace video: *file* | `refactor.preso.insert.video` | `<!-- video: file -->`, or swaps the slide's clip |
| Slide: insert a new slide after | `refactor.preso.slide.insertAfter` | `---` and a `# New slide` heading |
| Slide: duplicate | `refactor.preso.slide.duplicate` | A copy after it |
| Slide: move up / down | `refactor.preso.slide.moveUp` / `.moveDown` | Swaps it with its neighbour |
| Slide: hide / unhide | `refactor.preso.slide.hide` / `.unhide` | Sets or clears `hidden` |

Each action writes ordinary preso markdown, the same text you'd type by hand,
and changes only the lines it has to.

### Styling what's under the cursor

With the cursor on an image, in a table or in a code block, the menu starts
with styles for that element. They appear only there, so the menu stays
short everywhere else:

| On… | Offered | Writes |
|---|---|---|
| **An image** | width 25 / 50 / 75 / 100%, centre, align left or right, add or remove border and shadow, `plain`; in an image row, `fit`; remove styling | The image's `{…}` group: `![Logo](logo.png){width=50% align=center shadow}` |
| **A table** | text size 28 / 24 / 20 or normal; centre, or align left or right, the column the cursor is in | `<!-- table: size=NN -->` above the table; the column's `:---:` in the delimiter row |
| **A code block** | highlight (or stop highlighting) the line the cursor is on; width 50 / 75 / 100%; centre, align left or right; with click-through stages, zoom onto each stage's lines (or stop) | The fence's `{…}`: ` ```rust {2,4-6 width=100%} `, ` ```rust {all\|2\|4-6 zoom} ` |
| **A diagram** | width; transparent background on or off; with stages, zoom onto the nodes they name (or stop) | ` ```mermaid {width=75% transparent} ` |

Line highlighting isn't offered on a block with click-through stages
(`{1|2-3|all}`), because its lines differ from step to step.

Inside an image's or a fence's `{…}`, completion lists the attributes too.
An image group with an attribute preso doesn't know (`{width=50% boarder}`)
is ignored as a whole and shows as text on the slide. It's reported, with a
fix when the intended attribute is obvious.

**Table structure is left to your editor.** Adding rows and columns and
lining up the pipes is ordinary markdown, and editors already do it well:
Zed formats markdown tables with Prettier, and in VS Code the *Markdown All
in One* extension does. preso-lsp only adds what preso itself brings to a
table, which is its text size and the column alignment it renders.

### Inserting images and video

An editor can't show a file picker from this menu. Instead it offers the
**newest images and videos in the deck's folder that the file doesn't use
yet**: up to three images and two videos, newest first. So the quick way to
add a picture is to save the screenshot into the deck's folder (say
`assets/`), put the cursor where it belongs, and press
<kbd>Cmd</kbd>+<kbd>.</kbd>. It's at the top of the inserts.

The alt text comes from the file name (`net-diagram.png` becomes
`![net diagram](assets/net-diagram.png)`). The path is written relative to
the top-level deck, which is how preso resolves it, even in a chapter.

For any other file, type `![](` (or `<!-- video: `) and pick it from the
completion list.

A file whose name contains spaces or parentheses isn't offered, because
markdown can't hold that path as written. Rename it first.

### Where two columns split

**Layout: two columns** is offered on every single-column slide. The heading
stays on the left, where preso shows it across both columns. The rest splits
like this:

- **At the cursor**, if it's on a later bullet of a list or a later
  paragraph, image or code block. The menu then reads *Layout: two columns,
  split here*.
- **Otherwise, automatically:**
  - an image that follows the text moves to the right
  - a single list splits at its middle bullet
  - several blocks split in half
  - a slide with nothing to split gains an empty right column for you to
    fill

**Layout: single column** puts it back the way it was.

### Binding actions to keys

**VS Code.** Each action is also a command (**preso: Layout: two columns**,
and so on), so you can bind it in `keybindings.json`:

```json
[
  { "key": "ctrl+alt+2", "command": "preso.twoColumns" },
  { "key": "ctrl+alt+i", "command": "preso.insertImage" },
  { "key": "ctrl+alt+up", "command": "preso.moveSlideUp" },
  { "key": "ctrl+alt+down", "command": "preso.moveSlideDown" }
]
```

**Neovim.** Filter the code actions by kind:

```lua
vim.keymap.set('n', '<leader>p2', function()
  vim.lsp.buf.code_action({
    apply = true,
    filter = function(a) return a.kind == 'refactor.preso.layout.twoColumns' end,
  })
end)
```

**Zed.** Zed can't yet bind a key to one specific code action. Reach them
through the code-action menu (<kbd>Cmd</kbd>+<kbd>.</kbd>).

## Chapters and includes

A chapter pulled in with `<!-- include: … -->` resolves its image paths
against the **top-level deck's** folder, not its own. The server does the
same: it looks up to two folders above the chapter for the deck that
includes it. A path that works relative to the chapter itself (for
previewing the chapter on its own) is accepted too.

The outline is per file. In a chapter, slide numbers count from the start of
that chapter, not the whole talk.
