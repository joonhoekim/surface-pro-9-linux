#!/bin/bash
# SPDX-License-Identifier: MIT
# install.sh -- install the Surface Pro 9 (Intel) platform files.
#
#   sudo ./install.sh                        # default components into /
#   sudo ./install.sh --with hda-pins,audio-legacy
#   sudo ./install.sh --without cmdline
#   ./install.sh --dry-run                   # print what would change
#   sudo ./install.sh --uninstall            # remove everything this script installs
#   DESTDIR=/mnt/root ./install.sh           # files only, into a mounted root
#
# Components (default): iptsd initramfs cmdline speakers
# Components (opt-in):  hda-pins, audio-legacy, hibernate-reboot
#
# Runs only on SKU Surface_Pro_9_2038 (/sys/class/dmi/id/product_sku). With
# DESTDIR set nothing on the running system is touched (no initramfs or
# bootloader update, no kernel command line edit), so the check is skipped.
# No package manager is called; missing dependencies are reported.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
DESTDIR=${DESTDIR:-}
PREFIX=${PREFIX:-/usr}
SYSCONFDIR=${SYSCONFDIR:-/etc}
SKU=Surface_Pro_9_2038
DEFAULT="iptsd initramfs cmdline speakers"
ALL="iptsd initramfs cmdline speakers hda-pins audio-legacy hibernate-reboot"
state=$DESTDIR/var/lib/sp9-platform

dry=0
uninstall=0
with=""
without=""

usage() { sed -n '3,18p' "$0" | sed 's/^# \{0,1\}//'; }
die() { echo "error: $*" >&2; exit 1; }
note() { echo "note: $*" >&2; }

while [ $# -gt 0 ]; do
	case $1 in
	--dry-run) dry=1 ;;
	--uninstall) uninstall=1 ;;
	--with) [ $# -ge 2 ] || die "--with needs a list"; with="$with ${2//,/ }"; shift ;;
	--with=*) v=${1#*=}; with="$with ${v//,/ }" ;;
	--without) [ $# -ge 2 ] || die "--without needs a list"; without="$without ${2//,/ }"; shift ;;
	--without=*) v=${1#*=}; without="$without ${v//,/ }" ;;
	-h | --help) usage; exit 0 ;;
	*) usage >&2; exit 2 ;;
	esac
	shift
done

has() { # word list...
	local w=$1; shift
	for x in "$@"; do [ "$x" = "$w" ] && return 0; done
	return 1
}

for c in $with $without; do
	# shellcheck disable=SC2086
	has "$c" $ALL || die "unknown component: $c"
done

# --- device and environment ---------------------------------------------------

live=0
[ -z "$DESTDIR" ] && live=1
if [ $live = 1 ]; then
	sku=$(cat /sys/class/dmi/id/product_sku 2>/dev/null || true)
	[ "$sku" = "$SKU" ] || die "not a Surface Pro 9 Intel (product_sku '${sku:-unknown}', want $SKU)"
	[ $dry = 1 ] || [ "$(id -u)" = 0 ] || die "run as root (or with --dry-run)"
fi

family=unknown
if [ -r "$DESTDIR/etc/os-release" ]; then
	# shellcheck disable=SC1091
	ids=$(. "$DESTDIR/etc/os-release"; echo "${ID:-} ${ID_LIKE:-}")
	case " $ids " in
	*" debian "* | *" ubuntu "*) family=debian ;;
	*" fedora "*) family=fedora ;;
	*" arch "*) family=arch ;;
	esac
fi

pkg() { # what -> package name for this family
	case $1:$family in
	swh:fedora) echo ladspa-swh-plugins ;;
	swh:*) echo swh-plugins ;;
	iptsd:*) echo iptsd ;;
	esac
}

run() {
	if [ $dry = 1 ]; then echo "+ $*"; else "$@"; fi
}

changed_initramfs=0
changed_grub=0

put() { # mode src dst -> 0 if the file was written
	local d=$DESTDIR$3
	if [ -f "$d" ] && cmp -s "$here/$2" "$d"; then
		return 1
	fi
	run install -D -m "$1" "$here/$2" "$d"
	echo "installed $3"
}

del() { # dst -> 0 if the file was removed
	local d=$DESTDIR$1
	[ -e "$d" ] || return 1
	run rm -f "$d"
	echo "removed $1"
}

# --- components ----------------------------------------------------------------

iptsd_files="iptsd/surface-pro-9.conf:$PREFIX/share/iptsd/surface-pro-9.conf"
speakers_files="pipewire/pipewire.conf.d/60-sp9-speakers.conf:$PREFIX/share/pipewire/pipewire.conf.d/60-sp9-speakers.conf
wireplumber/wireplumber.conf.d/60-sp9-speaker-raw.conf:$PREFIX/share/wireplumber/wireplumber.conf.d/60-sp9-speaker-raw.conf"
legacy_files="optional/modprobe.d/sp9-audio-legacy.conf:$PREFIX/lib/modprobe.d/sp9-audio-legacy.conf"
hibernate_files="optional/sleep.conf.d/sp9-hibernate-reboot.conf:$PREFIX/lib/systemd/sleep.conf.d/sp9-hibernate-reboot.conf"
hda_conf=$PREFIX/lib/modprobe.d/sp9-alc274.conf

put_list() { # "src:dst" lines
	local l
	while IFS= read -r l; do
		[ -n "$l" ] && { put 644 "${l%%:*}" "${l#*:}" || true; }
	done <<< "$1"
}
del_list() {
	local l
	while IFS= read -r l; do
		[ -n "$l" ] && { del "${l#*:}" || true; }
	done <<< "$1"
}

# initramfs: one drop-in per generator present in the target root
initramfs_targets() {
	[ -e "$DESTDIR/usr/share/initramfs-tools/hook-functions" ] &&
		echo "initramfs/initramfs-tools/sp9-platform:$PREFIX/share/initramfs-tools/modules.d/sp9-platform"
	{ [ -d "$DESTDIR/usr/lib/dracut" ] || [ -d "$DESTDIR/lib/dracut" ]; } &&
		echo "initramfs/dracut/90-sp9-platform.conf:$PREFIX/lib/dracut/dracut.conf.d/90-sp9-platform.conf"
	[ -e "$DESTDIR/etc/mkinitcpio.conf" ] &&
		echo "initramfs/mkinitcpio/sp9-platform.conf:$SYSCONFDIR/mkinitcpio.conf.d/sp9-platform.conf"
	return 0
}
initramfs_all="initramfs/initramfs-tools/sp9-platform:$PREFIX/share/initramfs-tools/modules.d/sp9-platform
initramfs/dracut/90-sp9-platform.conf:$PREFIX/lib/dracut/dracut.conf.d/90-sp9-platform.conf
initramfs/mkinitcpio/sp9-platform.conf:$SYSCONFDIR/mkinitcpio.conf.d/sp9-platform.conf"

install_initramfs() {
	local t l
	t=$(initramfs_targets)
	[ -n "$t" ] || { note "no initramfs-tools, dracut or mkinitcpio found: add pinctrl_tigerlake to the initramfs yourself"; return 0; }
	while IFS= read -r l; do
		put 644 "${l%%:*}" "${l#*:}" && changed_initramfs=1
	done <<< "$t"
	return 0
}
remove_initramfs() {
	local l
	while IFS= read -r l; do
		del "${l#*:}" && changed_initramfs=1
	done <<< "$initramfs_all"
	return 0
}

rebuild_initramfs() {
	[ $changed_initramfs = 1 ] || return 0
	if [ $live = 0 ]; then
		note "rebuild the initramfs inside the target system"
		return 0
	fi
	if [ -e /usr/share/initramfs-tools/hook-functions ] && command -v update-initramfs > /dev/null; then
		run update-initramfs -u -k all
	elif command -v dracut > /dev/null; then
		run dracut --regenerate-all -f
	fi
	if [ -e /etc/mkinitcpio.conf ] && command -v mkinitcpio > /dev/null; then
		run mkinitcpio -P
	fi
}

# kernel command line ------------------------------------------------------------
params=$(tr -s ' \n' '  ' < "$here/cmdline/sp9-platform.cmdline" | sed 's/ *$//')
cmdline_ref=$PREFIX/share/sp9-platform/cmdline
grubd=$SYSCONFDIR/default/grub.d/sp9-platform.cfg
kcmd=$SYSCONFDIR/kernel/cmdline

grub_mkconfig() { command -v grub-mkconfig || command -v grub2-mkconfig || true; }

# pci= may appear several times (options add up); other keys take one value
single() { [ "${1%%=*}" != pci ]; }

value_of() { # key string -> value of the first key=... token, empty if absent
	local t
	for t in $2; do
		case $t in "$1="*) echo "${t#*=}"; return 0 ;; esac
	done
	return 0
}

warn_conflicts() { # string where
	local p k v
	for p in $params; do
		single "$p" || continue
		k=${p%%=*}
		v=$(value_of "$k" "$1")
		if [ -n "$v" ] && [ "$k=$v" != "$p" ]; then
			note "$2 already has $k=$v; this installs $p"
		fi
	done
}

install_cmdline() {
	put 644 cmdline/sp9-platform.cmdline "$cmdline_ref" || true
	if [ $live = 0 ]; then
		note "kernel command line not applied under DESTDIR: add '$params' in the target system"
		return 0
	fi
	warn_conflicts "$(cat /proc/cmdline)" "the running kernel's command line"
	local handled=0 mk cur t added=""

	# Fedora (BLS entries): grubby edits every entry and the default
	if command -v grubby > /dev/null; then
		cur=$(grubby --info=DEFAULT 2>/dev/null | sed -n 's/^args="\(.*\)"$/\1/p')
		for t in $params; do
			case " $cur " in *" $t "*) ;; *) added="$added $t" ;; esac
		done
		if [ -n "$added" ]; then
			run grubby --update-kernel=ALL --args="$params"
			echo "grubby: added$added"
		fi
		handled=1
	fi

	# Debian/Ubuntu GRUB: grub-mkconfig sources /etc/default/grub.d/*.cfg
	mk=$(grub_mkconfig)
	if [ $handled = 0 ] && [ -n "$mk" ] && grep -q 'default/grub.d' "$mk"; then
		if [ ! -f "$grubd" ] || ! grep -qF "$params" "$grubd"; then
			if [ $dry = 1 ]; then
				echo "+ write $grubd"
			else
				mkdir -p "$(dirname "$grubd")"
				# shellcheck disable=SC2016
				printf 'GRUB_CMDLINE_LINUX_DEFAULT="${GRUB_CMDLINE_LINUX_DEFAULT} %s"\n' "$params" > "$grubd"
			fi
			echo "installed $grubd"
			changed_grub=1
		fi
		handled=1
	fi

	# kernel-install (systemd-boot, UKIs): /etc/kernel/cmdline. A parameter is
	# appended only when absent; an existing i915.enable_psr value is kept.
	if [ -f "$kcmd" ] || { [ $handled = 0 ] && command -v bootctl > /dev/null && bootctl is-installed > /dev/null 2>&1; }; then
		local base new
		if [ -f "$kcmd" ]; then
			base=$(tr '\n' ' ' < "$kcmd")
			warn_conflicts "$base" "$kcmd"
		else
			base=$(tr ' ' '\n' < /proc/cmdline | grep -v -e '^BOOT_IMAGE=' -e '^initrd=' | tr '\n' ' ')
			[ $dry = 1 ] || { mkdir -p "$state"; : > "$state/kernel-cmdline-created"; }
		fi
		new=$base added=""
		for t in $params; do
			case " $base " in *" $t "*) continue ;; esac
			single "$t" && [ -n "$(value_of "${t%%=*}" "$base")" ] && continue
			new="$new $t" added="$added $t"
		done
		if [ -n "$added" ] || [ ! -f "$kcmd" ]; then
			new=$(echo "$new" | tr -s ' ' | sed 's/^ //; s/ $//')
			if [ $dry = 1 ]; then
				echo "+ write $kcmd: $new"
			else
				mkdir -p "$state" "$(dirname "$kcmd")"
				echo "$added" >> "$state/kernel-cmdline-added"
				echo "$new" > "$kcmd"
			fi
			echo "$kcmd: added$added"
			note "regenerate boot entries/UKIs (e.g. kernel-install add \$(uname -r) /usr/lib/modules/\$(uname -r)/vmlinuz)"
		fi
		handled=1
	fi

	if [ $handled = 0 ]; then
		note "bootloader not handled here: add '$params' to the kernel command line yourself (GRUB without grub.d: GRUB_CMDLINE_LINUX_DEFAULT in /etc/default/grub)"
	fi
}

remove_cmdline() {
	del "$cmdline_ref" || true
	[ $live = 1 ] || return 0
	if command -v grubby > /dev/null; then
		run grubby --update-kernel=ALL --remove-args="$params"
	fi
	del "$grubd" && changed_grub=1
	if [ -f "$state/kernel-cmdline-created" ]; then
		del "$kcmd" || true
	elif [ -f "$state/kernel-cmdline-added" ] && [ -f "$kcmd" ]; then
		local t out="" toks
		read -r -a toks < "$kcmd"
		for t in "${toks[@]}"; do
			grep -qxF -- "$t" <(tr ' ' '\n' < "$state/kernel-cmdline-added") || out="$out $t"
		done
		if [ $dry = 1 ]; then
			echo "+ write $kcmd:$out"
		else
			echo "${out# }" > "$kcmd"
		fi
		echo "$kcmd: removed the added parameters"
	fi
	[ $dry = 1 ] || rm -f "$state/kernel-cmdline-created" "$state/kernel-cmdline-added"
	[ $dry = 1 ] || rmdir "$state" 2>/dev/null || true
}

update_grub() {
	[ $changed_grub = 1 ] && [ $live = 1 ] || return 0
	if command -v update-grub > /dev/null; then
		run update-grub
	elif [ -n "$(grub_mkconfig)" ] && [ -f /boot/grub/grub.cfg ]; then
		run "$(grub_mkconfig)" -o /boot/grub/grub.cfg
	else
		note "regenerate grub.cfg yourself"
	fi
}

# HDA codec patch ----------------------------------------------------------------
install_hda() {
	put 644 "optional/firmware/sp9-alc274-pins.fw" "$PREFIX/lib/firmware/sp9-alc274-pins.fw" || true
	put 644 "optional/modprobe.d/sp9-alc274-pins.conf" "$hda_conf" || true
	if [ $live = 1 ] && [ -d /sys/module/snd_sof ]; then
		note "SOF drives the codec now; patch= only takes effect on the legacy path (component audio-legacy)"
	fi
}
remove_hda() {
	del "$PREFIX/lib/firmware/sp9-alc274-pins.fw" || true
	del "$hda_conf" || true
}

# shellcheck disable=SC2086
check_deps() {
	if has speakers $selected; then
		if ! ls "$DESTDIR"/usr/lib*/ladspa/sc4_1882.so "$DESTDIR"/usr/lib/*/ladspa/sc4_1882.so > /dev/null 2>&1; then
			note "speakers: LADSPA sc4/fastLookaheadLimiter not found; install $(pkg swh). Without it the filter chain is skipped"
		fi
	fi
	if has iptsd $selected && [ ! -x "$DESTDIR/usr/bin/iptsd" ]; then
		note "iptsd: daemon not installed; install $(pkg iptsd) (linux-surface repository)"
	fi
	return 0
}

# --- main -----------------------------------------------------------------------

if [ $uninstall = 1 ]; then
	del_list "$iptsd_files"
	del_list "$speakers_files"
	del_list "$legacy_files"
	del_list "$hibernate_files"
	remove_hda
	remove_initramfs
	remove_cmdline
	rebuild_initramfs
	update_grub
	echo "Uninstalled. Reboot (or restart PipeWire/WirePlumber and iptsd) for it to take effect."
	exit 0
fi

selected=""
for c in $DEFAULT $with; do
	# shellcheck disable=SC2086
	has "$c" $without || has "$c" $selected || selected="$selected $c"
done

[ $dry = 1 ] && echo "dry run; components:$selected (family: $family)"
for c in $selected; do
	case $c in
	iptsd) put_list "$iptsd_files" ;;
	speakers) put_list "$speakers_files" ;;
	initramfs) install_initramfs ;;
	cmdline) install_cmdline ;;
	hda-pins) install_hda ;;
	audio-legacy) put_list "$legacy_files" ;;
	hibernate-reboot) put_list "$hibernate_files" ;;
	esac
done
check_deps
rebuild_initramfs
update_grub
echo "Done. Reboot for the kernel command line, initramfs and modprobe changes; restart iptsd, PipeWire and WirePlumber for the rest."
