#!/usr/bin/env bash
# Rear camera focus check: what libcamera's autofocus does on the scene in front of the rear camera,
# and where the raw image is sharpest when the lens is stepped through its range.
# usage: cam-af-check.sh TAG [OUTDIR]
# Needs a libcamera with userspace/libcamera/*.patch (cam in PATH), v4l-utils, python3 with numpy and
# pillow, and cam-focus-sweep built next to this script (cc -O2 -o cam-focus-sweep cam-focus-sweep.c).
# Point the rear camera at a scene with detail and hold it for 15 s. Writes OUTDIR/af-TAG.log (cam
# with autofocus trace and metadata), sweep-TAG.txt (contrast per lens position, up and down) and
# sweep-TAG-<position>.raw + .png (the sharpest frame of the sweep). The images show the room: keep
# OUTDIR in private/.
set -eu
tag=$1; out=${2:-.}
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
mkdir -p "$out"

node_of() { for d in /sys/class/video4linux/*; do case "$(cat "$d/name")" in "$1"*) echo "/dev/${d##*/}"; return ;; esac; done; echo "no $1" >&2; exit 1; }

echo "libcamera autofocus, 8 s"
LIBCAMERA_LOG_LEVELS="IPASoftIspAf:DEBUG" cam -c 1 -s width=1920,height=1080,pixelformat=XBGR8888 -C240 --metadata >"$out/af-$tag.log" 2>&1 || true
grep -E "Lens range|Scan from|Coarse peak|Focused at|Nothing to focus" "$out/af-$tag.log" | sed 's/^.*af.cpp:[0-9]* /  /'
grep -E "AfState|LensPosition" "$out/af-$tag.log" | tail -2 | sed 's/^\s*/  final /'

echo "raw sweep, 6 s (exposure and gain as libcamera left them)"
"$here/cam-raw.sh" rear "$out/.sweep-setup.raw" frames=2 >/dev/null
rm -f "$out/.sweep-setup.raw"
"$here/cam-focus-sweep" "$(node_of "Intel IPU6 ISYS Capture 8")" "$(node_of dw9719)" 350 850 25 "$out/sweep-$tag" >"$out/sweep-$tag.txt"
awk '/^up/ { printf "  %s:%.5f", $3, $7 } END { print "" }' "$out/sweep-$tag.txt"
tail -2 "$out/sweep-$tag.txt" | sed 's/^/  /'
raw=$(sed -n 's/^wrote \(.*\.raw\) .*/\1/p' "$out/sweep-$tag.txt")
python3 "$here/cam-raw-stats.py" 2112 1188 "$raw" --black 64 --png "${raw%.raw}.png" | sed 's/^/  /'
