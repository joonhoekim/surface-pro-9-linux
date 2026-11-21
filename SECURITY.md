# Security policy

## Reporting a vulnerability

Report security problems privately through GitHub's
[private vulnerability reporting](https://github.com/joonhoekim/surface-pro-9-linux/security/advisories/new) (Security, Report a vulnerability), not in a public
issue. Say what is affected (file and commit), how to reproduce it, and what an attacker gains. This is a one-person
project: expect an answer within about two weeks. Fixes land on `main`; there are no release branches.

## What runs with privileges

This is the attack surface the project adds to a system. Reports about any of it are in scope.

- **`sp9-helperd`** (`helper/`), the device helper. It runs as root and owns the system bus name
  `io.github.joonhoekim.OpenDeviceHelper1`. The bus policy lets any local user call it; every method that changes
  something asks polkit for one of the 13 actions `io.github.joonhoekim.opendevicehelper.sp9.*`, with the caller's
  bus name as the subject. Changing the power limits per profile, the skin temperature guard, smart charging, the
  panel ICC profile, the keyboard backlight, touch and pen settings and running diagnostics are allowed without a
  password to the user of the active local session; reloading the configuration, the fan request, the panel self
  refresh policy and pen coupling need an administrator (`auth_admin_keep`); inactive and remote sessions get
  nothing. A way to reach a changing method without the authorization its action asks for is a vulnerability.
  - It writes to `/sys` (RAPL power limits, EPP, `platform_profile`, input inhibit, debugfs for PSR),
    `/etc/sp9`, `/var/lib/sp9`, `/etc/iptsd.d` and `/run/lock`. Its unit has `ProtectSystem=strict` with those
    paths writable, `ProtectHome` and `PrivateTmp`, and no capability or device restrictions: it opens
    `/dev/surface/aggregator`, hidraw nodes, Bluetooth management and raw HCI sockets, talks to BlueZ over D-Bus and
    reads EFI variables.
  - Diagnostics run `tools/linux/post-install-check.sh` (bash) as root, and the ICC profile runs
    `tools/sdc-blob.py` (python3) as root, both from the helper's data directory; the diagnostics archive is
    handed to the caller. The check masks MAC addresses, UUIDs and serial lines by pattern, which is a design
    limit: an identifier in an unexpected format can pass. A leak of something the check is meant to mask is in
    scope.
  - `--no-polkit`, `--session` and `--fake` are for tests on a private bus. A packaged unit that starts the
    daemon with `--no-polkit` is a bug.
- **`sp9-digitizerd`** (`helper/crates/sp9-digitizerd/`), the touch and pen daemon. It runs as root, reads the
  digitizer's hidraw node and creates the virtual touch and pen devices through `/dev/uinput`. Its system bus name
  `io.github.joonhoekim.Sp9Digitizer1` lets anyone read properties, introspect and call `Diagnostics.GetConfig`;
  only root may call the setters (the bus policy says so and each setter checks the caller's uid again), so users
  change its settings through `sp9-helperd` and polkit. Its unit is hardened (no capabilities,
  `DevicePolicy=closed` with only `/dev/uinput` and hidraw, no network, `SystemCallFilter=@system-service`).
  Input injected into the session, or a crash on crafted digitizer reports, is in scope.
- **D-Bus and polkit policies** (`helper/data/*.conf`, `helper/data/*.policy`), as installed by
  `helper/install.sh` or the NixOS module.
- **systemd units**: `sp9-helperd.service`, `sp9-digitizerd.service`, and the pen tool units in
  `userspace/pen/systemd/`, which run as root with `ProtectSystem=strict`.
- **udev rules**: `userspace/platform/udev/rules.d/` has one rule, which writes a sensor attribute of the
  accelerometer and grants no device access. A rule that gives users access to a device is in scope.
- **Install scripts**: `userspace/platform/install.sh` and `helper/install.sh` run as root and write to `/etc` and
  `/usr/local`, including the kernel command line and the initramfs configuration. Unsafe handling of paths or temporary files there is in scope.
- **The NixOS module** (`nix/`), for what it installs and enables.
- **The packages** (`packaging/`): their maintainer scripts (`sp9-cmdline` runs `update-grub`, `sp9-helper` removes its
  run-time files on purge, `sp9-keyring` runs `pacman-key --populate`), the pacman hook of `sp9-platform` (runs
  `mkinitcpio -P`), and the units, rules and policies they install and enable. `sp9-libcamera` puts its own PipeWire
  plugin and libcamera into every user's WirePlumber.
- **The package repositories and the release workflow** (`.github/workflows/packages.yml`, `pages.yml`): a way to get
  a package signed or published that was not built from a tagged commit of this repository is in scope.

The kernel patches change drivers that talk to the device's firmware (the Surface Aggregator Module, the touch
controller, the cameras' power control, Bluetooth pairing). A memory-safety or privilege problem in one of this
repository's patches is in scope; for the linux-surface series and upstream code, see below.

## Package signing

The apt and pacman repositories on GitHub Pages and the `SHA256SUMS` of each release are signed with one OpenPGP key:

    Fingerprint: CB98 481D 6B44 CF00 41E3 0327 6E63 5872 1157 CCCC

The public key is [packaging/keyring/sp9-archive-keyring.asc](packaging/keyring/README.md) and, once installed, the
packages `sp9-archive-keyring` (apt) and `sp9-keyring` (pacman). What a user trusts, and why:

- **Key.** An Ed25519 primary key that only certifies, kept offline by the maintainer, and an Ed25519 signing subkey
  that expires after two years. Only the subkey is in CI, as the repository secret `SP9_REPO_SIGNING_KEY`, readable
  by the publish job of the Packages workflow, which runs only for a pushed tag `v*`. A leaked subkey is revoked with
  the primary key and replaced through an update of the keyring packages ([docs/releasing.md](docs/releasing.md)).
- **What is signed.** apt: `InRelease` and `Release.gpg` of every suite; the package files are covered by the hashes
  in them. pacman: every package (`.sig`) and the database `sp9.db`. Users pin the key: apt through `Signed-By` on this
  repository only (the key is not added to apt's global keyring, so it cannot vouch for other repositories), pacman
  through a local signature (`pacman-key --lsign-key`) of this key.
- **What it proves.** That the files were produced by this repository's release workflow from a tag, built in the
  distributions' official containers. It does not prove more than GitHub's account and Actions security: whoever can
  push a tag to this repository can get packages signed. The first key download is trust on first use; compare the
  fingerprint above, which is in the git history, with what `gpg --show-keys` prints.
- **What the packages do at run time.** Nothing of this project downloads or installs anything: updates come only
  through apt or pacman and their signature checks. `sp9-helperd` reports installed versions by reading the package
  database; it opens no network connection.
- **linux-surface.** Its kernel, iptsd and Secure Boot key come from linux-surface's repository and key, which this
  project neither controls nor re-signs.

## Out of scope

The upstream Linux kernel, the linux-surface patch series and packages, iptsd and libcamera apart from this
repository's patches, the device's firmware (UEFI, SAM, touch, PD controller), and distributions' own packages.
Report those to their maintainers. Someone with physical access to an unlocked device is outside what this project
can protect against.

## DKMS modules (`sp9-dkms`, `sp9-dkms-bluetooth`)

On Debian, Ubuntu and Arch the kernel patches can come as DKMS packages ([dkms/](dkms/README.md)). What that
means for trust:

- **They run in the kernel.** The packages put source into `/usr/src`, and dkms compiles it as root on your machine
  for every installed linux-surface kernel and installs the result to `updates/dkms`, where it replaces the stock
  module of the same name. Trusting the packages means trusting this repository's patches as kernel code, and the
  generator that applied them; the package repository's signature covers the source, not the built modules.
- **What they contain.** Copies of the target kernel's own files (each linux-surface release rebuilt from pinned
  inputs: the kernel.org tarball's sha256, the linux-surface commit, Arch's patch sha256 in `dkms/targets.tsv`)
  with the patches of `kernel/patches/` and `dkms/patches/` applied; `SOURCES` in the package lists every file's
  base hash and the patches. A module is built only for the kernel release it was generated from
  (`BUILD_EXCLUSIVE_KERNEL`), so it never replaces a newer stable or linux-surface fix with an older copy; for
  other releases the stock modules stay. `sp9-dkms-bluetooth` replaces the whole Bluetooth core, which every
  Bluetooth device and every local process with a Bluetooth socket talks to: install it only if you need the pen's
  cover coupling.
- **Signing key.** With Secure Boot, dkms signs the modules with a key it generates on the machine
  (`/var/lib/shim-signed/mok/MOK.priv` on Ubuntu, `/var/lib/dkms/mok.key` on Debian and Arch), which you enrol as a
  MOK. Once enrolled, the kernel accepts any module signed with that key: root-readable private key material on
  disk becomes a way to load kernel code under Secure Boot. This is how every DKMS package works; it is not specific
  to this project. Remove the key from the MOK list (`mokutil --delete`) when you stop using DKMS.
- **In scope:** a patch or DKMS-only adjustment that introduces a memory-safety or privilege problem, a generator
  bug that ships a file other than the target release's own (or a variant to the wrong release), and the package
  scripts. The dkms tool, shim and linux-surface's kernel are out of scope (see above).
