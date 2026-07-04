# Installation

preso is a single self-contained binary — fonts and the built-in themes are
baked in, so there's nothing else to install alongside it.

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

> 💡 **macOS Gatekeeper.** The binaries aren't notarized, so the first launch
> may be blocked. Right-click the binary → **Open**, or clear the quarantine
> flag with `xattr -d com.apple.quarantine /usr/local/bin/preso`.

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
