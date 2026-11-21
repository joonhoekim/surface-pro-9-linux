#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# test-arch.sh -- install test of the built Arch packages in an archlinux container that runs
# systemd as PID 1 (packaging/docker.sh arch-test, or the CI workflow). Root.
#
#   packaging/test-arch.sh PKGDIR
#
# The same steps as test-deb.sh: dependencies first and a list of every file; install every
# sp9-*.pkg.tar.zst of PKGDIR; the checks of test-common.sh, with the units enabled by hand as
# the documentation says (Arch packages enable nothing); disable and remove (pacman -Rn: the
# dependencies installed before the file list stay), then
# compare the file list.
set -eu
pkgs=${1:?usage: test-arch.sh PKGDIR}
here=$(cd "$(dirname "$0")" && pwd)
LIBEXEC=/usr/lib
SNAPSHOT_EXCLUDE="-o -path /var/cache/pacman -o -path /var/lib/pacman -o -path /etc/pacman.d/gnupg -o -path $pkgs"
. "$here/test-common.sh"

systemctl is-system-running --wait > /dev/null 2>&1 || true

files=""
names=""
for f in "$pkgs"/sp9-*.pkg.tar.zst; do
	[ -e "$f" ] || { echo "no sp9-*.pkg.tar.zst in $pkgs"; exit 1; }
	n=$(bsdtar -xOf "$f" .PKGINFO | sed -n 's/^pkgname = //p')
	# sp9-libcamera has its own test; sp9-keyring needs pacman-key set up (tested separately below)
	case $n in sp9-libcamera | sp9-keyring | *-debug) continue ;; esac
	files="$files $f"
	names="$names $n"
done

step "dependencies and test tools"
pacman-key --init > /dev/null 2>&1 || true
pacman -Syu --noconfirm > /dev/null
# shellcheck disable=SC2086
deps=$(for f in $files; do bsdtar -xOf "$f" .PKGINFO | sed -n 's/^depend = //p'; done | grep -v -e '^sp9' -e '^iptsd' | sed 's/[<>=].*//' | sort -u)
echo "dependencies:" $deps
# shellcheck disable=SC2086
pacman -S --noconfirm --needed --asdeps $deps libxml2 desktop-file-utils appstream pacman-contrib fakeroot util-linux diffutils > /dev/null
systemctl start dbus.service polkit.service 2> /dev/null || true
snapshot > /tmp/before

step "install:$names"
# shellcheck disable=SC2086
pacman -U --noconfirm $files
systemctl enable --now sp9-helperd.service sp9-digitizerd.service || true

checks

step "release check (sp9-update-check)"
if [ "${NO_SYSTEMD:-0}" = 1 ]; then
	# the script as an unprivileged user, with the directories the unit would give it
	d=$(mktemp -d)
	chmod 755 "$d"
	mkdir "$d/state" "$d/cache"
	chown nobody "$d/state" "$d/cache"
	if runuser -u nobody -- env STATE_DIRECTORY="$d/state" CACHE_DIRECTORY="$d/cache" /usr/lib/sp9-helper/sp9-update-check; then
		ok "sp9-update-check ran as nobody; notice: '$(cat "$d/state/notice")'"
	else
		bad "sp9-update-check failed"
	fi
	rm -rf "$d"
elif systemctl start sp9-update-check.service; then
	ok "sp9-update-check ran; notice: '$(cat /var/lib/sp9-update-check/notice)'"
else
	bad "sp9-update-check failed"
	journalctl -u sp9-update-check -n 20 --no-pager || true
fi

if ls "$pkgs"/sp9-keyring-*.pkg.tar.zst > /dev/null 2>&1; then
	step "sp9-keyring"
	pacman -U --noconfirm "$pkgs"/sp9-keyring-*.pkg.tar.zst
	fpr=$(sed -n 's/:4:$//p' /usr/share/pacman/keyrings/sp9-trusted | head -n 1)
	pacman-key --list-keys "$fpr" > /dev/null 2>&1 && ok "pacman-key knows the repository key" || bad "repository key not populated"
	pacman -Rns --noconfirm sp9-keyring
fi

step "remove"
systemctl disable --now sp9-helperd.service sp9-digitizerd.service || true
# shellcheck disable=SC2086
pacman -Rn --noconfirm $names
# what systemd made for the dynamic user, which the documentation says to remove
# and the helper's run-time files: what docs/install-arch.md section 7 removes by hand on Arch
rm -rf /var/lib/private/sp9-update-check /var/lib/sp9-update-check /var/cache/private/sp9-update-check /var/cache/sp9-update-check
rm -rf /etc/sp9 /var/lib/sp9 /etc/iptsd.d/zz-sp9-helper.conf
rmdir /etc/iptsd.d 2> /dev/null || true
systemctl daemon-reload 2> /dev/null || true
snapshot > /tmp/after
if diff /tmp/before /tmp/after > /tmp/diff; then ok "no files left behind, none missing"; else
	bad "the file system differs after removal (< missing, > left behind):"
	cat /tmp/diff
fi

echo
[ $fail = 0 ] && echo "install test passed" || echo "install test FAILED"
exit $fail
