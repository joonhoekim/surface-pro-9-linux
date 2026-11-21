# packaging/keyring

`sp9-archive-keyring.asc` here is the public OpenPGP key that signs the apt and pacman repositories of this project.
The keyring packages (`sp9-archive-keyring`, `sp9-keyring`) install it, and CI checks that the signing secret belongs
to it. It is created once, offline, with [tools/repo-key.sh](../../tools/repo-key.sh); its fingerprint is in
[SECURITY.md](../../SECURITY.md).
