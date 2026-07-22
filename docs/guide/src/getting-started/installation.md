# Installation

preso is a single self-contained binary — fonts and the built-in themes are
baked in, so there's nothing else to install alongside it.

## Homebrew (macOS & Linux) — recommended

On macOS (Apple Silicon) and Linux (x86_64), the easiest way to install preso
is from its Homebrew [tap](https://github.com/camjjack/homebrew-preso). A tap is
a third-party formula repository, so you add — and thereby trust — it once, then
install from it:

```sh
brew tap camjjack/preso     # one-time: add & trust the preso tap
brew install preso          # …or: brew install preso-video  (inline video)
```

`brew install camjjack/preso/preso` does both at once (the qualified name taps
implicitly). A few notes:

- **`preso-video`** compiles in [inline video](../writing/video.md) and declares
  GStreamer as a Homebrew dependency, so `brew` installs it for you — nothing
  else to set up for playback. `preso` and `preso-video` both provide the
  `preso` command, so install one or the other.
- **Coverage:** macOS is Apple Silicon only; Linux is x86_64 via
  [Linuxbrew](https://docs.brew.sh/Homebrew-on-Linux). On Windows, use a
  prebuilt binary or build from source.

Upgrade later with `brew upgrade preso` (or `preso-video`) — keeping the tap is
what lets `brew` find new releases.

## Prebuilt binaries

Download the archive for your platform from the
[Releases page](https://github.com/camjjack/preso/releases):

| Platform | Archive |
|----------|---------|
| macOS (Apple Silicon) | `preso-<version>-aarch64-apple-darwin.tar.gz` |
| Linux (x86_64) | `preso-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Windows (x86_64) | `preso-<version>-x86_64-pc-windows-msvc.zip` |

Unpack it and put the `preso` binary somewhere on your `PATH`:

```sh
tar xzf preso-*-aarch64-apple-darwin.tar.gz
sudo mv preso-*/preso /usr/local/bin/
preso --version
```

> 💡 **macOS Gatekeeper.** These manually-downloaded binaries aren't notarized,
> so the first launch may be blocked. Right-click the binary → **Open**, or
> clear the quarantine flag with
> `xattr -d com.apple.quarantine /usr/local/bin/preso`. (A Homebrew install
> avoids this — see above.)

## Build from source

You need a recent stable Rust toolchain (edition 2024, Rust ≥ 1.88).

```sh
git clone https://github.com/camjjack/preso
cd preso
cargo build --release
# binary at target/release/preso
```

### Linux build dependencies

preso's GPU backend (wgpu) loads its driver at runtime, so the build needs no
GPU or GTK dev packages — only the libraries it links for X11 clipboard support
and keyboard handling
(Debian/Ubuntu names):

```sh
sudo apt-get install -y \
  libxkbcommon-dev libxcb1-dev libxcb-render0-dev \
  libxcb-shape0-dev libxcb-xfixes0-dev
```

Wayland and X11 windowing are loaded at runtime, so they need no build-time
packages.

## Check it works

```sh
preso --version
```

Then head to **[Your First Deck](first-deck.md)**.
