#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# stage.sh -- install one package's files into a package root. Debian's rules and the PKGBUILD
# both call it, so the file lists exist once; they follow nix/module.nix, helper/install.sh and
# userspace/platform/install.sh.
#
#   packaging/stage.sh --layout deb|arch --component NAME [--build DIR] DESTDIR
#
# Components: helper, settings, digitizerd, platform, cmdline, keyring.
# --build: the cargo target directory with the release binaries (default helper/target/release).
# Only files are written; nothing on the running system is touched.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
top=$(cd "$here/.." && pwd)
layout=
comp=
build=$top/helper/target/release
while [ $# -gt 1 ]; do
	case $1 in
	--layout) layout=$2; shift 2 ;;
	--component) comp=$2; shift 2 ;;
	--build) build=$2; shift 2 ;;
	*) break ;;
	esac
done
[ $# = 1 ] || { echo "usage: stage.sh --layout deb|arch --component NAME [--build DIR] DESTDIR" >&2; exit 2; }
d=$1

case $layout in
# Debian policy 4.6.2 allows /usr/libexec; Arch keeps private executables in /usr/lib/<name>.
deb) libexec=/usr/libexec ;;
arch) libexec=/usr/lib ;;
*) echo "stage.sh: --layout deb or arch" >&2; exit 2 ;;
esac
unitdir=/usr/lib/systemd/system
userunitdir=/usr/lib/systemd/user
hd=$top/helper/data
pf=$top/userspace/platform
app=io.github.joonhoekim.OpenDeviceHelper.SurfacePro9
bus=io.github.joonhoekim.OpenDeviceHelper1

f() { install -Dm644 "$1" "$d$2"; }      # file
x() { install -Dm755 "$1" "$d$2"; }      # executable
subst() { # src dst sed-expression
	install -d "$d$(dirname "$2")"
	sed "$3" "$1" > "$d$2"
	chmod 644 "$d$2"
}
bin() { [ -x "$build/$1" ] && [ -s "$build/$1" ] || { echo "stage.sh: $build/$1 not built" >&2; exit 1; }; x "$build/$1" "$2"; }
# The kernel command line of userspace/platform/cmdline, on one line.
params() { tr -s ' \n' '  ' < "$pf/cmdline/sp9-platform.cmdline" | sed 's/^ //; s/ $//'; }

case $comp in
helper)
	bin sp9-helperd "$libexec/sp9-helperd"
	bin sp9-ctl /usr/bin/sp9-ctl
	# the daemon runs these from <its own directory>/sp9-helper
	x "$top/tools/linux/post-install-check.sh" "$libexec/sp9-helper/post-install-check.sh"
	f "$top/tools/sdc-blob.py" "$libexec/sp9-helper/sdc-blob.py"
	subst "$hd/sp9-helperd.service" "$unitdir/sp9-helperd.service" "s|@LIBEXECDIR@|$libexec|"
	f "$here/systemd/sp9-helperd-modprobe.conf" "$unitdir/sp9-helperd.service.d/10-sp9-modprobe.conf"
	f "$hd/$bus.conf" "/usr/share/dbus-1/system.d/$bus.conf"
	f "$hd/$bus.service" "/usr/share/dbus-1/system-services/$bus.service"
	f "$hd/io.github.joonhoekim.opendevicehelper.sp9.policy" /usr/share/polkit-1/actions/io.github.joonhoekim.opendevicehelper.sp9.policy
	f "$hd/sp9-helper-modules.conf" /usr/lib/modules-load.d/sp9-helper.conf
	f "$hd/helper.toml.example" /usr/share/doc/sp9-helper/helper.toml.example
	# the unit writes the iptsd drop-in here (ProtectSystem=strict needs the directory to exist)
	install -d "$d/etc/iptsd.d"
	if [ "$layout" = arch ]; then
		# the optional, report-only release check (off until its timer is enabled)
		x "$here/arch/sp9-update-check" /usr/lib/sp9-helper/sp9-update-check
		f "$here/arch/sp9-update-check.service" "$unitdir/sp9-update-check.service"
		f "$here/arch/sp9-update-check.timer" "$unitdir/sp9-update-check.timer"
	fi
	;;
settings)
	bin sp9-settings /usr/bin/sp9-settings
	f "$hd/$app.desktop" "/usr/share/applications/$app.desktop"
	f "$hd/$app.svg" "/usr/share/icons/hicolor/scalable/apps/$app.svg"
	f "$hd/$app.metainfo.xml" "/usr/share/metainfo/$app.metainfo.xml"
	;;
digitizerd)
	bin sp9-digitizerd "$libexec/sp9-digitizerd"
	bin sp9-digitizer-replay /usr/bin/sp9-digitizer-replay
	subst "$hd/sp9-digitizerd.service" "$unitdir/sp9-digitizerd.service" "s|@BINDIR@|$libexec|"
	f "$here/systemd/sp9-digitizerd-modprobe.conf" "$unitdir/sp9-digitizerd.service.d/10-sp9-modprobe.conf"
	f "$hd/io.github.joonhoekim.Sp9Digitizer1.conf" /usr/share/dbus-1/system.d/io.github.joonhoekim.Sp9Digitizer1.conf
	f "$here/systemd/sp9-digitizerd-modules.conf" /usr/lib/modules-load.d/sp9-digitizerd.conf
	# sp9-helperd keeps the touch and pen settings in a drop-in there, which the daemon reads
	install -d "$d/etc/iptsd.d"
	;;
platform)
	f "$pf/iptsd/surface-pro-9.conf" /usr/share/iptsd/surface-pro-9.conf
	f "$pf/libinput/sp9-stylus.quirks" /usr/share/libinput/60-sp9-stylus.quirks
	f "$pf/udev/rules.d/61-sp9-sensors.rules" /usr/lib/udev/rules.d/61-sp9-sensors.rules
	for c in 60-sp9-speakers 61-sp9-echo-cancel; do
		f "$pf/pipewire/pipewire.conf.d/$c.conf" "/usr/share/pipewire/pipewire.conf.d/$c.conf"
	done
	for c in 60-sp9-speaker-raw 60-sp9-mic; do
		f "$pf/wireplumber/wireplumber.conf.d/$c.conf" "/usr/share/wireplumber/wireplumber.conf.d/$c.conf"
	done
	# raw speaker sink at 100 % after every WirePlumber start; wanted by wireplumber.service
	# through a link in the package, so no `systemctl --global enable` is needed
	# platform scripts live in /usr/lib/sp9-platform on both layouts (sp9-psr1.service names it)
	x "$pf/pipewire/sp9-speaker-raw-volume" /usr/lib/sp9-platform/sp9-speaker-raw-volume
	f "$here/systemd/sp9-speaker-raw-volume.service" "$userunitdir/sp9-speaker-raw-volume.service"
	install -d "$d$userunitdir/wireplumber.service.wants"
	ln -sf ../sp9-speaker-raw-volume.service "$d$userunitdir/wireplumber.service.wants/sp9-speaker-raw-volume.service"
	# PSR1 only through i915 debugfs (no tainting kernel parameter): started by the udev rule
	x "$pf/psr/sp9-psr1" /usr/lib/sp9-platform/sp9-psr1
	f "$pf/psr/sp9-psr1.service" "$unitdir/sp9-psr1.service"
	f "$pf/udev/rules.d/61-sp9-psr1.rules" /usr/lib/udev/rules.d/61-sp9-psr1.rules
	# pinctrl_tigerlake in the initramfs (power and volume buttons)
	f "$pf/initramfs/dracut/90-sp9-platform.conf" /usr/lib/dracut/dracut.conf.d/90-sp9-platform.conf
	if [ "$layout" = deb ]; then
		f "$pf/initramfs/initramfs-tools/sp9-platform" /usr/share/initramfs-tools/modules.d/sp9-platform
	else
		f "$pf/initramfs/mkinitcpio/sp9-platform.conf" /etc/mkinitcpio.conf.d/sp9-platform.conf
		f "$here/arch/sp9-platform-initcpio.hook" /usr/share/libalpm/hooks/90-sp9-platform-initcpio.hook
	fi
	# opt-in files that are not needed with the current firmware: copied by hand if ever
	for o in firmware/sp9-alc274-pins.fw modprobe.d/sp9-alc274-pins.conf modprobe.d/sp9-audio-legacy.conf sleep.conf.d/sp9-hibernate-reboot.conf; do
		f "$pf/optional/$o" "/usr/share/sp9-platform/optional/$o"
	done
	;;
cmdline)
	p=$(params)
	install -d "$d/usr/share/sp9-platform"
	printf '%s\n' "$p" > "$d/usr/share/sp9-platform/cmdline"
	chmod 644 "$d/usr/share/sp9-platform/cmdline"
	if [ "$layout" = deb ]; then
		install -d "$d/etc/default/grub.d"
		{
			echo "# sp9-cmdline: the kernel parameters of userspace/platform/cmdline/sp9-platform.cmdline."
			echo "# update-grub reads this file; removing the package removes the parameters again."
			# shellcheck disable=SC2016
			printf 'GRUB_CMDLINE_LINUX_DEFAULT="${GRUB_CMDLINE_LINUX_DEFAULT} %s"\n' "$p"
		} > "$d/etc/default/grub.d/sp9-platform.cfg"
		chmod 644 "$d/etc/default/grub.d/sp9-platform.cfg"
	else
		# mkinitcpio reads /etc/cmdline.d/*.conf when it builds a unified kernel image
		install -d "$d/etc/cmdline.d"
		printf '%s\n' "$p" > "$d/etc/cmdline.d/sp9-platform.conf"
		chmod 644 "$d/etc/cmdline.d/sp9-platform.conf"
	fi
	;;
keyring)
	k=$here/keyring/sp9-archive-keyring.asc
	[ -s "$k" ] || { echo "stage.sh: $k missing (tools/repo-key.sh creates it)" >&2; exit 1; }
	g=$(mktemp -d)
	if [ "$layout" = deb ]; then
		install -d "$d/usr/share/keyrings"
		gpg --homedir "$g" --batch --quiet --dearmor < "$k" > "$d/usr/share/keyrings/sp9-archive-keyring.gpg"
		chmod 644 "$d/usr/share/keyrings/sp9-archive-keyring.gpg"
	else
		f "$k" /usr/share/pacman/keyrings/sp9.gpg
		install -d "$d/usr/share/pacman/keyrings"
		# every primary key in the file is trusted fully (4); none is revoked
		gpg --homedir "$g" --batch --quiet --with-colons --show-keys "$k" |
			awk -F: '$1 == "pub" { p = 1; next } p && $1 == "fpr" { print $10 ":4:"; p = 0 }' > "$d/usr/share/pacman/keyrings/sp9-trusted"
		: > "$d/usr/share/pacman/keyrings/sp9-revoked"
		chmod 644 "$d/usr/share/pacman/keyrings/sp9-trusted" "$d/usr/share/pacman/keyrings/sp9-revoked"
	fi
	rm -rf "$g"
	;;
*)
	echo "stage.sh: unknown component '$comp'" >&2
	exit 2
	;;
esac
