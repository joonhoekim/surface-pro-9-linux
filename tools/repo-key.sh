#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# repo-key.sh -- create the OpenPGP key that signs the apt and pacman repositories, offline.
#
#   tools/repo-key.sh OUTDIR                 create a new key in OUTDIR (must not exist yet)
#   tools/repo-key.sh --renew-subkey OUTDIR  add a new signing subkey to the key in OUTDIR
#   options: --uid "NAME <EMAIL>"  --subkey-expiry 2y  --no-passphrase (tests only)
#
# The key is an Ed25519 primary key that only certifies (no expiry) and an Ed25519 signing subkey
# that expires (default 2 years). Only the subkey goes to CI: if the CI secret leaks, the subkey is
# revoked with the primary key and replaced; the primary key never leaves OUTDIR.
#
# OUTDIR gets:
#   gnupg/                     the GnuPG home with both secret keys: keep it offline (an encrypted
#                              USB stick, a password manager), never in the repository
#   primary-secret.asc         the same, exported, for a second offline copy
#   signing-subkey-secret.asc  the signing subkey without the primary key: the CI secret
#   sp9-archive-keyring.asc    the public key: copy it to packaging/keyring/ and commit it
#   fingerprint.txt            the primary key's fingerprint: put it into SECURITY.md
#
# Nothing is uploaded and no network is used. OUTDIR inside this repository is refused. The
# primary key gets a passphrase (asked on the terminal) unless --no-passphrase; the subkey export
# for CI has none, because CI cannot type one (GitHub encrypts the secret at rest).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
top=$(cd "$here/.." && pwd)
uid="surface-pro-9-linux repository signing key <26rote@gmail.com>"
expiry=2y
pass=1
renew=0
while [ $# -gt 1 ]; do
	case $1 in
	--uid) uid=$2; shift 2 ;;
	--subkey-expiry) expiry=$2; shift 2 ;;
	--no-passphrase) pass=0; shift ;;
	--renew-subkey) renew=1; shift ;;
	*) break ;;
	esac
done
[ $# = 1 ] || { sed -n '3,25p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
out=$1
command -v gpg > /dev/null || { echo "repo-key.sh: gpg not found" >&2; exit 1; }

if [ $renew = 0 ]; then
	[ ! -e "$out" ] || { echo "repo-key.sh: $out exists; pick a new directory (or --renew-subkey)" >&2; exit 1; }
	mkdir -p "$out"
fi
out=$(cd "$out" && pwd -P)
case $out/ in "$(cd "$top" && pwd -P)"/*)
	[ $renew = 1 ] || rmdir "$out"
	echo "repo-key.sh: $out is inside the repository; secret keys must not be there" >&2
	exit 1
	;;
esac
umask 077
export GNUPGHOME="$out/gnupg"
mkdir -p "$GNUPGHOME"
chmod 700 "$GNUPGHOME"
q="--batch --quiet --pinentry-mode loopback"

if [ $renew = 0 ]; then
	# shellcheck disable=SC2086
	gpg $q --passphrase '' --quick-generate-key "$uid" ed25519 cert never
	fpr=$(gpg --batch --with-colons --list-secret-keys | awk -F: '$1 == "fpr" { print $10; exit }')
	# shellcheck disable=SC2086
	gpg $q --passphrase '' --quick-add-key "$fpr" ed25519 sign "$expiry"
else
	fpr=$(cat "$out/fingerprint.txt")
	echo "The primary key's passphrase, to add the subkey:"
	gpg --pinentry-mode loopback --quick-add-key "$fpr" ed25519 sign "$expiry"
fi
# the newest signing subkey
sub=$(gpg --batch --with-colons --list-secret-keys "$fpr" |
	awk -F: '$1 == "ssb" { s = ($12 ~ /s/); next } s && $1 == "fpr" { f = $10; s = 0 } END { print f }')

# That subkey alone, without a passphrase ("!" exports exactly that subkey). A renewed key has
# a passphrase: it is removed in a throwaway copy of the keyring, the original keeps it.
if [ $renew = 0 ]; then
	gpg --batch --armor --export-secret-subkeys "$sub!" > "$out/signing-subkey-secret.asc"
else
	tmp=$(mktemp -d)
	cp -a "$GNUPGHOME/." "$tmp/"
	echo "Once more for the CI copy: the passphrase, then an empty one (only the copy loses it):"
	GNUPGHOME=$tmp gpg --pinentry-mode loopback --passwd "$fpr"
	GNUPGHOME=$tmp gpg --batch --armor --export-secret-subkeys "$sub!" > "$out/signing-subkey-secret.asc"
	rm -rf "$tmp"
fi
if [ $renew = 0 ] && [ $pass = 1 ]; then
	echo "Choose a passphrase for the primary key (it protects gnupg/ and primary-secret.asc):"
	gpg --pinentry-mode loopback --passwd "$fpr"
fi
gpg --batch --armor --export-secret-keys "$fpr" > "$out/primary-secret.asc"
umask 022
gpg --batch --armor --export-options export-minimal --export "$fpr" > "$out/sp9-archive-keyring.asc"
echo "$fpr" > "$out/fingerprint.txt"

cat <<EOF

Primary key: $fpr
Signing subkey: $sub (expires in $expiry)

Next steps (once; see docs/releasing.md):
  1. Public key into the repository, then commit it:
       cp "$out/sp9-archive-keyring.asc" packaging/keyring/sp9-archive-keyring.asc
     and put the fingerprint into SECURITY.md (section "Package signing").
  2. The subkey as the CI secret (GitHub encrypts it; only the publish job of
     .github/workflows/packages.yml reads it):
       gh secret set SP9_REPO_SIGNING_KEY < "$out/signing-subkey-secret.asc"
     then delete that file: shred -u "$out/signing-subkey-secret.asc"
  3. Move $out (gnupg/, primary-secret.asc) offline. It is needed again only to renew the
     subkey before it expires (tools/repo-key.sh --renew-subkey $out) or to revoke it.

The public key:
EOF
cat "$out/sp9-archive-keyring.asc"
