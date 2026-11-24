#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# gen.sh -- generate the DKMS source trees of sp9-dkms and sp9-dkms-bluetooth from kernel/patches
# and the exact sources of linux-surface kernel releases (dkms/targets.tsv).
#
#   dkms/gen.sh [--cache DIR] [--ls-repo DIR] COMMAND ...
#
#   tree --out DIR [--package main|bluetooth] [--version V] TAG...
#             Write the DKMS source tree (what goes to /usr/src/<name>-<version>) with one source
#             variant per distinct result; dkms.conf builds only for the kernel releases of TAG...
#   prepare TAG...
#             Fetch and patch the target sources only (into the cache).
#   lock TAG...
#             Record the files of the main package's modules, as TAG ships them, as reviewed
#             (dkms/lock.tsv; a file may have several reviewed versions, one per locked tag). Run
#             after reading the diff that `tree` printed.
#   targets [KIND]
#             Print the tags of targets.tsv (all, or only "debian" or "arch").
#   version   Print the package version derived from `git describe` (vX.Y.Z tags).
#
# For each TAG the source is rebuilt the way that linux-surface release was: the kernel.org stable
# tarball (sha256 pinned), for arch the archlinux/linux release patch (sha256 pinned), the packaging
# patches (debian) and patches/<series> of the pinned linux-surface commit. Then, per module in
# dkms/modules/, the module's files are copied, this repository's patches are applied to them (strict,
# no fuzz), and dkms/patches/ adjustments on top.
#
# A patch is skipped when the target already contains it: declared in mainline for the target's
# series (UPSTREAM=, checked by reverse-applying), or found by reverse-applying, or when the target has
# its own version of it (SUPERSEDED="NNNN=STRING": the target's files contain STRING). A patch that
# neither applies nor reverse-applies is tried as its DKMS-only rebase (ALT_PATCHES="NNNN=FILE" in
# dkms/patches/) and otherwise stops the generator. The generator also stops when
#   - a main-package file differs from the copy recorded in dkms/lock.tsv (a stable or linux-surface
#     change to a file this package replaces; it prints the diff when the reviewed tag is known),
#   - the set of linux-surface patches that change a module's files differs from its LS_PATCHES (or
#     from each of its "|"-separated alternatives),
#   - the targets give different module sets.
#
# Cache: ${XDG_CACHE_HOME:-~/.cache}/sp9-dkms (about 1.8 GB per prepared target); --ls-repo uses an
# existing linux-surface clone instead of a blob-less clone in the cache.
set -euo pipefail
export LC_ALL=C

SELF=$(readlink -f "${BASH_SOURCE[0]}")
DKMS=$(dirname "$SELF")
REPO=$(dirname "$DKMS")
CACHE=${XDG_CACHE_HOME:-$HOME/.cache}/sp9-dkms
LS_REPO=
LS_URL=https://github.com/linux-surface/linux-surface.git
# The licence tag of generated files, split so that licence scanners do not read it as this file's.
SPDX_TAG="SPDX-License""-Identifier"
# Another targets file (dkms/ci/new-targets.sh writes one with releases not yet listed).
TARGETS=${SP9_DKMS_TARGETS:-$DKMS/targets.tsv}

die() { echo "gen.sh: $*" >&2; exit 1; }
note() { echo "gen.sh: $*" >&2; }

while [ $# -gt 0 ]; do
	case $1 in
	--cache) CACHE=$2; shift 2 ;;
	--ls-repo) LS_REPO=$(readlink -f "$2"); shift 2 ;;
	-h | --help) sed -n '4,40p' "$SELF" | sed 's/^# \{0,1\}//'; exit 0 ;;
	*) break ;;
	esac
done
[ $# -gt 0 ] || die "no command (see --help)"
CMD=$1; shift

# ---------------------------------------------------------------- targets

# target TAG: sets T_TAG T_KIND T_REL T_STABLE T_COMMIT T_TARSHA T_DISTSHA T_SERIES
target() {
	local line
	line=$(awk -F'\t' -v t="$1" '!/^#/ && $1 == t' "$TARGETS")
	[ -n "$line" ] || die "$1: not in targets.tsv"
	IFS=$'\t' read -r T_TAG T_REL T_STABLE T_COMMIT T_TARSHA T_DISTSHA <<<"$line"
	T_KIND=${T_TAG%%-*}
	T_SERIES=$(echo "$T_STABLE" | cut -d. -f1-2)
	case $T_KIND in debian | arch) ;; *) die "$T_TAG: unknown kind $T_KIND" ;; esac
	[[ $T_COMMIT =~ ^[0-9a-f]{40}$ ]] || die "$T_TAG: commit is not a full hash"
}

fetch() { # URL FILE SHA256
	if [ ! -f "$2" ] || ! echo "$3  $2" | sha256sum -c --status; then
		mkdir -p "$(dirname "$2")"
		note "download $1"
		curl -fsSL --retry 3 -o "$2.part" "$1"
		mv "$2.part" "$2"
	fi
	echo "$3  $2" | sha256sum -c --status || die "$2: sha256 does not match the pin"
}

ls_git() {
	local d=${LS_REPO:-$CACHE/linux-surface.git}
	if [ ! -d "$d" ]; then
		git clone -q --bare --filter=blob:none "$LS_URL" "$d"
	fi
	git -C "$d" "$@"
}

ls_checkout() { # COMMIT DIR: patches/ and pkg/ of a linux-surface commit
	if ! ls_git cat-file -e "$1^{commit}" 2>/dev/null; then
		ls_git fetch -q origin "$1" || die "linux-surface commit $1 not found"
	fi
	rm -rf "$2"; mkdir -p "$2"
	ls_git archive "$1" patches pkg/debian/kernel pkg/arch/kernel | tar -x -C "$2"
}

# The patch files a target's kernel package applies, in order (one path per line).
target_patches() { # LSDIR
	local s=$T_SERIES
	[ -d "$1/patches/$s" ] || die "$T_TAG: linux-surface has no patches/$s"
	case $T_KIND in
	debian)
		find "$1/pkg/debian/kernel" -name '*.patch' -type f | sort
		find "$1/patches/$s" -name '*.patch' -type f | sort
		;;
	arch)
		echo "$CACHE/dl/$(arch_patch_name).patch"
		# The PKGBUILD's source array, in its order: the patches it ships are links into patches/<series>.
		(cd "$1/pkg/arch/kernel" && bash -c 'source PKGBUILD >/dev/null 2>&1; printf "%s\n" "${source[@]}"') |
			sed 's|.*/||; s|::.*||' | grep '\.patch$' | while read -r p; do
				[ -e "$1/pkg/arch/kernel/$p" ] || die "$T_TAG: PKGBUILD names $p, which is missing"
				readlink -f "$1/pkg/arch/kernel/$p"
			done
		;;
	esac
}

arch_patch_name() { # linux-v7.3.4-arch1 from the release string 7.3.4-arch1-2-surface
	local a
	a=$(echo "$T_REL" | grep -oE '^[0-9.]+-arch[0-9]+') || die "$T_TAG: no archN in $T_REL"
	echo "linux-v$a"
}

# prepare TAG: $CACHE/src/TAG/linux = the target's source, $CACHE/src/TAG/touch = "patch<TAB>path"
prepare() {
	target "$1"
	local base=$CACHE/src/$T_TAG stamp
	stamp="$T_STABLE $T_COMMIT $T_TARSHA $T_DISTSHA v2"
	if [ -f "$base/stamp" ] && [ "$(cat "$base/stamp")" = "$stamp" ]; then return 0; fi
	local major=${T_STABLE%%.*}
	fetch "https://cdn.kernel.org/pub/linux/kernel/v$major.x/linux-$T_STABLE.tar.xz" \
		"$CACHE/dl/linux-$T_STABLE.tar.xz" "$T_TARSHA"
	if [ "$T_KIND" = arch ]; then
		local an; an=$(arch_patch_name)
		fetch "https://github.com/archlinux/linux/releases/download/${an#linux-}/$an.patch.zst" \
			"$CACHE/dl/$an.patch.zst" "$T_DISTSHA"
		zstd -q -d -f "$CACHE/dl/$an.patch.zst" -o "$CACHE/dl/$an.patch"
	fi
	rm -rf "$base"; mkdir -p "$base"
	note "$T_TAG: unpack linux-$T_STABLE"
	tar -xJf "$CACHE/dl/linux-$T_STABLE.tar.xz" -C "$base"
	mv "$base/linux-$T_STABLE" "$base/linux"
	ls_checkout "$T_COMMIT" "$base/ls"
	: >"$base/touch"
	local p n
	while read -r p; do
		n=$(basename "$p")
		case $p in "$base/ls/pkg/"*) n="pkg/$T_KIND/$n" ;; "$base/ls/patches/"*) n="patches/$T_SERIES/$n" ;; esac
		grep -E '^\+\+\+ b/' "$p" | sed 's|^+++ b/||' | sort -u | sed "s|^|$n\t|" >>"$base/touch"
		if [ "$T_KIND" = debian ]; then
			GIT_CEILING_DIRECTORIES=$base git -C "$base/linux" apply --whitespace=nowarn "$p" ||
				die "$T_TAG: $n does not apply"
		else
			patch -d "$base/linux" -Np1 -s <"$p" || die "$T_TAG: $n does not apply"
		fi
	done < <(target_patches "$base/ls")
	echo "$stamp" >"$base/stamp"
	note "$T_TAG: source ready"
}

# ---------------------------------------------------------------- modules

modules_of() { # PACKAGE: module conf files of that package
	local f
	for f in "$DKMS"/modules/*.conf; do
		if (PACKAGE=main; . "$f"; [ "$PACKAGE" = "$1" ]); then echo "$f"; fi
	done
}

load_module() {
	NAME= DIR= FILES= PATCHES= UPSTREAM= ALT_PATCHES= SUPERSEDED= PUBLIC_HEADERS= GENKSYMS_PATCH=
	DKMS_PATCHES= KBUILD= KBUILD_DROP= CHECK_CRCS= LS_PATCHES= PACKAGE=main
	. "$1"
}

patch_file() {
	local f
	f=$(ls "$REPO"/kernel/patches/"$1"-*.patch 2>/dev/null) || die "kernel/patches/$1-*.patch not found"
	echo "$f"
}

ver_ge() { [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -n1)" = "$2" ]; }

expand() { # DIR PATTERN...: existing paths under DIR matching the patterns
	local d=$1 p; shift
	(cd "$d" && for p in "$@"; do compgen -G "$p" || true; done) | sort -u
}

lock_line() { awk -F'\t' -v k="$1" -v p="$2" '!/^#/ && $1 == k && $2 == p' "$DKMS/lock.tsv" 2>/dev/null; }

# module_variant MODCONF OUTDIR LOGFILE: one module for the prepared target T_*
module_variant() {
	load_module "$1"
	local src=$CACHE/src/$T_TAG/linux touch=$CACHE/src/$T_TAG/touch
	local w=$WORK/$T_TAG/$NAME out=$2/$NAME log=$3 f p pf incl=() why
	rm -rf "$w"; mkdir -p "$w/a"
	# shellcheck disable=SC2086
	for f in $(expand "$src" $FILES); do
		mkdir -p "$w/a/$(dirname "$f")"; cp -p "$src/$f" "$w/a/$f"
	done
	cp -a "$w/a" "$w/base"
	# shellcheck disable=SC2086
	for p in $FILES; do incl+=(--include="$p"); done
	local applied=() skipped=()
	for p in $PATCHES; do
		pf=$(patch_file "$p")
		local up; up=$(echo " $UPSTREAM " | grep -oE " $p=[0-9.]+ " | cut -d= -f2 | tr -d ' ') || true
		if [ -n "$up" ] && ver_ge "$T_SERIES" "$up"; then
			GIT_CEILING_DIRECTORIES=$w git -C "$w/a" apply -R --check "${incl[@]}" "$pf" 2>/dev/null ||
				die "$T_TAG $NAME: $p is declared in v$up but does not reverse-apply: review it"
			skipped+=("$p (in v$up)"); continue
		fi
		# SUPERSEDED="NNNN=STRING": a target whose files contain STRING has its own version of NNNN.
		local sup; sup=$(echo " $SUPERSEDED " | grep -oE " $p=[^ ]+ " | cut -d= -f2- | tr -d ' ') || true
		if [ -n "$sup" ] && grep -rqF -- "$sup" "$w/base"; then
			skipped+=("$p (superseded in the target: $sup)"); continue
		fi
		if GIT_CEILING_DIRECTORIES=$w git -C "$w/a" apply --check "${incl[@]}" "$pf" 2>/dev/null; then
			GIT_CEILING_DIRECTORIES=$w git -C "$w/a" apply "${incl[@]}" "$pf"
			applied+=("$p")
		elif GIT_CEILING_DIRECTORIES=$w git -C "$w/a" apply -R --check "${incl[@]}" "$pf" 2>/dev/null; then
			skipped+=("$p (already in the target)")
		else
			# ALT_PATCHES="NNNN=FILE": dkms/patches/FILE is NNNN for targets whose files changed around it.
			local alt; alt=$(echo " $ALT_PATCHES " | grep -oE " $p=[^ ]+ " | cut -d= -f2- | tr -d ' ') || true
			if [ -n "$alt" ] && GIT_CEILING_DIRECTORIES=$w git -C "$w/a" apply --check "${incl[@]}" "$DKMS/patches/$alt" 2>/dev/null; then
				GIT_CEILING_DIRECTORIES=$w git -C "$w/a" apply "${incl[@]}" "$DKMS/patches/$alt"
				applied+=("$p (as dkms/patches/$alt)"); continue
			fi
			GIT_CEILING_DIRECTORIES=$w git -C "$w/a" apply --check "${incl[@]}" "$pf" || true
			die "$T_TAG $NAME: kernel/patches/$(basename "$pf") does not apply to the target's files"
		fi
	done
	[ ${#applied[@]} -gt 0 ] || { note "$T_TAG $NAME: every patch is in the target, module not shipped"; return 0; }
	for p in $DKMS_PATCHES; do
		GIT_CEILING_DIRECTORIES=$w git -C "$w/a" apply "${incl[@]}" "$DKMS/patches/$p" ||
			die "$T_TAG $NAME: dkms/patches/$p does not apply"
	done
	# Bundled public headers: only when this repository's patches changed them.
	local h bundled_pub=()
	for h in $PUBLIC_HEADERS; do
		if ! cmp -s "$w/a/$h" "$w/base/$h"; then
			bundled_pub+=("$h")
			if [ -n "$GENKSYMS_PATCH" ]; then
				GIT_CEILING_DIRECTORIES=$w git -C "$w/a" apply --include="$h" "$DKMS/patches/$GENKSYMS_PATCH" ||
					die "$T_TAG $NAME: dkms/patches/$GENKSYMS_PATCH does not apply to the patched $h"
			fi
		fi
	done
	# linux-surface (and distribution) patches that change these files.
	local lsp
	lsp=$(cd "$w/base" && find . -type f | sed 's|^\./||' | sort >"$w/files"; awk -F'\t' 'NR==FNR { f[$1]; next } ($2 in f) { print $1 }' "$w/files" "$touch" | sed 's|.*/||' | sort -u | tr '\n' ' ' | sed 's/ $//')
	# LS_PATCHES may list alternatives separated by "|" (targets before and after a linux-surface change).
	local ok=0 alt_set
	local IFS_old=$IFS; IFS='|'
	for alt_set in ${LS_PATCHES:-}; do [ "$lsp" = "$(echo "$alt_set" | xargs)" ] && ok=1; done
	IFS=$IFS_old
	[ -n "$LS_PATCHES" ] || { [ -z "$lsp" ] && ok=1; }
	if [ $ok = 0 ]; then
		die "$T_TAG $NAME: the files are changed by [${lsp}], modules/$NAME.conf states LS_PATCHES=\"$LS_PATCHES\": review what those patches do to the files and update LS_PATCHES"
	fi
	# Review gate (main package): the target's files must be the reviewed ones.
	if [ "$PACKAGE" = main ]; then
		local sha l bad=0
		while read -r f; do
			sha=$(sha256sum "$w/base/$f" | cut -d' ' -f1)
			l=$(lock_line "$T_KIND" "$f")
			if [ -z "$l" ]; then
				note "$T_TAG $NAME: $f is not in dkms/lock.tsv"; bad=1
			elif ! echo "$l" | cut -f3 | grep -qx "$sha"; then
				local rt; rt=$(echo "$l" | tail -n1 | cut -f4)
				note "$T_TAG $NAME: $f differs from the reviewed copy (reviewed at $rt)"
				if [ -n "$rt" ] && [ "$rt" != "$T_TAG" ] && awk -F'\t' -v t="$rt" '$1 == t { f = 1 } END { exit !f }' "$TARGETS"; then
					(prepare "$rt" >&2) && diff -u "$CACHE/src/$rt/linux/$f" "$w/base/$f" >&2 || true
				fi
				bad=1
			fi
		done <"$w/files"
		[ $bad = 0 ] || die "$T_TAG $NAME: files changed since review; read the change, then: dkms/gen.sh lock $T_TAG"
	fi
	# Lay out the module directory.
	mkdir -p "$out"
	# shellcheck disable=SC2086
	for f in $(expand "$w/a" $FILES); do
		case " ${PUBLIC_HEADERS} " in
		*" $f "*)
			case " ${bundled_pub[*]-} " in *" $f "*) ;; *) continue ;; esac
			mkdir -p "$out/$(dirname "$f")"; cp -p "$w/a/$f" "$out/$f" ;;
		*)
			[ "${f#"$DIR"/}" != "$f" ] || die "$NAME: $f is outside $DIR"
			mkdir -p "$out/$(dirname "${f#"$DIR"/}")"; cp -p "$w/a/$f" "$out/${f#"$DIR"/}" ;;
		esac
	done
	# Modules that export symbols: genksyms must see the types as the kernel's own compiler saw them.
	# include/linux/compiler-gcc.h decides from the compiler running now whether __typeof_unqual__
	# exists (gcc 14+), and with it whether per-CPU pointers carry __seg_gs, which enters the CRCs.
	# A kernel built with gcc 11 and a module built with gcc 14 would get different CRCs for the
	# same source, and the stock modules importing them would refuse to load.
	local compat=0
	if [ -n "$CHECK_CRCS" ]; then
		compat=1
		mkdir -p "$out/genksyms-compat/linux"
		cat >"$out/genksyms-compat/linux/compiler-gcc.h" <<H
/* $SPDX_TAG: GPL-2.0-only */
/*
 * Generated by dkms/gen.sh. Only while genksyms reads the source (__GENKSYMS__): decide
 * CC_HAS_TYPEOF_UNQUAL by the compiler the kernel was built with (CONFIG_GCC_VERSION), not by
 * the one building this module, so the symbol CRCs match the kernel's Module.symvers. The
 * compiled code is unaffected.
 */
#include_next <linux/compiler-gcc.h>
#ifdef __GENKSYMS__
#undef CC_HAS_TYPEOF_UNQUAL
#define CC_HAS_TYPEOF_UNQUAL (CONFIG_GCC_VERSION >= 140000)
#endif
H
	fi
	{
		echo "# Generated by dkms/gen.sh."
		if [ "${KBUILD#@}" != "$KBUILD" ]; then
			# The target's own build file of the directory, without the lines KBUILD_DROP matches.
			[ -f "$src/$DIR/${KBUILD#@}" ] || die "$T_TAG $NAME: $DIR/${KBUILD#@} not found"
			grep -Ev "${KBUILD_DROP:-^\$}" "$src/$DIR/${KBUILD#@}" || true
		else
			printf '%s\n' "$KBUILD"
		fi
		# shellcheck disable=SC2016
		[ ${#bundled_pub[@]} = 0 ] || echo 'LINUXINCLUDE := -I$(src)/include $(LINUXINCLUDE)'
		[ $compat = 0 ] || echo 'LINUXINCLUDE := -I$(src)/genksyms-compat $(LINUXINCLUDE)'
	} >"$out/Kbuild"
	{
		echo "module $NAME"
		while read -r f; do echo "  base $(sha256sum "$w/base/$f" | cut -d' ' -f1) $f"; done <"$w/files"
		echo "  applied ${applied[*]}"
		[ ${#skipped[@]} = 0 ] || printf '  skipped %s\n' "${skipped[@]}"
		local dp; dp=$(echo "$DKMS_PATCHES ${bundled_pub[*]:+$GENKSYMS_PATCH}" | xargs)
		[ -z "$dp" ] || echo "  dkms-patches $dp"
		[ ${#bundled_pub[@]} = 0 ] || echo "  bundled-public ${bundled_pub[*]}"
		echo "  linux-surface ${lsp:-none}"
		[ -z "$CHECK_CRCS" ] || echo "  check-crcs $CHECK_CRCS"
	} >>"$log"
	echo "$NAME" >>"$2/.modules"
}

# ---------------------------------------------------------------- tree

# The project version as every package of this repository gets it (docs/releasing.md):
# packaging/version.sh when it exists, otherwise the same scheme here: 0.2.0 at the tag v0.2.0,
# 0.2.0+git5.gabc1234 five commits after it, 0.2.0~git49.gabc1234 before v0.2.0 is tagged.
git_version() {
	if [ -f "$REPO/packaging/version.sh" ]; then sh "$REPO/packaging/version.sh"; return; fi
	local v n h sep=+
	v=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\([^"]*\)".*/\1/p' "$REPO/helper/Cargo.toml")
	[ -n "$v" ] || die "no [workspace.package] version in helper/Cargo.toml"
	h=g$(git -C "$REPO" rev-parse --short=7 HEAD)
	if git -C "$REPO" rev-parse -q --verify "refs/tags/v$v" >/dev/null; then
		n=$(git -C "$REPO" rev-list --count "v$v..HEAD")
	else
		n=$(git -C "$REPO" rev-list --count HEAD); sep="~"
	fi
	if [ "$n" = 0 ] && [ "$sep" = + ]; then echo "$v"; else echo "$v${sep}git$n.$h"; fi
}

cmd_tree() {
	local out= pkg=main version=
	while [ $# -gt 0 ]; do
		case $1 in
		--out) out=$2; shift 2 ;;
		--package) pkg=$2; shift 2 ;;
		--version) version=$2; shift 2 ;;
		*) break ;;
		esac
	done
	[ -n "$out" ] && [ $# -gt 0 ] || die "tree --out DIR [--package main|bluetooth] [--version V] TAG..."
	case $pkg in main | bluetooth) ;; *) die "unknown package $pkg" ;; esac
	[ -n "$version" ] || version=$(git_version)
	[[ $version =~ ^[0-9][A-Za-z0-9.+~]*$ ]] || die "version $version: use digits, letters, . + ~"
	local name=sp9; [ $pkg = main ] || name=sp9-bluetooth
	WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
	rm -rf "$out"; mkdir -p "$out/variants"
	: >"$WORK/kernels"; : >"$WORK/sources"
	local tag mods= confs; confs=$(modules_of $pkg)
	[ -n "$confs" ] || die "no modules for package $pkg"
	for tag in "$@"; do
		prepare "$tag"
		target "$tag"
		local v=$WORK/v/$tag; rm -rf "$v"; mkdir -p "$v"; : >"$v/.modules"
		local log=$WORK/log.$tag; : >"$log"
		local c
		for c in $confs; do module_variant "$c" "$v" "$log"; target "$tag"; done
		local m; m=$(sort "$v/.modules" | tr '\n' ' ')
		[ -n "$m" ] || die "$tag: no module left to ship"
		[ -z "$mods" ] || [ "$m" = "$mods" ] || die "$tag ships [$m], an earlier target [$mods]: generate separate trees"
		mods=$m
		rm "$v/.modules"
		# The variant id is a hash over the files: targets with equal results share one variant.
		local id; id=$(cd "$v" && find . -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -c1-12)
		[ -d "$out/variants/$id" ] || mv "$v" "$out/variants/$id"
		printf '%s\t%s\t%s\n' "$T_REL" "$id" "$tag" >>"$WORK/kernels"
		{ echo "target $tag $T_REL variant $id"; cat "$log"; echo; } >>"$WORK/sources"
	done
	local kernels_re
	kernels_re=$(cut -f1 "$WORK/kernels" | sed 's/[.+]/\\&/g' | paste -sd'|')
	{
		echo "# $SPDX_TAG: GPL-2.0-only"
		echo "# Generated by dkms/gen.sh from $(git -C "$REPO" rev-parse --short=12 HEAD). Kernel release, source variant, linux-surface tag."
		cat "$WORK/kernels"
	} >"$out/kernels"
	{
		echo "# What each source variant contains: for every module the sha256 of the target's own files the"
		echo "# patches were applied to, the patches applied and skipped, and the linux-surface patches that"
		echo "# also change these files (a copy of one release would hide later changes to them, so each"
		echo "# kernel release gets the variant generated from its own source)."
		echo
		cat "$WORK/sources"
	} >"$out/SOURCES"
	local i=0 m
	{
		echo "# $SPDX_TAG: GPL-2.0-only"
		echo "# Generated by dkms/gen.sh. Builds only for the kernel releases in ./kernels, each from the"
		echo "# source variant generated from that release's own source (see ./SOURCES)."
		echo "PACKAGE_NAME=\"$name\""
		echo "PACKAGE_VERSION=\"$version\""
		echo "BUILD_EXCLUSIVE_KERNEL=\"^($kernels_re)\$\""
		# shellcheck disable=SC2016
		echo 'PRE_BUILD="select-variant $kernelver"'
		# The build directory is a fresh copy for every build; "make clean" before select-variant has nothing to clean.
		echo 'CLEAN="true"'
		for m in $mods; do
			echo "BUILT_MODULE_NAME[$i]=\"$m\""
			echo "BUILT_MODULE_LOCATION[$i]=\"$m/\""
			echo "DEST_MODULE_LOCATION[$i]=\"/updates/dkms\""
			i=$((i + 1))
		done
		echo 'AUTOINSTALL="yes"'
	} >"$out/dkms.conf"
	{
		echo "# $SPDX_TAG: GPL-2.0-only"
		echo "# Generated by dkms/gen.sh. The module directories are copied here from variants/ by select-variant."
		for m in $mods; do echo "obj-m += $m/"; done
	} >"$out/Kbuild"
	install -m 0755 "$DKMS/select-variant" "$out/select-variant"
	note "wrote $out ($name $version): $(cut -f1 "$WORK/kernels" | tr '\n' ' ')"
}

case $CMD in
tree) cmd_tree "$@" ;;
prepare) for t in "$@"; do prepare "$t"; done ;;
lock)
	[ $# -gt 0 ] || die "lock TAG..."
	WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
	for t in "$@"; do
		prepare "$t"; target "$t"
		for c in $(modules_of main); do
			load_module "$c"
			# shellcheck disable=SC2086
			for f in $(expand "$CACHE/src/$t/linux" $FILES); do
				printf '%s\t%s\t%s\t%s\n' "$T_KIND" "$f" "$(sha256sum "$CACHE/src/$t/linux/$f" | cut -d' ' -f1)" "$t"
			done
		done >"$WORK/new"
		{
			grep '^#' "$DKMS/lock.tsv" 2>/dev/null || true
			grep -v '^#' "$DKMS/lock.tsv" 2>/dev/null | awk -F'\t' -v t="$t" '$4 != t' || true
			cat "$WORK/new"
		} | awk '/^#/ { print; next } { print | "sort" }' >"$WORK/lock"
		cp "$WORK/lock" "$DKMS/lock.tsv"
		note "lock.tsv: $(wc -l <"$WORK/new") files of $t"
	done
	;;
targets) awk -F'\t' -v k="${1:-}" '!/^#/ && NF { if (k == "" || index($1, k "-") == 1) print $1 }' "$TARGETS" ;;
version) git_version ;;
*) die "unknown command $CMD" ;;
esac
