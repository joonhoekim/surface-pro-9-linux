#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# test-deb.sh -- install test of the built .deb files in a Debian or Ubuntu container that runs
# systemd as PID 1 (packaging/docker.sh deb-test, or the CI workflow). Root.
#
#   packaging/test-deb.sh DEBDIR
#
# 1. installs what the packages depend on from the distribution, and the test tools, then takes a
#    list of every file;
# 2. installs all packages of DEBDIR (sp9-*.deb);
# 3. checks what can run without the hardware: the units (systemd-analyze verify), the udev rule,
#    the D-Bus, polkit, desktop and AppStream files; that sp9-helperd is skipped on this machine
#    (ConditionFirmware) and sp9-digitizerd runs and owns its bus name; and sp9-helperd with fake
#    backends on the system bus, read by an unprivileged sp9-ctl through the installed bus policy,
#    with a change refused by polkit;
# 4. purges the packages and compares the file list with the one of step 1: nothing may be left
#    behind, nothing of the system may be missing.
set -eu
debs=${1:?usage: test-deb.sh DEBDIR}
export DEBIAN_FRONTEND=noninteractive
here=$(cd "$(dirname "$0")" && pwd)
LIBEXEC=/usr/libexec
ENABLED=1
SNAPSHOT_EXCLUDE="-o -path /var/cache -o -path /var/lib/apt -o -path /var/lib/dpkg -o -path $debs"
. "$here/test-common.sh"

# systemd as PID 1 is up (degraded is fine in a container)
systemctl is-system-running --wait > /dev/null 2>&1 || true

pkgs=""
for f in "$debs"/sp9-*.deb; do
	[ -e "$f" ] || { echo "no sp9-*.deb in $debs"; exit 1; }
	n=$(dpkg-deb -f "$f" Package)
	# sp9-libcamera has its own test (packaging/libcamera/install-test.sh)
	[ "$n" = sp9-libcamera ] && continue
	pkgs="$pkgs $n"
done
files=$(for n in $pkgs; do ls "$debs"/"${n}"_*.deb; done)

step "dependencies and test tools"
apt-get update -q
# every dependency group of our packages that names none of them (or iptsd, which is not in the
# distribution: sp9-digitizerd stands for it here)
deps=$(for f in $files; do dpkg-deb -f "$f" Pre-Depends Depends; done |
	sed 's/^[A-Za-z-]*: //' | tr ',' '\n' | sed 's/^ *//; s/ *$//' | grep -v -e '^$' -e 'sp9' -e 'iptsd' | sort -u | paste -sd, -)
echo "dependencies: $deps"
apt-get satisfy -y -q --no-install-recommends "$deps"
apt-get install -y -q --no-install-recommends systemd dbus-daemon libxml2-utils desktop-file-utils appstream udev polkitd util-linux diffutils > /dev/null 2>&1 ||
	apt-get install -y -q --no-install-recommends systemd dbus libxml2-utils desktop-file-utils appstream udev polkitd util-linux
# dpkg may run dbus's reload trigger before polkitd's postinst creates the user polkitd (Ubuntu
# 26.04, 26.10): a bus whose policy was loaded before that user existed refuses polkitd its name,
# and polkit.service fails until its start limit. Reload the bus with the user in place.
systemctl reload dbus.service 2> /dev/null || true
systemctl reset-failed polkit.service 2> /dev/null || true
systemctl start dbus.service polkit.service 2> /dev/null || true

snapshot > /tmp/before

step "install:$pkgs"
# nothing but our packages may be installed here, or the comparison below would see their files
extra=$(apt-get install -s --no-install-recommends $files | sed -n 's/^Inst \([^ ]*\) .*/\1/p' | grep -v '^sp9' || true)
[ -z "$extra" ] || bad "installing the packages pulls more than the dependencies installed above: $extra"
apt-get install -y -q --no-install-recommends $files

checks

step "purge"
for n in $pkgs; do systemctl stop "$n.service" 2> /dev/null || true; done
apt-get purge -y -q $pkgs
systemctl daemon-reload 2> /dev/null || true
snapshot > /tmp/after
if diff /tmp/before /tmp/after > /tmp/diff; then ok "no files left behind, none missing"; else
	bad "the file system differs after purge (< missing, > left behind):"
	cat /tmp/diff
fi

echo
[ $fail = 0 ] && echo "install test passed" || echo "install test FAILED"
exit $fail
