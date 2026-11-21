#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# new-targets.sh -- read-only: list linux-surface's debian-* and arch-* kernel releases newer than
# the newest of each kind in dkms/targets.tsv and print a targets.tsv line for each, with every pin
# looked up: the commit the tag points to, the kernel release string, the stable tarball's sha256
# from kernel.org's sha256sums.asc and, for arch, the archlinux patch's sha256 from that commit's
# PKGBUILD. Needs gh (any token; the repositories are public) and curl.
#
#   dkms/ci/new-targets.sh [--all]     --all: also unlisted releases older than the newest listed one
#                                      (never older series)
set -euo pipefail
export LC_ALL=C
DKMS=$(dirname "$(dirname "$(readlink -f "$0")")")
LS=linux-surface/linux-surface
all=0; [ "${1:-}" = --all ] && all=1

newest_listed() { awk -F'\t' -v k="$1-" '!/^#/ && index($1, k) == 1 { print $1 }' "$DKMS/targets.tsv" | sed "s/^$1-//" | sort -V | tail -n1; }

gh api --paginate "repos/$LS/releases" --jq '.[].tag_name' | grep -E '^(debian|arch)-[0-9]+\.[0-9]+\.[0-9]+-[0-9]+$' |
	sort -t- -k2,2V -k3,3n | while read -r tag; do
		kind=${tag%%-*} rest=${tag#*-}
		listed=$(newest_listed "$kind")
		# Nothing older than the newest listed series.
		if [ -n "$listed" ]; then
			ls_series=$(echo "$listed" | cut -d. -f1-2) t_series=$(echo "$rest" | cut -d. -f1-2)
			[ "$(printf '%s\n%s\n' "$ls_series" "$t_series" | sort -V | head -n1)" = "$ls_series" ] || continue
		fi
		if [ $all = 0 ] && [ -n "$listed" ] && [ "$(printf '%s\n%s\n' "$listed" "$rest" | sort -V | tail -n1)" = "$listed" ]; then continue; fi
		if awk -F'\t' -v t="$tag" '$1 == t { f = 1 } END { exit !f }' "$DKMS/targets.tsv"; then continue; fi
		stable=${rest%-*} rev=${rest##*-}
		obj=$(gh api "repos/$LS/git/ref/tags/$tag" --jq '.object.type + " " + .object.sha')
		[ "${obj%% *}" = commit ] || obj="commit $(gh api "repos/$LS/git/tags/${obj#* }" --jq .object.sha)"
		commit=${obj#* }
		sums=$(curl -fsSL "https://cdn.kernel.org/pub/linux/kernel/v${stable%%.*}.x/sha256sums.asc")
		tarsha=$(echo "$sums" | awk -v f="linux-$stable.tar.xz" '$2 == f { print $1 }')
		[ -n "$tarsha" ] || { echo "new-targets.sh: no sha256 for linux-$stable.tar.xz" >&2; exit 1; }
		if [ "$kind" = debian ]; then
			rel="$stable-$rev-surface" dist=-
		else
			pkgb=$(gh api -H 'Accept: application/vnd.github.raw' "repos/$LS/contents/pkg/arch/kernel/PKGBUILD?ref=$commit")
			read -r pkgver dist < <(bash -c 'eval "$(cat)" >/dev/null 2>&1
				for i in "${!source[@]}"; do case ${source[$i]} in *-arch*.patch.zst) echo "$pkgver ${sha256sums[$i]}" ;; esac; done' <<<"$pkgb")
			[ "${pkgver%.arch*}" = "$stable" ] || { echo "new-targets.sh: $tag: PKGBUILD pkgver $pkgver" >&2; exit 1; }
			rel="$stable-${pkgver##*.}-$rev-surface"
		fi
		printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$tag" "$rel" "$stable" "$commit" "$tarsha" "$dist"
	done
