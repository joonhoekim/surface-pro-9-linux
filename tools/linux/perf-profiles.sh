#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Surface Pro 9 (Intel): the same CPU load under each platform profile, then one long run (test plan SAM-5, PWR-9).
# Per run: 20 s idle, LOAD seconds of "stress-ng --cpu N", COOL seconds idle. Recorded every 5 s: turbostat
# (Busy%, Bzy_MHz, PkgTmp, PkgWatt, CorWatt, GFXWatt) and sensors (fan rpm, package, the hottest surface_thermal
# channel and temp9, the skin sensor). summary.tsv has one row per run. The Linux side of
# tools/windows/perf-modes.ps1.
#
# Usage: sudo perf-profiles.sh [-l LOAD] [-c COOL] [-s SUSTAINED] [-o OUTDIR] [PROFILE...]
#   -l 120   seconds of load per profile
#   -c 70    seconds of cooling after each load
#   -s 600   seconds of the long run under "performance" after the profiles (0: skip)
#   -o DIR   output directory (default ~/sp9-perf-<host>-<time>/ of the invoking user)
#   PROFILE  default: every entry of platform_profile_choices
# The profile is written to /sys/firmware/acpi/platform_profile and put back on exit. power-profiles-daemon
# keeps its own idea of the profile; a change made through it during the run would be recorded as it is.
# The numbers mean something only on an otherwise idle machine.
set -u
export LC_ALL=C

LOAD=120
COOL=70
SUSTAINED=600
OUT=
PROFILES=()
while [ $# -gt 0 ]; do
	case $1 in
		-l) LOAD=$2; shift 2 ;;
		-c) COOL=$2; shift 2 ;;
		-s) SUSTAINED=$2; shift 2 ;;
		-o) OUT=$2; shift 2 ;;
		-h|--help) sed -n '2,18p' "$0"; exit 0 ;;
		-*) echo "unknown option: $1" >&2; exit 2 ;;
		*) PROFILES+=("$1"); shift ;;
	esac
done

die() { echo "perf-profiles: $*" >&2; exit 1; }
[ "$(id -u)" -eq 0 ] || die "run as root (sudo)"
for t in stress-ng turbostat; do command -v "$t" >/dev/null 2>&1 || die "$t not found"; done
PP=/sys/firmware/acpi/platform_profile
[ -w "$PP" ] || die "$PP is not writable"
[ ${#PROFILES[@]} -gt 0 ] || read -r -a PROFILES < /sys/firmware/acpi/platform_profile_choices

RUN_USER=${SUDO_USER:-root}
RUN_HOME=$(getent passwd "$RUN_USER" | cut -d: -f6)
[ -n "$OUT" ] || OUT=${RUN_HOME:-/root}/sp9-perf-$(hostname -s)-$(date +%Y%m%d-%H%M%S)
mkdir -p "$OUT" || die "cannot create $OUT"

hwmon_of() { local f; for f in /sys/class/hwmon/hwmon*/name; do [ "$(cat "$f")" = "$1" ] && { dirname "$f"; return; }; done; }
FAN=$(hwmon_of surface_fan)
THM=$(hwmon_of surface_thermal)
PKG=
for z in /sys/class/thermal/thermal_zone*; do [ "$(cat "$z/type")" = x86_pkg_temp ] && PKG=$z/temp; done
NCPU=$(nproc)
# Without msr turbostat still runs but has no temperature and no RAPL columns on this CPU.
modprobe msr 2>/dev/null

ORIG=$(cat "$PP")
trap 'echo "$ORIG" > "$PP"; pkill -x stress-ng 2>/dev/null' EXIT

mc() { [ -r "$1" ] && echo $(( $(cat "$1") / 1000 )) || echo -; }

sample() { # sample FILE SECONDS
	local end=$((SECONDS + $2)) f t mx
	printf 't\tfan_rpm\tpkg_c\tsam_max_c\tsam_temp9_c\tprofile\tcpu0_khz\n' > "$1"
	while [ $SECONDS -lt $end ]; do
		mx=0
		if [ -n "$THM" ]; then
			for f in "$THM"/temp*_input; do t=$(cat "$f" 2>/dev/null || echo 0); [ "$t" -gt "$mx" ] && mx=$t; done
		fi
		printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$(date +%T)" "$( [ -n "$FAN" ] && cat "$FAN/fan1_input" || echo -)" \
			"$(mc "$PKG")" "$((mx / 1000))" "$(mc "${THM:-/nonexistent}/temp9_input")" "$(cat "$PP")" \
			"$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq)" >> "$1"
		sleep 5
	done
}

printf 'run\tprofile\tload_s\tidle_w\tload_w_avg\tload_w_last30\tpkg_max_c\tsam_max_c\ttemp9_end_c\tfan_max_rpm\tfan_end_load_rpm\tmhz_last30\tbogo_ops_s\n' > "$OUT/summary.tsv"

run() { # run LABEL PROFILE LOAD_SECONDS
	local label=$1 prof=$2 load=$3 total=$((20 + $3 + COOL)) sp tp
	echo "$prof" > "$PP"
	echo "== $label: profile $(cat "$PP"), idle 20 s, load $load s, cool $COOL s  ($(date +%T))"
	sample "$OUT/$label-sensors.tsv" "$total" &
	sp=$!
	turbostat --quiet --Summary --interval 5 --num_iterations $((total / 5)) \
		--show Busy%,Bzy_MHz,PkgTmp,PkgWatt,CorWatt,GFXWatt > "$OUT/$label-turbostat.txt" 2>&1 &
	tp=$!
	sleep 20
	stress-ng --cpu "$NCPU" -t "$load" --metrics-brief > "$OUT/$label-stress.txt" 2>&1
	wait "$sp" "$tp"
	# turbostat rows: 4 idle, then load/5 rows of load. Columns: Busy% Bzy_MHz PkgTmp PkgWatt CorWatt GFXWatt.
	local n=$((load / 5)) stats bogo fanmax fanend sam t9
	stats=$(awk -v n="$n" '$1+0==$1 { r++; if (r<=4) { iw+=$4; ic++ } else if (r<=4+n) { w+=$4; c++; if ($3>t) t=$3;
			if (r>4+n-6) { lw+=$4; lm+=$2; lc++ } } }
		END { printf "%.1f\t%.1f\t%.1f\t%d\t%d", ic?iw/ic:0, c?w/c:0, lc?lw/lc:0, t, lc?lm/lc:0 }' "$OUT/$label-turbostat.txt")
	bogo=$(awk '$4=="cpu" || $5=="cpu" { v=$(NF-1) } END { print v+0 }' "$OUT/$label-stress.txt")
	fanmax=$(awk -F'\t' 'NR>1 && $2+0>m { m=$2+0 } END { print m+0 }' "$OUT/$label-sensors.tsv")
	fanend=$(awk -F'\t' -v r=$((1 + 4 + n)) 'NR==r { print $2 }' "$OUT/$label-sensors.tsv")
	sam=$(awk -F'\t' 'NR>1 && $4+0>m { m=$4+0 } END { print m+0 }' "$OUT/$label-sensors.tsv")
	t9=$(awk -F'\t' -v r=$((1 + 4 + n)) 'NR==r { print $5 }' "$OUT/$label-sensors.tsv")
	IFS=$'\t' read -r iw w lw tmax mhz <<< "$stats"
	printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$label" "$prof" "$load" "$iw" "$w" "$lw" "$tmax" \
		"$sam" "$t9" "$fanmax" "$fanend" "$mhz" "$bogo" >> "$OUT/summary.tsv"
	echo "   idle $iw W, load avg $w W (last 30 s $lw W, $mhz MHz), pkg max $tmax C, SAM max $sam C, temp9 $t9 C, fan max $fanmax rpm"
}

for p in "${PROFILES[@]}"; do run "$p" "$p" "$LOAD"; done
[ "$SUSTAINED" -gt 0 ] && run "sustained-${SUSTAINED}s" performance "$SUSTAINED"
echo "$ORIG" > "$PP"
echo "profile restored: $(cat "$PP")"
echo "summary: $OUT/summary.tsv"
column -t -s$'\t' "$OUT/summary.tsv" 2>/dev/null || cat "$OUT/summary.tsv"
