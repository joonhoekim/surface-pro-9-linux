#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
#
# Adjust the libcamera SPA plugin of a PipeWire 1.6 source tree to libcamera 0.8, which renamed the
# enumerators of properties::Location (CameraLocationFront -> LocationFront, ...) and replaced
# libcamera::Span with std::span. PipeWire 1.6.9 and later already use std::span. Each change is
# made only where the old spelling is present, so the script is a no-op on sources that have them.
#
# Usage: pipewire-libcamera-0.8.sh PIPEWIRE_SOURCE_DIR

set -eu

dir=${1:?usage: $0 PIPEWIRE_SOURCE_DIR}/spa/plugins/libcamera
[ -d "$dir" ] || { echo "no $dir" >&2; exit 1; }

for f in "$dir"/*.cpp "$dir"/*.hpp "$dir"/*.h; do
	[ -f "$f" ] || continue
	if grep -q 'properties::CameraLocation\(Front\|Back\|External\)' "$f"; then
		sed -i 's/properties::CameraLocation\(Front\|Back\|External\)/properties::Location\1/g' "$f"
		echo "pipewire-libcamera-0.8: Location enumerators renamed in ${f##*/}"
	fi
	if grep -q 'libcamera::Span<' "$f"; then
		sed -i 's/libcamera::Span</std::span</g' "$f"
		grep -q '^#include <span>' "$f" ||
			sed -i '0,/^#include </s//#include <span>\n#include </' "$f"
		echo "pipewire-libcamera-0.8: libcamera::Span replaced in ${f##*/}"
	fi
done

if grep -n 'properties::CameraLocation\|libcamera::Span<' "$dir"/*; then
	echo "pipewire-libcamera-0.8: old spellings remain" >&2
	exit 1
fi
