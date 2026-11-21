#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# build-arch.sh -- build the Arch packages inside an archlinux container (root).
#
#   packaging/build-arch.sh SOURCE_TARBALL OUTDIR
#
# Takes packaging/arch/ from the tarball, fills in pkgver, _srcver and the tarball's checksum,
# builds as an unprivileged user (makepkg refuses root), runs namcap on the PKGBUILD and on every
# package, and copies to OUTDIR: the packages, the filled-in PKGBUILD with its install files and
# .SRCINFO (what an AUR package or a rebuild from the release takes). sp9-keyring is left out when
# packaging/keyring/sp9-archive-keyring.asc does not exist. NAMCAP=0 skips namcap.
set -eu
src=${1:?usage: build-arch.sh SOURCE_TARBALL OUTDIR}
out=${2:?usage: build-arch.sh SOURCE_TARBALL OUTDIR}
work=/build

pacman -Syu --noconfirm --needed base-devel namcap > /dev/null
id builder > /dev/null 2>&1 || useradd -m builder
rm -rf "$work"
install -d -o builder "$work"
tar -xJf "$src" -C "$work"
top=$(echo "$work"/sp9-linux-*)
srcver=$(cat "$top/VERSION")
# the same conversion as packaging/version.sh --arch
pkgver=$(echo "$srcver" | sed 's/+git\([0-9]*\)\./.r\1./; s/~git\([0-9]*\)\./pre.r\1./')
cp "$top"/packaging/arch/PKGBUILD "$top"/packaging/arch/*.install "$work/"
cp "$src" "$work/sp9-linux-$srcver.tar.xz"
sum=$(sha256sum "$src" | cut -d' ' -f1)
sed -i "s/^pkgver=.*/pkgver=$pkgver/; s/^_srcver=.*/_srcver=$srcver/; s/^sha256sums=.*/sha256sums=('$sum')/" "$work/PKGBUILD"
if [ ! -s "$top/packaging/keyring/sp9-archive-keyring.asc" ]; then
	echo "build-arch.sh: no repository key in packaging/keyring: sp9-keyring is not built"
	sed -i 's/^\(pkgname=(.*\) sp9-keyring)/\1)/' "$work/PKGBUILD"
fi
rm -rf "$top"
chown -R builder "$work"

# the build dependencies and the dependencies of every split package (check() runs the tests)
deps=$(cd "$work" && runuser -u builder -- makepkg --printsrcinfo | sed -n 's/^\t\(make\)\{0,1\}depends = //p' | grep -v '^sp9' | sed 's/[<>=].*//' | sort -u)
# shellcheck disable=SC2086
pacman -S --noconfirm --needed $deps > /dev/null

(cd "$work" && runuser -u builder -- makepkg --noconfirm --nodeps -f)
(cd "$work" && runuser -u builder -- makepkg --printsrcinfo > .SRCINFO)

if [ "${NAMCAP:-1}" != 0 ]; then
	echo "== namcap"
	# errors fail the build; warnings are printed
	(cd "$work" && namcap PKGBUILD && for p in *.pkg.tar.zst; do namcap "$p"; done) | tee "$work/namcap.log"
	! grep -q ' E: ' "$work/namcap.log" || { echo "build-arch.sh: namcap reported errors" >&2; exit 1; }
fi
mkdir -p "$out"
cp "$work"/*.pkg.tar.zst "$work/PKGBUILD" "$work/.SRCINFO" "$work"/*.install "$out/"
ls -l "$out"
