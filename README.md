<p align="center">
  <img src="assets/preso.png" alt="preso logo" width="160">
</p>

# preso

A native markdown presentation app, written in Rust. Write your talk in a
single markdown file; present it in a dual-window setup (a clean **audience**
window plus a **presenter** view with notes, timer, and a preview of what's
next), or export it to PDF.

No browser, no Electron, no network. Slides render on the GPU (wgpu) by
default, with a software rasterizer (tiny-skia) fallback via `--software` for
machines where the GPU backend misbehaves.

## Features

- **Plain-markdown decks** — slides separated by `---`, with YAML frontmatter.
- **Dual-window presenting** — audience window + presenter view (current slide,
  next step/slide preview, speaker notes, elapsed + countdown timer).
- **Themes** — TOML themes (two built in: `dark`, `light`) controlling colours,
  fonts, gradients, accent bars, logos, background images, and slide numbers.
- **Slide kinds** — `title` / `section` / normal slides, each themable
  independently via `[title]` / `[section]` overlays.
- **Layouts** — two-column (`<!-- layout: TwoColumn 2:1 -->`) with per-slide
  ratios and aligned column bodies.
- **Alignment** — vertical (`align`) and horizontal (`halign`:
  left/centre/right) per-slide or per-theme.
- **Reveal steps** — `<!-- pause -->` builds a slide up incrementally.
- **Code** — syntax highlighting, line highlighting (` ```rust {2,4-6} `), and a
  "focus mode" that dims everything except the selected lines.
- **Diagrams & math** — Mermaid and Graphviz (with transparent backgrounds),
  plus LaTeX math via `$$ … $$`.
- **Images** — sizing, borders, and drop shadows (per-image or theme-wide);
  full-bleed cover backgrounds.
- **Annotation** — laser pointer and pen drawing over the live audience window.
- **Video** — mark a slide with `<!-- video: clip.mp4 -->`; plays inline on the
  slide with the `video` feature (GStreamer), or via a fullscreen external
  player otherwise.
- **PDF export** — one page per slide, one page per reveal step, or a 2-up
  handout layout. Fully headless (no window opened).

## Install / build

Prebuilt binaries for macOS (Apple Silicon), Linux (x86_64), and Windows
(x86_64) are attached to each tagged release on the
[Releases page](https://github.com/camjjack/preso/releases). The binary is
self-contained (fonts and the built-in themes are embedded), so just unpack
and run it.

Each release also ships a `-video` variant with inline video playback compiled
in (the `video` feature). Those link GStreamer, so they need it installed at run
time — see [Video](#video) for the per-platform install. The plain binaries have
no such dependency and fall back to an external player for video slides.

### Nix

The flake builds the **video** variant and pins GStreamer as a real
dependency, so inline playback works with nothing else installed:

```sh
nix run github:camjjack/preso -- talk.md      # or:
nix profile install github:camjjack/preso
```

To make preso part of a NixOS or home-manager configuration instead, add
the flake as an input and either apply the overlay (then it's `pkgs.preso`)
or reference the package directly:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    preso.url = "github:camjjack/preso";
    # Build preso with *your* nixpkgs instead of the flake's pin, so
    # GStreamer and the GUI libraries match the rest of your system:
    preso.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { nixpkgs, preso, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      modules = [
        # Overlay style — preso becomes a normal pkgs attribute:
        { nixpkgs.overlays = [ preso.overlays.default ]; }
        ({ pkgs, ... }: { environment.systemPackages = [ pkgs.preso ]; })
      ];
    };
  };
}
```

With home-manager it's the same overlay plus `home.packages = [ pkgs.preso ]`,
or skip the overlay entirely and use
`preso.packages.${pkgs.system}.default` wherever a package is expected.
(The overlay builds against your nixpkgs; the direct package uses the
flake's own pin unless you add the `follows` line.)

`nix develop` gives a shell with the C-level dependencies for
`cargo build --features video` (GStreamer, pkg-config), if you'd rather use
your own toolchain.

### From source

To build from source you need a recent stable Rust toolchain (edition 2024,
Rust ≥ 1.88).

```sh
git clone https://github.com/camjjack/preso
cd preso
cargo build --release
# binary at target/release/preso
```

On Linux, install the build dependencies first (Debian/Ubuntu names — these
cover X11 clipboard support and keyboard handling; no GTK packages are needed,
and the wgpu backend uses your existing Vulkan/GL drivers at runtime):

```sh
sudo apt-get install -y \
  libxkbcommon-dev libxcb1-dev libxcb-render0-dev \
  libxcb-shape0-dev libxcb-xfixes0-dev
```

## Usage

```sh
preso path/to/deck.md                  # present (dual window)
preso deck.md --theme light            # built-in theme by name
preso deck.md --theme themes/mine.toml # custom theme file
preso deck.md --audience-only          # single window (rehearsing on a laptop)
preso deck.md --duration 30            # 30-minute countdown in the presenter view

# Export (headless — no window is opened):
preso deck.md --export-pdf out.pdf            # one page per slide
preso deck.md --export-pdf out.pdf --export-steps  # one page per reveal step
preso deck.md --export-pdf out.pdf --export-2up    # 2-up handout
```

The renderer defaults to the GPU (wgpu) backend; both backends are compiled into
the standard binary and `--software` forces the tiny-skia software renderer at
runtime (use it if wgpu misbehaves on your GPU). Building without default
features (`cargo build --no-default-features`) drops wgpu for a software-only
binary.

### Keyboard controls

| Key | Action |
|-----|--------|
| `→` `↓` `Space` `PageDown` | Next step / slide |
| `←` `↑` `Backspace` `PageUp` | Previous |
| `Home` / `End` | First / last slide |
| digits then `Enter` | Jump to slide number |
| `Esc` | Toggle the slide overview grid |
| `f` | Toggle fullscreen |
| `v` | Play/pause the current slide's video |
| `r` | Reset the timer |
| `l` | Toggle laser pointer (audience window) |
| `p` | Toggle pen annotation |
| `c` | Clear annotations |

## Video

Mark a slide as playable with a comment (path relative to the deck file):

```markdown
## Live demo

![poster](demo-poster.png)

<!-- video: clips/demo.mp4 -->
```

The slide shows a ▶ badge and <kbd>v</kbd> plays the clip. How it plays depends
on the build:

- **Inline** (on the slide, with audio) — needs a binary built with the `video`
  feature and the wgpu backend (the default). The `-video` release artifacts and
  a `cargo build --release --features video` both provide this.
- **External** — the plain binaries (and any run with `--software`) hand the clip
  to a fullscreen external player: [`mpv`](https://mpv.io) if it's on your `PATH`
  (recommended), otherwise the OS default opener.

Inline playback decodes via GStreamer, so a `video` build needs the GStreamer
runtime installed to **build and run** — including for the prebuilt `-video`
binaries:

- **macOS** — `brew install gstreamer` (or the official framework from
  [gstreamer.freedesktop.org](https://gstreamer.freedesktop.org/download/)).
  Building from source also needs the dev files and pkg-config:
  `brew install gstreamer pkg-config`.
- **Linux (Debian/Ubuntu)** — the runtime + plugins:
  `sudo apt-get install -y gstreamer1.0-plugins-base gstreamer1.0-plugins-good`.
  Building from source additionally needs
  `libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev`.
- **Windows** — install the **MSVC runtime** package from
  [gstreamer.freedesktop.org/download](https://gstreamer.freedesktop.org/download/)
  (choose *complete* so the plugins are included). Building from source needs the
  **development** package plus `pkg-config` on `PATH`, and the env var
  `GSTREAMER_1_0_ROOT_MSVC_X86_64` pointing at the install root
  (e.g. `C:\Program Files\gstreamer\1.0\msvc_x86_64`).

The plain (non-`video`) binaries have no GStreamer dependency. See
[`docs/guide/src/writing/video.md`](docs/guide/src/writing/video.md) for the full
details.

## Writing a deck

````markdown
---
title: My Talk
theme: dark
---

<!-- slide: kind=title halign=center -->
# My Talk
## A subtitle

---

## A normal slide

- A point
- Another point
<!-- pause -->
- Revealed on the next step

<!-- note: remember to mention the demo here -->

---

<!-- layout: TwoColumn 2:1 -->

## Left column

```rust {2}
fn main() {
    println!("highlighted line");
}
```

## Right column

Text on the right.
````

See [`docs/example-talk.md`](docs/example-talk.md) for a full worked deck and
[`docs/themes/corporate.toml`](docs/themes/corporate.toml) for an annotated
theme.

## Project layout

| Crate | Responsibility |
|-------|----------------|
| `preso-core` | Markdown deck parser and document model |
| `preso-style` | TOML theme model and the built-in themes |
| `preso-diagram` | Mermaid / Graphviz / LaTeX-math rendering to images |
| `preso-export` | PDF page assembly |
| `preso-app` | The `preso` binary: iced GUI, presenter view, PDF export |

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. This is the standard Rust-ecosystem convention: you may use the
project under whichever of the two licenses suits you.
