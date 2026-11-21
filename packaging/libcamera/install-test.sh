#!/bin/bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
#
# Install test of the sp9-libcamera package in a container of its target distribution (as root):
#
#   packaging/libcamera/install-test.sh sp9-libcamera_*.deb        (Debian, Ubuntu)
#   packaging/libcamera/install-test.sh sp9-libcamera-*.pkg.tar.zst (Arch)
#
# Installs the package, checks that every ELF file finds libcamera in /usr/lib/sp9-libcamera and
# the rest in the distribution, that the IPA module signature verifies against the public key built
# into libcamera, that cam --list runs, that spa-inspect loads the plugin, and that a PipeWire daemon
# started with the drop-in's SPA_PLUGIN_DIR takes the libcamera plugin from /usr/lib/sp9-libcamera
# and everything else from the distribution. Then removes the package and checks that nothing is
# left. A container has no cameras, so no frame is captured.

set -euo pipefail

[[ $# -ge 1 ]] || { echo "usage: $0 PACKAGE..." >&2; exit 1; }
[[ $(id -u) == 0 ]] || { echo "run as root inside the test container" >&2; exit 1; }

PREFIX=/usr/lib/sp9-libcamera
PLUGIN=$PREFIX/spa-0.2/libcamera/libspa-libcamera.so
fail=0
ok() { echo "PASS: $*"; }
bad() { echo "FAIL: $*"; fail=1; }

pkgs=()
for p in "$@"; do pkgs+=("$(realpath "$p")"); done

. /etc/os-release
if command -v dpkg >/dev/null; then
	kind=deb
	export DEBIAN_FRONTEND=noninteractive
	target=()
	if [[ ${VERSION_CODENAME:-} == trixie ]]; then
		cat > /etc/apt/sources.list.d/trixie-backports.sources <<-EOF
		Types: deb
		URIs: http://deb.debian.org/debian
		Suites: trixie-backports
		Components: main
		Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
		EOF
		target=(-t trixie-backports)
	fi
	apt-get update -q
	# The tools of the test: binutils (readelf, nm), openssl, pipewire-bin (spa-inspect, pipewire).
	apt-get install -y -q --no-install-recommends "${target[@]}" binutils openssl pipewire-bin "${pkgs[@]}"
	DISTRO_SPA=/usr/lib/$(dpkg-architecture -qDEB_HOST_MULTIARCH 2>/dev/null || echo x86_64-linux-gnu)/spa-0.2
	files() { dpkg -L sp9-libcamera; }
else
	kind=arch
	pacman -Syu --noconfirm --needed binutils openssl file pipewire
	pacman -U --noconfirm "${pkgs[@]}"
	DISTRO_SPA=/usr/lib/spa-0.2
	files() { pacman -Qlq sp9-libcamera; }
fi
echo "== $PRETTY_NAME, package files:"
files | grep -v '/$' | sed 's/^/  /'

# Every ELF file: RUNPATH is the prefix (none on libcamera itself), libcamera resolves into the
# prefix, nothing is missing.
mapfile -t elfs < <(files | while read -r f; do
	[[ -f $f && ! -L $f ]] && [[ $(head -c4 "$f" | od -An -c | tr -d ' ') == 177ELF ]] && echo "$f"
done)
[[ ${#elfs[@]} -ge 5 ]] && ok "${#elfs[@]} ELF files" || bad "only ${#elfs[@]} ELF files"
for f in "${elfs[@]}"; do
	rp=$(readelf -d "$f" | sed -n 's/.*(RUNPATH).*\[\(.*\)\]/\1/p')
	case $f in
	"$PREFIX"/lib/libcamera.so.*|"$PREFIX"/lib/libcamera-base.so.*)
		# libcamera with a RUNPATH considers itself not installed (source_paths.cpp)
		[[ -z $rp ]] && ok "no RUNPATH $f" || bad "RUNPATH of $f is '$rp'" ;;
	*)
		[[ $rp == "$PREFIX/lib" ]] && ok "RUNPATH $f" || bad "RUNPATH of $f is '$rp'" ;;
	esac
	grep -q '(RPATH)' <<<"$(readelf -d "$f")" && bad "$f has an RPATH"
	case $f in
	# No RUNPATH (above): libcamera.so finds libcamera-base through the RUNPATH of its users, each
	# of which needs both libraries directly (checked below).
	"$PREFIX"/lib/libcamera.so.*) out=$(LD_LIBRARY_PATH=$PREFIX/lib ldd "$f") ;;
	*) out=$(ldd "$f") ;;
	esac
	if [[ $f != "$PREFIX"/lib/libcamera*.so.* ]]; then
		needed=$(readelf -d "$f")
		if grep -q 'libcamera.so.0.8' <<<"$needed" && ! grep -q 'libcamera-base.so.0.8' <<<"$needed"; then
			bad "$f needs libcamera but not libcamera-base directly"
		fi
	fi
	if grep -q 'not found' <<<"$out"; then bad "ldd $f: $(grep 'not found' <<<"$out" | xargs)"; fi
	while read -r lib _ path _; do
		case $lib in
		libcamera.so.*|libcamera-base.so.*)
			[[ $path == "$PREFIX/lib/"* ]] || bad "$f: $lib resolves to $path" ;;
		esac
	done <<<"$out"
	if grep -q 'libcamera' <<<"$out"; then
		grep 'libcamera' <<<"$out" | sed "s|^|  ${f##*/}: |"
	fi
done
grep -q "$PREFIX/lib/libcamera.so" <<<"$(ldd "$PLUGIN")" && ok "plugin links the patched libcamera" ||
	bad "plugin does not resolve libcamera into $PREFIX"
grep -q "$PREFIX/lib/libcamera.so" <<<"$(ldd "$PREFIX/bin/cam")" && ok "cam links the patched libcamera" ||
	bad "cam does not resolve libcamera into $PREFIX"

# IPA module signature, against the public key compiled into libcamera (IPAManager::publicKeyData_).
lib=$(ls "$PREFIX"/lib/libcamera.so.0.8.*)
syms=$(nm -D --defined-only -S "$lib")
sym=$(awk '/publicKeyData_/ { print $1, $2 }' <<<"$syms")
if [[ -n $sym ]]; then
	read -r addr size <<<"$sym"
	# Virtual address to file offset through the LOAD segment that contains it.
	a=$((16#$addr)) off=
	while read -r type o v _ s _; do
		[[ $type == LOAD ]] || continue
		if (( a >= v && a < v + s )); then off=$(( a - v + o )); break; fi
	done < <(readelf -lW "$lib")
	tmp=$(mktemp -d)
	dd if="$lib" of="$tmp/pub.der" bs=1 skip="$off" count=$((16#$size)) status=none
	for m in "$PREFIX"/lib/libcamera/ipa/*.so; do
		if openssl dgst -sha256 -verify "$tmp/pub.der" -keyform DER -signature "$m.sign" "$m" >/dev/null; then
			ok "signature of ${m##*/} verifies: the module runs in the camera's process"
		else
			bad "signature of ${m##*/} does not verify: the module would run isolated"
		fi
	done
	rm -rf "$tmp"
else
	bad "no IPAManager::publicKeyData_ in $lib (signature not checked)"
fi

# Tuning files and the compiled-in paths.
for t in ov5693 ov13858; do
	y=$PREFIX/share/libcamera/ipa/softisp/$t.yaml
	[[ -f $y ]] && grep -q 'flickerPeriod: 8333' "$y" && ok "tuning file $t (60 Hz)" || bad "tuning file $y"
done
strs=$(strings "$lib")
grep -qx "$PREFIX/lib/libcamera/ipa" <<<"$strs" && ok "IPA module path compiled in" ||
	bad "IPA module path not found in $lib"
# The sysconfdir (its ipa/ subdirectory is the first system directory for tuning files): libcamera
# names the global configuration file it fails to parse. The compiler merges string constants, so
# the path cannot be looked up in the binary itself.
mkdir -p /etc/sp9-libcamera/libcamera
echo '{' > /etc/sp9-libcamera/libcamera/configuration.yaml
out=$(HOME=/nonexistent XDG_CONFIG_HOME=/nonexistent "$PREFIX/bin/cam" --list 2>&1 || true)
rm -rf /etc/sp9-libcamera
grep -q 'configuration file "/etc/sp9-libcamera/libcamera/configuration.yaml"' <<<"$out" &&
	ok "sysconfdir /etc/sp9-libcamera/libcamera" || bad "sysconfdir not used: $(grep -i configuration <<<"$out")"

# cam runs (no cameras in a container).
if out=$(LIBCAMERA_LOG_LEVELS='*:INFO' "$PREFIX/bin/cam" --list 2>&1); then ok "cam --list"; else bad "cam --list: $out"; fi
grep -q 'not installed' <<<"$out" && bad "libcamera considers itself not installed"
echo "$out" | sed 's/^/  cam: /'
"$PREFIX/bin/cam" --version 2>&1 | sed 's/^/  cam: /' || true

# spa-inspect loads the plugin and instantiates its factories.
if out=$(SPA_PLUGIN_DIR=$PREFIX/spa-0.2:$DISTRO_SPA timeout 20 spa-inspect "$PLUGIN" 2>&1); then
	grep -q 'api.libcamera.enum.manager' <<<"$out" && grep -q 'api.libcamera.source' <<<"$out" &&
		ok "spa-inspect lists the libcamera factories" || bad "spa-inspect: factories missing"
else
	bad "spa-inspect failed: $(tail -3 <<<"$out")"
fi

# Drop-ins and the lookup order of SPA_PLUGIN_DIR in a PipeWire daemon.
for unit in pipewire wireplumber; do
	d=/usr/lib/systemd/user/$unit.service.d/sp9-libcamera.conf
	want="Environment=SPA_PLUGIN_DIR=$PREFIX/spa-0.2:$DISTRO_SPA"
	grep -qx "$want" "$d" && ok "$d" || bad "$d does not contain $want"
done
[[ -d $DISTRO_SPA/support ]] && ok "distribution SPA directory $DISTRO_SPA" || bad "no $DISTRO_SPA/support"
spa_dir=$(sed -n 's/^Environment=SPA_PLUGIN_DIR=//p' /usr/lib/systemd/user/pipewire.service.d/sp9-libcamera.conf)
run=$(mktemp -d)
mkdir -p "$run/conf"
cat > "$run/conf/sp9-test.conf" <<-'EOF'
context.properties = { support.dbus = false }
context.spa-libs = {
    api.libcamera.* = libcamera/libspa-libcamera
    support.*       = support/libspa-support
}
context.modules = [
    { name = libpipewire-module-protocol-native }
    { name = libpipewire-module-spa-device-factory }
]
context.objects = [
    { factory = spa-device-factory args = { factory.name = api.libcamera.enum.manager } }
]
EOF
log=$run/pipewire.log
XDG_RUNTIME_DIR=$run PIPEWIRE_RUNTIME_DIR=$run SPA_PLUGIN_DIR=$spa_dir PIPEWIRE_DEBUG=4 \
	timeout 5 pipewire -c "$run/conf/sp9-test.conf" >"$log" 2>&1 || true
if grep -q "loaded plugin:'$PLUGIN'" "$log"; then
	ok "PipeWire loads the libcamera plugin from $PREFIX"
else
	bad "PipeWire did not load $PLUGIN"; grep -i 'libcamera\|plugin' "$log" | tail -10
fi
grep -q "loaded plugin:'$DISTRO_SPA/support/libspa-support.so'" "$log" &&
	ok "PipeWire loads the support plugin from $DISTRO_SPA" || bad "support plugin not loaded from $DISTRO_SPA"
grep "loaded plugin:" "$log" | sed 's/.*loaded plugin:/  pipewire: loaded /'
rm -rf "$run"

# Remove and check that nothing is left.
mapfile -t owned < <(files)
if [[ $kind == deb ]]; then
	apt-get purge -y -q sp9-libcamera
	dpkg -s sp9-libcamera >/dev/null 2>&1 && bad "sp9-libcamera still installed"
else
	pacman -Rns --noconfirm sp9-libcamera
	pacman -Q sp9-libcamera >/dev/null 2>&1 && bad "sp9-libcamera still installed"
fi
left=0
for f in "${owned[@]}"; do
	case $f in
	"$PREFIX"*|*/sp9-libcamera*|/etc/sp9-libcamera*)
		[[ -e $f || -L $f ]] && { echo "  left: $f"; left=1; } ;;
	esac
done
[[ -e $PREFIX || -e /etc/sp9-libcamera ]] && { echo "  left: $PREFIX or /etc/sp9-libcamera"; left=1; }
[[ $left == 0 ]] && ok "removed without leftovers" || bad "files left after removal"

if [[ $fail == 0 ]]; then echo "install-test: all checks passed"; else echo "install-test: FAILED"; fi
exit "$fail"
