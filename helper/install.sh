#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Joonhoe Kim
# install.sh -- install sp9-helperd and sp9-ctl (after `cargo build --release`), the touch and pen
# daemon sp9-digitizerd in place of iptsd, and the settings app sp9-settings when it was built
# (`cargo build --release -p sp9-settings`, needs GTK 4 and libadwaita development files).
#   ./install.sh [--keep-iptsd] [--uninstall]
#   --keep-iptsd  iptsd stays the touch and pen daemon: sp9-digitizerd is installed but its unit is
#                 not enabled (NixOS: digitizerd.package). Without it iptsd's units are masked and
#                 sp9-digitizerd is enabled (NixOS: touch.daemon = "digitizerd"); only one of the two
#                 may run.
#   PREFIX=/usr DESTDIR=/tmp/pkg ./install.sh          # packaging: files only, the running system is not touched
#   BUILD_DIR=target/debug ./install.sh                # binaries from another build directory
# Defaults: PREFIX=/usr/local, LIBEXECDIR=$PREFIX/libexec. systemd, dbus-daemon, polkit and
# modules-load read /usr/local only for some of their directories, so a /usr/local install puts
# the unit, the D-Bus policy and the module list into /etc; PREFIX=/usr uses the /usr paths.
set -eu
PREFIX=${PREFIX:-/usr/local}
DESTDIR=${DESTDIR:-}
LIBEXECDIR=${LIBEXECDIR:-$PREFIX/libexec}
BINDIR=${BINDIR:-$PREFIX/bin}
if [ "$PREFIX" = /usr ]; then
	UNITDIR=${UNITDIR:-/usr/lib/systemd/system}
	DBUSCONF=${DBUSCONF:-/usr/share/dbus-1/system.d}
	MODLOADDIR=${MODLOADDIR:-/usr/lib/modules-load.d}
else
	UNITDIR=${UNITDIR:-/etc/systemd/system}
	DBUSCONF=${DBUSCONF:-/etc/dbus-1/system.d}
	MODLOADDIR=${MODLOADDIR:-/etc/modules-load.d}
fi
DBUSSVC=${DBUSSVC:-/usr/share/dbus-1/system-services}
POLKIT=${POLKIT:-/usr/share/polkit-1/actions}
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
B=${BUILD_DIR:-$here/target/release}
NAME=io.github.joonhoekim.OpenDeviceHelper1
APP=io.github.joonhoekim.OpenDeviceHelper.SurfacePro9
DNAME=io.github.joonhoekim.Sp9Digitizer1
# Written when this script masked iptsd, so that --uninstall unmasks only what it masked.
MASKED=$LIBEXECDIR/sp9-helper/iptsd-masked

keep_iptsd=false
uninstall=false
for a in "$@"; do
	case $a in
	--keep-iptsd) keep_iptsd=true ;;
	--uninstall) uninstall=true ;;
	-h | --help) sed -n '4,16s/^# \{0,1\}//p' "$0"; exit 0 ;;
	*) echo "unknown argument $a (--help)" >&2; exit 2 ;;
	esac
done

files() {
	echo "$LIBEXECDIR/sp9-helperd $BINDIR/sp9-ctl $UNITDIR/sp9-helperd.service"
	echo "$LIBEXECDIR/sp9-helper/post-install-check.sh $LIBEXECDIR/sp9-helper/sdc-blob.py"
	echo "$DBUSCONF/$NAME.conf $DBUSSVC/$NAME.service"
	echo "$POLKIT/io.github.joonhoekim.opendevicehelper.sp9.policy"
	echo "$MODLOADDIR/sp9-helper.conf $PREFIX/share/doc/sp9-helper/helper.toml.example"
	echo "$BINDIR/sp9-settings $PREFIX/share/applications/$APP.desktop"
	echo "$PREFIX/share/icons/hicolor/scalable/apps/$APP.svg $PREFIX/share/metainfo/$APP.metainfo.xml"
	echo "$BINDIR/sp9-digitizerd $BINDIR/sp9-digitizer-replay $UNITDIR/sp9-digitizerd.service"
	echo "$DBUSCONF/$DNAME.conf $MODLOADDIR/sp9-digitizerd.conf $MASKED"
}

# iptsd runs one instance per hidraw node (iptsd@<node>.service, started by its udev rule).
iptsd_units() { systemctl list-units --plain --no-legend 'iptsd@*.service' 2>/dev/null | awk '{ print $1 }'; }

if $uninstall; then
	if [ -z "$DESTDIR" ]; then
		systemctl disable --now sp9-helperd.service sp9-digitizerd.service 2>/dev/null || true
		if [ -e "$MASKED" ]; then
			systemctl unmask iptsd@.service || true
			echo "iptsd unmasked; it starts again with the next device event or boot"
		fi
	fi
	for f in $(files); do rm -f "$DESTDIR$f"; done
	rmdir "$DESTDIR$LIBEXECDIR/sp9-helper" 2>/dev/null || true
	[ -z "$DESTDIR" ] && systemctl daemon-reload || true
	echo "removed (settings kept in /etc/sp9, reports in /var/lib/sp9, the iptsd drop-in in /etc/iptsd.d)"
	exit 0
fi

if [ -z "$DESTDIR" ]; then
	# The power limits, fan request and SAM requests are this device's (the unit has the same condition).
	sku=$(cat /sys/class/dmi/id/product_sku 2>/dev/null || true)
	[ "$sku" = Surface_Pro_9_2038 ] || { echo "not a Surface Pro 9 Intel (product_sku '${sku:-unknown}'); use DESTDIR to package" >&2; exit 1; }
	[ "$(id -u)" = 0 ] || { echo "run as root (or set DESTDIR)" >&2; exit 1; }
fi
[ -x "$B/sp9-helperd" ] && [ -s "$B/sp9-helperd" ] || { echo "build first: cargo build --release" >&2; exit 1; }
install -Dm755 "$B/sp9-helperd" "$DESTDIR$LIBEXECDIR/sp9-helperd"
install -Dm755 "$B/sp9-ctl" "$DESTDIR$BINDIR/sp9-ctl"
install -Dm755 "$repo/tools/linux/post-install-check.sh" "$DESTDIR$LIBEXECDIR/sp9-helper/post-install-check.sh"
install -Dm644 "$repo/tools/sdc-blob.py" "$DESTDIR$LIBEXECDIR/sp9-helper/sdc-blob.py"
tmp=$(mktemp)
sed "s|@LIBEXECDIR@|$LIBEXECDIR|" "$here/data/sp9-helperd.service" > "$tmp"
install -Dm644 "$tmp" "$DESTDIR$UNITDIR/sp9-helperd.service"
rm -f "$tmp"
install -Dm644 "$here/data/$NAME.conf" "$DESTDIR$DBUSCONF/$NAME.conf"
install -Dm644 "$here/data/$NAME.service" "$DESTDIR$DBUSSVC/$NAME.service"
install -Dm644 "$here/data/io.github.joonhoekim.opendevicehelper.sp9.policy" "$DESTDIR$POLKIT/io.github.joonhoekim.opendevicehelper.sp9.policy"
install -Dm644 "$here/data/sp9-helper-modules.conf" "$DESTDIR$MODLOADDIR/sp9-helper.conf"
install -Dm644 "$here/data/helper.toml.example" "$DESTDIR$PREFIX/share/doc/sp9-helper/helper.toml.example"
# sp9-digitizerd: the daemon, its replay tool, the unit, its D-Bus policy and uinput at boot
# (nix/digitizerd.nix and the touch.daemon = "digitizerd" part of nix/module.nix).
[ -x "$B/sp9-digitizerd" ] && [ -s "$B/sp9-digitizerd" ] || { echo "build first: cargo build --release (sp9-digitizerd is missing)" >&2; exit 1; }
install -Dm755 "$B/sp9-digitizerd" "$DESTDIR$BINDIR/sp9-digitizerd"
install -Dm755 "$B/sp9-digitizer-replay" "$DESTDIR$BINDIR/sp9-digitizer-replay"
tmp=$(mktemp)
sed "s|@BINDIR@|$BINDIR|" "$here/data/sp9-digitizerd.service" > "$tmp"
install -Dm644 "$tmp" "$DESTDIR$UNITDIR/sp9-digitizerd.service"
rm -f "$tmp"
install -Dm644 "$here/data/$DNAME.conf" "$DESTDIR$DBUSCONF/$DNAME.conf"
tmp=$(mktemp)
printf '%s\n' "# sp9-digitizerd opens /dev/uinput once, when it first sees the digitizer." uinput > "$tmp"
install -Dm644 "$tmp" "$DESTDIR$MODLOADDIR/sp9-digitizerd.conf"
rm -f "$tmp"
if [ -x "$B/sp9-settings" ] && [ -s "$B/sp9-settings" ]; then
	install -Dm755 "$B/sp9-settings" "$DESTDIR$BINDIR/sp9-settings"
	install -Dm644 "$here/data/$APP.desktop" "$DESTDIR$PREFIX/share/applications/$APP.desktop"
	install -Dm644 "$here/data/$APP.svg" "$DESTDIR$PREFIX/share/icons/hicolor/scalable/apps/$APP.svg"
	install -Dm644 "$here/data/$APP.metainfo.xml" "$DESTDIR$PREFIX/share/metainfo/$APP.metainfo.xml"
	if [ -z "$DESTDIR" ]; then
		command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
		command -v update-desktop-database >/dev/null && update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true
	fi
else
	echo "note: sp9-settings not built (cargo build --release -p sp9-settings); installing the service and CLI only"
fi
if [ -z "$DESTDIR" ]; then
	modprobe surface_aggregator_cdev 2>/dev/null || echo "note: surface_aggregator_cdev not loaded (smart charging and the fan request stay unavailable)"
	# the unit may write the iptsd drop-in here (ProtectSystem=strict needs the directory to exist)
	install -d /etc/iptsd.d
	systemctl daemon-reload
	systemctl reload dbus 2>/dev/null || true
	if $keep_iptsd; then
		systemctl disable --now sp9-digitizerd.service 2>/dev/null || true
		if [ -e "$MASKED" ]; then
			systemctl unmask iptsd@.service || true
			rm -f "$MASKED"
		fi
		echo "sp9-digitizerd installed, not enabled: iptsd stays the touch and pen daemon"
	else
		modprobe uinput 2>/dev/null || true
		# One daemon at a time: mask the template so the udev rule starts no new instance, then
		# stop the running ones. A mask that was there before is left alone on --uninstall.
		if [ "$(systemctl is-enabled iptsd@.service 2>/dev/null || true)" != masked ]; then
			systemctl mask iptsd@.service >/dev/null
			install -Dm644 /dev/null "$MASKED"
		fi
		for u in $(iptsd_units); do systemctl stop "$u" || true; done
		systemctl enable sp9-digitizerd.service
		systemctl restart sp9-digitizerd.service
		echo "sp9-digitizerd enabled, iptsd masked (--keep-iptsd keeps iptsd)"
	fi
	# after the digitizer daemon: the helper hands a running sp9-digitizerd its settings
	systemctl enable sp9-helperd.service
	systemctl restart sp9-helperd.service
	echo "installed; sp9-ctl status"
fi
