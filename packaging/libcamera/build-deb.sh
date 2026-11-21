#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
#
# Build the sp9-libcamera .deb (experimental): libcamera 0.8.0 with userspace/libcamera/*.patch in
# /usr/lib/sp9-libcamera and PipeWire's libcamera SPA plugin, built from the PipeWire source package
# of the target distribution, against it.
#
# Runs inside the target container (debian:13, debian:testing, ubuntu:26.04, ubuntu:26.10) as root,
# from the repository root:
#
#   SP9_VERSION=0.2.0 DEB_SUFFIX=deb13 DEB_CODENAME=trixie packaging/libcamera/build-deb.sh OUTDIR
#
# Package version: 0.8.0+sp9.${SP9_VERSION}-1~${DEB_SUFFIX}. On trixie the script enables
# trixie-backports and builds against its PipeWire (1.6). Every download happens before the build;
# the build itself does not use the network (meson runs with --wrap-mode=nodownload).
# SP9_LINTIAN=0 skips the lintian run at the end.

set -eu

LIBCAMERA_URL=https://git.libcamera.org/libcamera/libcamera.git
LIBCAMERA_TAG=v0.8.0
LIBCAMERA_COMMIT=df8e348d64b4932a0676256ed24f0da3ee9438bc

OUTDIR=${1:?usage: $0 OUTDIR}
: "${SP9_VERSION:?set SP9_VERSION}" "${DEB_SUFFIX:?set DEB_SUFFIX}" "${DEB_CODENAME:?set DEB_CODENAME}"
[ "$(id -u)" = 0 ] || { echo "run as root inside the build container" >&2; exit 1; }
[ -d userspace/libcamera ] && [ -d packaging/libcamera/debian ] ||
	{ echo "run from the repository root" >&2; exit 1; }

REPO=$(pwd)
PKGDIR=$REPO/packaging/libcamera
VERSION=0.8.0+sp9.${SP9_VERSION}-1~${DEB_SUFFIX}
WORK=${SP9_WORKDIR:-/tmp/sp9-libcamera-deb}
TREE=$WORK/sp9-libcamera-0.8.0
mkdir -p "$OUTDIR"
OUTDIR=$(cd "$OUTDIR" && pwd)

export DEBIAN_FRONTEND=noninteractive

# Source lists: deb-src for the PipeWire source package; trixie-backports on trixie.
for f in /etc/apt/sources.list.d/*.sources; do
	[ -f "$f" ] && sed -i 's/^Types: deb$/Types: deb deb-src/' "$f"
done
APT_TARGET=
if [ "$DEB_CODENAME" = trixie ]; then
	cat > /etc/apt/sources.list.d/trixie-backports.sources <<-EOF
	Types: deb deb-src
	URIs: http://deb.debian.org/debian
	Suites: trixie-backports
	Components: main
	Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
	EOF
	APT_TARGET="-t trixie-backports"
fi
apt-get update
apt-get install -y --no-install-recommends ca-certificates git dpkg-dev
# The PipeWire version to build the plugin from is the one this system installs.
# shellcheck disable=SC2086
apt-get install -y --no-install-recommends $APT_TARGET libspa-0.2-modules
PW_SRCVER=$(dpkg-query -W -f='${source:Version}' libspa-0.2-modules)
PW_SRCPKG=$(dpkg-query -W -f='${source:Package}' libspa-0.2-modules)
echo "build-deb: PipeWire source $PW_SRCPKG $PW_SRCVER"

# Fetch.
rm -rf "$WORK"
mkdir -p "$WORK"
git clone --quiet --depth 1 --branch "$LIBCAMERA_TAG" "$LIBCAMERA_URL" "$TREE"
got=$(git -C "$TREE" rev-parse HEAD)
[ "$got" = "$LIBCAMERA_COMMIT" ] ||
	{ echo "libcamera $LIBCAMERA_TAG is $got, expected $LIBCAMERA_COMMIT" >&2; exit 1; }
rm -rf "$TREE/.git"
(cd "$WORK" && apt-get source --download-only "$PW_SRCPKG=$PW_SRCVER")
dpkg-source -x "$WORK"/"${PW_SRCPKG}"_*.dsc "$TREE/pipewire-src"

# Assemble the source tree: debian/ and the patches as a quilt series.
cp -a "$PKGDIR/debian" "$TREE/debian"
cp "$PKGDIR/sp9-libcamera.conf.in" "$PKGDIR/pipewire-libcamera-0.8.sh" "$TREE/debian/"
mkdir -p "$TREE/debian/patches"
for p in "$REPO"/userspace/libcamera/[0-9]*.patch; do
	cp "$p" "$TREE/debian/patches/"
	basename "$p" >> "$TREE/debian/patches/series"
done
DATE=$(git -C "$REPO" log -1 --format=%cD 2>/dev/null || date -R)
cat > "$TREE/debian/changelog" <<EOF
sp9-libcamera ($VERSION) $DEB_CODENAME; urgency=medium

  * libcamera 0.8.0 with the Surface Pro 9 patches of surface-pro-9-linux
    $SP9_VERSION, and the libcamera SPA plugin of PipeWire $PW_SRCVER.

 -- Joonhoe Kim <26rote@gmail.com>  $DATE
EOF

# Build dependencies (the backports PipeWire is not one of them: the plugin builds from pipewire-src/).
apt-get build-dep -y --no-install-recommends "$TREE"

# Build.
(cd "$TREE" && dpkg-buildpackage -b -us -uc)

DEB=$WORK/sp9-libcamera_${VERSION}_amd64.deb
CHANGES=$WORK/sp9-libcamera_${VERSION}_amd64.changes
[ -f "$DEB" ] || { echo "no $DEB" >&2; exit 1; }

if [ "${SP9_LINTIAN:-1}" != 0 ]; then
	apt-get install -y --no-install-recommends lintian
	echo "build-deb: lintian"
	lintian --info --display-info --fail-on error "$CHANGES"
fi

cp "$DEB" "$CHANGES" "$WORK"/sp9-libcamera_"${VERSION}"_amd64.buildinfo "$OUTDIR/"
echo "build-deb: $OUTDIR/${DEB##*/}"
