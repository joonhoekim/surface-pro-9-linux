#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# build-arch.sh OUTDIR SP9_TREE BLUETOOTH_TREE -- build sp9-dkms and sp9-dkms-bluetooth
# (.pkg.tar.zst, arch any) with dkms/arch/PKGBUILD from trees written by `dkms/gen.sh tree` for
# the arch-* targets. Run as a user that may run makepkg (not root). The pkgver is the dkms version
# in pacman form, as packaging/version.sh --arch gives it (0.2.0+git5.gabc -> 0.2.0.r5.gabc).
set -euo pipefail
[ $# = 3 ] || { echo "usage: build-arch.sh OUTDIR SP9_TREE BLUETOOTH_TREE" >&2; exit 2; }
HERE=$(dirname "$(readlink -f "$0")"); DKMS=$(dirname "$HERE")
out=$(mkdir -p "$1" && readlink -f "$1"); main=$(readlink -f "$2"); bt=$(readlink -f "$3")
ver() { sed -n 's/^PACKAGE_VERSION="\(.*\)"/\1/p' "$1/dkms.conf"; }
v=$(ver "$main"); [ "$v" = "$(ver "$bt")" ] || { echo "the trees have different versions" >&2; exit 1; }
pv=$(echo "$v" | sed 's/+git\([0-9]*\)\./.r\1./; s/~git\([0-9]*\)\./pre.r\1./')
st=$(mktemp -d); trap 'rm -rf "$st"' EXIT
mkdir -p "$st/t"
cp -a "$main" "$st/t/sp9"; cp -a "$bt" "$st/t/sp9-bluetooth"; install -m 0755 "$DKMS/sp9-dkms-status" "$st/t/"
tar -C "$st/t" --owner=0 --group=0 --sort=name -czf "$st/sp9-dkms-trees-$v.tar.gz" .
sed -e "s/^pkgver=.*/pkgver=$pv/" -e "s/^_dkmsver=.*/_dkmsver='$v'/" "$DKMS/arch/PKGBUILD" >"$st/PKGBUILD"
(cd "$st" && PKGDEST="$out" makepkg -f --noconfirm --nodeps)
ls -l "$out"/*.pkg.tar.zst
