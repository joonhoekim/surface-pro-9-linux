#!/bin/bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
#
# Build the sp9-libcamera Arch package (experimental) from packaging/libcamera/arch/PKGBUILD.
# Runs inside archlinux:latest as root, from the repository root:
#
#   SP9_VERSION=0.2.0 packaging/libcamera/build-arch.sh OUTDIR
#
# pkgver is 0.8.0_sp9.<SP9_ARCH_VERSION>; SP9_ARCH_VERSION defaults to SP9_VERSION with '-' replaced
# by '.', or 0. makepkg runs as an unprivileged user; the sources are fetched and verified before the
# build. namcap checks the PKGBUILD and the package.

set -euo pipefail

OUTDIR=${1:?usage: $0 OUTDIR}
[[ $(id -u) == 0 ]] || { echo "run as root inside archlinux:latest" >&2; exit 1; }
[[ -d userspace/libcamera && -f packaging/libcamera/arch/PKGBUILD ]] ||
	{ echo "run from the repository root" >&2; exit 1; }

REPO=$(pwd)
PKGDIR=$REPO/packaging/libcamera
WORK=${SP9_WORKDIR:-/tmp/sp9-libcamera-arch}
if [[ -z ${SP9_ARCH_VERSION:-} && -n ${SP9_VERSION:-} ]]; then
	SP9_ARCH_VERSION=${SP9_VERSION//-/.}
fi
export SP9_ARCH_VERSION=${SP9_ARCH_VERSION:-0}
mkdir -p "$OUTDIR"
OUTDIR=$(cd "$OUTDIR" && pwd)

pacman -Syu --noconfirm --needed base-devel git namcap file
# shellcheck disable=SC1091
depends=() makedepends=()
# shellcheck source=arch/PKGBUILD
source "$PKGDIR/arch/PKGBUILD"
pacman -S --noconfirm --needed "${depends[@]}" "${makedepends[@]}"

# The plugin must come from the PipeWire release Arch ships.
pw_installed=$(pacman -Q pipewire | awk '{ print $2 }')
pw_installed=${pw_installed#*:}
pw_installed=${pw_installed%-*}
if [[ $pw_installed != "$_pwver" ]]; then
	echo "pipewire is $pw_installed, PKGBUILD builds the plugin from $_pwver:" >&2
	echo "set _pwver in packaging/libcamera/arch/PKGBUILD and run updpkgsums" >&2
	exit 1
fi

# Every patch of userspace/libcamera, and only those, is in the PKGBUILD.
want=$(cd userspace/libcamera && ls -1 [0-9]*.patch)
have=$(printf '%s\n' "${_patches[@]}")
if [[ $want != "$have" ]]; then
	echo "_patches in the PKGBUILD differ from userspace/libcamera/*.patch" >&2
	diff <(echo "$want") <(echo "$have") >&2 || true
	exit 1
fi

id builder >/dev/null 2>&1 || useradd --create-home --user-group builder
rm -rf "$WORK"
mkdir -p "$WORK"
cp "$PKGDIR/arch/PKGBUILD" "$PKGDIR/sp9-libcamera.conf.in" "$PKGDIR/pipewire-libcamera-0.8.sh" \
	userspace/libcamera/[0-9]*.patch "$WORK/"
chown -R builder:builder "$WORK"

run() { su builder -s /bin/bash -c "cd '$WORK' && SP9_ARCH_VERSION='$SP9_ARCH_VERSION' $*"; }
# Fetch and verify; the build that follows uses the downloaded trees only.
run makepkg --verifysource --noconfirm
run makepkg --noconfirm --holdver

echo "build-arch: namcap"
namcap "$WORK/PKGBUILD"
pkg=$(ls "$WORK"/sp9-libcamera-*.pkg.tar.zst)
namcap "$pkg"

cp "$pkg" "$OUTDIR/"
echo "build-arch: $OUTDIR/${pkg##*/}"
