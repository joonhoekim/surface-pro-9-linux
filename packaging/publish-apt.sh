#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# publish-apt.sh -- make the signed apt repository from the built packages (reprepro).
#
#   packaging/publish-apt.sh IN OUT
#
# IN holds one directory per CI artifact:
#   pkg-deb-<codename>-<name>/*.deb   packages for that suite (built in its container)
#   pkg-deb-all-<name>/*.deb          architecture-independent packages for every suite (the
#                                     hook for packages built once, such as sp9-dkms)
# OUT/apt becomes the repository: dists/<codename>/{InRelease,Release,Release.gpg,main/...} and
# pool/. Every codename of packaging/targets.tsv is a suite, component main, architecture amd64.
# The repository is made new each time from the current release only (apt needs nothing older).
# Signing: SIGNING_KEY is the fingerprint of a secret key in the gpg keyring (GNUPGHOME).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
in=${1:?usage: publish-apt.sh IN OUT}
out=${2:?usage: publish-apt.sh IN OUT}
: "${SIGNING_KEY:?set SIGNING_KEY to the signing key fingerprint}"
command -v reprepro > /dev/null || { echo "publish-apt.sh: reprepro not installed" >&2; exit 1; }

conf=$(mktemp -d)
trap 'rm -rf "$conf"' EXIT
mkdir -p "$conf/conf" "$out/apt"
codenames=$(awk -F'\t' '!/^#/ && NF >= 3 { print $3 }' "$here/targets.tsv")
for c in $codenames; do
	cat >> "$conf/conf/distributions" <<-EOF
	Origin: surface-pro-9-linux
	Label: surface-pro-9-linux
	Suite: $c
	Codename: $c
	Architectures: amd64
	Components: main
	Description: Surface Pro 9 (Intel) packages of surface-pro-9-linux for $c
	SignWith: $SIGNING_KEY

	EOF
done
cat > "$conf/conf/options" <<-EOF
	outdir $(cd "$out/apt" && pwd)
	dbdir $conf/db
	EOF

n=0
for c in $codenames; do
	for d in "$in"/pkg-deb-"$c"-* "$in"/pkg-deb-all-*; do
		[ -d "$d" ] || continue
		for f in "$d"/*.deb; do
			[ -e "$f" ] || continue
			reprepro -b "$conf" --keepunreferencedfiles includedeb "$c" "$f"
			n=$((n + 1))
		done
	done
done
[ $n -gt 0 ] || { echo "publish-apt.sh: no packages in $in" >&2; exit 1; }
reprepro -b "$conf" export
for c in $codenames; do
	gpgv_out=$(gpg --batch --verify "$out/apt/dists/$c/InRelease" 2>&1) || { echo "$gpgv_out" >&2; exit 1; }
done
echo "apt repository: $n package files in $(echo $codenames | wc -w) suites, signed by $SIGNING_KEY"
