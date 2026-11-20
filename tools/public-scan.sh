#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# public-scan.sh -- check a tree for content that must not be published. Read-only.
#
#   tools/public-scan.sh [-q] [PATH...]     (default: the repository this script is in)
#
# PATH is a directory or a file. One line per finding, "file:line: KIND: text" (line 0 for findings
# about a whole file), then a count per kind. Exit status 1 when there is any finding, 0 when clean,
# 2 on a usage error. -q prints only the counts.
#
# Kinds:
#   hangul     Hangul text. The tree is English. Allowed: output strings (not comment lines) of the
#              Windows tools under tools/windows/, and the Korean localisation of the settings app
#              (HANGUL_FILES below).
#   address    a token shaped like a MAC or Bluetooth address (aa:bb:cc:dd:ee:ff, aa-bb-..., or
#              twelve hex digits after Dev_ / BTHLE / addr). Allowed: the synthetic values in
#              ADDRESS_OK, which the pen tools' tests and examples use.
#   homepath   /home/<name> or C:\Users\<name> with a real-looking name (use ~, $HOME, %LOCALAPPDATA%).
#   private    names of things that exist only outside this repository: the Perl regular expression in
#              the file named by $PUBLIC_SCAN_PRIVATE (default: .git/info/public-scan-private of the
#              repository, which is never committed). Not scanned for when that file is absent.
#   position   positional references into driver binaries or firmware blobs: module@0x..., RVA,
#              blob:<n>, and the Korean for "file offset". Facts taken from a driver are stated
#              without saying where in the file they are. Not a finding: the message arguments of
#              i2ctransfer (w2@0x36, r1@0x0c: a count and an I2C address).
#   binary     a non-empty file that `file --mime-encoding` calls binary.
#   size       a file over 200 kB (200000 bytes).
#
# Not scanned: .git, LICENSES/ (licence texts), the git-ignored directories /private/, /external/,
# /hw/, out/, target/, result*, __pycache__, the website's dependencies and generated files
# (node_modules/, .next/, site/public/_repo/, site/public/third-party-licenses.txt, *.tsbuildinfo),
# and this script (it has to spell out what it looks for).
# shellcheck disable=SC2016

set -u
export LC_ALL=C.UTF-8

SELF=$(readlink -f "${BASH_SOURCE[0]}")
REPO=$(cd "$(dirname "$SELF")/.." && pwd)

QUIET=0
[ "${1:-}" = "-q" ] && { QUIET=1; shift; }
case ${1:-} in -h | --help) sed -n '4,31p' "$SELF" | sed 's/^# \{0,1\}//'; exit 0 ;; -*) echo "unknown option: $1" >&2; exit 2 ;; esac
[ $# -eq 0 ] && set -- "$REPO"
for t in grep file find; do command -v "$t" >/dev/null 2>&1 || { echo "$t not found" >&2; exit 2; }; done
echo x | grep -qP 'x' 2>/dev/null || { echo "grep without -P (PCRE) support" >&2; exit 2; }

# ---------------------------------------------------------------- what is looked for

HANGUL_RE='[\x{1100}-\x{11FF}\x{3130}-\x{318F}\x{A960}-\x{A97F}\x{AC00}-\x{D7AF}\x{D7B0}-\x{D7FF}]'
# A comment line of a PowerShell, Python or C# source file.
COMMENT_RE='^\s*(#|//|/\*|\*)'
# Localisation data: Korean is the content. Paths relative to the scanned root.
HANGUL_FILES='
helper/crates/sp9-settings/src/i18n/ko.rs
helper/data/io.github.joonhoekim.OpenDeviceHelper.SurfacePro9.desktop
helper/data/io.github.joonhoekim.OpenDeviceHelper.SurfacePro9.metainfo.xml
'

ADDRESS_RE='(?<![0-9A-Fa-f:-])([0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}(?![0-9A-Fa-f:-])|(?i:dev_|bthle[a-z]*[\\#_]|addr(ess)?[=: ]+)[0-9A-Fa-f]{12}(?![0-9A-Fa-f])'
# Synthetic addresses (upper case; the comparison ignores case and the separator).
ADDRESS_OK='
AA:BB:CC:DD:EE:FF
11:22:33:44:55:66
66:55:44:33:22:11
00:00:00:00:00:00
FF:FF:FF:FF:FF:FF
ED:08:D9:17:E1:E7
C3:D5:15:AC:4B:7E
'

# A name directly after the directory; placeholders (<name>, $USER, %USERNAME%, *, ...) do not match
# because they do not start with a letter or digit. Generic names are not findings.
HOMEPATH_RE='/home/[A-Za-z0-9][A-Za-z0-9._-]*|[A-Za-z]:\\{1,2}Users\\{1,2}[A-Za-z0-9][A-Za-z0-9._ -]*'
HOMEPATH_OK='^(/home/(user|username|you|me|name)|[A-Za-z]:\\{1,2}Users\\{1,2}(Public|Default|user|username|name))$'

# The names themselves stay out of the repository: one Perl regular expression per line, joined with |.
PRIVATE_FILE=${PUBLIC_SCAN_PRIVATE:-$(git -C "$REPO" rev-parse --git-common-dir 2>/dev/null | sed "s|^[^/]|$REPO/&|")/info/public-scan-private}
PRIVATE_RE=""
if [ -r "$PRIVATE_FILE" ]; then
	PRIVATE_RE=$(grep -v -e '^#' -e '^$' "$PRIVATE_FILE" | paste -sd '|')
fi

# The lookahead leaves out i2ctransfer message arguments, <r|w><count>@0x<address>.
POSITION_RE='\b(?![rw][0-9]+@)[A-Za-z_][A-Za-z0-9_.]*@0x[0-9A-Fa-f]+|\bRVA\b|\bblob:[0-9]+|파일 오프셋'

MAX_SIZE=200000

# ---------------------------------------------------------------- scan

FOUND=$(mktemp) || exit 2
trap 'rm -f "$FOUND"' EXIT

# hit KIND FILE LINE TEXT
hit() { printf '%s:%s: %s: %s\n' "$2" "$3" "$1" "$(printf '%s' "$4" | cut -c1-200)" >> "$FOUND"; }

in_list() { # in_list VALUE LIST (one per line)
	local v=$1 l
	while IFS= read -r l; do [ -n "$l" ] && [ "$l" = "$v" ] && return 0; done <<<"$2"
	return 1
}

scan_file() { # scan_file FILE REL (REL: path relative to the scanned root, for the allow-lists)
	local f=$1 rel=$2 size enc n text tok norm

	[ "$(readlink -f "$f")" = "$SELF" ] && return
	size=$(wc -c < "$f")
	[ "$size" -eq 0 ] && return
	[ "$size" -gt "$MAX_SIZE" ] && hit size "$f" 0 "$size bytes"
	enc=$(file -b --mime-encoding "$f")
	if [ "$enc" = binary ]; then hit binary "$f" 0 "$(file -b "$f")"; return; fi

	# grep -a: a text file with a stray byte is still read line by line.
	if ! in_list "$rel" "$HANGUL_FILES"; then
		while IFS=: read -r n text; do
			case $rel in
				tools/windows/*.md) ;;
				tools/windows/*) grep -qP "$COMMENT_RE" <<<"$text" || continue ;;
			esac
			hit hangul "$f" "$n" "$text"
		done < <(grep -anP "$HANGUL_RE" "$f")
	fi

	while IFS=: read -r n tok; do
		norm=$(printf '%s' "$tok" | tr 'a-f-' 'A-F:')
		in_list "$norm" "$ADDRESS_OK" || hit address "$f" "$n" "$tok"
	done < <(grep -anoP "$ADDRESS_RE" "$f")

	while IFS=: read -r n tok; do
		grep -qiP "$HOMEPATH_OK" <<<"$tok" || hit homepath "$f" "$n" "$tok"
	done < <(grep -anoP "$HOMEPATH_RE" "$f")

	if [ -n "$PRIVATE_RE" ]; then
		while IFS=: read -r n text; do hit private "$f" "$n" "$text"; done < <(grep -anP "$PRIVATE_RE" "$f")
	fi
	while IFS=: read -r n text; do hit position "$f" "$n" "$text"; done < <(grep -anP "$POSITION_RE" "$f")
}

for root in "$@"; do
	[ -e "$root" ] || { echo "no such path: $root" >&2; exit 2; }
	if [ -f "$root" ]; then
		# A single file: its path relative to this repository decides the allow-lists.
		abs=$(readlink -f "$root")
		scan_file "$root" "${abs#"$REPO"/}"
		continue
	fi
	root=${root%/}
	# Allow-lists are relative to the repository root, also when a subdirectory is scanned.
	absroot=$(readlink -f "$root")
	case $absroot in "$REPO") prefix= ;; "$REPO"/*) prefix=${absroot#"$REPO"/}/ ;; *) prefix= ;; esac
	while IFS= read -r -d '' f; do
		scan_file "$f" "$prefix${f#"$root"/}"
	done < <(find "$root" \( -name .git -o -name LICENSES -o -name target -o -name out -o -name __pycache__ \
		-o -name 'result*' -o -name node_modules -o -name .next -o -name '*.tsbuildinfo' \
		-o -path "$root/private" -o -path "$root/external" -o -path "$root/hw" \
		-o -path '*site/public/_repo' -o -path '*site/public/third-party-licenses.txt' \) -prune \
		-o -type f -print0 | sort -z)
done

[ "$QUIET" -eq 1 ] || cat "$FOUND"
total=$(wc -l < "$FOUND")
if [ "$total" -eq 0 ]; then
	echo "public-scan: clean"
	exit 0
fi
echo "public-scan: $total finding(s)"
sed -E 's/^.*:[0-9]+: ([a-z]+): .*/\1/' "$FOUND" | sort | uniq -c | awk '{ printf "  %-9s %s\n", $2, $1 }'
exit 1
