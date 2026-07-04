#!/usr/bin/env bash
#
# Build and package this machine's release artifacts locally — the
# no-Actions-hours fallback for release.yml. Run it on each OS (macOS,
# Linux, Windows Git Bash); artifact names and contents match the CI
# job exactly, so the Homebrew formula generator and .deb consumers see
# no difference.
#
# Usage: scripts/release-local.sh <tag> [--skip-video]
#   <tag>          the release tag the artifacts are for (e.g. v0.1.0)
#   --skip-video   skip the `--features video` variant (it needs the
#                  GStreamer development libraries to build)
#
# Artifacts land in dist/. Collect every machine's dist/ together, then
# create the release from any of them:
#   gh release create <tag> --draft --generate-notes dist/*
# (or upload through the GitHub web UI). Linux additionally produces the
# preso/preso-video .debs (requires cargo-deb).
set -euo pipefail

cd "$(dirname "$0")/.."

tag="${1:?usage: release-local.sh <tag> [--skip-video]}"
skip_video=0
[ "${2:-}" = "--skip-video" ] && skip_video=1

host="$(rustc -vV | awk '/^host:/ {print $2}')"
exe=""
case "$host" in *windows*) exe=".exe" ;; esac
mkdir -p dist

if [ -n "$(git status --porcelain)" ]; then
  echo "warning: working tree is dirty; artifacts won't match the tag." >&2
fi

# Package the freshly built target/release binaries as
# dist/preso-<tag>-<host><suffix>.{tar.gz,zip}. The standard archive
# carries preso-convert; the video one must not (CI builds them on
# separate runners, so its archive never has the leftover binary —
# locally target/release keeps it from the standard build).
package() {
  local suffix="$1" with_convert="$2"
  local name="preso-${tag}-${host}${suffix}"
  local bin="target/release"
  rm -rf "$name"
  mkdir "$name"
  cp "$bin/preso$exe" "$name/"
  [ "$with_convert" = yes ] && cp "$bin/preso-convert$exe" "$name/"
  # No CHANGELOG.md: it's private-only and absent from the public repo.
  cp README.md LICENSE-MIT LICENSE-APACHE "$name/"
  if [ -n "$exe" ]; then
    # Windows' bsdtar picks the zip format from the extension with -a.
    tar -a -cf "dist/$name.zip" "$name"
  else
    tar czf "dist/$name.tar.gz" "$name"
  fi
  rm -rf "$name"
  echo "    dist/$name${exe:+.zip}${exe:-.tar.gz}"
}

echo "==> Standard build ($host)"
cargo build --release --bin preso --bin preso-convert
package "" yes

if [ "$skip_video" = 0 ]; then
  echo "==> Video build ($host)"
  cargo build --release --features video --bin preso
  package "-video" no
fi

# Debian packages, Linux only (requires cargo-deb: `cargo install cargo-deb`).
case "$host" in
  *linux*)
    echo "==> Debian packages"
    cargo deb -p preso-app
    if [ "$skip_video" = 0 ]; then
      cargo deb -p preso-app --variant video
    fi
    cp target/debian/*.deb dist/
    ;;
esac

echo "==> Done. Artifacts in dist/:"
ls -l dist/
