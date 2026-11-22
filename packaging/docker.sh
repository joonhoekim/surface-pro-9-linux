#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# docker.sh -- run the CI package builds and install tests locally in the official containers.
#
#   packaging/docker.sh deb TARGET SOURCE_TARBALL OUTDIR     TARGET: an id of targets.tsv
#   packaging/docker.sh deb-test TARGET DEBDIR               install test of the built packages
#   packaging/docker.sh arch SOURCE_TARBALL OUTDIR
#   packaging/docker.sh arch-test PKGDIR
#
# The Debian build runs in two containers: the first installs the build dependencies (network),
# the second builds from a snapshot of it with --network none, which shows that the build is
# offline. Needs docker (or podman with DOCKER=podman). Images: the ones named in targets.tsv and
# archlinux:latest.
#
# The install tests run in one of two modes:
#   systemd  systemd as PID 1 in a privileged container: units started and checked. Only where CI
#            is set (GitHub's runners are throwaway VMs) or with SP9_TEST_SYSTEMD=1, and never on a
#            Surface Pro 9: a privileged container shares the host's /dev and /sys, so the packaged
#            daemons would drive the host's hardware, and the container's systemd its devices.
#   plain    (default) an unprivileged container without systemd: everything but starting units;
#            the daemons run by hand on a private system bus, with fake backends or no device.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
D=${DOCKER:-docker}
cmd=${1:?usage: docker.sh deb|deb-test|arch|arch-test ...}
shift

target() { # id -> "image codename suffix"
	awk -F'\t' -v id="$1" '!/^#/ && $1 == id { print $2, $3, $4 }' "$here/targets.tsv"
}
abs() { (cd "$(dirname "$1")" && echo "$(pwd)/$(basename "$1")"); }

# systemd as PID 1, for the install tests. The Debian and Ubuntu images have no systemd: it is
# installed into a snapshot of the image first (removed again by unboot).
mode=plain
if [ -n "${CI:-}" ] || [ "${SP9_TEST_SYSTEMD:-0}" = 1 ]; then
	mode=systemd
fi
if [ $mode = systemd ] && [ "$(cat /sys/class/dmi/id/product_sku 2> /dev/null)" = Surface_Pro_9_2038 ]; then
	echo "docker.sh: this host is a Surface Pro 9: no privileged test container here (plain mode)" >&2
	mode=plain
fi

boot() { # image name mount...
	img=$1 name=$2
	shift 2
	if [ $mode = plain ]; then
		$D run -d --rm --name "$name" -e NO_SYSTEMD=1 "$@" "$img" sleep infinity > /dev/null
		return
	fi
	case $img in
	archlinux*) init=/usr/lib/systemd/systemd ;;
	*)
		$D run --name "$name-prep" "$img" sh -c 'apt-get update -q && DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends systemd systemd-sysv dbus > /dev/null' ||
			{ $D rm "$name-prep" > /dev/null; exit 1; }
		$D commit "$name-prep" "$name:systemd" > /dev/null
		$D rm "$name-prep" > /dev/null
		img=$name:systemd init=/lib/systemd/systemd
		;;
	esac
	# /tmp is a mount from the start: an image whose systemd mounts a tmpfs on /tmp (Ubuntu 26.10)
	# would otherwise hide what the test had put there, mid-run.
	$D run -d --rm --name "$name" --privileged --cgroupns=host -v /sys/fs/cgroup:/sys/fs/cgroup:rw \
		--tmpfs /run --tmpfs /run/lock --tmpfs /tmp "$@" "$img" "$init" > /dev/null
}
unboot() { # name
	$D stop -t 2 "$1" > /dev/null
	$D image rm "$1:systemd" > /dev/null 2>&1 || true
}

case $cmd in
deb)
	set -- $(target "$1") "$2" "$3"
	[ $# = 5 ] || { echo "docker.sh: unknown target" >&2; exit 2; }
	img=$1 codename=$2 suffix=$3 src=$(abs "$4") out=$5
	mkdir -p "$out"
	out=$(cd "$out" && pwd)
	name=sp9pkg-deb-$suffix-$$
	$D run --name "$name" -v "$here/build-deb.sh:/build-deb.sh:ro" -v "$src:/src/$(basename "$src"):ro" \
		-e DEB_SUFFIX="$suffix" -e DEB_CODENAME="$codename" "$img" sh /build-deb.sh deps "/src/$(basename "$src")" ||
		{ $D rm "$name" > /dev/null; exit 1; }
	$D commit "$name" "$name:deps" > /dev/null
	$D rm "$name" > /dev/null
	rc=0
	$D run --rm --network none -v "$here/build-deb.sh:/build-deb.sh:ro" -v "$src:/src/$(basename "$src"):ro" -v "$out:/out" \
		-e DEB_SUFFIX="$suffix" -e DEB_CODENAME="$codename" "$name:deps" sh /build-deb.sh build "/src/$(basename "$src")" /out || rc=$?
	$D image rm "$name:deps" > /dev/null
	exit $rc
	;;
deb-test)
	set -- $(target "$1") "$2"
	img=$1 debs=$(cd "$4" && pwd)
	name=sp9pkg-test-$3-$$
	boot "$img" "$name" -v "$debs:/debs:ro" -v "$here:/packaging:ro"
	rc=0
	$D exec "$name" sh /packaging/test-deb.sh /debs || rc=$?
	unboot "$name"
	exit $rc
	;;
arch)
	src=$(abs "$1") out=$2
	mkdir -p "$out"
	out=$(cd "$out" && pwd)
	$D run --rm -v "$here/build-arch.sh:/build-arch.sh:ro" -v "$src:/src/$(basename "$src"):ro" -v "$out:/out" \
		archlinux:latest sh /build-arch.sh "/src/$(basename "$src")" /out
	;;
arch-test)
	pkgs=$(cd "$1" && pwd)
	name=sp9pkg-test-arch-$$
	boot archlinux:latest "$name" -v "$pkgs:/pkgs:ro" -v "$here:/packaging:ro"
	rc=0
	$D exec "$name" sh /packaging/test-arch.sh /pkgs || rc=$?
	unboot "$name"
	exit $rc
	;;
*)
	echo "usage: docker.sh deb|deb-test|arch|arch-test ..." >&2
	exit 2
	;;
esac
