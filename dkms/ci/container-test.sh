#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# container-test.sh -- inside a throw-away Debian/Ubuntu or Arch container: install linux-surface's
# header package and unpack its kernel's modules, build the generated DKMS trees with
# dkms/check-build.sh and then with dkms itself (add, build, install), and check where the modules
# land, what modprobe would load, the signature dkms applied, and that an unknown kernel release of
# the same series is skipped. Never run this on a real system: it installs packages and writes to
# /usr/src, /var/lib/dkms and /lib/modules.
#
#   dkms/ci/container-test.sh KIND PKGDIR TREE...
#
#   KIND    debian or arch
#   PKGDIR  linux-surface's packages of one release: debian: linux-headers-*.deb and
#           linux-image-*.deb; arch: linux-surface-headers-*.pkg.tar.zst and linux-surface-*.pkg.tar.zst
#   TREE    trees written by `dkms/gen.sh tree`
#   PACKAGES  (environment) the sp9-dkms packages (.deb or .pkg.tar.zst) to install instead of
#           running dkms by hand: their install scripts (debian) or pacman hook (arch) run dkms
set -euo pipefail
export LC_ALL=C DEBIAN_FRONTEND=noninteractive
[ -f /run/.containerenv ] || [ -f /.dockerenv ] || [ -n "${CI:-}" ] || { echo "container-test.sh: run this in a container" >&2; exit 2; }
[ $# -ge 3 ] || { sed -n '4,22p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
KIND=$1 PKG=$(readlink -f "$2"); shift 2
HERE=$(dirname "$(readlink -f "$0")")
fail=0
step() { echo; echo "==== $*"; }

step "install build tools, dkms and the header package"
case $KIND in
debian)
	apt-get update -qq
	apt-get install -y -qq --no-install-recommends dkms libelf1t64 kmod build-essential openssl mokutil shim-signed zstd xz-utils ca-certificates >/dev/null
	apt-get install -y -qq --no-install-recommends "$PKG"/linux-headers-*.deb >/dev/null
	img=$(ls "$PKG"/linux-image-*.deb)
	mkdir -p /tmp/img && dpkg-deb -x "$img" /tmp/img
	KREL=$(ls /tmp/img/lib/modules)
	cp -a /tmp/img/lib/modules/"$KREL"/. /lib/modules/"$KREL"/
	;;
arch)
	pacman -Syu --noconfirm --needed base-devel dkms kmod openssl zstd >/dev/null
	pacman -U --noconfirm "$PKG"/linux-surface-headers-*.pkg.tar.zst >/dev/null
	kp=$(ls "$PKG"/linux-surface-[0-9]*.pkg.tar.zst)
	mkdir -p /tmp/img && tar --zstd -xf "$kp" -C /tmp/img usr/lib/modules
	KREL=$(ls /tmp/img/usr/lib/modules)
	cp -a /tmp/img/usr/lib/modules/"$KREL"/. /usr/lib/modules/"$KREL"/
	;;
*) echo "unknown kind $KIND" >&2; exit 2 ;;
esac
depmod "$KREL"
MODDIR=/lib/modules/$KREL
echo "kernel $KREL; dkms $(dkms --version 2>/dev/null | tail -n1); $(gcc --version | head -n1); headers built with $(sed -n 's/^CONFIG_CC_VERSION_TEXT=//p' "$MODDIR/build/.config")"

step "out-of-tree build and checks"
for t in "$@"; do
	"$HERE/../check-build.sh" --modules "$MODDIR/kernel" "$t" "$MODDIR/build" || fail=1
done

if [ -n "${PACKAGES:-}" ]; then
	step "install the packages: $PACKAGES"
	# shellcheck disable=SC2086
	case $KIND in
	debian) apt-get install -y --no-install-recommends $PACKAGES || fail=1 ;;
	arch) pacman -U --noconfirm $PACKAGES || fail=1 ;;
	esac
	command -v sp9-dkms-status >/dev/null && sp9-dkms-status "$KREL"
else
	step "dkms add, build, install"
	for t in "$@"; do
		name=$(sed -n 's/^PACKAGE_NAME="\(.*\)"/\1/p' "$t/dkms.conf")
		ver=$(sed -n 's/^PACKAGE_VERSION="\(.*\)"/\1/p' "$t/dkms.conf")
		rm -rf "/usr/src/$name-$ver"; cp -a "$t" "/usr/src/$name-$ver"
		dkms add "$name/$ver"
		dkms build "$name/$ver" -k "$KREL" || { fail=1; cat "/var/lib/dkms/$name/$ver/build/make.log" 2>/dev/null | tail -n 30; }
		dkms install "$name/$ver" -k "$KREL" || fail=1
	done
fi
dkms status

step "where the modules are and what modprobe would load"
for t in "$@"; do
	for m in $(sed -n 's/^BUILT_MODULE_NAME\[[0-9]*\]="\(.*\)"/\1/p' "$t/dkms.conf"); do
		f=$(modinfo -k "$KREL" -n "$m" 2>/dev/null || true)
		case $f in
		*/updates/dkms/*) echo "ok: $m -> ${f#"$MODDIR"/}" ;;
		*) echo "FAIL: $m resolves to '${f}'"; fail=1 ;;
		esac
		signer=$(modinfo -k "$KREL" -F signer "$m" 2>/dev/null || true)
		echo "    signer: ${signer:-none}; vermagic: $(modinfo -k "$KREL" -F vermagic "$m")"
		modprobe -S "$KREL" --dry-run --show-depends "$m" 2>&1 | sed 's/^/    modprobe: /'
	done
done
for k in /var/lib/dkms/mok.pub /var/lib/shim-signed/mok/MOK.der; do
	[ -f "$k" ] && echo "dkms signing certificate: $k ($(openssl x509 -inform der -in "$k" -noout -subject 2>/dev/null || echo '?'))"
done
grep -rhs '^[^#]*\(mok_\|sign_file\)' /etc/dkms/framework.conf /etc/dkms/framework.conf.d/ | sed 's/^/framework.conf: /' || true

step "an unknown release of the same series is skipped"
fake=$(echo "$KREL" | sed -E 's/^([0-9]+\.[0-9]+)\.[0-9]+/\1.999/')
mkdir -p "/lib/modules/$fake"; ln -sfn "$MODDIR/build" "/lib/modules/$fake/build"
for t in "$@"; do
	name=$(sed -n 's/^PACKAGE_NAME="\(.*\)"/\1/p' "$t/dkms.conf")
	ver=$(sed -n 's/^PACKAGE_VERSION="\(.*\)"/\1/p' "$t/dkms.conf")
	set +e; out=$(dkms build "$name/$ver" -k "$fake" 2>&1); rc=$?; set -e
	echo "$out" | tail -n 3 | sed 's/^/    /'
	if [ $rc = 0 ] && ! echo "$out" | grep -qi 'BUILD_EXCLUSIVE\|not supported\|does not match'; then
		echo "FAIL: $name built for $fake"; fail=1
	else
		echo "ok: $name not built for $fake (exit $rc)"
	fi
done
set +e; dkms autoinstall -k "$fake" >/tmp/auto.log 2>&1; rc=$?; set -e
echo "dkms autoinstall -k $fake: exit $rc"; tail -n 5 /tmp/auto.log | sed 's/^/    /'
rm -rf "/lib/modules/$fake"

echo
[ $fail = 0 ] && echo "==== container test passed ($KIND $KREL)" || echo "==== container test FAILED ($KIND $KREL)"
exit $fail
