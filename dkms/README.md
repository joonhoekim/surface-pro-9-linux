# dkms/

The kernel patches of this repository as DKMS modules on top of linux-surface's packaged kernels, for
Debian/Ubuntu and Arch. Two packages:

| Package | dkms module | What it replaces |
|---|---|---|
| `sp9-dkms` | `sp9` | eleven modules of the linux-surface kernel (table below) |
| `sp9-dkms-bluetooth` | `sp9-bluetooth` | `bluetooth.ko`, built only from the exact source of the running kernel release |

The modules go to `/lib/modules/<release>/updates/dkms/`, which depmod searches before `kernel/`, so they
replace the stock modules of the same name without touching the kernel package. Nothing here replaces
the kernel: without the packages (or for a kernel release they do not cover) the stock linux-surface
modules are used.

**Status: loaded on the device under Ubuntu 26.04, Debian 13, Arch and Ubuntu 24.04 with Secure Boot off, for
linux-surface's 7.3.4 releases; otherwise container-tested only.** Built and installed in containers against
linux-surface's real kernel and header packages. On a Surface Pro 9 with Ubuntu 26.04 and linux-surface's
7.3.4-2-surface all 12 modules loaded from `updates/dkms` and the hardware they drive worked when checked ("What was
verified, and what was not" below). A linux-surface kernel release that `targets.tsv` does not list gets none of
them; what that loses and how to hold the kernel: [docs/install.md](../docs/install.md#kernel-updates).
Reports: [docs/install.md](../docs/install.md#report-what-you-see).

The NixOS path does not use this: it builds the whole kernel with all patches ([kernel/](../kernel/README.md)).

## What is shipped

| Module | Patches | Files copied from the target kernel | Also changed by linux-surface (could be hidden by an older copy) |
|---|---|---|---|
| `ipu-bridge` | 0001–0005 (in mainline v7.4), 0011 | `ipu-bridge.c`; `include/media/ipu-bridge.h` while the patches change it | `0013-cameras` (Surface Pro 9 camera support, front and rear rotation quirks) |
| `ov5693` | 0006 (in v7.4), 0020, 0025 | `ov5693.c` | `0013-cameras` (camera support, the gain range 0025 narrows) |
| `ov13858` | 0022, 0026 | `ov13858.c` | `0013-cameras` |
| `dw9719` | 0021 | `dw9719.c` | `0013-cameras` (probe fix for the Surface Go 2) |
| `intel_skl_int3472_discrete` | 0015 | `discrete.c`, `discrete_quirks.c`, `clk_and_regulator.c`, `led.c` | `0013-cameras` (`discrete.c`) |
| `ucsi_surface_hid` (new) | 0008, 0019, [`patches/ucsi-surface-hid-generic.patch`](patches/README.md) | private `ucsi.h` | none |
| `surface_charger` | 0009, 0010 | `surface_charger.c` | none |
| `surface_aggregator_registry` | 0012 | `surface_aggregator_registry.c` | `0007-surface-sam` (Surface Laptop 7, Surface Pro 12 entries) |
| `surface_platform_profile` | 0013 | `surface_platform_profile.c` | `0007-surface-sam` (fan power state; the `pm_prepare` return value fix that linux-surface added after the pin in [kernel/README.md](../kernel/README.md)) |
| `hid-surface` | 0016, 0017 | `hid-surface.c` (from linux-surface), private `hid-ids.h` | `0015-hid-surface` (the driver itself) |
| `ithc` | 0018 | the whole `drivers/hid/ithc/` with its `Kbuild` | `0006-ithc` (the driver itself) |
| `bluetooth` (`sp9-dkms-bluetooth`) | 0023, 0024 | `net/bluetooth/*.c`, `*.h` and its `Makefile` without rfcomm, bnep, hidp, 6lowpan | none |

The per-module definitions are in [`modules/`](modules/). Not shipped, and why:

- **0007** (HID core group for the UCSI collection): not needed. `ucsi_surface_hid` matches the device
  (`0019:045E:005D`) in the generic HID group instead. `hid-generic` does not bind a device that another
  driver's table matches (`hid_generic_match`), and when `ucsi_surface_hid` is loaded after hid-generic has
  bound the device, the HID core reprobes it (`__hid_bus_driver_added`), so the device is handed over; read
  in the 7.3.4 source, not yet seen on a device without 0007. The sensor hub on the same ID is in
  `HID_GROUP_SENSOR_HUB` and is not matched. On Arch the HID core is built in (`CONFIG_HID=y`), so 0007
  could not be shipped there at all.
- **0014** (i915 PSR2 quirk): not as DKMS, it would mean rebuilding i915. On other distributions the
  platform component `psr` forces PSR1 through debugfs without tainting the kernel
  ([userspace/platform](../userspace/platform/README.md)).
- **0023, 0024** (Bluetooth): in the separate package `sp9-dkms-bluetooth`, see below.
- **0027–0031** (IR camera, IPU6 capture path): not yet. It would take a new `vd55g0` module, 0027 in
  `ipu-bridge`, which must not ship without the driver (the IPU6 would wait for the IR sensor), and 0030 in
  `intel-ipu6-isys`. The IR camera needs this repository's kernel. Without 0030 the packed and 8-bit formats
  still arrive truncated on linux-surface's kernel; libcamera uses the unpacked formats, so it is not affected.

## How the sources are made

[`gen.sh`](gen.sh) generates the tree that a package puts in `/usr/src/<name>-<version>`:

1. **Targets.** [`targets.tsv`](targets.tsv) lists linux-surface release tags (`debian-7.3.4-2`,
   `arch-7.3.4-2`) with every input pinned: the kernel release string, the kernel.org tarball's sha256,
   the linux-surface commit of the tag, and for Arch the archlinux/linux patch's sha256. For each target
   the generator rebuilds the source the way that release's package was built (Debian: stable tarball,
   `pkg/debian/kernel/*.patch`, `patches/<series>/*.patch` with `git apply`; Arch: tarball, the archlinux
   patch and the PKGBUILD's patches with `patch -Np1`).
2. **Patches.** Per module the files are copied from that source and this repository's patches are
   applied with `git apply` restricted to them (no fuzz). A patch is skipped when the target already has
   it: declared upstream for the target's series (`UPSTREAM=0001=7.4`, confirmed by reverse-applying) or
   found by reverse-applying, or when the target has its own version of it (`SUPERSEDED=0010=adapter_rating_mw`:
   linux-surface releases from 7.3.5 on read the adapter rating themselves, as `dptf/adapter_rating_mw`).
   A module with nothing left to apply is not shipped. A patch that neither applies nor reverse-applies is
   tried as its DKMS-only rebase (`ALT_PATCHES=0012=0012-registry-sp9-dptf.patch`, in
   [`patches/`](patches/README.md)) and otherwise stops the generator.
3. **Review gate.** [`lock.tsv`](lock.tsv) holds the sha256 of every file the main package replaces, as
   reviewed. When a target ships a different version of one (a stable or linux-surface change), the
   generator stops and prints the diff against the reviewed version; after reading it,
   `dkms/gen.sh lock TAG` records it. It also stops when the set of linux-surface patches touching a
   module's files differs from the module's `LS_PATCHES` (alternatives separated by `|` cover releases
   before and after a linux-surface change). `sp9-dkms-bluetooth` has no review gate: its
   files change in most stable releases and are always that release's own.
4. **Variants.** Each target gets its own source variant (`variants/<hash>/`; identical results share
   one). `dkms.conf` sets `BUILD_EXCLUSIVE_KERNEL` to exactly the kernel releases in `kernels`, all of one
   series, and its `PRE_BUILD` step [`select-variant`](select-variant) copies the variant for the kernel
   being built. A module built for one release is therefore never the copy of another release, and a
   stable fix in a file this package replaces is never hidden. For any other kernel release dkms skips the
   build (exit 77, "BUILD_EXCLUSIVE directive which does not match"; `dkms autoinstall` exits 0, so a
   kernel update does not fail) and the stock modules are used until the package is updated.
5. **DKMS-only adjustments** ([`patches/`](patches/README.md)): `ipu-bridge-genksyms.patch` keeps the
   MODVERSIONS CRCs of the stock `ipu-bridge.h` (Debian's kernel has `CONFIG_MODVERSIONS=y`; the importers
   intel-ipu6, intel-ipu7, ipu3-cio2, ivsc-csi and intel_cvs pass ipu-bridge's own `ipu_bridge_parse_ssdb`,
   atomisp's own callback writes only fields whose offsets do not move); `ucsi-surface-hid-generic.patch` as
   above. For modules that export symbols (`ipu-bridge`, `bluetooth`) the generator also adds a
   `genksyms-compat/linux/compiler-gcc.h` that makes genksyms decide `CC_HAS_TYPEOF_UNQUAL` by the kernel's
   compiler (`CONFIG_GCC_VERSION`): from gcc 14 on, per-CPU pointer types carry `__seg_gs`, which changed the
   CRCs of 2 of 3 `ipu-bridge` and 83 of 93 `bluetooth` exports when Debian 13 (gcc 14) or Ubuntu 26.04
   (gcc 15) built against linux-surface's gcc 11 kernel. The compiled code is unaffected.

`SOURCES` in the tree records, per variant and module, the base sha256 of each file, the patches applied
and skipped, and the linux-surface patches on the same files.

```sh
dkms/gen.sh targets debian                                     # tags of one kind
dkms/gen.sh tree --out out/sp9 $(dkms/gen.sh targets debian)   # main package tree
dkms/gen.sh tree --out out/sp9-bt --package bluetooth $(dkms/gen.sh targets debian)
dkms/check-build.sh out/sp9 /usr/src/linux-headers-7.3.4-2-surface   # out-of-tree build and checks
```

The cache (`~/.cache/sp9-dkms`, about 1.8 GB per prepared target) holds the tarballs and the prepared
sources. The package version is the project version as every package of this repository gets it
([docs/releasing.md](../docs/releasing.md)): `0.2.0` at the tag `v0.2.0`, `0.2.0+git5.gabc1234` after it,
`0.2.0~git49.gabc1234` before it (`0.2.0.r5.gabc1234`, `0.2.0pre.r49.gabc1234` as an Arch pkgver).

## Bluetooth: `sp9-dkms-bluetooth`

0023 (LE legacy pairing with out-of-band data: the Slim Pen 2's coupling in the Type Cover's pen slot)
and 0024 (re-bonded devices reconnect) are in `net/bluetooth`, so the whole `bluetooth.ko` is rebuilt.
That module is large, security-relevant and changed in most stable releases, so it is built only from
the `net/bluetooth` of exactly the running kernel release, which the package carries for every target.
For any other release it is not built and the stock `bluetooth.ko` stays (`sp9-dkms-status` says so), so
after a kernel update the pen's cover coupling is missing until the package is updated. While it is
installed, every Bluetooth device uses the rebuilt module. With MODVERSIONS the CRCs of all 93 exported
symbols were compared with the kernel's `Module.symvers` and match, so btusb, btintel, rfcomm, bnep and
hidp load against it. Skip the package if you do not need the pen's cover coupling.

## Installing

From this repository's package repository (see [docs/install.md](../docs/install.md)), on a
system that runs linux-surface's kernel with its header package:

```sh
sudo apt install sp9-dkms                 # Debian, Ubuntu; add sp9-dkms-bluetooth for the pen coupling
sudo pacman -S sp9-dkms                   # Arch; the same for sp9-dkms-bluetooth
sp9-dkms-status                           # per installed kernel: covered or not, built or not, which file loads
sudo reboot
```

On Ubuntu dkms recommends a distribution kernel's headers (`linux-headers-generic | … | linux-headers`),
which `linux-headers-surface` does not provide, so with recommends on apt installs headers nothing here uses (on
Ubuntu 26.04 `linux-headers-7.0.0-1002-gke`). Install dkms first with `--no-install-recommends`, or remove those
headers afterwards ([docs/install-debian-ubuntu.md](../docs/install-debian-ubuntu.md), section 1). Not on Debian 13: its dkms
recommends no headers but the compiler, so `--no-install-recommends` leaves no `gcc` and the build fails; install
dkms with its recommends there.

The packages run dkms for every installed kernel with headers (Debian: the dkms maintainer scripts;
Arch: dkms's pacman hook), and again when a kernel or header package is installed later. The modules are
used from the next boot (or after unloading the stock ones). `surface_aggregator_registry` is one of the
modules linux-surface's wiki suggests putting in the initramfs; if you did, regenerate it after installing
(`sudo update-initramfs -u`, `sudo dracut --regenerate-all -f` where dracut builds it, as on Ubuntu 26.04,
`sudo mkinitcpio -P`), otherwise the stock copy in the initramfs is loaded
first and the RTC node of 0012 is missing.

To remove: `sudo apt remove sp9-dkms sp9-dkms-bluetooth` or `sudo pacman -R sp9-dkms sp9-dkms-bluetooth`;
dkms removes the modules and the stock ones are used again.

## Secure Boot

### Debian and Ubuntu

linux-surface's Debian kernel has `CONFIG_IMA_ARCH_POLICY=y`, which makes module signatures mandatory
when Secure Boot is on, so unsigned DKMS modules do not load there. dkms signs what it builds:

- **Ubuntu** (dkms 3.0.11 on 24.04 and 3.2.2 on 26.04, seen in containers): with `shim-signed` installed, dkms uses
  `/var/lib/shim-signed/mok/MOK.priv` and `MOK.der`, creating them on first use, and signs every module
  (`Sign command: /usr/bin/kmodsign`). Without `update-secureboot-policy` it prints "modules won't be
  signed".
- **Debian** (dkms 3.2.2 on Debian 13, seen in a container): dkms creates `/var/lib/dkms/mok.key` and `mok.pub` and
  signs with them.

The certificate has to be enrolled once, through shim's MokManager:

```sh
sudo mokutil --import /var/lib/shim-signed/mok/MOK.der     # Ubuntu; Debian: /var/lib/dkms/mok.pub
# choose a one-time password; it is needed once, at the next boot
sudo reboot
# blue MokManager screen: Enroll MOK -> Continue -> Yes -> the password -> Reboot
mokutil --test-key /var/lib/shim-signed/mok/MOK.der        # "is already enrolled"
```

On Ubuntu, installing a DKMS package while Secure Boot is on may also ask for that password through
debconf and import the key for you. What happens otherwise, read in shim's `MokManager.c`:

- MokManager reads the request and **deletes it from the firmware variables at once**, so the request is
  good for one boot only. Letting the 10-second countdown pass, or choosing "Continue boot", drops it.
- A wrong password can be retyped: after **three** wrong entries MokManager shows "Failed to enroll keys"
  and nothing is enrolled. In both cases run `mokutil --import` again and reboot.
- Until the key is enrolled, the DKMS modules fail to load under Secure Boot ("Key was rejected by
  service") and the stock module of the same name is **not** loaded instead: the device that module drives
  (cameras, Type Cover keys, touch) stops working until the key is enrolled or the package is removed.
  Enrol before rebooting into a kernel with the modules installed.

The linux-surface kernel itself is signed with linux-surface's own key, which its wiki has you enrol
already; the DKMS key is a second, local MOK. Keep its private key on the device: anyone who can read it
can sign modules your kernel accepts.

### Arch

linux-surface's Arch kernel does not set `CONFIG_IMA_ARCH_POLICY` or `CONFIG_MODULE_SIG_FORCE`, so it loads
unsigned modules (they taint the kernel with `E`). Signing is optional. To sign with your own key, enrolled
as a MOK or in db (with shim or sbctl), point dkms at it:

```sh
# /etc/dkms/framework.conf.d/sp9.conf
mok_signing_key=/etc/dkms/keys/MOK.key
mok_certificate=/etc/dkms/keys/MOK.der      # DER certificate of the same key
```

and rebuild (`sudo dkms autoinstall -k <release>`, or reinstall the package). Without that file, dkms on
Arch (3.4.5, seen in a container) creates its own `/var/lib/dkms/mok.key` and signs with it, which does no harm and matters only
once that certificate is enrolled.

## CI

[`.github/workflows/dkms.yml`](../.github/workflows/dkms.yml) runs [`ci/run.sh`](ci/run.sh) for `debian`
and `arch`, on changes to `dkms/` or `kernel/patches/`, daily, and when a release workflow calls it:

- [`ci/new-targets.sh`](ci/new-targets.sh) reads linux-surface's GitHub releases (read-only) and prints a
  `targets.tsv` line, with every pin looked up, for each release newer than the listed ones;
- the trees are generated for the listed targets and, separately, with the new ones;
- the packages are built from the listed targets (Debian in `ubuntu:24.04`, Arch in `archlinux`);
- for every target, linux-surface's real header and kernel packages of that release are downloaded and
  [`ci/container-test.sh`](ci/container-test.sh) runs in fresh `ubuntu:24.04`, `ubuntu:26.04`, `debian:13`
  and `archlinux` containers: out-of-tree build ([`check-build.sh`](check-build.sh): no modpost errors,
  vermagic equal to the stock modules', exported CRCs equal to the kernel's `Module.symvers`), install of
  the packages, `modinfo` and `modprobe --dry-run` resolve to `updates/dkms`, the dkms signature, and that
  an unknown release of the same series is skipped.

It fails when a patch no longer applies, a replaced file changed since review, or a module does not build
or install. It never edits `targets.tsv`: the job summary lists the new lines; review the diff the
generator printed, add them, run `dkms/gen.sh lock TAG` if asked, and commit. The packages are the
artifacts `pkg-deb-all-sp9-dkms` and `pkg-arch-sp9-dkms`, the names the Packages workflow publishes from:
there the workflow is a job (`dkms: uses: ./.github/workflows/dkms.yml`) listed in the publish job's
`needs`, and the packages go to the same apt and pacman repositories as the others.

A new kernel series (7.4) needs nothing more as long as the patches apply: 0001–0006 are skipped there
(upstream), `ipu-bridge` then ships with 0011 only and without a bundled header.

## What was verified, and what was not

Verified on the development machine (NixOS, nothing loaded or installed on it), with linux-surface's
`debian-7.3.4-2` and `arch-7.3.4-2` packages, in rootless containers: see the table in
[kernel/PROVENANCE.md](../kernel/PROVENANCE.md#dkms). The later releases in `targets.tsv` (7.3.5 and 7.3.6) are
generated and container-tested in CI only. They read the adapter rating themselves (`dptf/adapter_rating_mw`, on devices with the `has_dptf` property) and give the Surface Pro 9 its own registry group: there
the package does not ship patch 0010, and the helper reads that file instead of `input_power_limit`.

Verified on a Surface Pro 9 with Ubuntu 26.04, linux-surface's `7.3.4-2-surface` and Secure Boot off (one run,
[docs/test-results.md](../docs/test-results.md#ubuntu-2604-packaged-route)): `sp9-dkms` and `sp9-dkms-bluetooth`
build and install for the running kernel, all 12 modules load from `updates/dkms` (`sp9-dkms-status`, the file of
each loaded module), `ucsi_surface_hid` binds (two `typec` ports), the DKMS `surface_aggregator_registry` is in the dracut initramfs,
and the cameras, Type Cover, touch, pen and the pen's cover coupling work (observed). The kernel is tainted O and E
(12288): out-of-tree modules, unsigned by an enrolled key, as expected with no MOK enrolled.

Ubuntu 24.04, Debian 13 and Arch (linux-surface's `7.3.4.arch1-2`), one run each on the device with Secure Boot off:
all 12 modules load from `updates/dkms`, tainted 12288
([Ubuntu 24.04](../docs/test-results.md#ubuntu-2404-packaged-route),
[Debian 13](../docs/test-results.md#debian-13-packaged-route),
[Arch](../docs/test-results.md#arch-linux-packaged-route)). On Arch the mkinitcpio image holds none of them.

Not verified: Ubuntu 26.10 and Debian testing on the device, MOK enrolment and signed loading under
Secure Boot, and the upgrade path from one kernel release to the next.
