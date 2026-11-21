#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# build-deb.sh OUTDIR SP9_TREE BLUETOOTH_TREE -- build sp9-dkms and sp9-dkms-bluetooth (.deb,
# Architecture: all) from trees written by `dkms/gen.sh tree` for the debian-* targets. Needs
# build-essential, dpkg-dev, debhelper and dh-dkms (Debian 12+, Ubuntu 24.04+). The version is the trees' one.
set -euo pipefail
[ $# = 3 ] || { echo "usage: build-deb.sh OUTDIR SP9_TREE BLUETOOTH_TREE" >&2; exit 2; }
HERE=$(dirname "$(readlink -f "$0")"); DKMS=$(dirname "$HERE")
out=$(mkdir -p "$1" && readlink -f "$1"); main=$(readlink -f "$2"); bt=$(readlink -f "$3")
ver() { sed -n 's/^PACKAGE_VERSION="\(.*\)"/\1/p' "$1/dkms.conf"; }
v=$(ver "$main"); [ "$v" = "$(ver "$bt")" ] || { echo "the trees have different versions" >&2; exit 1; }
grep -q '^PACKAGE_NAME="sp9"$' "$main/dkms.conf" && grep -q '^PACKAGE_NAME="sp9-bluetooth"$' "$bt/dkms.conf" ||
	{ echo "expected the sp9 tree, then the sp9-bluetooth tree" >&2; exit 1; }
st=$(mktemp -d); trap 'rm -rf "$st"' EXIT
s=$st/sp9-dkms-$v; mkdir -p "$s"
cp -a "$DKMS/debian" "$s/debian"
cp -a "$main" "$s/sp9"; cp -a "$bt" "$s/sp9-bluetooth"
install -m 0755 "$DKMS/sp9-dkms-status" "$s/sp9-dkms-status"
kernels=$(grep -hv '^#' "$main/kernels" | cut -f1 | paste -sd' ')
cat >"$s/debian/changelog" <<CL
sp9-dkms ($v) unstable; urgency=medium

  * Generated for linux-surface kernels: $kernels.

 -- Joonhoe Kim <26rote@gmail.com>  $(date -R ${SOURCE_DATE_EPOCH:+-d @$SOURCE_DATE_EPOCH})
CL
(cd "$s" && dpkg-buildpackage -b -us -uc)
mv "$st"/*.deb "$out"/
ls -l "$out"/*.deb
