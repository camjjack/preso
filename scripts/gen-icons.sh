#!/usr/bin/env bash
#
# Regenerate every icon derivative from the master logo
# (assets/preso-logo-512.png) into assets/icons/. Run after changing the
# logo; the outputs are committed, so builds and packaging never need this
# script or its tools.
#
# Produces:
#   preso-{16,24,32,48,64,128,256,512}.png  hicolor set (Linux packaging:
#                                           /usr/share/icons/hicolor/<N>x<N>/apps/)
#   preso.ico                               Windows (embedded by preso-app's
#                                           build.rs); all-PNG entries, valid
#                                           since Vista
#   preso.icns                              macOS (attached to the .app bundle
#                                           at packaging time)
#
# Tools: macOS built-ins only — sips, iconutil, python3 (packs the .ico).
set -euo pipefail

cd "$(dirname "$0")/.."
master="assets/preso-logo-512.png"
out="assets/icons"
mkdir -p "$out"

echo "==> PNG sizes (sips)"
for n in 16 24 32 48 64 128 256 512; do
  sips -z "$n" "$n" "$master" --out "$out/preso-$n.png" >/dev/null
done

echo "==> preso.ico (python3)"
# ICO container: ICONDIR header, one ICONDIRENTRY per image, then the raw
# PNG blobs. Width/height bytes of 0 mean 256.
python3 - "$out" <<'PY'
import struct, sys
out = sys.argv[1]
sizes = [16, 24, 32, 48, 256]
blobs = [open(f"{out}/preso-{n}.png", "rb").read() for n in sizes]
with open(f"{out}/preso.ico", "wb") as f:
    f.write(struct.pack("<HHH", 0, 1, len(sizes)))
    offset = 6 + 16 * len(sizes)
    for n, blob in zip(sizes, blobs):
        wh = 0 if n == 256 else n
        f.write(struct.pack("<BBBBHHII", wh, wh, 0, 0, 1, 32, len(blob), offset))
        offset += len(blob)
    for blob in blobs:
        f.write(blob)
PY

echo "==> preso.icns (iconutil)"
# No 512@2x slot: upscaling the 512 master to 1024 would only blur, and
# Apple tolerates the missing entry.
iconset="$(mktemp -d)/preso.iconset"
mkdir -p "$iconset"
cp "$out/preso-16.png" "$iconset/icon_16x16.png"
cp "$out/preso-32.png" "$iconset/icon_16x16@2x.png"
cp "$out/preso-32.png" "$iconset/icon_32x32.png"
cp "$out/preso-64.png" "$iconset/icon_32x32@2x.png"
cp "$out/preso-128.png" "$iconset/icon_128x128.png"
cp "$out/preso-256.png" "$iconset/icon_128x128@2x.png"
cp "$out/preso-256.png" "$iconset/icon_256x256.png"
cp "$out/preso-512.png" "$iconset/icon_256x256@2x.png"
cp "$out/preso-512.png" "$iconset/icon_512x512.png"
iconutil -c icns "$iconset" -o "$out/preso.icns"
rm -rf "$(dirname "$iconset")"

echo "==> Done:"
ls -la "$out"
