#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# build-deb.sh -- build the sp9-linux binary packages inside a Debian or Ubuntu container (root).
#
#   packaging/build-deb.sh [deps|build|all] SOURCE_TARBALL [OUTDIR]
#
#   deps   install the build dependencies and unpack the source into /build (needs the network)
#   build  build, run lintian and copy the packages to OUTDIR (no network needed)
#   all    both (default)
#
# Environment: DEB_SUFFIX (e.g. deb13, ubuntu26.04) and DEB_CODENAME (e.g. trixie), from
# packaging/targets.tsv. The package version is <release version>-1~<suffix>.
# LINTIAN=0 skips lintian. The keyring package is built only when
# packaging/keyring/sp9-archive-keyring.asc exists (build profile nokeyring otherwise).
set -eu
phase=all
case ${1:-} in deps | build | all) phase=$1; shift ;; esac
src=${1:?usage: build-deb.sh [deps|build|all] SOURCE_TARBALL [OUTDIR]}
out=${2:-}
: "${DEB_SUFFIX:?set DEB_SUFFIX}" "${DEB_CODENAME:?set DEB_CODENAME}"
export DEBIAN_FRONTEND=noninteractive
work=/build

unpack() {
	rm -rf "$work"
	mkdir -p "$work"
	tar -xJf "$src" -C "$work"
	dir=$(echo "$work"/sp9-linux-*)
	ver=$(cat "$dir/VERSION")
	cp -r "$dir/packaging/debian" "$dir/debian"
	ln -sf "$(cd "$(dirname "$src")" && pwd)/$(basename "$src")" "$work/sp9-linux_$ver.orig.tar.xz"
	# the commit time (the tarball's file times), so the package does not depend on the build date
	date=$(date -R -r "$dir/VERSION")
	cat > "$dir/debian/changelog" <<-EOF
	sp9-linux ($ver-1~$DEB_SUFFIX) $DEB_CODENAME; urgency=medium

	  * Release $ver for $DEB_CODENAME (CHANGELOG.md has the changes).

	 -- Joonhoe Kim <26rote@gmail.com>  $date
	EOF
}

profiles() {
	[ -s "$dir/packaging/keyring/sp9-archive-keyring.asc" ] || echo nokeyring
}

deps() {
	apt-get update -q
	apt-get install -y -q --no-install-recommends dpkg-dev debhelper xz-utils ca-certificates lintian
	unpack
	p=$(profiles)
	(cd "$dir" && apt-get build-dep -y -q ${p:+-P "$p"} ./)
}

build() {
	dir=$(echo "$work"/sp9-linux-*)
	ver=$(cat "$dir/VERSION")
	p=$(profiles)
	[ -z "$p" ] || echo "build-deb.sh: no repository key in packaging/keyring: sp9-archive-keyring is not built"
	(cd "$dir" && DEB_BUILD_OPTIONS="${DEB_BUILD_OPTIONS:+$DEB_BUILD_OPTIONS }parallel=$(nproc)" dpkg-buildpackage -b -uc -us ${p:+-P"$p"})
	changes=$(echo "$work"/sp9-linux_*_amd64.changes)
	if [ "${LINTIAN:-1}" != 0 ]; then
		# errors fail the build; warnings are printed
		# only the suppressed tags this lintian knows (an older one stops at an unknown tag)
		grep -v '^#' "$dir/packaging/debian/lintian-suppress" | while read -r t; do
			[ -n "$t" ] && lintian-explain-tags "$t" > /dev/null 2>&1 && echo "$t"
		done > /tmp/lintian-suppress || true
		lintian --info --display-info --fail-on error --suppress-tags-from-file /tmp/lintian-suppress "$changes"
	fi
	if [ -n "$out" ]; then
		mkdir -p "$out"
		cp "$work"/*.deb "$changes" "$work"/sp9-linux_*.buildinfo "$out/"
		ls -l "$out"
	fi
}

case $phase in
deps) deps ;;
build) build ;;
all) deps; build ;;
esac
