#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# version.sh -- the version every package is built with (docs/releasing.md).
#
#   packaging/version.sh             Debian upstream version
#   packaging/version.sh --arch      pacman pkgver
#   packaging/version.sh --describe  git describe for SP9_GIT_DESCRIBE, empty at a release tag
#   packaging/version.sh --tag       the release tag vX.Y.Z when HEAD is exactly at it, else empty
#
# The base is [workspace.package] version of helper/Cargo.toml, the project's one version.
#   at the tag v<base>:                0.2.0                  0.2.0
#   N commits after the tag v<base>:   0.2.0+git5.gabc1234    0.2.0.r5.gabc1234
#   v<base> not tagged yet:            0.2.0~git49.gabc1234   0.2.0pre.r49.gabc1234
# (the last sorts before the release in both apt and pacman; N counts every commit then). A source
# tarball (packaging/source-tarball.sh) has no git: its VERSION and DESCRIBE files hold the result.
set -eu
top=$(cd "$(dirname "$0")/.." && pwd)
mode=${1:-}

base=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\([^"]*\)".*/\1/p' "$top/helper/Cargo.toml")
case $base in [0-9]*.[0-9]*.[0-9]*) ;; *) echo "version.sh: no version in helper/Cargo.toml" >&2; exit 1 ;; esac

# A tarball unpacked inside another checkout must not take that checkout's history.
if [ ! -e "$top/.git" ]; then
	[ -f "$top/VERSION" ] || { echo "version.sh: no git checkout and no VERSION file" >&2; exit 1; }
	v=$(cat "$top/VERSION")
	case $mode in
	'') echo "$v" ;;
	--arch) echo "$v" | sed 's/+git\([0-9]*\)\./.r\1./; s/~git\([0-9]*\)\./pre.r\1./' ;;
	--describe) cat "$top/DESCRIBE" 2> /dev/null || true ;;
	--tag) case $v in *git*) ;; *) echo "v$v" ;; esac ;;
	*) echo "usage: version.sh [--arch|--describe|--tag]" >&2; exit 2 ;;
	esac
	exit 0
fi

h=g$(git -C "$top" rev-parse --short=7 HEAD)
if git -C "$top" rev-parse -q --verify "refs/tags/v$base" > /dev/null; then
	n=$(git -C "$top" rev-list --count "v$base..HEAD")
	sep=+
else
	n=$(git -C "$top" rev-list --count HEAD)
	sep="~"
fi
describe=$(git -C "$top" describe --tags --match 'v[0-9]*' --long --abbrev=7 2> /dev/null || echo "${h#g}")

case $mode in
'')
	if [ "$n" = 0 ] && [ "$sep" = + ]; then echo "$base"; else echo "$base${sep}git$n.$h"; fi
	;;
--arch)
	if [ "$n" = 0 ] && [ "$sep" = + ]; then echo "$base"
	elif [ "$sep" = + ]; then echo "$base.r$n.$h"
	else echo "${base}pre.r$n.$h"; fi
	;;
--describe) [ "$n" = 0 ] && [ "$sep" = + ] || echo "$describe" ;;
--tag) [ "$n" = 0 ] && [ "$sep" = + ] && echo "v$base" || true ;;
*) echo "usage: version.sh [--arch|--describe|--tag]" >&2; exit 2 ;;
esac
