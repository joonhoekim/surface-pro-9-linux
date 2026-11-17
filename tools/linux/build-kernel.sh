#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# build-kernel.sh -- build the Surface Pro 9 kernel of kernel/ on an x86-64 Linux host.
#
#   tools/linux/build-kernel.sh [options]
#
#   --work DIR          work directory (default: <repo>/out/kernel; sources, build tree and results live here)
#   --jobs N            make jobs (default: nproc)
#   --tarball           build a plain tree (boot/vmlinuz-REL, config, System.map, lib/modules/REL)
#                       as a .tar.zst instead of Debian packages
#   --base-config FILE  starting config (default: linux-surface pkg/debian/kernel/config.amd64,
#                       the Ubuntu mainline config linux-surface builds its Debian kernel from).
#                       /boot/config-$(uname -r) of the target distribution also works
#   --localversion S    kernel release suffix (default: -sp9, release 7.3.4-sp9)
#   --revision N        Debian package revision (default: 1; version 7.3.4-N)
#   --rust              keep CONFIG_RUST as the base config and toolchain decide (default: off)
#   --no-ccache         do not put ccache in front of gcc even if it is installed
#   --sign-key FILE     MOK private key (PEM), or env KBUILD_SIGN_KEY
#   --sign-cert FILE    MOK certificate (PEM or DER), or env KBUILD_SIGN_CERT
#                       with both: sbsign the vmlinuz and sign modules with this key
#   --prepare-only      fetch, apply patches, configure and verify the config; no build
#   --clean             remove the build tree and results first (sources and git objects stay)
#   --kernel-url URL    stable git (default: git.kernel.org stable)
#   --make-key DIR      create a MOK key pair in DIR (MOK.key, MOK.crt, MOK.der) and exit
#                       (default DIR: ${XDG_DATA_HOME:-~/.local/share}/sp9-kernel/mok). Never put it in the repo
#
# Steps: v7.3.4 (shallow, hash checked) -> linux-surface patches/7.3/*.patch at the pinned commit
# -> kernel/patches/*.patch, all with git am (any failure stops the build). Config: base
# + linux-surface pkg/debian/kernel/ubuntu.config + configs/surface-7.3.config + kernel/config/sp9.config
# + build settings, merged with merge_config.sh, then olddefconfig; every option the fragments
# request must survive olddefconfig or the build stops. The source tree is reset and re-patched only
# when the inputs change. Results: WORK/dist/ (linux-image and linux-headers .deb, or the tarball,
# and SHA256SUMS). linux-libc-dev is dropped so it never replaces the distribution's.
#
# Needs: git make gcc flex bison bc perl openssl libelf-dev libssl-dev cpio kmod; deb: dpkg-dev;
# tarball: zstd; signing: sbsigntool. Pins below must match kernel/README.md (checked).
set -Eeuo pipefail
export LC_ALL=C

BASE_TAG=v7.3.4
BASE_SHA=7595a62d45ee1a2297eafa5f2ef5d0a45142d84d
LS_URL=https://github.com/linux-surface/linux-surface.git
LS_SHA=ea852b4eff5ed641571e13766bda5bd9cf121983
LS_SERIES=7.3

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
WORK=$REPO/out/kernel
JOBS=$(nproc 2>/dev/null || echo 4)
MODE=deb
BASE_CONFIG=
LOCALVER=-sp9
REVISION=1
RUST=0
CCACHE=1
SIGN_KEY=${KBUILD_SIGN_KEY:-}
SIGN_CERT=${KBUILD_SIGN_CERT:-}
PREPARE_ONLY=0
CLEAN=0
KERNEL_URL=https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git
MAKE_KEY=

usage() { sed -n '2,35p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-2}"; }
die() { echo "build-kernel: error: $*" >&2; exit 1; }
step() { echo "==> $*"; }
have() { command -v "$1" >/dev/null 2>&1; }

while [ $# -gt 0 ]; do
	case $1 in
	--work) WORK=$2; shift ;;
	--jobs|-j) JOBS=$2; shift ;;
	--tarball) MODE=tar ;;
	--base-config) BASE_CONFIG=$2; shift ;;
	--localversion) LOCALVER=$2; shift ;;
	--revision) REVISION=$2; shift ;;
	--rust) RUST=1 ;;
	--no-ccache) CCACHE=0 ;;
	--sign-key) SIGN_KEY=$2; shift ;;
	--sign-cert) SIGN_CERT=$2; shift ;;
	--prepare-only) PREPARE_ONLY=1 ;;
	--clean) CLEAN=1 ;;
	--kernel-url) KERNEL_URL=$2; shift ;;
	--make-key)
		if [ $# -gt 1 ] && [ "${2#--}" = "$2" ]; then MAKE_KEY=$2; shift
		else MAKE_KEY=${XDG_DATA_HOME:-$HOME/.local/share}/sp9-kernel/mok; fi ;;
	-h|--help) usage 0 ;;
	*) echo "unknown option: $1" >&2; usage ;;
	esac
	shift
done

# ---- MOK key pair ---------------------------------------------------------------------------
if [ -n "$MAKE_KEY" ]; then
	have openssl || die "openssl not found"
	case $(realpath -m "$MAKE_KEY")/ in
	"$REPO"/private/*) ;;
	"$REPO"/*) die "$MAKE_KEY is inside the repository; use a path outside it (or under private/)" ;;
	esac
	[ -e "$MAKE_KEY/MOK.key" ] && die "$MAKE_KEY/MOK.key exists; not overwriting"
	mkdir -p "$MAKE_KEY"
	chmod 700 "$MAKE_KEY"
	(
		umask 077
		# No module-signing-only EKU (1.3.6.1.4.1.2312.16.1.2): the same key signs the kernel image.
		openssl req -new -x509 -newkey rsa:2048 -nodes -sha256 -days 3650 \
			-subj "/CN=SP9 local kernel signing key/" \
			-addext "basicConstraints=critical,CA:FALSE" \
			-addext "keyUsage=digitalSignature" \
			-addext "extendedKeyUsage=codeSigning" \
			-keyout "$MAKE_KEY/MOK.key" -out "$MAKE_KEY/MOK.crt" 2>/dev/null
	)
	openssl x509 -in "$MAKE_KEY/MOK.crt" -outform DER -out "$MAKE_KEY/MOK.der"
	echo "created $MAKE_KEY/MOK.key (private, 0600), MOK.crt (PEM), MOK.der (for mokutil)"
	echo "enroll on the target: sudo mokutil --import $MAKE_KEY/MOK.der, reboot, MokManager -> Enroll MOK"
	exit 0
fi

# ---- checks ---------------------------------------------------------------------------------
[ "$(uname -m)" = x86_64 ] || die "x86-64 host required (no cross build)"
[[ $JOBS =~ ^[0-9]+$ ]] && [ "$JOBS" -gt 0 ] || die "--jobs must be a positive number"
[[ $REVISION =~ ^[0-9A-Za-z.+~]+$ ]] || die "bad --revision $REVISION"
[[ $LOCALVER =~ ^-[0-9A-Za-z.+_-]+$ ]] || die "--localversion must start with - (e.g. -sp9)"
for t in git make gcc flex bison bc perl openssl; do have "$t" || die "$t not found"; done
if [ "$PREPARE_ONLY" -eq 0 ]; then
	if [ "$MODE" = deb ]; then have dpkg-buildpackage || die "dpkg-buildpackage not found (dpkg-dev)"
	else have zstd || die "zstd not found"; fi
fi

SIGN=0
if [ -n "$SIGN_KEY$SIGN_CERT" ]; then
	[ -n "$SIGN_KEY" ] && [ -n "$SIGN_CERT" ] || die "signing needs both a key and a certificate"
	[ -r "$SIGN_KEY" ] || die "cannot read $SIGN_KEY"
	[ -r "$SIGN_CERT" ] || die "cannot read $SIGN_CERT"
	have sbsign || die "sbsign not found (sbsigntool)"
	openssl pkey -in "$SIGN_KEY" -noout 2>/dev/null || die "$SIGN_KEY is not an unencrypted PEM private key"
	SIGN_KEY=$(realpath "$SIGN_KEY")
	SIGN_CERT=$(realpath "$SIGN_CERT")
	case $SIGN_KEY in "$REPO"/private/*) ;; "$REPO"/*) die "the signing key is inside the repository; move it out" ;; esac
	SIGN=1
fi

for p in "$BASE_SHA" "$LS_SHA"; do
	grep -q "$p" "$REPO/kernel/README.md" || die "pin $p is not in kernel/README.md; update both together"
done

mkdir -p "$WORK"
WORK=$(cd "$WORK" && pwd)
SRC=$WORK/linux
LS=$WORK/linux-surface
CFG=$WORK/config
BUILD=$WORK/build
OBJ=$BUILD/obj
DIST=$WORK/dist
KEYS=$WORK/keys

cleanup() { rm -rf "$KEYS"; }
trap cleanup EXIT

if [ "$CLEAN" -eq 1 ]; then
	step "clean: $BUILD $DIST"
	rm -rf "$BUILD" "$DIST"
fi

git_q() { git -C "$1" -c advice.detachedHead=false "${@:2}"; }

# ---- sources --------------------------------------------------------------------------------
step "linux $BASE_TAG"
[ -d "$SRC/.git" ] || { git init -q "$SRC"; git -C "$SRC" remote add origin "$KERNEL_URL"; }
git -C "$SRC" remote set-url origin "$KERNEL_URL"
if ! git -C "$SRC" rev-parse -q --verify "$BASE_SHA^{commit}" >/dev/null; then
	git -C "$SRC" fetch --depth 1 --no-tags origin "+refs/tags/$BASE_TAG:refs/tags/$BASE_TAG"
fi
got=$(git -C "$SRC" rev-parse "$BASE_TAG^{commit}" 2>/dev/null || true)
[ "$got" = "$BASE_SHA" ] || die "$BASE_TAG is $got, expected $BASE_SHA"

step "linux-surface $LS_SHA"
[ -d "$LS/.git" ] || { git init -q "$LS"; git -C "$LS" remote add origin "$LS_URL"; }
if ! git -C "$LS" rev-parse -q --verify "$LS_SHA^{commit}" >/dev/null; then
	git -C "$LS" fetch --depth 1 origin "$LS_SHA"
fi
git_q "$LS" checkout -qf --detach "$LS_SHA"
git -C "$LS" clean -qfdx
[ "$(cat "$LS/patches/$LS_SERIES/BASE")" = "$BASE_TAG" ] || die "linux-surface patches/$LS_SERIES/BASE is not $BASE_TAG"

LS_PATCHES=("$LS/patches/$LS_SERIES"/*.patch)
OUR_PATCHES=("$REPO/kernel/patches"/*.patch)
[ -e "${LS_PATCHES[0]}" ] || die "no linux-surface patches"
[ -e "${OUR_PATCHES[0]}" ] || die "no kernel/patches"
want_commits=$(cat "${LS_PATCHES[@]}" "${OUR_PATCHES[@]}" | grep -cE '^From [0-9a-f]{40} ')

stamp=$( { echo "$BASE_SHA $LS_SHA"; cat "${OUR_PATCHES[@]}"; } | sha256sum | cut -d' ' -f1)
stampfile=$SRC/.git/sp9-applied
if [ -f "$stampfile" ] && [ "$(cat "$stampfile")" = "$stamp $(git -C "$SRC" rev-parse HEAD)" ] &&
	[ -z "$(git -C "$SRC" status --porcelain --untracked-files=all)" ]; then
	step "patches already applied ($want_commits commits)"
else
	step "apply ${#LS_PATCHES[@]} linux-surface files, then ${#OUR_PATCHES[@]} kernel/patches"
	rm -f "$stampfile"
	[ -d "$SRC/.git/rebase-apply" ] && git -C "$SRC" am --abort
	git_q "$SRC" checkout -qf --detach "$BASE_SHA"
	git -C "$SRC" clean -qfdx
	export GIT_COMMITTER_NAME=sp9-build GIT_COMMITTER_EMAIL=sp9-build@localhost
	am() {
		local label=$1; shift
		if ! git -C "$SRC" am -q --whitespace=nowarn "$@"; then
			echo "build-kernel: git am failed in the $label series at:" >&2
			git -C "$SRC" am --show-current-patch=raw 2>/dev/null | sed -n '1,/^---$/p' | grep -E '^(From|Subject):' >&2 || true
			git -C "$SRC" am --abort || true
			exit 1
		fi
	}
	am linux-surface "${LS_PATCHES[@]}"
	am kernel/patches "${OUR_PATCHES[@]}"
	n=$(git -C "$SRC" rev-list --count "$BASE_SHA..HEAD")
	[ "$n" -eq "$want_commits" ] || die "applied $n commits, the patch files hold $want_commits"
	echo "$stamp $(git -C "$SRC" rev-parse HEAD)" > "$stampfile"
	step "applied $n commits"
fi

# ---- config ---------------------------------------------------------------------------------
mkdir -p "$CFG" "$OBJ"
BASE_CONFIG=${BASE_CONFIG:-$LS/pkg/debian/kernel/config.amd64}
[ -r "$BASE_CONFIG" ] || die "cannot read base config $BASE_CONFIG"
cp "$BASE_CONFIG" "$CFG/base.config"

MODSIGN_PEM=$KEYS/modsign.pem
{
	echo "## Generated by tools/linux/build-kernel.sh (release suffix comes from LOCALVERSION)"
	echo 'CONFIG_LOCALVERSION=""'
	echo '# CONFIG_LOCALVERSION_AUTO is not set'
	[ "$RUST" -eq 1 ] || echo '# CONFIG_RUST is not set'
	if [ "$SIGN" -eq 1 ]; then
		echo 'CONFIG_MODULE_SIG=y'
		echo 'CONFIG_MODULE_SIG_ALL=y'
		echo "CONFIG_MODULE_SIG_KEY=\"$MODSIGN_PEM\""
	fi
} > "$CFG/build.config"

FRAGMENTS=(
	"$LS/pkg/debian/kernel/ubuntu.config"
	"$LS/configs/surface-$LS_SERIES.config"
	"$REPO/kernel/config/sp9.config"
	"$CFG/build.config"
)

if [ "$SIGN" -eq 1 ]; then
	# kbuild wants key and certificate in one PEM; it lives only for this run.
	(umask 077; mkdir -p "$KEYS"
	 cat "$SIGN_KEY" > "$MODSIGN_PEM"
	 if openssl x509 -in "$SIGN_CERT" -noout 2>/dev/null; then openssl x509 -in "$SIGN_CERT"
	 else openssl x509 -inform DER -in "$SIGN_CERT"; fi > "$KEYS/MOK.pem")
	cat "$KEYS/MOK.pem" >> "$MODSIGN_PEM"
fi

step "merge config: $(basename "$BASE_CONFIG") + ${FRAGMENTS[*]##*/}"
(cd "$SRC" && scripts/kconfig/merge_config.sh -m -O "$OBJ" "$CFG/base.config" "${FRAGMENTS[@]}") > "$CFG/merge.log" 2>&1 ||
	{ cat "$CFG/merge.log" >&2; die "merge_config.sh failed"; }
make -s -C "$SRC" O="$OBJ" olddefconfig

# Every option a fragment requests (the last fragment wins) must be what olddefconfig kept.
declare -A want from got
for f in "${FRAGMENTS[@]}"; do
	while IFS= read -r line || [ -n "$line" ]; do
		if [[ $line =~ ^(CONFIG_[A-Za-z0-9_]+)=(.*)$ ]]; then
			k=${BASH_REMATCH[1]} v=${BASH_REMATCH[2]}
		elif [[ $line =~ ^#\ (CONFIG_[A-Za-z0-9_]+)\ is\ not\ set ]]; then
			k=${BASH_REMATCH[1]} v=n
		else
			continue
		fi
		want[$k]=$v
		from[$k]=${f#"$REPO"/}
	done < "$f"
done
while IFS= read -r line; do
	if [[ $line =~ ^(CONFIG_[A-Za-z0-9_]+)=(.*)$ ]]; then got[${BASH_REMATCH[1]}]=${BASH_REMATCH[2]}; fi
done < "$OBJ/.config"
bad=0
for k in $(printf '%s\n' "${!want[@]}" | sort); do
	g=${got[$k]:-n}
	if [ "$g" != "${want[$k]}" ]; then
		printf 'config mismatch: %s requested %s, got %s (%s)\n' "$k" "${want[$k]}" "$g" "${from[$k]#"$WORK"/}" >&2
		bad=$((bad + 1))
	fi
done
[ "$bad" -eq 0 ] || die "$bad option(s) from the fragments did not survive olddefconfig (missing dependency or symbol)"
cp "$OBJ/.config" "$CFG/final.config"

export LOCALVERSION=$LOCALVER KBUILD_BUILD_USER=sp9 KBUILD_BUILD_HOST=build
RELEASE=$(make -s -C "$SRC" O="$OBJ" kernelrelease)
step "config ok: ${#want[@]} requested options as asked, release $RELEASE"

if [ "$PREPARE_ONLY" -eq 1 ]; then
	echo "prepared: $SRC (patched), $OBJ/.config"
	exit 0
fi

# ---- build ----------------------------------------------------------------------------------
if [ "$CCACHE" -eq 1 ] && have ccache; then
	for d in /usr/lib/ccache /usr/lib64/ccache; do
		[ -x "$d/gcc" ] && { export PATH=$d:$PATH; step "ccache: $d ($(ccache --get-config cache_dir 2>/dev/null || echo default))"; break; }
	done
fi

rm -rf "$DIST"
mkdir -p "$DIST"
KVER=${BASE_TAG#v}

sign_image() {
	sbsign --key "$SIGN_KEY" --cert "$KEYS/MOK.pem" --output "$1.signed" "$1"
	mv "$1.signed" "$1"
	have sbverify && sbverify --cert "$KEYS/MOK.pem" "$1" >/dev/null
	echo "signed $(basename "$1")"
}

if [ "$MODE" = deb ]; then
	step "make bindeb-pkg -j$JOBS ($RELEASE, $KVER-$REVISION)"
	rm -f "$BUILD"/*.deb "$BUILD"/*.buildinfo "$BUILD"/*.changes
	make -C "$SRC" O="$OBJ" -j"$JOBS" INSTALL_MOD_STRIP=1 KDEB_PKGVERSION="$KVER-$REVISION" bindeb-pkg
	for d in "$BUILD"/*.deb; do
		case ${d##*/} in
		linux-libc-dev_*) rm -f "$d" ;;
		*) mv "$d" "$DIST/" ;;
		esac
	done
	if [ "$SIGN" -eq 1 ]; then
		imgs=("$DIST"/linux-image-"$RELEASE"_*.deb)
		[ "${#imgs[@]}" -eq 1 ] && [ -f "${imgs[0]}" ] || die "expected one linux-image-$RELEASE .deb"
		img=${imgs[0]}
		x=$BUILD/repack
		rm -rf "$x"
		dpkg-deb -R "$img" "$x"
		sign_image "$x/boot/vmlinuz-$RELEASE"
		if [ -f "$x/DEBIAN/md5sums" ]; then
			sum=$(cd "$x" && md5sum "boot/vmlinuz-$RELEASE")
			sed -i "s|^[0-9a-f]\{32\}  boot/vmlinuz-$RELEASE\$|$sum|" "$x/DEBIAN/md5sums"
		fi
		dpkg-deb --root-owner-group -Zxz -b "$x" "$img" >/dev/null
		rm -rf "$x"
	fi
else
	step "make dir-pkg -j$JOBS ($RELEASE)"
	make -C "$SRC" O="$OBJ" -j"$JOBS" INSTALL_MOD_STRIP=1 dir-pkg
	t=$OBJ/tar-install
	rm -f "$t/lib/modules/$RELEASE/build" "$t/lib/modules/$RELEASE/source"
	if [ "$SIGN" -eq 1 ]; then sign_image "$t/boot/vmlinuz-$RELEASE"; fi
	tar -C "$t" --owner=0 --group=0 --numeric-owner -cf - boot lib | zstd -q -T0 -19 > "$DIST/linux-$RELEASE-x86_64.tar.zst"
fi

(cd "$DIST" && sha256sum -- * > SHA256SUMS)
step "done: $DIST"
ls -l "$DIST"
