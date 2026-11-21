# packaging

Native packages of this repository for Debian, Ubuntu and Arch Linux, and the signed apt and pacman repositories they
are published in. Users install them as described in [docs/install.md](../docs/install.md); this file is
for whoever builds or changes them. The release steps are in [docs/releasing.md](../docs/releasing.md).

**Status: supported on Ubuntu 26.04 and Debian 13 (with NixOS, which does not use these packages).** Every package builds, installs
and is removed cleanly in the distributions' official containers, and the repositories are signed and read back there.
On a Surface Pro 9 the Ubuntu 26.04 packages, built locally with these scripts and installed from an unsigned local
source, were run once with Secure Boot off, and what they drive worked when checked
([docs/test-results.md](../docs/test-results.md#ubuntu-2604-packaged-route)). Ubuntu 24.04, checked the same way,
works without cameras and is not a supported target. Debian 13 (supported) and Arch (works, best effort) were each
checked once on the device the same way ([Debian 13](../docs/test-results.md#debian-13-packaged-route),
[Arch](../docs/test-results.md#arch-linux-packaged-route)). Debian testing and Ubuntu 26.10 are best effort: built
and install-tested in containers only. The signed repositories are untested on the device. Reports:
[docs/install.md](../docs/install.md#report-what-you-see).

The packages sit on top of linux-surface: its kernel, its iptsd and its Secure Boot key come from its own repository.
This project adds the platform files, the helper, the touch and pen daemon, the kernel modules (`sp9-dkms`, packaged
separately) and, experimental, a patched libcamera. Nothing in them downloads or installs anything at run time: apt
and pacman do all updating.

## Packages

| Package | Contents | Debian, Ubuntu | Arch |
|---|---|---|---|
| `sp9` | Metapackage | Depends on `sp9-platform`, `sp9-helper`, `sp9-digitizerd` (for iptsd, install the parts without `sp9`); recommends `sp9-settings`, `sp9-dkms`, `sp9-cmdline` | Depends on `sp9-platform`, `sp9-helper`, `sp9-digitizerd` (for iptsd, install the parts without `sp9`); the rest as optional dependencies |
| `sp9-helper` | `sp9-helperd`, `sp9-ctl`, unit, D-Bus policy and activation file, polkit actions, `surface_aggregator_cdev` at boot, the diagnostics and ICC scripts | Unit enabled and started (it starts only on this model) | Enable it yourself |
| `sp9-settings` | The GTK settings app, desktop entry, icon, AppStream metadata | | |
| `sp9-digitizerd` | The touch and pen daemon, its replay tool, unit, D-Bus policy, `uinput` at boot | Conflicts with `iptsd`; enabled and started | Conflicts with `iptsd`; enable it yourself |
| `sp9-platform` | `userspace/platform`: iptsd preset, libinput quirk, accelerometer udev rule, PSR1 through debugfs (`sp9-psr1`), `pinctrl_tigerlake` in the initramfs, PipeWire and WirePlumber files for speakers and microphones, the raw speaker volume user unit; the opt-in files under `/usr/share/sp9-platform/optional/` | initramfs rebuilt by the `update-initramfs` trigger | initramfs rebuilt by a pacman hook (mkinitcpio) |
| `sp9-cmdline` | The kernel command line of `userspace/platform/cmdline` (now `pci=hpiosize=0`) | `/etc/default/grub.d/sp9-platform.cfg`, `update-grub` on install and purge | `/etc/cmdline.d/sp9-platform.conf` (mkinitcpio UKIs); other boot loaders by hand |
| `sp9-archive-keyring` / `sp9-keyring` | The repository's public key | `/usr/share/keyrings/sp9-archive-keyring.gpg` | `/usr/share/pacman/keyrings/sp9*`, `pacman-key --populate sp9` |
| `sp9-libcamera` | Patched libcamera 0.8.0 in its own prefix with a PipeWire plugin built against it; experimental, opt-in ([libcamera/README.md](libcamera/README.md)) | Not for Ubuntu 24.04 | |
| `sp9-dkms`, `sp9-dkms-bluetooth` | The kernel patches as DKMS modules for the linux-surface kernel ([dkms/README.md](../dkms/README.md)) | Built by the DKMS workflow, published here (see "Hooks") | |

Not packaged: the pen prototypes of `userspace/pen` (the helper does what they did), the iptsd patches (they need a
rebuilt iptsd; `sp9-digitizerd` carries the same processing), `psr-param` of `install.sh` (`i915.enable_psr=1`, which
taints the kernel; add it by hand where debugfs is locked down), and this repository's full kernel.

Why `sp9-digitizerd` conflicts with iptsd instead of diverting its files: both daemons read the same hidraw node and
create input devices with the same names, so exactly one may run; iptsd is started per device by its own udev rule,
and diverting that rule would leave an installed iptsd that silently never runs. A conflict lets apt and pacman say
what happens. `helper/install.sh` makes the same choice by default (it masks iptsd). The cost: iptsd's tools
(`iptsd-check-device`) go too, and the diagnostics skip that section.

## Layout

| Path | What |
|---|---|
| [`version.sh`](version.sh) | The version: `[workspace.package] version` of `helper/Cargo.toml`, with a git suffix off a release tag |
| [`source-tarball.sh`](source-tarball.sh) | The release source: `git archive`, `VERSION`, `DESCRIBE` and the helper's crates vendored (`helper/vendor`) so that every build is offline |
| [`stage.sh`](stage.sh) | The file list of each package, used by both `debian/rules` and the PKGBUILD |
| [`targets.tsv`](targets.tsv) | The Debian and Ubuntu releases built, their apt suites and version suffixes; the CI matrix is made from it |
| [`debian/`](debian/) | The debhelper source package `sp9-linux` (copied to `debian/` of the unpacked tarball) |
| [`arch/`](arch/) | The split PKGBUILD `sp9-linux` with its install scripts and pacman hook |
| [`systemd/`](systemd/) | Unit drop-ins and units that only the packages need |
| [`keyring/`](keyring/) | `sp9-archive-keyring.asc`, the public repository key, once it exists ([tools/repo-key.sh](../tools/repo-key.sh)) |
| [`build-deb.sh`](build-deb.sh), [`build-arch.sh`](build-arch.sh) | Build inside the official container, with lintian or namcap |
| [`test-deb.sh`](test-deb.sh), [`test-arch.sh`](test-arch.sh), [`test-common.sh`](test-common.sh) | Install test in a container that boots systemd |
| [`publish-apt.sh`](publish-apt.sh), [`publish-pacman.sh`](publish-pacman.sh) | The signed repositories (reprepro; `repo-add --sign`) |
| [`docker.sh`](docker.sh) | Runs the builds and tests locally the way CI does |
| [`libcamera/`](libcamera/README.md) | The experimental `sp9-libcamera` |

## Versions

One version per release, `X.Y.Z` from `helper/Cargo.toml`, tagged `vX.Y.Z` ([docs/releasing.md](../docs/releasing.md)).
The packages take it from [`version.sh`](version.sh):

| Build | Debian, Ubuntu | Arch |
|---|---|---|
| At the tag `v0.2.0` | `0.2.0-1~deb13` | `0.2.0-1` |
| 5 commits after it | `0.2.0+git5.gabc1234-1~deb13` | `0.2.0.r5.gabc1234-1` |
| `0.2.0` not tagged yet | `0.2.0~git57.gabc1234-1~deb13` (sorts before `0.2.0`) | `0.2.0pre.r57.gabc1234-1` |

The same version is built once per suite (`~deb13`, `~deb14`, `~ubuntu24.04`, `~ubuntu26.04`, `~ubuntu26.10`):
the binaries link against each release's libraries, and the shared pool needs distinct file names. `--version` of
the binaries shows `git describe` on development builds (`SP9_GIT_DESCRIBE` from the tarball's `DESCRIBE`).

## Build dependencies per release

| Release | Rust | GTK 4 / libadwaita | Notes |
|---|---|---|---|
| Ubuntu 24.04 | `cargo-1.97` (the default 1.75 is too old; `debian/rules` puts `/usr/lib/rust-1.*/bin` first) | 4.14 / 1.5 | WirePlumber 0.4: its `.conf` drop-ins (speaker raw sink name, microphone gain) have no effect, so the speaker filter does not attach to the amplifier sink; no `sp9-libcamera` |
| Ubuntu 26.04, 26.10 | 1.93, 1.97 | 4.22 / 1.9, 4.24 / 1.10 | |
| Debian 13 | `cargo-web` 1.96 (the default 1.85 is too old) | 4.18 / 1.7 | `sp9-libcamera` needs trixie-backports (PipeWire 1.6) |
| Debian testing | 1.98 | 4.24 / 1.10 | |
| Arch | current | current | |

`Build-Depends` names the alternatives (`cargo (>= 1.89) | cargo-web (>= 1.89) | cargo-1.97`), which
`apt-get build-dep` resolves per release. The helper needs Rust 1.89 (`rust-version` in `helper/Cargo.toml`).

## Building locally

Needs docker (or podman, `DOCKER=podman`) and, for the tarball, git and cargo:

```sh
packaging/source-tarball.sh out/src                         # needs the network (cargo vendor)
packaging/docker.sh deb debian-13 out/src/sp9-linux-*.tar.xz out/debian-13
packaging/docker.sh deb-test debian-13 out/debian-13
packaging/docker.sh arch out/src/sp9-linux-*.tar.xz out/arch
packaging/docker.sh arch-test out/arch
```

The Debian build installs its dependencies in one container and builds in a second one with `--network none`. The
tarball is made from committed files only. Without `packaging/keyring/sp9-archive-keyring.asc` the keyring packages
are skipped (build profile `nokeyring`; the PKGBUILD loses `sp9-keyring`).

## CI and publishing

[`.github/workflows/packages.yml`](../.github/workflows/packages.yml) runs on pushes to `main` and pull requests that
touch the packaged sources, and on tags `v*`:

1. the source tarball, and the matrix from `targets.tsv`;
2. one build per release in its official container (`ubuntu:24.04`, …, `archlinux:latest`), lintian (errors fail)
   and namcap;
3. the install tests: dependencies installed and every file listed, then the packages installed, units verified
   (`systemd-analyze verify`, `udevadm verify`), D-Bus, polkit, desktop and AppStream files checked,
   `sp9-helperd` skipped by its model condition, `sp9-digitizerd` running and on the bus, `sp9-helperd --fake` on the
   system bus read by an unprivileged `sp9-ctl` through the installed policy and a change refused by polkit, then
   purged and the file list compared;
4. on a tag only: the repositories signed with the subkey in the secret `SP9_REPO_SIGNING_KEY`, after checking it
   belongs to the committed public key; a GitHub release with every package, the source tarball, the filled-in
   PKGBUILD, `SHA256SUMS` and its signature, and `pages-packages.tar.gz`. The Website workflow
   ([`pages.yml`](../.github/workflows/pages.yml)) unpacks that under `/packages/` of the site whenever it deploys,
   and runs once more when a release run of this workflow succeeds.

Repository layout on the site:

```
packages/sp9-archive-keyring.asc, sp9-archive-keyring.gpg     the public key
packages/apt/dists/<codename>/InRelease ...                    deb https://joonhoekim.github.io/surface-pro-9-linux/packages/apt <codename> main
packages/apt/pool/main/...
packages/arch/x86_64/sp9.db, sp9.db.sig, *.pkg.tar.zst(.sig)   Server = https://joonhoekim.github.io/surface-pro-9-linux/packages/arch/$arch
```

The repositories hold the latest release only; older packages stay attached to their releases. GitHub Pages allows
1 GB per site, which these packages fit many times over; kernels would not, which is one reason the kernel stays
linux-surface's.

## Hooks

For packages built elsewhere: a job uploads an artifact named `pkg-deb-<codename>-<name>` (packages for one
suite), `pkg-deb-all-<name>` (architecture-independent packages for every suite) or `pkg-arch-<name>`, and is listed
in `needs:` of the publish job, which takes every `pkg-*` artifact into the repositories and the release. The `dkms`
job does this for `sp9-dkms` and `sp9-dkms-bluetooth`: on a release tag it calls the DKMS workflow
([`dkms.yml`](../.github/workflows/dkms.yml)), whose artifacts are `pkg-deb-all-sp9-dkms` and `pkg-arch-sp9-dkms`,
versioned by the same `version.sh`. The metapackage recommends `sp9-dkms` (Arch: optional dependency), and
`sp9-ctl packages` reports every DKMS source whose name starts with `sp9`.

## Arch: own repository and AUR

The pacman repository is the supported route: its packages are built in CI from the tagged source and signed with
the same key as the apt repository, so one trust decision covers both. The PKGBUILD is also usable as an AUR
package: the release attaches it with `pkgver` and the source checksum filled in (`sp9-linux.PKGBUILD`,
`sp9-linux.SRCINFO`), and it builds from the release tarball offline. Uploading it to the AUR is left to the
maintainer; an AUR package would then have to be updated with each release.
