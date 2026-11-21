#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Surface Pro 9 (Intel): unattended s2idle (suspend-to-idle, the Linux side of Modern Standby) measurement.
# Each cycle suspends with "rtcwake -m freeze" for --seconds and wakes on the RTC alarm, so it runs unattended.
# Per cycle it keeps: pmc_core residencies (SLP_S0, LPM substates, package C-states), low_power_idle_*_residency_us,
# suspend_stats, wakeup sources and the wakeup IRQ, the dmesg lines of that cycle (with pm_debug_messages: LPS0 _DSM
# calls and "LPI: Constraint not met"), ftrace power and surface_aggregator events (device PM callbacks, Surface SAM requests and
# events, wakeup sources), and battery energy before/after. summary.tsv has one row per cycle.
#
# Usage: sudo s2idle-trace.sh [-n CYCLES] [-s SECONDS] [-a AWAKE_SECONDS] [-t PM_TEST] [-o OUTDIR] [--turbostat]
#   -n 3          cycles (default 3)
#   -s 120        seconds in s2idle per cycle (default 120)
#   -a 20         seconds awake between cycles (default 20)
#   -t devices    /sys/power/pm_test level for every cycle: none (default), freezer, devices, platform.
#                 s2idle accepts only these; with anything but none the system does not really sleep (driver test).
#   -o DIR        output directory (default ~/sp9-s2idle-<host>-<time>/ of the invoking user)
#   --turbostat   run each cycle under turbostat (SYS%LPI, Pkg%pc10, PkgWatt over the whole cycle)
# Battery drain is meaningful only on battery; on AC energy_now does not fall. Every setting the script changes
# (pm_test, pm_debug_messages, pm_print_times, dynamic debug, ftrace) is put back on exit.
set -u
export LC_ALL=C

CYCLES=3
SECS=120
AWAKE=20
PMTEST=none
OUT=
TURBO=0
while [ $# -gt 0 ]; do
	case $1 in
		-n) CYCLES=$2; shift 2 ;;
		-s) SECS=$2; shift 2 ;;
		-a) AWAKE=$2; shift 2 ;;
		-t) PMTEST=$2; shift 2 ;;
		-o) OUT=$2; shift 2 ;;
		--turbostat) TURBO=1; shift ;;
		-h|--help) sed -n '2,20p' "$0"; exit 0 ;;
		*) echo "unknown option: $1" >&2; exit 2 ;;
	esac
done

die() { echo "s2idle-trace: $*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }
[ "$(id -u)" -eq 0 ] || die "run as root (sudo)"
have rtcwake || die "rtcwake (util-linux) not found"
case $PMTEST in none|freezer|devices|platform) ;; *) die "pm_test for s2idle must be none, freezer, devices or platform" ;; esac
grep -qw freeze /sys/power/state || die "/sys/power/state has no freeze"

RUN_USER=${SUDO_USER:-root}
RUN_HOME=$(getent passwd "$RUN_USER" | cut -d: -f6)
[ -n "$OUT" ] || OUT=${RUN_HOME:-/root}/sp9-s2idle-$(hostname -s)-$(date +%Y%m%d-%H%M%S)
mkdir -p "$OUT" || die "cannot create $OUT"

DBG=/sys/kernel/debug
mountpoint -q "$DBG" || mount -t debugfs none "$DBG" 2>/dev/null
TR=/sys/kernel/tracing
[ -d "$TR/events" ] || TR=$DBG/tracing
PMC=$DBG/pmc_core
LPI=/sys/devices/system/cpu/cpuidle

# ---- settings changed for the run, restored by cleanup ---------------------------------------------------------------
readw() { [ -r "$1" ] && cat "$1" 2>/dev/null; }
writew() { [ -w "$1" ] && printf '%s\n' "$2" > "$1" 2>/dev/null; }
pmtest_now() { sed -n 's/.*\[\(.*\)\].*/\1/p' /sys/power/pm_test 2>/dev/null; }
OLD_PMTEST=$(pmtest_now)
OLD_DEBUG=$(readw /sys/power/pm_debug_messages)
OLD_TIMES=$(readw /sys/power/pm_print_times)
OLD_BUF=$(readw "$TR/buffer_size_kb")
DYNDBG=(
	'file drivers/acpi/x86/s2idle.c +p'
	'module surface_aggregator +p'
	'module surface_platform_profile +p'
	'file drivers/base/power/wakeup.c +p'
)
TRACE_EVENTS=(
	power/suspend_resume power/device_pm_callback_start power/device_pm_callback_end
	power/wakeup_source_activate power/wakeup_source_deactivate surface_aggregator
)

cleanup() {
	[ -n "$OLD_PMTEST" ] && writew /sys/power/pm_test "$OLD_PMTEST"
	[ -n "$OLD_DEBUG" ] && writew /sys/power/pm_debug_messages "$OLD_DEBUG"
	[ -n "$OLD_TIMES" ] && writew /sys/power/pm_print_times "$OLD_TIMES"
	local d
	for d in "${DYNDBG[@]}"; do writew "$DBG/dynamic_debug/control" "${d%+p}-p"; done
	if [ -d "$TR/events" ]; then
		writew "$TR/tracing_on" 0
		[ -n "$OLD_BUF" ] && writew "$TR/buffer_size_kb" "$OLD_BUF"
		local e
		for e in "${TRACE_EVENTS[@]}"; do writew "$TR/events/$e/enable" 0; done
	fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

writew /sys/power/pm_debug_messages 1
writew /sys/power/pm_print_times 1
for d in "${DYNDBG[@]}"; do writew "$DBG/dynamic_debug/control" "$d"; done
writew /sys/power/pm_test "$PMTEST" || [ "$PMTEST" = none ] || die "cannot set pm_test=$PMTEST (CONFIG_PM_DEBUG?)"
TRACING=0
if [ -d "$TR/events" ]; then
	writew "$TR/tracing_on" 0
	writew "$TR/buffer_size_kb" 8192
	for e in "${TRACE_EVENTS[@]}"; do writew "$TR/events/$e/enable" 1 && TRACING=1; done
fi

# ---- static facts ------------------------------------------------------------------------------------------------
{
	echo "kernel $(uname -r)"
	echo "cmdline $(cat /proc/cmdline)"
	echo "mem_sleep $(readw /sys/power/mem_sleep)"
	echo "pm_test $(readw /sys/power/pm_test) (run: $PMTEST)"
	echo "cycles=$CYCLES seconds=$SECS awake=$AWAKE turbostat=$TURBO tracing=$TRACING"
	for p in /sys/class/power_supply/*; do
		[ -r "$p/type" ] || continue
		echo "power_supply $(basename "$p") type=$(readw "$p/type") online=$(readw "$p/online") status=$(readw "$p/status")"
	done
	printf 'pmc_core:'; for f in "$PMC"/*; do [ -e "$f" ] && printf ' %s' "${f##*/}"; done; echo
	echo "wakeup-enabled devices:"
	for w in /sys/bus/*/devices/*/power/wakeup; do
		[ "$(readw "$w")" = enabled ] && echo "  ${w%/power/wakeup}"
	done
	echo "ACPI wakeup:"; readw /proc/acpi/wakeup
} > "$OUT/run.txt" 2>&1
[ -r "$PMC/substate_requirements" ] && cat "$PMC/substate_requirements" > "$OUT/substate_requirements.txt" 2>&1
[ -r "$PMC/pch_ip_power_gating_status" ] && cat "$PMC/pch_ip_power_gating_status" > "$OUT/pg_status-before.txt" 2>&1

BAT=
for p in /sys/class/power_supply/*; do [ "$(readw "$p/type")" = Battery ] && { BAT=$p; break; }; done

# energy in uWh (energy_now, or charge_now * voltage_now)
energy() {
	[ -n "$BAT" ] || { echo NA; return; }
	local e c v
	e=$(readw "$BAT/energy_now")
	if [ -n "$e" ]; then echo "$e"; return; fi
	c=$(readw "$BAT/charge_now"); v=$(readw "$BAT/voltage_now")
	if [ -n "$c" ] && [ -n "$v" ]; then echo $(( c * (v / 1000) / 1000 )); else echo NA; fi
}

# snapshot DIR TAG: every counter that changes across a suspend
snapshot() {
	local d=$1 t=$2 f
	for f in slp_s0_residency_usec substate_residencies package_cstate_show die_c6_us_show ltr_show substate_status_registers; do
		[ -r "$PMC/$f" ] && cat "$PMC/$f" > "$d/$t-pmc-$f.txt" 2>&1
	done
	for f in low_power_idle_cpu_residency_us low_power_idle_system_residency_us; do
		[ -r "$LPI/$f" ] && cat "$LPI/$f" > "$d/$t-$f.txt"
	done
	[ -d /sys/power/suspend_stats ] && grep -r . /sys/power/suspend_stats/ 2>/dev/null | sed 's|/sys/power/suspend_stats/||' > "$d/$t-suspend_stats.txt"
	[ -r "$DBG/wakeup_sources" ] && cat "$DBG/wakeup_sources" > "$d/$t-wakeup_sources.txt"
	cat /proc/interrupts > "$d/$t-interrupts.txt"
	energy > "$d/$t-energy_uwh.txt"
}

num() { [ -r "$1" ] && tr -dc '0-9' < "$1"; }
# delta DIR NAME: after - before of a single-number counter file, NA if missing
delta() {
	local a b
	a=$(num "$1/after-$2.txt"); b=$(num "$1/before-$2.txt")
	if [ -n "$a" ] && [ -n "$b" ]; then echo $(( a - b )); else echo NA; fi
}
kv() { sed -n "s/^$2:\(.*\)/\1/p" "$1" 2>/dev/null | tr -d ' '; }

# wakeup sources whose event_count changed (name delta)
wake_delta() {
	[ -r "$1" ] && [ -r "$2" ] || return 0
	awk 'NR==FNR { if (FNR > 1) b[$1] = $3; next } FNR > 1 && ($3 - b[$1]) > 0 { printf "%s+%d ", $1, $3 - b[$1] }' "$1" "$2"
}

printf 'cycle\trequested_s\telapsed_s\trc\tslp_s0_us\tlpi_sys_us\tlpi_cpu_us\thw_sleep_us\tenergy_uwh\tdrain_mw\twakeup_irq\twake_sources\tconstraints_not_met\n' > "$OUT/summary.tsv"

for c in $(seq 1 "$CYCLES"); do
	d=$OUT/cycle$c
	mkdir -p "$d"
	snapshot "$d" before
	[ "$TRACING" -eq 1 ] && { writew "$TR/trace" ""; writew "$TR/tracing_on" 1; }
	echo "s2idle-trace: cycle $c begin" > /dev/kmsg
	t0=$(date +%s.%N)
	if [ "$TURBO" -eq 1 ] && have turbostat; then
		turbostat --quiet --out "$d/turbostat.txt" rtcwake -m freeze -s "$SECS" > "$d/rtcwake.txt" 2>&1
	else
		rtcwake -m freeze -s "$SECS" > "$d/rtcwake.txt" 2>&1
	fi
	rc=$?
	t1=$(date +%s.%N)
	echo "s2idle-trace: cycle $c end" > /dev/kmsg
	[ "$TRACING" -eq 1 ] && { writew "$TR/tracing_on" 0; cat "$TR/trace" > "$d/ftrace.txt"; }
	readw /sys/power/pm_wakeup_irq > "$d/pm_wakeup_irq.txt"
	snapshot "$d" after
	dmesg | sed -n "/s2idle-trace: cycle $c begin/,/s2idle-trace: cycle $c end/p" > "$d/dmesg.txt"

	el=$(awk -v a="$t0" -v b="$t1" 'BEGIN { printf "%.1f", b - a }')
	s0=$(delta "$d" pmc-slp_s0_residency_usec)
	ls=$(delta "$d" low_power_idle_system_residency_us)
	lc=$(delta "$d" low_power_idle_cpu_residency_us)
	hw=$(kv "$d/after-suspend_stats.txt" last_hw_sleep)
	e0=$(cat "$d/before-energy_uwh.txt"); e1=$(cat "$d/after-energy_uwh.txt")
	if [ "$e0" != NA ] && [ "$e1" != NA ]; then
		ed=$(( e0 - e1 ))
		mw=$(awk -v e="$ed" -v t="$el" 'BEGIN { if (t > 0) printf "%.0f", e / t * 3.6; else print "NA" }')
	else ed=NA; mw=NA; fi
	irq=$(cat "$d/pm_wakeup_irq.txt" 2>/dev/null)
	ws=$(wake_delta "$d/before-wakeup_sources.txt" "$d/after-wakeup_sources.txt")
	cn=$(grep -c 'Constraint not met' "$d/dmesg.txt")
	printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$c" "$SECS" "$el" "$rc" "$s0" "$ls" "$lc" "${hw:-NA}" \
		"$ed" "$mw" "${irq:-NA}" "${ws:-none}" "$cn" >> "$OUT/summary.tsv"
	echo "cycle $c: elapsed ${el}s rc=$rc slp_s0=${s0}us lpi_sys=${ls}us drain=${mw}mW wake_irq=${irq:-NA} wake=${ws:-none}"

	[ "$c" -lt "$CYCLES" ] && sleep "$AWAKE"
done

# substate residency deltas of the last cycle (name before after)
last=$OUT/cycle$CYCLES
if [ -r "$last/before-pmc-substate_residencies.txt" ]; then
	paste "$last/before-pmc-substate_residencies.txt" "$last/after-pmc-substate_residencies.txt" > "$OUT/substate_residencies-last.txt"
fi
[ -r "$PMC/pch_ip_power_gating_status" ] && cat "$PMC/pch_ip_power_gating_status" > "$OUT/pg_status-after.txt" 2>&1
chown -R "$RUN_USER" "$OUT" 2>/dev/null
echo "output: $OUT"
column -t -s "$(printf '\t')" "$OUT/summary.tsv" 2>/dev/null || cat "$OUT/summary.tsv"
