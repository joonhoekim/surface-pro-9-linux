#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# source-tarball.sh -- the release source: the committed tree, a VERSION file and the helper's
# Rust dependencies vendored, so that every package builds from it without the network.
#
#   packaging/source-tarball.sh OUTDIR [TREEISH]     default TREEISH: HEAD
#
# Writes OUTDIR/sp9-linux-<version>.tar.xz (top directory sp9-linux-<version>/) and prints its
# path. The Debian packages use it as their orig tarball, the PKGBUILD as its source, and the
# release attaches it. DESCRIBE holds git describe for --version (empty for a release). Needs git, cargo (only for `cargo vendor`, which downloads the crates
# named in helper/Cargo.lock and checks them against it) and xz. Reproducible: file order,
# owners and times are fixed (times from the commit).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
top=$(cd "$here/.." && pwd)
out=${1:?usage: source-tarball.sh OUTDIR [TREEISH]}
tree=${2:-HEAD}

ver=$("$here/version.sh")
name=sp9-linux-$ver
epoch=$(git -C "$top" log -1 --format=%ct "$tree")
mkdir -p "$out"
out=$(cd "$out" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

git -C "$top" archive --format=tar --prefix="$name/" "$tree" | tar -xf - -C "$work"
echo "$ver" > "$work/$name/VERSION"
"$here/version.sh" --describe > "$work/$name/DESCRIBE"

# cargo writes the source replacement to stdout; it goes where cargo looks for it.
mkdir -p "$work/$name/helper/.cargo"
(cd "$work/$name/helper" && cargo vendor --locked --versioned-dirs vendor) > "$work/$name/helper/.cargo/config.toml" 2> "$work/vendor.log" ||
	{ cat "$work/vendor.log" >&2; exit 1; }
grep -q 'vendored-sources' "$work/$name/helper/.cargo/config.toml" || { echo "source-tarball.sh: cargo vendor wrote no source replacement" >&2; exit 1; }

tar -C "$work" --sort=name --mtime="@$epoch" --owner=0 --group=0 --numeric-owner \
	--pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime -cf - "$name" |
	xz -T1 -6 > "$out/$name.tar.xz"
echo "$out/$name.tar.xz"
