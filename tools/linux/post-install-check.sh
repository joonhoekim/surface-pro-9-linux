#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Surface Pro 9 (Intel): gather everything the post-install test items of docs/research/gaps ask for that can be read unattended.
# Read-only: no module loads, no sysfs/debugfs writes, no i2c or hda-verb access, no camera streaming, no network.
# Usage: post-install-check.sh [OUTDIR]      (default: ~/sp9-check-<host>/ of the invoking user)
#        sudo post-install-check.sh [OUTDIR]  (adds root-only parts: dmesg, debugfs, ACPI tables, libinput, turbostat)
# Output: one file per area plus summary.txt. MAC addresses (also in the dev_XX_XX form of BlueZ), UUIDs, serial
# lines and Wi-Fi network names are masked in every file; Bluetooth device names other than the pen's are left out,
# and the EDID is kept only as edid-decode text without its hex dump.
# Status words in summary.txt: MATCH (as docs expect), DIFF (differs), MISSING, INFO, SKIP (needs root or a tool).
# MATCH is not "works": the tests that decide it are in docs/test-plan.md.
# DIFF and MISSING are statements about the machine. A check whose tool is not installed reports SKIP with the
# tool name, or reads sysfs/procfs instead; it never reports MISSING or a count of 0.

# sum() always succeeds, so "A && sum || sum" is a plain if/else; sh -c bodies are single-quoted on purpose.
# shellcheck disable=SC2015,SC2016,SC2012,SC2129
set -u
export LC_ALL=C

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$SCRIPT_DIR/../.." 2>/dev/null && pwd)
IS_ROOT=0
[ "$(id -u)" -eq 0 ] && IS_ROOT=1

RUN_USER=${SUDO_USER:-$(id -un)}
RUN_HOME=$(getent passwd "$RUN_USER" | cut -d: -f6)
RUN_UID=$(id -u "$RUN_USER")
OUT=${1:-${RUN_HOME:-$HOME}/sp9-check-$(hostname -s)}
mkdir -p "$OUT" || { echo "cannot create $OUT" >&2; exit 1; }
SUMMARY=$OUT/summary.txt
: > "$SUMMARY"

# NixOS has no /usr/bin: system tools live in these profiles, and root's PATH (sudo secure_path, a systemd unit)
# does not always include them. Appended, so a distro's own PATH order is unchanged.
for d in /run/current-system/sw/bin "/etc/profiles/per-user/$RUN_USER/bin" "${RUN_HOME:-/nonexistent}/.nix-profile/bin"; do
	[ -d "$d" ] && case ":$PATH:" in *":$d:"*) ;; *) PATH=$PATH:$d ;; esac
done
export PATH

have() { command -v "$1" >/dev/null 2>&1; }

redact() {
	sed -E \
		-e 's/([0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}/XX:XX:XX:XX:XX:XX/g' \
		-e 's/([0-9A-Fa-f]{2}_){5}[0-9A-Fa-f]{2}/XX_XX_XX_XX_XX_XX/g' \
		-e 's/^([[:space:]]*(ssid|SSID)[[:space:]:=]+).*/\1<masked>/' \
		-e 's/[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}/<uuid>/g' \
		-e 's/(([Ss]erial|SERIAL)([ _]?([Nn]umber|NUMBER|[Nn]o|SHORT))?[[:space:]]*[:=][[:space:]]*).*/\1<masked>/'
}
# The serial rule needs ":" or "=" right after the word (plus an optional number/no suffix). Allowing other
# text in between also blanks driver lines such as "surface_serial_hub serial0-0: SAM firmware version: ...".

# Reference hashes of firmware-derived files (the files themselves are not in the repository).
# refhash NAME -> sha256, refsize NAME -> bytes, refname SHA256 PREFIX -> name of the row with that hash.
# NAME is matched as a prefix, first row wins. All print nothing when the table or the row is absent.
REFHASH=$REPO/docs/reference/firmware-hashes.tsv
refhash() { [ -r "$REFHASH" ] && awk -F'\t' -v n="$1" '!/^#/ && index($3, n) == 1 { print $1; exit }' "$REFHASH"; }
refsize() { [ -r "$REFHASH" ] && awk -F'\t' -v n="$1" '!/^#/ && index($3, n) == 1 { print $2; exit }' "$REFHASH"; }
refname() { [ -r "$REFHASH" ] && awk -F'\t' -v h="$1" -v n="$2" '!/^#/ && $1 == h && index($3, n) == 1 { print $3; exit }' "$REFHASH"; }

# sum AREA ID STATUS TEXT
sum() { printf '%-6s %-10s %-8s %s\n' "$1" "$2" "$3" "$4" >> "$SUMMARY"; }

# run FILE TITLE CMD...  (output appended, masked, 60 s limit)
run() {
	local f=$OUT/$1 t=$2; shift 2
	{
		printf '\n### %s\n$ %s\n' "$t" "$*"
		if have "$1"; then timeout 60 "$@" 2>&1 </dev/null | redact; else echo "(not installed: $1)"; fi
	} >> "$f"
}

# runsh FILE TITLE TOOL SCRIPT  (sh -c SCRIPT, only when TOOL exists: "sh" always exists, so run() alone would
# leave an empty section that reads as "the tool found nothing")
runsh() {
	if have "$3"; then run "$1" "$2" sh -c "$4"; else printf '\n### %s\n(not installed: %s)\n' "$2" "$3" >> "$OUT/$1"; fi
}

# rootrun FILE TITLE CMD...
rootrun() {
	if [ "$IS_ROOT" -eq 1 ]; then run "$@"; else printf '\n### %s\n(root needed: skipped)\n' "$2" >> "$OUT/$1"; fi
}

# userrun FILE TITLE CMD...  (session tools such as wpctl, run as the desktop user)
userrun() {
	if ! have "$3"; then
		printf '\n### %s\n(not installed: %s)\n' "$2" "$3" >> "$OUT/$1"
	elif [ "$IS_ROOT" -eq 1 ] && [ "$RUN_USER" != root ]; then
		run "$1" "$2" sudo -u "$RUN_USER" XDG_RUNTIME_DIR="/run/user/$RUN_UID" "${@:3}"
	else
		run "$@"
	fi
}

# cats FILE TITLE PATH...  (print "path: value" for readable files)
cats() {
	local f=$OUT/$1 t=$2 p; shift 2
	printf '\n### %s\n' "$t" >> "$f"
	for p in "$@"; do
		[ -e "$p" ] || continue
		if [ -r "$p" ] && [ -f "$p" ]; then
			printf '%s: %s\n' "$p" "$(timeout 5 cat "$p" 2>&1 | tr '\n' ' ' | redact)" >> "$f"
		else
			printf '%s: (not readable)\n' "$p" >> "$f"
		fi
	done
}

rd() { [ -r "$1" ] && timeout 5 cat "$1" 2>/dev/null; }

echo "writing to $OUT (root: $IS_ROOT)"

# ---------------------------------------------------------------- kernel log
# The longer of journal and dmesg is kept. The dmesg ring buffer wraps (a streaming camera fills it within
# minutes) and then the boot-time lines the checks below look for are gone; a live session may have no journal.
KLOG=$OUT/kernel-log.txt
journalctl -k -b -o short-monotonic --no-hostname --no-pager 2>/dev/null | redact > "$KLOG.journal"
dmesg 2>/dev/null | redact > "$KLOG.dmesg"
if [ "$(wc -l < "$KLOG.journal")" -ge "$(wc -l < "$KLOG.dmesg")" ]; then mv "$KLOG.journal" "$KLOG"; else mv "$KLOG.dmesg" "$KLOG"; fi
rm -f "$KLOG.journal" "$KLOG.dmesg"
if [ ! -s "$KLOG" ]; then
	echo "(kernel log not readable: run with sudo or join group adm)" > "$KLOG"
	sum SYS klog SKIP "kernel log not readable"
fi
kgrep() { grep -aiE "$1" "$KLOG" 2>/dev/null; }

# ---------------------------------------------------------------- system
F=00-system.txt
run $F "kernel" uname -srvm
cats $F "cmdline" /proc/cmdline
cats $F "os" /etc/os-release
cats $F "dmi (model-level only)" /sys/class/dmi/id/{sys_vendor,product_name,product_sku,product_family,bios_vendor,bios_version,bios_date,board_name}
run $F "secure boot" mokutil --sb-state
runsh $F "enrolled MOK subjects" mokutil 'mokutil --list-enrolled 2>&1 | grep -E "Subject:|CN="'
cats $F "lockdown" /sys/kernel/security/lockdown
run $F "boot entries" efibootmgr
run $F "lsmod" lsmod
run $F "pci" lspci -nnk
run $F "usb" lsusb
run $F "usb tree" lsusb -t
run $F "block devices" lsblk -o NAME,SIZE,TYPE,FSTYPE,PARTTYPENAME,MOUNTPOINTS
run $F "failed units" systemctl --failed --no-pager
{ printf '\n### firmware load failures\n'; kgrep 'firmware.*(fail|error|-2)|direct firmware load'; } >> "$OUT/$F"
{ printf '\n### kernel errors/warnings (level err/warn)\n'; } >> "$OUT/$F"
rootrun $F "dmesg -l err,warn" dmesg -l err,warn

# Whether the linux-surface patches are in the running kernel. The release string only says so for the
# linux-surface packages; this repo's Nix kernel is plain "7.3.4". ithc exists only in the linux-surface series
# (not in mainline), so its config symbol or module is the evidence.
kver=$(uname -r)
kev=""
case $kver in *surface*) kev="release string" ;; esac
if [ -z "$kev" ]; then
	if [ -r /proc/config.gz ] && have zcat && zcat /proc/config.gz 2>/dev/null | grep -q '^CONFIG_HID_ITHC=[ym]'; then kev="CONFIG_HID_ITHC in /proc/config.gz"
	elif grep -qs '^CONFIG_HID_ITHC=[ym]' "/boot/config-$kver"; then kev="CONFIG_HID_ITHC in /boot/config-$kver"
	elif [ -d /sys/module/ithc ]; then kev="ithc module loaded"
	elif have modinfo && modinfo ithc >/dev/null 2>&1; then kev="ithc module installed"
	fi
fi
[ -n "$kev" ] && sum SYS kernel MATCH "$kver, linux-surface patches present ($kev)" || sum SYS kernel DIFF "$kver: no ithc in config or modules (linux-surface patches not in this kernel)"
sku=$(rd /sys/class/dmi/id/product_sku)
[ "$sku" = Surface_Pro_9_2038 ] && sum SYS sku MATCH "$sku" || sum SYS sku DIFF "sku=$sku"
# The SecureBoot EFI variable is 4 attribute bytes and one value byte; reading it needs no mokutil.
sbv=$(od -An -tu1 -j4 -N1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c 2>/dev/null | tr -d ' ')
case $sbv in 1) sb="SecureBoot enabled" ;; 0) sb="SecureBoot disabled" ;; *) sb="SecureBoot state not readable" ;; esac
printf '\n### secure boot (EFI variable)\n%s\n' "$sb" >> "$OUT/$F"
sum SYS secureboot INFO "$sb; lockdown: $(rd /sys/kernel/security/lockdown || echo "no lockdown LSM")"
cmd=$(rd /proc/cmdline)
for p in pci=hpiosize=0 i915.enable_psr snd_intel_dspcfg.dsp_driver; do
	case " $cmd " in *" $p"*) sum SYS cmdline INFO "has $(grep -oE "${p}[^ ]*" <<<"$cmd")" ;; *) sum SYS cmdline INFO "no $p" ;; esac
done
nerr=$(kgrep 'firmware.*(fail|error)|direct firmware load' | wc -l)
if grep -q '^(kernel log not readable' "$KLOG"; then sum SYS fw-load SKIP "kernel log not readable"
elif [ "$nerr" -eq 0 ]; then sum SYS fw-load MATCH "no firmware load failures"
else sum SYS fw-load DIFF "$nerr firmware load failure lines (00-system.txt)"; fi

# ---------------------------------------------------------------- pen / touch
F=10-pen-touch.txt
{ printf '\n### kernel log: ithc / ipts / hid 0C52\n'; kgrep 'ithc|ipts|0c52'; } >> "$OUT/$F"
run $F "iptsd units" systemctl list-units --all --no-pager 'iptsd*'
run $F "iptsd journal" journalctl -b --no-pager --no-hostname -o short-monotonic -u 'iptsd*'
# The iptsd tools sit next to the daemon binary. On NixOS that is a store path that is not on PATH, and the
# presets are under the same prefix instead of /usr/share.
# As root the tool is run, so a directory found through a process or a unit is used only when the tool and
# every directory above it belong to root, and only root can write to them or, for a sticky directory such as
# /nix/store, rename what is in them. Anyone can start a program named iptsd from /dev/shm.
trusted_tool() {
	local p m
	p=$(readlink -f "$1" 2>/dev/null) || return 1
	[ -f "$p" ] && [ -x "$p" ] || return 1
	[ "$(stat -c %u "$p")" = 0 ] || return 1
	case $(stat -c %A "$p") in ?????w*|????????w*) return 1 ;; esac
	while [ "$p" != / ]; do
		p=$(dirname "$p")
		[ "$(stat -c %u "$p" 2>/dev/null)" = 0 ] || return 1
		m=$(stat -c %A "$p")
		case $m in ?????????[tT]) ;; ?????w*|????????w*) return 1 ;; esac
	done
}
IPTSD_BIN=""
if have iptsd-check-device && { [ "$IS_ROOT" -ne 1 ] || trusted_tool "$(command -v iptsd-check-device)"; }; then
	IPTSD_BIN=$(dirname "$(command -v iptsd-check-device)")
else
	for pid in $(pgrep -x -u 0 iptsd 2>/dev/null); do
		e=$(readlink "/proc/$pid/exe" 2>/dev/null)
		[ -n "$e" ] && trusted_tool "$(dirname "$e")/iptsd-check-device" && IPTSD_BIN=$(dirname "$e") && break
	done
	if [ -z "$IPTSD_BIN" ]; then
		e=$(systemctl cat 'iptsd@.service' 2>/dev/null | sed -n 's/^ExecStart=[-@+!]*\([^ ]*\).*/\1/p' | head -1)
		[ -n "$e" ] && trusted_tool "$(dirname "$e")/iptsd-check-device" && IPTSD_BIN=$(dirname "$e")
	fi
fi
export IPTSD_BIN
run $F "iptsd packages" sh -c 'dpkg -l "iptsd*" "libwacom*" 2>/dev/null || rpm -qa "iptsd*" "libwacom*" 2>/dev/null || pacman -Q iptsd libwacom 2>/dev/null || echo "no dpkg/rpm/pacman; iptsd tools in: ${IPTSD_BIN:-not found}"'
run $F "iptsd config" sh -c 'ls -l /etc/iptsd.d /usr/share/iptsd /usr/share/iptsd/presets ${IPTSD_BIN:+"$IPTSD_BIN/../share/iptsd"} 2>&1; cat /etc/iptsd.conf /etc/iptsd.d/*.conf 2>/dev/null'
run $F "input devices" cat /proc/bus/input/devices
rootrun $F "libinput devices" libinput list-devices
run $F "libwacom" libwacom-list-local-devices
mkdir -p "$OUT/hid"
{
	printf '\n### HID devices (vendor 045E)\n'
	for d in /sys/bus/hid/devices/*; do
		[ -e "$d" ] || continue
		n=$(basename "$d")
		case $n in *045E*) ;; *) continue ;; esac
		drv=$(basename "$(readlink "$d/driver" 2>/dev/null)" 2>/dev/null)
		hr=$(ls "$d/hidraw" 2>/dev/null | tr '\n' ' ')
		name=$(grep -m1 '^HID_NAME=' "$d/uevent" 2>/dev/null | cut -d= -f2-)
		echo "$n driver=${drv:-none} hidraw=${hr:-none} name=$name"
		if [ -r "$d/report_descriptor" ]; then
			cp "$d/report_descriptor" "$OUT/hid/$n.bin" 2>/dev/null
			have hid-decode && hid-decode "$d/report_descriptor" > "$OUT/hid/$n.txt" 2>&1
		fi
	done
} >> "$OUT/$F"
if [ "$IS_ROOT" -ne 1 ]; then
	printf '\n### iptsd-check-device\n(root needed: skipped)\n' >> "$OUT/$F"
	sum TOUCH check-dev SKIP "root needed for iptsd-check-device"
elif [ -z "$IPTSD_BIN" ]; then
	printf '\n### iptsd-check-device\n(not installed: iptsd-check-device)\n' >> "$OUT/$F"
	sum TOUCH check-dev SKIP "iptsd-check-device not found (PATH, running iptsd, iptsd@.service)"
else
	for d in /sys/bus/hid/devices/*0C52*/hidraw/hidraw*; do
		[ -e "$d" ] || continue
		h=$(basename "$d")
		o=$(timeout 60 "$IPTSD_BIN/iptsd-check-device" "/dev/$h" 2>&1 </dev/null); rc=$?
		printf '\n### iptsd-check-device %s\n$ %s\n%s\n(exit %s)\n' "$h" "$IPTSD_BIN/iptsd-check-device /dev/$h" "$(redact <<<"$o")" "$rc" >> "$OUT/$F"
		[ "$rc" -eq 0 ] && sum TOUCH check-dev MATCH "iptsd-check-device accepts $h" || sum TOUCH check-dev DIFF "iptsd-check-device exit $rc on $h (10-pen-touch.txt)"
	done
fi
# Names of other paired devices are often a person's name.
runsh $F "bluetooth devices (names other than the pen's left out)" bluetoothctl 'bluetoothctl devices | sed -E "/[Pp]en|[Ss]urface/! s/^(Device [^ ]+) .*/\1 <name left out>/"'
run $F "upower" upower -d
rootrun $F "fwupd devices" fwupdmgr get-devices --no-unreported-check

if lsmod | grep -q '^ithc'; then sum TOUCH ithc MATCH "ithc loaded"; else sum TOUCH ithc MISSING "ithc not loaded"; fi
nie=$(kgrep 'ithc.*(timeout|fail|error)' | wc -l)
[ "$nie" -gt 0 ] && sum TOUCH ithc-err DIFF "$nie ithc error lines in kernel log (10-pen-touch.txt; gap pen-touch #13)"
if systemctl is-active --quiet 'iptsd@*' 2>/dev/null || systemctl list-units --no-legend 'iptsd*' 2>/dev/null | grep -q running; then
	sum TOUCH iptsd MATCH "iptsd running"
else
	sum TOUCH iptsd MISSING "no running iptsd unit"
fi
nipts=$(grep -c '^N: Name=".*IPTS' /proc/bus/input/devices 2>/dev/null)
sum TOUCH inputs INFO "${nipts:-0} input devices named IPTS (iptsd uinput)"
if grep -q '^N: Name=".*045E:0C52' /proc/bus/input/devices 2>/dev/null; then
	sum TOUCH dup INFO "kernel input device 045E:0C52 present: check duplicate events (gap pen-touch #12, PEN-8)"
fi
ls /etc/iptsd.d/*.conf >/dev/null 2>&1 && sum TOUCH config INFO "iptsd local config present" || sum TOUCH config INFO "no /etc/iptsd.d/*.conf (defaults 40/36/15)"
if have bluetoothctl; then
	bt=$(timeout 10 bluetoothctl devices 2>/dev/null </dev/null | grep -ci 'pen')
	sum PEN ble INFO "${bt:-0} paired bluetooth devices with 'pen' in the name"
else
	sum PEN ble SKIP "bluetoothctl not installed"
fi

# ---------------------------------------------------------------- camera
F=20-camera.txt
{ printf '\n### kernel log: camera\n'; kgrep 'int3472|ipu-bridge|ipu6|intel-ipu|ov5693|ov13858|vcm|dw9|vd55'; } >> "$OUT/$F"
{
	printf '\n### i2c devices\n'
	for d in /sys/bus/i2c/devices/*; do
		[ -e "$d/name" ] || continue
		drv=$(basename "$(readlink "$d/driver" 2>/dev/null)" 2>/dev/null)
		echo "$(basename "$d") name=$(cat "$d/name") driver=${drv:-none}"
	done
} >> "$OUT/$F"
# The firmware loader's search path: the firmware_class parameter first (the only place on NixOS), then the
# default directories. Compressed files (.zst, .xz) are what the loader actually opens on most distros.
{
	printf '\n### IPU6 firmware files\n'
	nfw=0
	for d in "$(rd /sys/module/firmware_class/parameters/path)" /lib/firmware/updates /lib/firmware /usr/lib/firmware; do
		[ -n "$d" ] && [ -d "$d/intel/ipu" ] || continue
		ls -lL "$d/intel/ipu/" 2>&1
		nfw=$((nfw + $(find -L "$d/intel/ipu/" -maxdepth 1 -name 'ipu6ep_fw.bin*' 2>/dev/null | wc -l)))
	done
	[ "$nfw" -eq 0 ] && echo "(no ipu6ep_fw.bin* in the firmware search path)"
} >> "$OUT/$F"
{
	printf '\n### media / v4l2 nodes (sysfs)\n'
	for m in /sys/bus/media/devices/media*; do [ -e "$m" ] && echo "$(basename "$m") model=$(rd "$m/model")"; done
	for v in /sys/class/video4linux/v4l-subdev*; do [ -e "$v" ] && echo "$(basename "$v") name=$(rd "$v/name")"; done
	echo "video nodes: $(ls -d /sys/class/video4linux/video* 2>/dev/null | wc -l)"
} >> "$OUT/$F"
run $F "v4l2 devices" v4l2-ctl --list-devices
for m in /dev/media*; do [ -e "$m" ] && run $F "media-ctl -p $m" media-ctl -d "$m" -p; done
for s in /dev/v4l-subdev*; do [ -e "$s" ] && run $F "controls $s" v4l2-ctl -d "$s" -l; done
run $F "libcamera cameras" cam -l
run $F "leds" ls /sys/class/leds
rootrun $F "regulator summary" cat /sys/kernel/debug/regulator/regulator_summary
rootrun $F "clk summary" cat /sys/kernel/debug/clk/clk_summary
rootrun $F "gpio" cat /sys/kernel/debug/gpio

i2c_has() { compgen -G "/sys/bus/i2c/devices/*$1*" >/dev/null; }
i2c_has OVTI5693 && sum CAM front MATCH "OVTI5693 i2c device present" || sum CAM front MISSING "no OVTI5693 i2c device"
i2c_has OVTID858 && sum CAM rear MATCH "OVTID858 i2c device present" || sum CAM rear MISSING "no OVTID858 i2c device"
i2c_has SMO55F0 &&sum CAM ir INFO "SMO55F0 i2c device present (no driver expected)" || sum CAM ir INFO "no SMO55F0 i2c device"
kgrep 'Unknown VCM type' >/dev/null && sum CAM vcm DIFF "'Unknown VCM type' in log (gap camera #4, no autofocus)" || sum CAM vcm INFO "no 'Unknown VCM type' line"
# ipu6 names the firmware file only when loading it fails; success is "CSE authenticate_run done".
if kgrep 'intel-ipu6.*(Requesting signed firmware|CSE .*(fail|timeout)|Timed out while waiting for CSE)' >/dev/null; then
	sum CAM ipu-fw DIFF "IPU6 firmware load/authentication failed (20-camera.txt)"
elif kgrep 'intel-ipu6.*CSE authenticate_run done' >/dev/null; then
	sum CAM ipu-fw MATCH "IPU6 firmware authenticated (CSE authenticate_run done)"
elif grep -qsx 'ipu6' /sys/bus/media/devices/media*/model; then
	sum CAM ipu-fw INFO "ipu6 media device registered; no CSE line in the kernel log (log incomplete)"
elif [ -e /sys/bus/pci/devices/0000:00:05.0/driver ]; then
	sum CAM ipu-fw DIFF "driver bound to 00:05.0 but no ipu6 media device and no CSE line"
else
	sum CAM ipu-fw MISSING "no driver on IPU6 00:05.0"
fi
sens=$(cat /sys/class/video4linux/v4l-subdev*/name 2>/dev/null | grep -E '^(ov5693|ov13858|dw9719) ' | cut -d' ' -f1 | sort | tr '\n' ' ')
case $sens in
	*ov13858*ov5693*) sum CAM subdevs MATCH "sensor subdevices registered: $sens" ;;
	*) sum CAM subdevs DIFF "sensor subdevices registered: ${sens:-none} (ov5693 and ov13858 expected)" ;;
esac
if have cam; then
	n=$(timeout 60 cam -l 2>/dev/null </dev/null | grep -cE '^[0-9]+:')
	sum CAM libcamera INFO "${n:-0} cameras listed by cam -l"
else
	sum CAM libcamera SKIP "cam not installed (libcamera tools)"
fi

# ---------------------------------------------------------------- SAM / platform
F=30-sam-platform.txt
{ printf '\n### kernel log: ssam / surface\n'; kgrep 'ssam|surface_|surface-|soc_button|tabletsw|thermal'; } >> "$OUT/$F"
{
	printf '\n### surface_aggregator devices\n'
	for d in /sys/bus/surface_aggregator/devices/*; do
		[ -e "$d" ] || continue
		drv=$(basename "$(readlink "$d/driver" 2>/dev/null)" 2>/dev/null)
		echo "$(basename "$d") driver=${drv:-none}"
	done
} >> "$OUT/$F"
{
	printf '\n### hwmon\n'
	for h in /sys/class/hwmon/hwmon*; do
		echo "$h name=$(rd "$h/name")"
		for v in "$h"/temp*_input "$h"/temp*_label "$h"/fan*_input; do
			[ -e "$v" ] && echo "  $(basename "$v")=$(rd "$v")"
		done
	done
} >> "$OUT/$F"
run $F "sensors" sensors
{
	printf '\n### thermal zones / cooling devices\n'
	for z in /sys/class/thermal/thermal_zone* /sys/class/thermal/cooling_device*; do
		[ -e "$z" ] && echo "$(basename "$z") type=$(rd "$z/type") temp=$(rd "$z/temp") cur_state=$(rd "$z/cur_state")"
	done
} >> "$OUT/$F"
cats $F "platform profile" /sys/firmware/acpi/platform_profile /sys/firmware/acpi/platform_profile_choices
{
	printf '\n### rtc\n'
	for r in /sys/class/rtc/rtc*; do
		[ -e "$r" ] && echo "$(basename "$r") name=$(rd "$r/name") date=$(rd "$r/date") time=$(rd "$r/time") hctosys=$(rd "$r/hctosys") wakealarm=$([ -e "$r/wakealarm" ] && echo yes || echo no)"
	done
	echo "system UTC: $(date -u '+%F %T')"
} >> "$OUT/$F"
run $F "timedatectl" timedatectl
run $F "typec" ls -l /sys/class/typec
{
	printf '\n### power supplies\n'
	for p in /sys/class/power_supply/*; do
		[ -e "$p/uevent" ] && { echo "[$(basename "$p")]"; redact < "$p/uevent"; }
	done
} >> "$OUT/$F"
{
	printf '\n### ACPI interrupts (nonzero)\n'
	for g in /sys/firmware/acpi/interrupts/*; do
		v=$(rd "$g" | sed 's/^ *//')
		[ -n "$v" ] && [ "${v%% *}" != 0 ] && echo "$(basename "$g"): $v"
	done
} >> "$OUT/$F"
cats $F "gpe52" /sys/firmware/acpi/interrupts/gpe52
run $F "acpi wakeup" cat /proc/acpi/wakeup
rootrun $F "ioports" cat /proc/ioports
run $F "iio devices" sh -c 'for d in /sys/bus/iio/devices/iio:device*; do echo "$d name=$(cat $d/name 2>/dev/null)"; ls "$d" | grep -E "^in_" | tr "\n" " "; echo; done'
run $F "tablet mode / button input devices" sh -c 'grep -B1 -A8 -iE "Name=.*(tablet|button|gpio-keys|lid)" /proc/bus/input/devices'
run $F "suspend stats" sh -c 'grep -r . /sys/power/suspend_stats/ 2>/dev/null'

expect="01:02:01:01:00 01:02:01:01:01 01:03:01:00:01 01:03:01:00:02 01:05:01:01:01 01:26:01:00:01 01:15:01:06:00 01:15:01:07:00"
for e in $expect; do
	d=/sys/bus/surface_aggregator/devices/$e
	if [ -e "$d" ]; then
		drv=$(basename "$(readlink "$d/driver" 2>/dev/null)" 2>/dev/null)
		[ -n "$drv" ] && sum SAM "$e" MATCH "bound to $drv" || sum SAM "$e" DIFF "present, no driver"
	else
		sum SAM "$e" MISSING "node absent"
	fi
done
# Device names on the bus are bare "DD:TC:TID:IID:FN". The "ssam:" prefix belongs to the modalias only.
nkip=$(ls -d /sys/bus/surface_aggregator/devices/01:15:02:0* 2>/dev/null | wc -l)
sum SAM kip INFO "$nkip KIP HID nodes (4 expected with the Type Cover attached)"
[ -e /sys/bus/surface_aggregator/devices/01:01:01:00:00 ] && sum SAM rtc-node INFO "SAM RTC node present (gap sam G5 filled)" || sum SAM rtc-node INFO "no SAM RTC node (gap sam G5)"
# The module is surface_temp, the hwmon device it registers is named surface_thermal.
nt=0; fanv=""; tname=surface_thermal; tdup=""
for h in /sys/class/hwmon/hwmon*; do
	case $(rd "$h/name") in
		surface_thermal | surface_temp)
			tname=$(rd "$h/name")
			nt=$(ls "$h"/temp*_input 2>/dev/null | wc -l)
			tdup=$(cat "$h"/temp*_label 2>/dev/null | sort | uniq -d | tr '\n' ' ')
			;;
		surface_fan) fanv=$(rd "$h/fan1_input") ;;
	esac
done
if [ "$nt" -eq 11 ]; then sum SAM temps MATCH "$tname 11 channels"
elif [ "$nt" -eq 0 ]; then sum SAM temps MISSING "no surface_thermal hwmon"
else sum SAM temps DIFF "$tname $nt channels (11 expected)"; fi
[ -n "$tdup" ] && sum SAM temp-lbl INFO "labels used by more than one channel: $tdup(pick channels by temp<n>, not by label)"
[ -n "$fanv" ] && sum SAM fan INFO "surface_fan ${fanv} RPM (gap sam G2: high RPM at idle right after Windows reboot)" || sum SAM fan MISSING "no surface_fan hwmon"
sum SAM profile INFO "platform_profile=$(rd /sys/firmware/acpi/platform_profile) choices=$(rd /sys/firmware/acpi/platform_profile_choices)"
sum SAM rtc INFO "rtc: $(for r in /sys/class/rtc/rtc*; do printf '%s=%s ' "$(basename "$r")" "$(rd "$r/name")"; done)"
ntc=$(find /sys/class/typec/ -maxdepth 1 -name 'port[0-9]' 2>/dev/null | wc -l)
[ "$ntc" -eq 2 ] && sum SAM typec MATCH "2 typec ports" || sum SAM typec DIFF "$ntc typec ports (0 without UCSI driver, gap sam G1)"
if grep -qi 'Name=".*tablet mode switch' /proc/bus/input/devices 2>/dev/null; then sum SAM tabletsw MATCH "tablet mode switch input present"; else sum SAM tabletsw MISSING "no tablet mode switch input"; fi
if grep -qiE 'Name=".*(surface button|gpio-keys|soc_button)' /proc/bus/input/devices 2>/dev/null; then sum SAM buttons MATCH "button input present"; else sum SAM buttons MISSING "no power/volume button input (wiki: pinctrl_tigerlake in initramfs)"; fi
sum SAM gpe52 INFO "gpe52: $(rd /sys/firmware/acpi/interrupts/gpe52)"
niio=$(ls -d /sys/bus/iio/devices/iio:device* 2>/dev/null | wc -l)
sum SAM iio INFO "$niio iio devices: $(for d in /sys/bus/iio/devices/iio:device*; do printf '%s ' "$(rd "$d/name")"; done)"
nthz=$(ls -d /sys/class/thermal/thermal_zone* 2>/dev/null | wc -l)
sum SAM thermal INFO "$nthz thermal zones (gap sam G4)"

# ---------------------------------------------------------------- audio
F=40-audio.txt
{ printf '\n### kernel log: audio\n'; kgrep 'snd_intel_dspcfg|SoundWire|DMICs detected|BT link|ssp2|BT_OFFLOAD|hda codecs found|sof-audio|sof_|skl_hda|ALC274|autoconfig|picked fixup|snd_hda'; } >> "$OUT/$F"
cats $F "dspcfg" /sys/module/snd_intel_dspcfg/parameters/dsp_driver
run $F "audio pci" lspci -nnk -s 00:1f.3
cats $F "1f.3 power" /sys/bus/pci/devices/0000:00:1f.3/power/runtime_status /sys/bus/pci/devices/0000:00:1f.3/power_state
run $F "aplay -l" aplay -l
run $F "arecord -l" arecord -l
cats $F "cards" /proc/asound/cards
{ printf '\n### /proc/asound/pcm\n'; cat /proc/asound/pcm 2>&1; } >> "$OUT/$F"
for h in /sys/class/sound/hwC*D*; do
	[ -e "$h" ] || continue
	{ printf '\n### %s\n' "$h"; for a in vendor_id subsystem_id chip_name modelname init_pin_configs driver_pin_configs user_pin_configs; do
		[ -r "$h/$a" ] && { echo "[$a]"; cat "$h/$a"; }
	done; } >> "$OUT/$F" 2>&1
done
for c in /proc/asound/card*/codec#*; do [ -r "$c" ] && cp "$c" "$OUT/codec-$(basename "$(dirname "$c")")-$(basename "$c" | tr -d '#').txt"; done
runsh $F "amixer jack controls" amixer 'amixer -c0 controls | grep -i jack'
run $F "amixer contents" amixer -c0 contents
run $F "SNDW acpi devices" sh -c 'grep -l SNDW /sys/bus/acpi/devices/*/path 2>/dev/null | while read p; do echo "$(dirname "$p") $(cat "$p") status=$(cat "$(dirname "$p")/status" 2>/dev/null)"; done'
userrun $F "wpctl status" wpctl status
userrun $F "pactl sinks" pactl list sinks
userrun $F "pactl sources" pactl list sources

adrv=$(basename "$(readlink /sys/bus/pci/devices/0000:00:1f.3/driver 2>/dev/null)" 2>/dev/null)
case $adrv in
	*sof*) sum AUD path INFO "SOF ($adrv): watch second s2idle (gap audio G1, PWR-3)" ;;
	snd_hda_intel) sum AUD path INFO "legacy snd_hda_intel" ;;
	*) sum AUD path MISSING "no driver on 00:1f.3 (${adrv:-none})" ;;
esac
ssid=$(cat /sys/class/sound/hwC*D0/subsystem_id 2>/dev/null | head -1)
[ "$ssid" = 0x10ec1284 ] && sum AUD ssid MATCH "codec SSID 0x10ec1284" || sum AUD ssid DIFF "codec SSID ${ssid:-?}"
pin=$(cat /sys/class/sound/hwC*D0/init_pin_configs 2>/dev/null | awk '$1=="0x19"||$1=="0x21"{printf "%s=%s ", $1, $2}')
sum AUD pins INFO "BIOS pins ${pin:-?} (0x411111f0 = not connected; gap audio G2/G6)"
# /proc/asound/pcm lists what "aplay -l" lists, without alsa-utils.
if [ -r /proc/asound/pcm ]; then
	sum AUD devices INFO "$(grep -c ': playback ' /proc/asound/pcm) playback, $(grep -c ': capture ' /proc/asound/pcm) capture PCM devices"
else
	sum AUD devices MISSING "no /proc/asound/pcm (no ALSA card)"
fi

# ---------------------------------------------------------------- display
F=50-display.txt
{ printf '\n### kernel log: display\n'; kgrep 'i915|xe |\[drm\]|psr|drrs|Refresh rate range|backlight'; } >> "$OUT/$F"
gdrv=$(basename "$(readlink /sys/bus/pci/devices/0000:00:02.0/driver 2>/dev/null)" 2>/dev/null)
echo "gpu driver: ${gdrv:-none}" >> "$OUT/$F"
cats $F "i915 params" /sys/module/i915/parameters/enable_psr /sys/module/i915/parameters/enable_psr2_sel_fetch /sys/module/i915/parameters/enable_dc
EDP=""
for c in /sys/class/drm/card*-eDP-*; do [ -e "$c" ] && EDP=$c && break; done
if [ -n "$EDP" ]; then
	cats $F "eDP" "$EDP/status" "$EDP/enabled" "$EDP/modes"
	# The raw EDID and its hex dump hold the panel's serial number.
	run $F "edid-decode" edid-decode --skip-hex-dump "$EDP/edid"
fi
run $F "drm_info" drm_info
mkdir -p "$OUT/debugfs"
# One debugfs directory per GPU. dri/ also holds the same files again under each minor number (dri/1, dri/128)
# and, on older kernels, only under the minor number; the connector and crtc files are one level further down.
DRI=""
EDPN=${EDP##*/}; EDPN=${EDPN#card*-}
if [ "$IS_ROOT" -eq 1 ]; then
	for d in /sys/kernel/debug/dri/0000:00:02.0 /sys/kernel/debug/dri/[0-9]*; do
		[ -e "$d/i915_display_info" ] && DRI=$d && break
	done
fi
if [ -n "$DRI" ]; then
	find "$DRI" -maxdepth 2 -type f \( -name '*psr*' -o -name '*drrs_status' -o -name 'vrr_range' -o -name 'i915_fbc_status' -o -name 'i915_display_info' -o -name 'i915_dmc_info' -o -name 'i915_frequency_info' \) ! -path '*/i915_params/*' 2>/dev/null |
		while read -r f; do
			o=$OUT/debugfs/$(echo "${f#"$DRI"/}" | tr '/' '_')
			timeout 5 cat "$f" > "$o" 2>&1
		done
	# debugfs pads i915_vbt with zeros to a multiple of 4 bytes. The hash that compares with a firmware image is
	# over the VBT itself, whose length is the little-endian u16 at offset 24 of the "$VBT" header.
	if [ -r "$DRI/i915_vbt" ]; then
		vlen=$(od -An -tu2 -j24 -N2 "$DRI/i915_vbt" 2>/dev/null | tr -d ' ')
		head -c "${vlen:-0}" "$DRI/i915_vbt" > "$OUT/debugfs/vbt.bin" 2>/dev/null
		vsum=$(sha256sum < "$OUT/debugfs/vbt.bin" | cut -d' ' -f1)
		echo "vbt: $vlen bytes, sha256 $vsum (debugfs file: $(wc -c < "$DRI/i915_vbt") bytes)" >> "$OUT/$F"
		have intel_vbt_decode && intel_vbt_decode --file "$OUT/debugfs/vbt.bin" > "$OUT/debugfs/vbt-decode.txt" 2>&1
		vhit=$(refname "$vsum" 'vbt/')
		if [ -n "$vhit" ]; then sum DSP vbt MATCH "runtime VBT ($vlen B) equals $vhit in docs/reference/firmware-hashes.tsv"
		elif [ -n "$(refhash 'vbt/')" ]; then sum DSP vbt DIFF "runtime VBT ($vlen B, sha256 $vsum) equals no vbt/ row in docs/reference/firmware-hashes.tsv"
		else sum DSP vbt INFO "runtime VBT $vlen B sha256 $vsum (no docs/reference/firmware-hashes.tsv next to the script)"; fi
	fi
elif [ "$IS_ROOT" -eq 1 ]; then
	echo "(no i915 debugfs directory under /sys/kernel/debug/dri)" >> "$OUT/$F"
else
	echo "(root needed for i915 debugfs: skipped)" >> "$OUT/$F"
fi
{ printf '\n### backlight\n'; for b in /sys/class/backlight/*; do [ -e "$b" ] && echo "$(basename "$b") type=$(rd "$b/type") max=$(rd "$b/max_brightness") brightness=$(rd "$b/brightness") actual=$(rd "$b/actual_brightness")"; done; } >> "$OUT/$F"
# A read of in_illuminance_raw powers the sensor hub channel up and waits for a sample; when none arrives the
# read still returns 0 after the driver's timeout. The read time is recorded so that a 0 is not taken as "dark".
{
	printf '\n### ALS\n'
	for d in /sys/bus/iio/devices/iio:device*; do
		compgen -G "$d/in_illuminance*" >/dev/null || continue
		echo "$d $(rd "$d/name")"
		for f in "$d"/in_illuminance*; do
			t0=$(date +%s.%N); v=$(rd "$f"); t1=$(date +%s.%N)
			echo "  $(basename "$f")=$v ($(awk -v a="$t0" -v b="$t1" 'BEGIN { printf "%.1f", b - a }') s)"
		done
	done
} >> "$OUT/$F"

sum DSP gpu INFO "00:02.0 driver ${gdrv:-none}"
if [ -n "$EDP" ]; then
	# sysfs "modes" has names only (two lines "2880x1920"); the refresh rates are in i915_display_info.
	rates=""
	[ -n "$DRI" ] && rates=$(awk -v c="$EDPN" '
		/^\[CONNECTOR:/ { on = index($0, ":" c "]") > 0; inm = 0 }
		on && /^\tmodes:/ { inm = 1; next }
		on && inm && /^\t\t"/ { print $2; next }
		{ inm = 0 }' "$DRI/i915_display_info" 2>/dev/null | sort -nu | tr '\n' ' ')
	if ! grep -q '2880x1920' "$EDP/modes" 2>/dev/null; then sum DSP modes DIFF "no 2880x1920 mode"
	elif [ -z "$rates" ]; then sum DSP modes INFO "2880x1920, $(wc -l < "$EDP/modes") mode lines (refresh rates need root: i915_display_info)"
	elif [ "$rates" = "60 120 " ]; then sum DSP modes MATCH "2880x1920 at ${rates}Hz"
	else sum DSP modes DIFF "2880x1920 at ${rates}Hz (60 and 120 expected)"; fi
	# The EDID carries a serial field that can differ per unit, so DIFF here is not by itself a different panel.
	b=$(refhash 'edid.bin')
	if [ -n "$b" ]; then
		a=$(sha256sum < "$EDP/edid" | cut -d' ' -f1)
		[ "$a" = "$b" ] && sum DSP edid MATCH "EDID equals the edid.bin hash in docs/reference/firmware-hashes.tsv" || sum DSP edid DIFF "EDID sha256 $a, reference edid.bin $b (the serial field may differ per unit: read edid-decode output)"
	else
		sum DSP edid INFO "docs/reference/firmware-hashes.tsv not found next to the script: compare the edid-decode output in 50-display.txt by hand"
	fi
	# The kernel prints the range only as a drm debug message; the connector's debugfs file always has it.
	vr=$OUT/debugfs/${EDPN}_vrr_range
	if [ -s "$vr" ]; then sum DSP vrr INFO "$EDPN vrr_range $(tr '\n' ' ' < "$vr")(0/0 = panel reports no range)"
	else sum DSP vrr SKIP "root needed for $EDPN/vrr_range"; fi
	dr=$OUT/debugfs/crtc-0_i915_drrs_status
	if [ -s "$dr" ]; then sum DSP drrs INFO "$(grep -E '^DRRS (capable|enabled|active)' "$dr" | tr '\n' ';')"
	else sum DSP drrs SKIP "root needed for crtc-0/i915_drrs_status"; fi
else
	sum DSP edp MISSING "no eDP connector in /sys/class/drm"
fi
# The status line is "Source PSR status" up to 6.x and "Source PSR/PanelReplay status" after.
psr=$(grep -iE '^(PSR mode|Source PSR[^:]* status|Sink support)' "$OUT/debugfs/i915_edp_psr_status" 2>/dev/null | tr '\n' ';')
[ "$IS_ROOT" -eq 1 ] && sum DSP psr INFO "enable_psr=$(rd /sys/module/i915/parameters/enable_psr) ${psr:-no i915_edp_psr_status}" || sum DSP psr SKIP "root needed for i915_edp_psr_status"
for b in /sys/class/backlight/*; do [ -e "$b" ] && sum DSP backlight INFO "$(basename "$b") max=$(rd "$b/max_brightness")"; done
ls /sys/bus/iio/devices/iio:device*/in_illuminance* >/dev/null 2>&1 && sum DSP als MATCH "illuminance channel present" || sum DSP als MISSING "no illuminance iio channel"

# ---------------------------------------------------------------- power
F=60-power.txt
cats $F "sleep" /sys/power/state /sys/power/mem_sleep /sys/power/disk /sys/power/resume
cats $F "lpi residency" /sys/devices/system/cpu/cpuidle/low_power_idle_cpu_residency_us /sys/devices/system/cpu/cpuidle/low_power_idle_system_residency_us
run $F "suspend stats" sh -c 'grep -r . /sys/power/suspend_stats/ 2>/dev/null'
for f in slp_s0_residency_usec substate_residencies substate_requirements substate_status_registers ltr_show package_cstate_show pch_ip_power_gating_status; do
	rootrun $F "pmc_core $f" cat /sys/kernel/debug/pmc_core/$f
done
run $F "pci runtime pm" sh -c 'for d in /sys/bus/pci/devices/*; do printf "%s %s %s %s\n" "$(basename $d)" "$(cat $d/power/runtime_status 2>/dev/null)" "$(cat $d/power_state 2>/dev/null)" "$(basename "$(readlink $d/driver)" 2>/dev/null)"; done'
cats $F "nvme apst" /sys/module/nvme_core/parameters/default_ps_max_latency_us
if [ "$IS_ROOT" -eq 1 ]; then
	runsh $F "nvme power features" nvme 'nvme get-feature /dev/nvme0 -f 0x0c -H 2>&1 | head -40; nvme get-feature /dev/nvme0 -f 0x02 -H'
else
	printf '\n### nvme power features\n(root needed: skipped)\n' >> "$OUT/$F"
fi
run $F "rapl" sh -c 'for z in /sys/class/powercap/intel-rapl:*; do echo "$z name=$(cat $z/name)"; for c in $z/constraint_*; do echo "  $(basename $c)=$(cat $c 2>/dev/null)"; done; done'
cats $F "cpufreq" /sys/devices/system/cpu/cpu0/cpufreq/scaling_driver /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference
run $F "thermald" systemctl status thermald --no-pager
run $F "power-profiles" powerprofilesctl
# turbostat exits without sampling when --show names a column it does not have, and the names are not regular
# (Pkg%pc8 but Pk%pc10). Only columns from its own --list are requested. Without the msr module it falls back
# to perf and has no PkgTmp; the module is not loaded here.
TSTAT=""
if [ "$IS_ROOT" -ne 1 ]; then
	printf '\n### turbostat 10 s\n(root needed: skipped)\n' >> "$OUT/$F"
elif ! have turbostat; then
	printf '\n### turbostat 10 s\n(not installed: turbostat)\n' >> "$OUT/$F"
else
	tlist=,$(timeout 20 turbostat --list 2>/dev/null | tail -1),
	tcols=""
	for c in PkgWatt PkgTmp Pkg%pc2 Pkg%pc6 Pkg%pc8 Pk%pc10 Pkg%pc10 CPU%LPI SYS%LPI; do
		case $tlist in *",$c,"*) tcols=${tcols:+$tcols,}$c ;; esac
	done
	if [ -n "$tcols" ]; then
		TSTAT=$(timeout 60 turbostat --quiet --Summary --show "$tcols" --num_iterations 2 --interval 5 2>&1 </dev/null)
		printf '\n### turbostat 10 s\n$ turbostat --quiet --Summary --show %s --num_iterations 2 --interval 5\n%s\n' "$tcols" "$TSTAT" >> "$OUT/$F"
	else
		printf '\n### turbostat 10 s\n(turbostat --list has none of the wanted columns)\n' >> "$OUT/$F"
	fi
	[ -e /dev/cpu/0/msr ] || echo "(msr module not loaded: no PkgTmp column)" >> "$OUT/$F"
fi
cats $F "hibernation" /sys/power/image_size /sys/kernel/security/lockdown
run $F "sleep.conf" sh -c 'cat /etc/systemd/sleep.conf /etc/systemd/sleep.conf.d/*.conf 2>/dev/null | grep -v "^#"'

ms=$(rd /sys/power/mem_sleep)
case $ms in *"[s2idle]"*) sum PWR mem_sleep MATCH "$ms" ;; *) sum PWR mem_sleep DIFF "$ms (s2idle expected)" ;; esac
sum PWR lpi INFO "cpu_us=$(rd /sys/devices/system/cpu/cpuidle/low_power_idle_cpu_residency_us) system_us=$(rd /sys/devices/system/cpu/cpuidle/low_power_idle_system_residency_us) (0 before any suspend is normal)"
if [ "$IS_ROOT" -eq 1 ]; then
	sum PWR slp_s0 INFO "slp_s0_residency_usec=$(rd /sys/kernel/debug/pmc_core/slp_s0_residency_usec)"
else
	sum PWR slp_s0 SKIP "root needed for pmc_core"
fi
if [ "$IS_ROOT" -ne 1 ]; then sum PWR turbostat SKIP "root needed for turbostat"
elif ! have turbostat; then sum PWR turbostat SKIP "turbostat not installed"
else
	# header line and last sample, paired as name=value
	tsum=$(awk -F'\t' '/^[A-Za-z]/ && /%|Watt/ { n = split($0, h); next } n && /^[0-9.]/ { for (i = 1; i <= n; i++) v[i] = $i; ok = 1 }
		END { if (ok) for (i = 1; i <= n; i++) printf "%s=%s ", h[i], v[i] }' <<<"$TSTAT")
	[ -n "$tsum" ] && sum PWR turbostat INFO "5 s sample: $tsum" || sum PWR turbostat SKIP "turbostat gave no sample (60-power.txt)"
fi
sum PWR suspend INFO "success=$(rd /sys/power/suspend_stats/success) fail=$(rd /sys/power/suspend_stats/fail)"
sum PWR hibernate INFO "disk=$(rd /sys/power/disk) lockdown=$(rd /sys/kernel/security/lockdown || echo "no lockdown LSM")"

# ---------------------------------------------------------------- network / usb4
F=70-net.txt
{ printf '\n### kernel log: iwlwifi / bluetooth / thunderbolt\n'; kgrep 'iwlwifi|sar|ppag|tas|wrds|wgds|bluetooth|btusb|btintel|thunderbolt|typec|ucsi'; } >> "$OUT/$F"
run $F "iw dev" iw dev
run $F "iw reg" iw reg get
run $F "rfkill" rfkill list
run $F "thunderbolt" sh -c 'for d in /sys/bus/thunderbolt/devices/*; do echo "$(basename $d) $(cat $d/device_name 2>/dev/null) $(cat $d/generation 2>/dev/null)"; done'
run $F "boltctl" boltctl list
wdrv=$(basename "$(readlink /sys/bus/pci/devices/0000:00:14.3/driver 2>/dev/null)" 2>/dev/null)
[ -n "$wdrv" ] && sum NET wifi INFO "00:14.3 driver $wdrv" || sum NET wifi MISSING "no driver on Wi-Fi 00:14.3"
kgrep 'iwlwifi.*(sar|ppag|tas)' >/dev/null && sum NET sar INFO "iwlwifi SAR/PPAG/TAS lines present (70-net.txt)" || sum NET sar INFO "no iwlwifi SAR/PPAG/TAS lines"
kgrep 'thunderbolt.*(software connection manager|firmware connection manager)' >/dev/null && sum NET tbt INFO "$(kgrep 'connection manager' | tail -1 | sed 's/.*\] //')" || sum NET tbt INFO "no connection manager line"

# ---------------------------------------------------------------- ACPI tables (root; MSDM is never copied)
if [ "$IS_ROOT" -eq 1 ] && [ -d /sys/firmware/acpi/tables ]; then
	mkdir -p "$OUT/acpi"
	for t in /sys/firmware/acpi/tables/*; do
		[ -f "$t" ] || continue
		n=$(basename "$t")
		case $n in MSDM* | SLIC*) continue ;; esac
		cp "$t" "$OUT/acpi/$n.dat" 2>/dev/null
	done
	for t in /sys/firmware/acpi/tables/dynamic/*; do [ -f "$t" ] && cp "$t" "$OUT/acpi/dynamic-$(basename "$t").dat" 2>/dev/null; done
	wsum=$(refhash 'acpi/dsdt.dat'); wsz=$(refsize 'acpi/dsdt.dat')
	if [ -n "$wsum" ] && [ -f "$OUT/acpi/DSDT.dat" ]; then
		# The firmware writes the addresses of its NVS OperationRegions into the DSDT at every boot and fixes
		# the checksum (byte 10), so two dumps of one firmware version differ in a few bytes. A different
		# firmware version changes the length or far more bytes. The reference is only a hash and a size, so
		# the differing bytes cannot be counted: same length with another hash is the normal result for the
		# reference firmware version and is reported as INFO with both hashes, not as MATCH or DIFF.
		lsz=$(wc -c < "$OUT/acpi/DSDT.dat"); lsum=$(sha256sum < "$OUT/acpi/DSDT.dat" | cut -d' ' -f1)
		if [ "$wsum" = "$lsum" ]; then
			sum SYS dsdt MATCH "DSDT equals the acpi/dsdt.dat hash in docs/reference/firmware-hashes.tsv"
		elif [ "$wsz" -ne "$lsz" ]; then
			sum SYS dsdt DIFF "DSDT length $lsz, reference acpi/dsdt.dat $wsz"
		else
			sum SYS dsdt INFO "DSDT same length ($lsz) as reference acpi/dsdt.dat, sha256 $lsum vs $wsum (boot-time checksum and NVS addresses differ between dumps; a hash cannot tell that from another change)"
		fi
	fi
else
	sum SYS acpi SKIP "root needed to copy ACPI tables"
fi

# ---------------------------------------------------------------- finish
{
	printf 'sp9 post-install check: kernel %s, root=%s\n' "$(uname -r)" "$IS_ROOT"
	printf 'files: 00-system 10-pen-touch 20-camera 30-sam-platform 40-audio 50-display 60-power 70-net, kernel-log, hid/, debugfs/, acpi/\n\n'
	sort -s -k1,1 "$SUMMARY"
} > "$SUMMARY.tmp" && mv "$SUMMARY.tmp" "$SUMMARY"
if [ "$IS_ROOT" -eq 1 ] && [ "$RUN_USER" != root ]; then chown -R "$RUN_USER:" "$OUT"; fi
cat "$SUMMARY"
echo
echo "report: $OUT"
