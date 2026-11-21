#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# publish-pacman.sh -- make the signed pacman repository "sp9" from the built packages.
#
#   packaging/publish-pacman.sh IN OUT
#
# IN holds one directory per CI artifact, pkg-arch-<name>/*.pkg.tar.zst (the hook for more
# packages, such as sp9-dkms, is another such artifact). OUT/arch/x86_64 becomes the repository:
# every package with its detached signature (pacman requires package signatures by default) and
# the database sp9.db / sp9.files, signed (repo-add --sign). Symbolic links are replaced by copies,
# since GitHub Pages serves none. Made new from the current release only.
# Signing: SIGNING_KEY is the fingerprint of a secret key in the gpg keyring (GNUPGHOME).
set -eu
in=${1:?usage: publish-pacman.sh IN OUT}
out=${2:?usage: publish-pacman.sh IN OUT}
: "${SIGNING_KEY:?set SIGNING_KEY to the signing key fingerprint}"
command -v repo-add > /dev/null || { echo "publish-pacman.sh: repo-add (pacman) not installed" >&2; exit 1; }

r=$out/arch/x86_64
mkdir -p "$r"
n=0
for f in "$in"/pkg-arch-*/*.pkg.tar.zst; do
	[ -e "$f" ] || continue
	cp "$f" "$r/"
	b=$r/$(basename "$f")
	gpg --batch --yes --local-user "$SIGNING_KEY" --detach-sign --no-armor -o "$b.sig" "$b"
	n=$((n + 1))
done
[ $n -gt 0 ] || { echo "publish-pacman.sh: no packages in $in" >&2; exit 1; }
(cd "$r" && repo-add --sign --key "$SIGNING_KEY" --verify sp9.db.tar.zst ./*.pkg.tar.zst)
for l in sp9.db sp9.db.sig sp9.files sp9.files.sig; do
	[ -L "$r/$l" ] && cp --remove-destination "$(readlink -f "$r/$l")" "$r/$l"
done
gpg --batch --verify "$r/sp9.db.sig" "$r/sp9.db"
echo "pacman repository: $n packages, signed by $SIGNING_KEY"
