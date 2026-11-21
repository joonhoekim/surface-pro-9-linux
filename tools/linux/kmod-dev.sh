#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# kmod-dev.sh -- rebuild and reload single kernel modules on NixOS without rebuilding the kernel derivation.
# docs/nixos.md section 5.1 (the module development loop) describes the loop. The shell must use the host's nixpkgs (same compiler):
#   nix develop .#kernel-dev --override-input nixpkgs github:nixos/nixpkgs/<nixpkgs rev in your host flake's flake.lock>
#
#   tools/linux/kmod-dev.sh prepare [--drv DRV] [--force]
#   tools/linux/kmod-dev.sh build <dir|module> [-j N]
#   tools/linux/kmod-dev.sh load <module> [extra-module...]
#   tools/linux/kmod-dev.sh patch [--all]
#   tools/linux/kmod-dev.sh git <args...>
#   tools/linux/kmod-dev.sh status
#
#   prepare  Inside `nix develop .#kernel-dev`. Builds a writable tree in $KDEV/src (default
#            ~/.cache/sp9-kdev) from the derivation of the *booted* kernel: its src tarball and its
#            `patches` list (linux-surface, kernel/patches, boot.kernelPatches, in derivation order),
#            committed as tag kdev-base. .config, Module.symvers, the randstruct seed and vmlinux come
#            from the derivation's dev output (rooted at $KDEV/kernel-dev). Then make modules_prepare.
#            Stops when uname -r, /proc/config.gz, the booted kernel path or the compiler do not match.
#            --drv DRV   kernel derivation to use (default: deriver of /run/booted-system/kernel)
#            --force     replace an existing tree made from another derivation (unexported commits are lost)
#   build    make M=<dir> modules for a tree directory (drivers/media/i2c) or for the directory of a
#            module (ov5693, ipu-bridge: located through the installed module). Prints the .ko files
#            and checks their vermagic against the installed module.
#   load     Needs root (sudo is used when not root). Unloads the module, every module that holds it
#            (recursively) and the extra modules given (device-link users such as a sensor on int3472
#            are not holders), insmods the new .ko from the tree, reloads the rest (tree .ko if built,
#            else modprobe) and prints the kernel log since the start.
#   patch    git format-patch of the tree's commits after kdev-base (or after the last export) into
#            kernel/patches/ with the next number. Uncommitted changes stop it. --all: from kdev-base.
#   git      git on the tree (its git directory is $KDEV/git, outside the tree): kmod-dev.sh git commit -as
#   status   tree, derivation and kernel in one screen.
#
# Env: KDEV (work directory), JOBS (make jobs).
set -Eeuo pipefail
export LC_ALL=C

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
KDEV=${KDEV:-${XDG_CACHE_HOME:-$HOME/.cache}/sp9-kdev}
SRC=$KDEV/src
# nix build names the link of a non-out output <link>-<output>: $KDEV/kernel-dev.
DEVLINK=$KDEV/kernel
STAMP=$KDEV/drv
JOBS=${JOBS:-$(nproc 2>/dev/null || echo 4)}
REL=$(uname -r)

usage() { awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"; exit "${1:-2}"; }
die() { echo "kmod-dev: error: $*" >&2; exit 1; }
warn() { echo "kmod-dev: warning: $*" >&2; }
step() { echo "==> $*"; }
have() { command -v "$1" >/dev/null 2>&1; }
# LOCALVERSION set but empty: without it setlocalversion appends "+" in a git tree and vermagic breaks.
# The derivation's own make flags (CC, LD, HOSTCC, ... as store paths) and Rust variables, saved by prepare:
# with other tools, or without RUST_LIB_SRC, modules_prepare rewrites .config (CONFIG_RUST and its
# dependents drop out) and the modules no longer match the kernel.
kmake() {
	local flags=()
	[ -f "$KDEV/makeflags" ] && mapfile -t flags <"$KDEV/makeflags"
	# shellcheck disable=SC1091
	[ -f "$KDEV/env" ] && . "$KDEV/env"
	make -C "$SRC" -j"$JOBS" LOCALVERSION= "${flags[@]}" "$@"
}
need_tree() { [ -f "$SRC/.config" ] && [ -f "$SRC/Module.symvers" ] || die "no prepared tree in $SRC; run: kmod-dev.sh prepare"; }
# The git directory lives outside the tree. With CONFIG_LOCALVERSION_AUTO=y (nixpkgs default) kbuild asks
# git in the source directory and appends -g<commit> to the release, which breaks the modules' vermagic;
# a tree without .git gets no suffix, like the tarball the kernel was built from.
GITDIR=$KDEV/git
kgit() { git --git-dir="$GITDIR" --work-tree="$SRC" "$@"; }
SUDO=
[ "$(id -u)" -eq 0 ] || SUDO=sudo

# Build directory inside the dev output (holds .config, Module.symvers, generated headers).
dev_build() { echo "$1/lib/modules/$REL/build"; }

# Attribute of a derivation (src, patches). Newer `nix derivation show` wraps the map in .derivations.
# With __structuredAttrs (buildLinux sets it) the attributes are in .structuredAttrs, not .env, and lists
# are JSON arrays: they are joined with spaces so both forms read the same.
drv_env() {
	nix derivation show "$1" 2>/dev/null | jq -r --arg k "$2" '
		(.derivations // .) | to_entries[0].value
		| (.structuredAttrs[$k] // .env[$k] // "")
		| if type == "array" then join(" ") else tostring end'
}

booted_drv() {
	local k
	k=$(readlink -f /run/booted-system/kernel) || die "no /run/booted-system/kernel (not NixOS?)"
	nix-store -q --deriver "${k%/*}" 2>/dev/null | grep -v '^unknown-deriver$' || true
}

realise_inputs() {
	# The src tarball and patch files are inputs of the derivation: input sources exist with the .drv,
	# fetched ones are realised from their input derivation (fixed output, cheap).
	local drv=$1 p d
	shift
	for p in "$@"; do
		[ -e "$p" ] && continue
		for d in $(nix-store -q --references "$drv" | grep '\.drv$'); do
			if nix-store -q --outputs "$d" | grep -qxF "$p"; then nix-store -r "$d" >/dev/null; break; fi
		done
		[ -e "$p" ] || die "cannot realise $p (input of $drv)"
	done
}

# ---- prepare --------------------------------------------------------------------------------
cmd_prepare() {
	local drv='' force=0
	while [ $# -gt 0 ]; do
		case $1 in
		--drv) drv=$2; shift ;;
		--force) force=1 ;;
		*) die "prepare: unknown option $1" ;;
		esac
		shift
	done
	[ -n "${IN_NIX_SHELL:-}" ] || warn "not inside nix develop .#kernel-dev; tool versions may not match the kernel build"
	local t
	for t in make gcc flex bison bc perl openssl pahole python3 depmod nix jq git patch tar xz zstd; do
		have "$t" || die "$t not found (enter: nix develop .#kernel-dev)"
	done

	[ -n "$drv" ] || drv=$(booted_drv)
	[ -n "$drv" ] && [ -e "$drv" ] || die "kernel derivation not found; pass --drv (in your host flake:
  nix eval --raw .#nixosConfigurations.<host>.config.boot.kernelPackages.kernel.drvPath)"
	drv=$(readlink -f "$drv")

	# buildLinux splits the image (out), the modules (modules) and the build tree (dev) into three outputs.
	# A kernel without the modules output keeps lib/modules in out.
	local out dev mods src
	dev=$(nix-store -q --outputs "$drv" | grep -- '-dev$' || true)
	mods=$(nix-store -q --outputs "$drv" | grep -- '-modules$' || true)
	out=$(nix-store -q --outputs "$drv" | grep -v -e '-dev$' -e '-modules$' | head -n1)
	[ -n "$mods" ] || mods=$out
	[ -n "$dev" ] || die "$drv has no dev output"
	src=$(drv_env "$drv" src)
	[ -n "$src" ] || die "nix derivation show $drv: no src"
	local patches=()
	read -r -a patches <<<"$(drv_env "$drv" patches)"

	# The derivation must be the running kernel.
	local booted
	booted=$(readlink -f /run/booted-system/kernel)
	[ "${booted%/*}" = "$out" ] || die "$drv builds $out, but the booted kernel is ${booted%/*}"
	[ -d "$mods/lib/modules/$REL" ] || die "uname -r is $REL but $mods has no lib/modules/$REL"

	# dev output: produced together with out when the kernel was built, but not in the system closure,
	# so garbage collection may have removed it. Realising it again is a full kernel build.
	if [ ! -d "$dev" ]; then
		[ "${KDEV_REBUILD_DEV:-0}" = 1 ] || die "$dev (dev output) is not in the store (garbage-collected?).
  Realising it rebuilds the kernel (1-2 h). Rerun with KDEV_REBUILD_DEV=1 to accept, and keep it with
  system.extraDependencies = [ config.boot.kernelPackages.kernel.dev ] in the host (docs/nixos.md)"
	fi
	mkdir -p "$KDEV"
	# The out-link is a GC root for the dev output while the tree exists.
	nix build --out-link "$DEVLINK" "$drv^dev" || die "nix build $drv^dev failed"
	local bdir
	bdir=$(dev_build "$dev")
	[ -f "$bdir/.config" ] && [ -f "$bdir/Module.symvers" ] || die "$bdir lacks .config or Module.symvers"

	# Config checks: kernel image config, release, compiler, LOCALVERSION_AUTO.
	if [ -r /proc/config.gz ]; then
		zcat /proc/config.gz | cmp -s - "$bdir/.config" || die "/proc/config.gz differs from $bdir/.config"
	else
		warn "/proc/config.gz missing (CONFIG_IKCONFIG_PROC off); config match not verified"
	fi
	local want_cc have_cc
	want_cc=$(sed -n 's/^CONFIG_CC_VERSION_TEXT="\(.*\)"$/\1/p' "$bdir/.config")
	have_cc=$(gcc --version | head -n1)
	[ "$want_cc" = "$have_cc" ] || die "compiler differs: kernel \"$want_cc\", shell \"$have_cc\".
  The shell's nixpkgs is not the one the host built with. Use the host's nixpkgs:
  nix develop .#kernel-dev --override-input nixpkgs github:nixos/nixpkgs/<nixpkgs rev in your host flake's flake.lock>"

	# Tree: reuse when made from this derivation; otherwise rebuild.
	if [ -d "$SRC" ]; then
		if [ "$(cat "$STAMP" 2>/dev/null)" = "$drv" ]; then
			step "tree in $SRC is from this derivation; reusing"
		elif [ "$force" -eq 1 ]; then
			unexported_note
			rm -rf "$SRC"
		else
			die "$SRC was made from $(cat "$STAMP" 2>/dev/null || echo '?'); export with 'patch', then prepare --force"
		fi
	fi
	if [ ! -d "$SRC" ]; then
		realise_inputs "$drv" "$src" "${patches[@]}"
		step "unpack $src"
		rm -rf "$SRC.tmp"
		mkdir -p "$SRC.tmp"
		tar -xf "$src" -C "$SRC.tmp" --strip-components=1
		local p n=0
		for p in "${patches[@]}"; do
			n=$((n + 1))
			case $p in
			*.gz) zcat "$p" ;;
			*.xz) xz -dc "$p" ;;
			*.bz2) bzip2 -dc "$p" ;;
			*) cat "$p" ;;
			esac | patch -d "$SRC.tmp" -p1 -s --no-backup-if-mismatch -f >/dev/null || die "patch failed: $p"
		done
		step "applied $n patches (derivation order)"
		# /bin/bash, /usr/bin/perl, /usr/bin/python3 do not exist on NixOS; the nix build patches
		# shebangs too. Part of the base commit, so exports never contain it.
		grep -rlE '^#! ?/(usr/)?bin/(bash|perl|python3?)\b' "$SRC.tmp/scripts" "$SRC.tmp/tools" 2>/dev/null \
			| xargs -r sed -i -E '1s@^#! ?/(usr/)?bin/(bash|perl|python3?)@#!/usr/bin/env \2@'
		step "commit base (git, a few minutes)"
		rm -rf "$GITDIR"
		git init -q --bare "$GITDIR"
		mv "$SRC.tmp" "$SRC"
		# The kernel's .gitignore hides top-level dot files such as .config; -f is not used on purpose.
		kgit add -A
		kgit -c user.name=kmod-dev -c user.email=kmod-dev@localhost \
			commit -q -m "base: $(basename "$drv")" -m "$n patches from the derivation patches list"
		kgit tag kdev-base
		echo "$drv" >"$STAMP"
	fi

	# One flag per line, straight from the JSON list: "--eval=undefine modules" is one flag, and split at
	# the space its second word is the make target that compiles every module. O= is the build's out-of-tree
	# directory; this tree is built in place.
	nix derivation show "$drv" 2>/dev/null \
		| jq -r '(.derivations // .) | to_entries[0].value | (.structuredAttrs.makeFlags // []) | .[]' \
		| grep -v -e '^O=' -e '^--eval' >"$KDEV/makeflags" || true
	{
		printf 'export RUST_LIB_SRC=%q\n' "$(drv_env "$drv" RUST_LIB_SRC)"
		printf 'export KRUSTFLAGS=%q\n' "$(drv_env "$drv" KRUSTFLAGS)"
	} >"$KDEV/env"

	step "config and symbols from $bdir"
	cp -f "$bdir/.config" "$SRC/.config"
	cp -f "$bdir/Module.symvers" "$SRC/Module.symvers"
	kmake modules_prepare
	cmp -s "$SRC/.config" "$bdir/.config" || {
		diff "$bdir/.config" "$SRC/.config" | head -n 20 >&2
		die "modules_prepare changed .config (toolchain differs from the kernel build)"
	}
	[ "$(cat "$SRC/include/config/kernel.release")" = "$REL" ] || die "tree release $(cat "$SRC/include/config/kernel.release") != $REL"
	# RANDSTRUCT: nixpkgs makes the seed deterministic per derivation; modules must use the same layout.
	if grep -qE '^CONFIG_RANDSTRUCT_(FULL|PERFORMANCE)=y' "$SRC/.config"; then
		local f
		for f in scripts/basic/randstruct.seed include/generated/randstruct_hash.h; do
			[ -f "$bdir/$f" ] || die "RANDSTRUCT is on but $bdir/$f is missing"
			cp -f "$bdir/$f" "$SRC/$f"
		done
	fi
	# BTF for modules is generated against vmlinux; without it Kbuild skips module BTF.
	if [ -f "$dev/vmlinux" ]; then ln -sfn "$dev/vmlinux" "$SRC/vmlinux"; fi
	signing_note
	step "ready: $SRC ($REL). Edit, commit, then: kmod-dev.sh build <dir|module>"
}

unexported_note() {
	local base
	base=$(kgit rev-parse -q --verify kdev-exported 2>/dev/null || echo kdev-base)
	if [ -n "$(kgit log --oneline "$base"..HEAD 2>/dev/null)" ] || [ -n "$(kgit status --porcelain 2>/dev/null)" ]; then
		warn "discarding unexported work in $SRC:"
		kgit log --oneline "$base"..HEAD >&2 || true
		kgit status --short >&2 || true
	fi
}

# ---- signing / lockdown ---------------------------------------------------------------------
signing_note() {
	local enforce=N lockdown=none cfg=$SRC/.config
	[ -r /sys/module/module/parameters/sig_enforce ] && enforce=$(cat /sys/module/module/parameters/sig_enforce)
	[ -r /sys/kernel/security/lockdown ] && lockdown=$(sed -n 's/.*\[\(.*\)\].*/\1/p' /sys/kernel/security/lockdown)
	if grep -qx 'CONFIG_MODULE_SIG_FORCE=y' "$cfg" || [ "$enforce" = Y ]; then
		echo "unsigned modules are refused (CONFIG_MODULE_SIG_FORCE or module.sig_enforce=1); insmod will fail with EKEYREJECTED."
		echo "  This loop needs a kernel without forced signing (the nix/kernel.nix default) or signing the .ko with the kernel's key."
		return 1
	fi
	if [ -n "$lockdown" ] && [ "$lockdown" != none ]; then
		echo "kernel lockdown is '$lockdown' (Secure Boot on?): unsigned modules are refused. docs/nixos.md section 3: Secure Boot set to None."
		return 1
	fi
	if grep -qx 'CONFIG_MODULE_SIG=y' "$cfg"; then
		echo "note: CONFIG_MODULE_SIG=y without enforcement: unsigned modules load and taint the kernel (E)."
	fi
	return 0
}

# ---- module lookup --------------------------------------------------------------------------
# Tree-relative directory of a module: from the installed module's path, else from the Makefiles.
# NixOS has no /lib/modules: the booted modules are under /run/booted-system/kernel-modules, and only the
# system's wrapped kmod knows that. The dev shell's plain kmod needs the base directory spelled out.
MODBASE=()
[ -d "/run/booted-system/kernel-modules/lib/modules/$REL" ] && MODBASE=(-b /run/booted-system/kernel-modules)
kmodinfo() { modinfo "${MODBASE[@]}" -k "$REL" "$@"; }

module_dir() {
	local m=${1//-/_} f rel
	f=$(kmodinfo -F filename "$m" 2>/dev/null || true)
	case $f in
	*/lib/modules/"$REL"/kernel/*)
		rel=${f#*/lib/modules/"$REL"/kernel/}
		echo "${rel%/*}"
		return ;;
	"(builtin)") die "$1 is built into the kernel (=y); changing it needs a kernel rebuild" ;;
	esac
	local alt=${m//_/-} mk
	mk=$(grep -rlE --include=Makefile --include=Kbuild "\b(${m}|${alt})\.o\b" "$SRC/drivers" "$SRC/sound" "$SRC/net" 2>/dev/null | head -n1)
	[ -n "$mk" ] || die "cannot find module $1 in the tree"
	rel=${mk#"$SRC"/}
	echo "${rel%/*}"
}

tree_ko() {
	# Newest built .ko for a module name in the tree (either - or _ spelling).
	local m=${1//-/_}
	find "$SRC" -type f \( -name "$m.ko" -o -name "${m//_/-}.ko" \) -printf '%T@ %p\n' 2>/dev/null \
		| sort -rn | head -n1 | cut -d' ' -f2-
}

# ---- build ----------------------------------------------------------------------------------
cmd_build() {
	[ $# -ge 1 ] || die "build: give a tree directory or a module name"
	need_tree
	local arg=$1 dir
	shift
	while [ $# -gt 0 ]; do
		case $1 in
		-j) JOBS=$2; shift ;;
		*) die "build: unknown option $1" ;;
		esac
		shift
	done
	if [ -d "$SRC/${arg%/}" ]; then dir=${arg%/}; else dir=$(module_dir "$arg"); fi
	[ -f "$SRC/$dir/Makefile" ] || [ -f "$SRC/$dir/Kbuild" ] || die "$dir has no Makefile"
	local mark
	mark=$(mktemp)
	step "make M=$dir modules"
	kmake M="$SRC/$dir" modules
	local ko vm want n=0
	while IFS= read -r ko; do
		n=$((n + 1))
		vm=$(modinfo -F vermagic "$ko")
		want=$(kmodinfo -F vermagic "$(basename "$ko" .ko)" 2>/dev/null || true)
		if [ -n "$want" ] && [ "$vm" != "$want" ]; then
			echo "  ${ko#"$SRC"/}  VERMAGIC MISMATCH: '$vm' vs installed '$want'"
		else
			echo "  ${ko#"$SRC"/}"
		fi
	done < <(find "$SRC/$dir" -name '*.ko' -newer "$mark" | sort)
	rm -f "$mark"
	[ "$n" -gt 0 ] || echo "  (no .ko rebuilt: nothing changed in $dir)"
}

# ---- load -----------------------------------------------------------------------------------
loaded() { [ -d "/sys/module/${1//-/_}" ]; }
loadable() { [ -f "/sys/module/${1//-/_}/initstate" ]; }

# Holders first (post-order), the module last.
holder_order() {
	local m=${1//-/_} h
	for h in /sys/module/"$m"/holders/*; do
		[ -e "$h" ] || continue
		holder_order "$(basename "$h")"
	done
	echo "$m"
}

load_one() {
	local m=$1 ko d
	loaded "$m" && return 0
	ko=$(tree_ko "$m")
	if [ -n "$ko" ]; then
		for d in $(modinfo -F depends "$ko" | tr ',' ' '); do load_one "$d"; done
		echo "  insmod ${ko#"$SRC"/}"
		$SUDO insmod "$ko"
	else
		echo "  modprobe $m"
		$SUDO modprobe "$m"
	fi
}

cmd_load() {
	[ $# -ge 1 ] || die "load: give a module name"
	need_tree
	signing_note || die "unsigned modules cannot be loaded on this kernel"
	local target=${1//-/_} m order=() ko
	ko=$(tree_ko "$target")
	[ -n "$ko" ] || die "no built $target.ko in $SRC; run: kmod-dev.sh build $1"
	# Extra modules first: they are given because they use the target without being its holders (the
	# IPU6 ISYS keeps a reference on a bound sensor), so the target cannot go before them.
	for m in "${@:2}" "$1"; do
		m=${m//-/_}
		if loaded "$m"; then
			loadable "$m" || die "$m is built in; cannot reload"
			local x
			while read -r x; do
				[[ " ${order[*]} " == *" $x "* ]] || order+=("$x")
			done < <(holder_order "$m")
		fi
	done
	local tag
	tag="kmod-dev-$$-$(date +%s)"
	echo "kmod-dev: $tag load $target" | $SUDO tee /dev/kmsg >/dev/null || true

	for m in "${order[@]}"; do
		local rc
		rc=$(cat "/sys/module/$m/refcnt" 2>/dev/null || echo 0)
		if [ "$rc" -gt 0 ]; then
			warn "$m refcnt $rc with no module holders left: a process holds it (pipewire/wireplumber on /dev/video*, a camera app)"
		fi
		echo "  rmmod $m"
		$SUDO rmmod "$m" || die "rmmod $m failed; stop the users and retry"
	done

	# Reload: target from the tree, then the others in reverse unload order.
	load_one "$target"
	local i
	for ((i = ${#order[@]} - 1; i >= 0; i--)); do
		[ "${order[i]}" = "$target" ] && continue
		load_one "${order[i]}" || warn "reloading ${order[i]} failed"
	done
	for m in "${@:2}"; do load_one "${m//-/_}" || warn "loading $m failed"; done
	sleep 2   # asynchronous probes
	echo "---- kernel log since load ----"
	$SUDO dmesg | sed -n "/$tag/,\$p" | tail -n +2
}

# ---- patch ----------------------------------------------------------------------------------
cmd_patch() {
	need_tree
	local from
	from=$(kgit rev-parse -q --verify kdev-exported 2>/dev/null || echo kdev-base)
	[ "${1:-}" = --all ] && from=kdev-base
	[ -z "$(kgit status --porcelain --untracked-files=no)" ] || die "uncommitted changes in $SRC; commit them first (kmod-dev.sh git commit -as)"
	local count
	count=$(kgit rev-list --count "$from"..HEAD)
	[ "$count" -gt 0 ] || die "no commits after $from"
	local last next
	last=$(find "$REPO/kernel/patches" -maxdepth 1 -name '[0-9][0-9][0-9][0-9]-*.patch' -printf '%f\n' | sort | tail -n1)
	next=1
	[ -z "$last" ] || next=$((10#${last:0:4} + 1))
	# Same form as the existing series: zero commit hash, no signature, [PATCH] without numbers.
	kgit format-patch -q --zero-commit --no-signature -N --start-number "$next" \
		-o "$REPO/kernel/patches" "$from"..HEAD | sed "s|^$REPO/||"
	kgit tag -f kdev-exported HEAD >/dev/null
	echo "exported $count patch(es) from $next. Next:"
	echo "  - kernel/PROVENANCE.md: one row per patch (origin, status, evidence); kernel/README.md patch range"
	echo "  - check the series still applies: tools/linux/build-kernel.sh --prepare-only"
	echo "  - commit, then in your host flake: nix flake update surface-pro-9-linux && rebuild (full kernel build)"
}

cmd_status() {
	echo "work dir   $KDEV"
	echo "kernel     $REL  $(readlink -f /run/booted-system/kernel 2>/dev/null || echo '?')"
	echo "tree drv   $(cat "$STAMP" 2>/dev/null || echo none)"
	echo "booted drv $(booted_drv)"
	if [ -d "$GITDIR" ]; then
		local from
		from=$(kgit rev-parse -q --verify kdev-exported 2>/dev/null || echo kdev-base)
		echo "unexported commits (after $from):"
		kgit log --oneline "$from"..HEAD | sed 's/^/  /'
		kgit status --short | sed 's/^/  /'
	fi
	[ -f "$SRC/.config" ] && signing_note || true
}

[ $# -ge 1 ] || usage
cmd=$1
shift
case $cmd in
prepare) cmd_prepare "$@" ;;
build) cmd_build "$@" ;;
load) cmd_load "$@" ;;
patch) cmd_patch "$@" ;;
status) cmd_status ;;
git) kgit "$@" ;;
-h|--help|help) usage 0 ;;
*) echo "unknown command: $cmd" >&2; usage ;;
esac
