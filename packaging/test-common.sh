#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# test-common.sh -- the checks test-deb.sh and test-arch.sh share; sourced, with LIBEXEC set
# (/usr/libexec on Debian and Ubuntu, /usr/lib on Arch) and SNAPSHOT_EXCLUDE naming the package
# manager's own state directories.
fail=0
ok() { echo "ok: $*"; }
bad() { echo "FAIL: $*"; fail=1; }
step() { echo; echo "== $*"; }

# every file of the system, except what changes by itself or belongs to the package manager
snapshot() {
	# shellcheck disable=SC2086
	find / -xdev \( -path /proc -o -path /sys -o -path /dev -o -path /run -o -path /tmp -o -path /var/tmp \
		-o -path /var/log -o -path /var/lib/lastlog -o -path /var/lib/systemd -o -path /var/lib/polkit-1 -o -path /root -o -path /home \
		$SNAPSHOT_EXCLUDE \) -prune -o -print | sort
}

# what can be checked without the hardware, after the packages are installed
# ENABLED=1: the packages enable sp9-helperd (Debian, Ubuntu); Arch leaves enabling to the user.
checks() {
	step "files"
	for u in /usr/lib/systemd/system/sp9-helperd.service /usr/lib/systemd/system/sp9-digitizerd.service \
		/usr/lib/systemd/system/sp9-psr1.service /usr/lib/systemd/user/sp9-speaker-raw-volume.service; do
		[ -e "$u" ] || continue
		# --man=no: the units name no man pages; the device nodes they allow do not exist here
		if out=$(systemd-analyze verify --man=no "$u" 2>&1); then ok "systemd-analyze verify $u"; else
			echo "$out" | grep -v -e 'Unit is bound to inactive' -e 'modprobe@' || true
			echo "$out" | grep -q -i -e 'error' -e 'invalid' -e 'unknown' && bad "systemd-analyze verify $u" || ok "systemd-analyze verify $u (warnings only)"
		fi
	done
	if udevadm verify --help > /dev/null 2>&1; then
		for r in /usr/lib/udev/rules.d/61-sp9-*.rules; do
			udevadm verify --no-style "$r" && ok "udevadm verify $r" || bad "udevadm verify $r"
		done
	fi
	for x in /usr/share/dbus-1/system.d/io.github.joonhoekim.*.conf /usr/share/polkit-1/actions/io.github.joonhoekim.*.policy; do
		xmllint --noout "$x" && ok "xmllint $x" || bad "xmllint $x"
	done
	if [ -e /usr/share/applications/io.github.joonhoekim.OpenDeviceHelper.SurfacePro9.desktop ]; then
		desktop-file-validate /usr/share/applications/io.github.joonhoekim.OpenDeviceHelper.SurfacePro9.desktop && ok desktop-file-validate || bad desktop-file-validate
		appstreamcli validate --no-net --explain /usr/share/metainfo/io.github.joonhoekim.OpenDeviceHelper.SurfacePro9.metainfo.xml && ok "appstreamcli validate" ||
			bad "appstreamcli validate"
	fi
	for b in $LIBEXEC/sp9-helperd /usr/bin/sp9-ctl /usr/bin/sp9-settings $LIBEXEC/sp9-digitizerd /usr/bin/sp9-digitizer-replay; do
		[ -e "$b" ] || continue
		if ldd "$b" | grep -q 'not found'; then bad "$b: missing libraries"; else ok "$b links"; fi
	done

	if [ "${NO_SYSTEMD:-0}" = 1 ]; then
		plain_services
	else
		systemd_services
	fi
}

# systemd as PID 1 (CI): the units themselves
systemd_services() {
	step "services"
	systemctl daemon-reload
	if [ "${ENABLED:-0}" = 1 ]; then
		[ "$(systemctl is-enabled sp9-helperd.service)" = enabled ] && ok "sp9-helperd enabled" || bad "sp9-helperd not enabled"
	fi
	systemctl start sp9-helperd.service || true
	r=$(systemctl show -P ConditionResult sp9-helperd.service)
	a=$(systemctl show -P ActiveState sp9-helperd.service)
	[ "$r" = no ] && [ "$a" = inactive ] && ok "sp9-helperd skipped on another model (ConditionFirmware)" || bad "sp9-helperd: condition $r, state $a"
	if [ -e /usr/lib/systemd/system/sp9-digitizerd.service ]; then
		systemctl restart sp9-digitizerd.service
		sleep 2
		# active and waiting: it takes its bus name once it has the digitizer, which CI does not have
		[ "$(systemctl is-active sp9-digitizerd.service)" = active ] && ok "sp9-digitizerd active (waiting for the digitizer)" || {
			bad "sp9-digitizerd not active"
			journalctl -u sp9-digitizerd -n 20 --no-pager || true
		}
	fi

	step "sp9-helperd with fake backends on the system bus"
	systemctl reload dbus.service 2> /dev/null || true
	# Type=exec, not dbus: systemd refuses a second unit with the BusName= of sp9-helperd.service,
	# which is loaded; the bus name is waited for instead
	systemd-run --unit sp9-test-helperd --property Type=exec $LIBEXEC/sp9-helperd --fake
	i=0
	until busctl --system status io.github.joonhoekim.OpenDeviceHelper1 > /dev/null 2>&1; do
		i=$((i + 1))
		if [ $i -ge 20 ] || ! systemctl is-active -q sp9-test-helperd.service; then
			bad "sp9-helperd --fake did not take its bus name"
			journalctl -u sp9-test-helperd -n 20 --no-pager || true
			break
		fi
		sleep 0.5
	done
	[ $i -lt 20 ] && systemctl is-active -q sp9-test-helperd.service && ok "sp9-helperd --fake owns its bus name as root"
	client_checks
	systemctl stop sp9-test-helperd.service 2> /dev/null || true
}

# no systemd (local runs): the daemons by hand on a system bus of their own, with the installed
# bus policies and polkit; the container has no hidraw or uinput nodes
plain_services() {
	step "daemons without systemd (plain mode: units not started)"
	if [ "${ENABLED:-0}" = 1 ]; then
		[ -L /etc/systemd/system/multi-user.target.wants/sp9-helperd.service ] && ok "sp9-helperd enabled (link)" || bad "sp9-helperd not enabled"
	fi
	mkdir -p /run/dbus
	dbus-daemon --system --fork --nopidfile --print-pid > /tmp/dbus.pid && ok "system bus started" || bad "dbus-daemon --system"
	p=$(ls /usr/lib/polkit-1/polkitd /usr/libexec/polkitd /usr/lib/polkit/polkitd 2> /dev/null | head -n 1)
	[ -n "$p" ] && { "$p" --no-debug > /tmp/polkitd.log 2>&1 & }
	pk=$!
	if [ -e $LIBEXEC/sp9-digitizerd ]; then
		$LIBEXEC/sp9-digitizerd > /tmp/digitizerd.log 2>&1 &
		dz=$!
	fi
	$LIBEXEC/sp9-helperd --fake > /tmp/helperd.log 2>&1 &
	hd=$!
	sleep 3
	if [ -e $LIBEXEC/sp9-digitizerd ]; then
		# it takes its bus name once it has the digitizer, which a container does not have
		kill -0 $dz 2> /dev/null && grep -q 'waiting for the digitizer' /tmp/digitizerd.log && ok "sp9-digitizerd runs, waiting for the digitizer" ||
			{ bad "sp9-digitizerd is not waiting for the digitizer"; cat /tmp/digitizerd.log; }
	fi
	kill -0 $hd 2> /dev/null && ok "sp9-helperd --fake owns its bus name as root" || { bad "sp9-helperd --fake exited"; cat /tmp/helperd.log; }
	client_checks
	kill $hd ${dz:-} $pk 2> /dev/null || true
	sleep 1
	kill "$(cat /tmp/dbus.pid)" 2> /dev/null || true
	rm -rf /run/dbus
}

# an unprivileged client through the installed bus policy; a change without an active session
client_checks() {
	if out=$(runuser -u nobody -- sp9-ctl status 2>&1); then
		ok "sp9-ctl status as an unprivileged user"
		echo "$out" | sed -n '/\[Install\]/,$p' | head -n 20
	else
		bad "sp9-ctl status: $out"
	fi
	if out=$(runuser -u nobody -- sp9-ctl kbd 1 2>&1); then
		bad "an unprivileged caller without a session changed a setting: $out"
	else
		case $out in
		*NotAuthorized* | *"not authorized"* | *AccessDenied*) ok "polkit refused a change from outside an active session ($out)" ;;
		*)
			bad "the change failed, but not by authorization: $out"
			if [ "${NO_SYSTEMD:-0}" != 1 ]; then
				systemctl status --no-pager polkit.service || true
				journalctl -b --no-pager -n 40 -u polkit.service || true
			fi
			;;
		esac
	fi
}
