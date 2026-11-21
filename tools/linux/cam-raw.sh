#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Raw Bayer capture straight from the IPU6 ISYS node, past libcamera.
# usage: cam-raw.sh front|rear OUT.raw [exposure=N] [gain=N] [dgain=N] [focus=N] [frames=N] [size=WxH] [hflip=0|1] [vflip=0|1]
# OUT.raw holds the last frame, 16-bit little endian, 10 significant bits. Needs v4l-utils; run as a user in group video.
set -eu
cam=$1; out=$2; shift 2
exposure=; gain=; dgain=; focus=; frames=6; size=; hflip=; vflip=
for a in "$@"; do
	case $a in
	exposure=*|gain=*|dgain=*|focus=*|frames=*|size=*|hflip=*|vflip=*) eval "${a%%=*}=\${a#*=}" ;;
	*) echo "unknown argument $a" >&2; exit 2 ;;
	esac
done

node_of() { # entity name -> /dev node
	for d in /sys/class/video4linux/*; do
		[ "$(cat "$d/name")" = "$1" ] && { echo "/dev/${d##*/}"; return; }
	done
	echo "no entity $1" >&2; exit 1
}
sensor_of() { for d in /sys/class/video4linux/v4l-subdev*; do
	case "$(cat "$d/name")" in "$1 "*) cat "$d/name"; return ;; esac; done; echo "no sensor $1" >&2; exit 1; }

case $cam in
front) sensor=$(sensor_of ov5693); csi="Intel IPU6 CSI2 2"; cap="Intel IPU6 ISYS Capture 16"; code=SBGGR10_1X10; fourcc=BG10; : "${size:=2592x1944}" ;;
rear) sensor=$(sensor_of ov13858); csi="Intel IPU6 CSI2 1"; cap="Intel IPU6 ISYS Capture 8"; code=SGRBG10_1X10; fourcc=BA10; : "${size:=2112x1188}" ;;
*) echo "front or rear" >&2; exit 2 ;;
esac
sd=$(node_of "$sensor"); vid=$(node_of "$cap")
w=${size%x*}; h=${size#*x}

if [ -n "$hflip$vflip" ]; then v4l2-ctl -d "$sd" -c horizontal_flip=${hflip:-0},vertical_flip=${vflip:-0}; fi
media-ctl -d /dev/media0 -V "\"$sensor\":0 [fmt:$code/$size]"
# a flip changes the Bayer order the sensor reports
code=$(media-ctl -d /dev/media0 --get-v4l2 "\"$sensor\":0" | sed -n 's/.*fmt:\([A-Z0-9_]*\)\/.*/\1/p')
got=$(media-ctl -d /dev/media0 --get-v4l2 "\"$sensor\":0" | sed -n 's/.*fmt:[A-Z0-9_]*\/\([0-9x]*\).*/\1/p')
[ "$got" = "$size" ] || { echo "sensor gave $got, not $size" >&2; exit 1; }
case $code in SBGGR10_1X10) fourcc=BG10 ;; SGBRG10_1X10) fourcc=GB10 ;; SGRBG10_1X10) fourcc=BA10 ;; SRGGB10_1X10) fourcc=RG10 ;; esac
media-ctl -d /dev/media0 -V "\"$csi\":0 [fmt:$code/$size]"
media-ctl -d /dev/media0 -V "\"$csi\":1 [fmt:$code/$size]"
media-ctl -d /dev/media0 -l "\"$csi\":1 -> \"$cap\":0 [1]"
v4l2-ctl -d "$vid" --set-fmt-video "width=$w,height=$h,pixelformat=$fourcc" >/dev/null
ctrls=
[ -n "$exposure" ] && ctrls="exposure=$exposure"
[ -n "$gain" ] && ctrls="${ctrls:+$ctrls,}analogue_gain=$gain"
[ -n "$dgain" ] && ctrls="${ctrls:+$ctrls,}digital_gain=$dgain"
[ -n "$ctrls" ] && v4l2-ctl -d "$sd" -c "$ctrls"
if [ -n "$focus" ]; then v4l2-ctl -d "$(node_of "$(sensor_of dw9719)")" -c focus_absolute="$focus"; fi
log=$(timeout 15 v4l2-ctl -d "$vid" --stream-mmap --stream-count=1 --stream-skip=$((frames - 1)) --stream-to="$out" --verbose 2>&1) || true
echo "$cam $code $size dqbuf=$(echo "$log" | grep -c 'cap dqbuf') errors=$(echo "$log" | grep 'cap dqbuf' | grep -c '(error') $(v4l2-ctl -d "$sd" -C exposure,analogue_gain,digital_gain | tr '\n' ' ')-> $out"
