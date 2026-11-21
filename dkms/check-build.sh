#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# check-build.sh -- build a generated DKMS tree out of tree against a kernel's headers and check the
# result. Read-only for the system: it builds in a temporary directory and loads nothing.
#
#   dkms/check-build.sh [--modules DIR] [--keep DIR] TREE KDIR
#
#   TREE       a tree written by `dkms/gen.sh tree` (sp9 or sp9-bluetooth)
#   KDIR       the kernel's build directory (the header package's /usr/src/linux-headers-*,
#              /usr/lib/modules/*/build)
#   --modules  the kernel's /lib/modules/<release> (from its image package): the vermagic of each
#              built module is compared with the stock module of the same name
#   --keep     build in DIR and leave it there
#
# Checks: the build has no modpost errors or unresolved symbols; every module in dkms.conf was
# built; its vermagic starts with the release in KDIR (and equals the stock module's with
# --modules); with CONFIG_MODVERSIONS, the CRC of each exported symbol named by "check-crcs" in
# SOURCES (or of every symbol the module exports, for "all") equals the one in KDIR/Module.symvers,
# so the stock modules that import them still load. Exit status 1 on any failure.
set -euo pipefail
export LC_ALL=C

STOCK= KEEP=
while [ $# -gt 0 ]; do
	case $1 in
	--modules) STOCK=$2; shift 2 ;;
	--keep) KEEP=$2; shift 2 ;;
	-h | --help) sed -n '4,22p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
	*) break ;;
	esac
done
[ $# = 2 ] || { echo "usage: check-build.sh [--modules DIR] [--keep DIR] TREE KDIR" >&2; exit 2; }
TREE=$(readlink -f "$1"); KDIR=$(readlink -f "$2")
KREL=$(cat "$KDIR/include/config/kernel.release")
fail=0
bad() { echo "FAIL: $*"; fail=1; }
ok() { echo "ok: $*"; }

if [ -n "$KEEP" ]; then B=$KEEP; rm -rf "$B"; mkdir -p "$B"; else B=$(mktemp -d); trap 'rm -rf "$B"' EXIT; fi
cp -a "$TREE/." "$B/"
(cd "$B" && ./select-variant "$KREL")
variant=$(awk -v k="$KREL" '!/^#/ && $1 == k { print $2 }' "$B/kernels")

echo "== build $(sed -n 's/^PACKAGE_NAME="\(.*\)"/\1/p' "$B/dkms.conf") for $KREL (variant $variant)"
if ! make -C "$KDIR" M="$B" -j"$(nproc)" modules >"$B/build.log" 2>&1; then
	tail -n 40 "$B/build.log"; bad "make failed"; exit 1
fi
if grep -E 'modpost: .*(undefined|ERROR)|^ERROR' "$B/build.log"; then bad "modpost reported problems"; fi
grep -E '^(warning|WARNING)|: warning:' "$B/build.log" | sort -u | sed 's/^/  build: /' || true

modversions=$(grep -c '^CONFIG_MODVERSIONS=y' "$KDIR/.config" || true)
mapfile -t mods < <(sed -n 's/^BUILT_MODULE_NAME\[[0-9]*\]="\(.*\)"/\1/p' "$B/dkms.conf")
for m in "${mods[@]}"; do
	ko=$B/$m/$m.ko
	[ -f "$ko" ] || { bad "$m: $m.ko not built"; continue; }
	vm=$(modinfo -F vermagic "$ko")
	[ "${vm%% *}" = "$KREL" ] || bad "$m: vermagic '$vm' is not for $KREL"
	if [ -n "$STOCK" ]; then
		s=$(find "$STOCK" -name "$m.ko*" -print -quit)
		if [ -z "$s" ]; then
			echo "  $m: no stock module (new module)"
		elif [ "$(modinfo -F vermagic "$s")" = "$vm" ]; then
			echo "  $m: vermagic equals stock ${s#"$STOCK"/}"
		else
			bad "$m: vermagic '$vm' differs from stock '$(modinfo -F vermagic "$s")'"
		fi
	fi
	ok "$m: built, vermagic '$vm'"
done

# CRCs of exported symbols against the kernel's Module.symvers.
awk '/^target / { t = ($5 == v) } t && /^module / { m = $2 } t && /check-crcs/ { $1 = ""; print m $0 }' v="$variant" "$B/SOURCES" |
	while read -r m syms; do
		[ "$syms" = all ] && syms=$(awk -F'\t' -v m="$m" '$3 ~ ("(^|/)" m "$") { print $2 }' "$B/Module.symvers" | sort)
		n=0 same=0
		for s in $syms; do
			n=$((n + 1))
			ours=$(awk -F'\t' -v s="$s" '$2 == s { print $1 }' "$B/Module.symvers")
			theirs=$(awk -F'\t' -v s="$s" '$2 == s { print $1 }' "$KDIR/Module.symvers")
			if [ -z "$ours" ]; then bad "$m: $s not exported by the built module"
			elif [ -z "$theirs" ]; then echo "  $m: $s is new (not in the kernel's Module.symvers)"; same=$((same + 1))
			elif [ "$ours" = "$theirs" ]; then same=$((same + 1))
			else bad "$m: CRC of $s is $ours, the kernel's is $theirs"
			fi
		done
		if [ "$modversions" = 0 ]; then
			ok "$m: $n exported symbols present (CONFIG_MODVERSIONS is off: no CRCs to compare)"
		else
			ok "$m: $same of $n exported symbols match the kernel's Module.symvers CRCs"
		fi
	done | tee "$B/crc.log"
grep -q '^FAIL' "$B/crc.log" && fail=1

[ $fail = 0 ] && echo "== all checks passed for $KREL" || echo "== checks FAILED for $KREL"
exit $fail
