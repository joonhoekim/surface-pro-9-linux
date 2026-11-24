# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html): one version for the whole repository, tagged
`vX.Y.Z` ([docs/releasing.md](docs/releasing.md)). The kernel and libcamera keep their upstream versions; what
a release fixes is the set of patches on them.

## [Unreleased]

### Added

- **DKMS for linux-surface 7.3.5 and 7.3.6** (`debian-7.3.5-1`, `debian-7.3.6-1`, `arch-7.3.5-1`; container-tested
  only). These releases read the adapter rating themselves (`dptf/adapter_rating_mw`), so `sp9-dkms` leaves out
  patch 0010 there, and patch 0012 is applied in a form rebased onto their registry. The generator gained
  `ALT_PATCHES`, `SUPERSEDED` and alternatives in `LS_PATCHES`. The helper reads either adapter rating file.

- **IR camera** (ST VD55G0), with this repository's kernel only (NixOS module or a source build; not in
  `sp9-dkms`): kernel patches 0027 (ipu-bridge entry), 0028 (ST's driver as adapted in linux-surface/kernel#164,
  with Surface Pro 9 changes) and 0029 (IR illuminator on sensor GPIO 1, 10-bit output), and
  `CONFIG_VIDEO_VD55G0=m` in `sp9.config`. It delivers 10-bit monochrome frames through libcamera and the
  illuminator lights a dark room; face sign-in is not provided. Test CAM-8.
- Kernel patch 0030: the IPU6 captures packed and 8-bit formats intact on this device (they were truncated on
  both camera ports); 0031 offers the IR camera's 8-bit output again.
- Kernel patch 0032: `intel_pmc_core`'s `substate_requirements` debugfs file reads back on Alder Lake-L (it was
  empty). Not in `sp9-dkms`.
- Standby power research ([docs/research/control/standby.md](docs/research/control/standby.md) section 12): what
  keeps S0i3.0 from being entered (MainPLL and Fast XTAL stay on) and the 17 causes ruled out.

## [0.1.0] - 2026-11-21

The first release. The hardware status is in [README.md](README.md) and
[docs/hardware-status.md](docs/hardware-status.md); a part is listed as working only where it was measured or
observed on the device. Of the 20 parts in the README's table, 16 work, 3 work with a caveat (speakers at full
volume, heat under sustained load, standby) and the IR camera does not work.

### Added

#### Kernel and drivers

- **Kernel**: stable v7.3.4 + linux-surface's 7.3 series + 26 patches in [`kernel/patches`](kernel/README.md)
  (6 upstream backports, 4 from a pending linux-surface pull request, 16 local), with a config fragment and
  [PROVENANCE.md](kernel/PROVENANCE.md). They cover the cameras (IPU6 bridge, OV5693, OV13858, DW9719 focus
  motor, INT3472 clock), USB-C through the Surface UCSI HID transport, the charger's adapter rating, the SAM RTC
  node and fan floor, the Type Cover's Fn usages, mic-mute key and keyboard backlight, the touch controller's
  feature responses, PSR2 off on this panel, and the pen's LE legacy OOB pairing (refused when the peer does not
  answer with the OOB flag, instead of falling back to Just Works) and reconnection.
- **DKMS packages** `sp9-dkms` and `sp9-dkms-bluetooth`, which build the patched modules for linux-surface's
  packaged kernels ([dkms/README.md](dkms/README.md)).

#### User space

- **Device helper** `sp9-helperd` with `sp9-ctl` and the settings app `sp9-settings`: power limits per profile,
  fan, battery, keyboard backlight, PSR policy, touch and pen settings, pen haptics, cover coupling of the pen,
  diagnostics ([docs/helper.md](docs/helper.md)). `--version` reports the project version, with a `git describe`
  suffix on development builds.
- **sp9-digitizerd**, a touch and pen daemon for this digitizer derived from iptsd, which keeps its input devices
  across suspend ([docs/digitizerd.md](docs/digitizerd.md)).
- **iptsd patches** 0001–0017 for the Slim Pen 2's position, tilt, contact, pressure and filtering
  ([userspace/iptsd](userspace/iptsd/README.md)).
- **libcamera patches** 0001–0010 for libcamera 0.8.0: frames on this device, no purple lines, a settling
  exposure, rear autofocus ([userspace/libcamera](userspace/libcamera/README.md)).
- **Platform files**: iptsd preset, libinput quirk, sensor udev rule, initramfs module lists, kernel command
  line, PSR1 through i915 debugfs (no tainting kernel parameter), speaker filter chain and microphone settings,
  optional codec pin patch ([userspace/platform](userspace/platform/README.md)).
- **Pen tools** `pen-haptic`, `surface-pen-connect`, `surface-pen-feedback` ([userspace/pen](userspace/pen/README.md)).

#### Distributions

- **NixOS**: the module `nixosModules.default` (`hardware.surface-pro-9.*`) and the flake packages `kernel`,
  `surface-pen-tools`, `sp9-helper`, `sp9-digitizerd`, `libcamera`, `pipewire`, `wireplumber`
  ([docs/nixos.md](docs/nixos.md)).
- **Debian, Ubuntu and Arch packages** (`sp9` and its parts, `sp9-cmdline`, `sp9-libcamera`, the keyrings),
  published as signed apt and pacman repositories with each release ([packaging/README.md](packaging/README.md)).

#### Documentation

- Installation pages: [docs/install.md](docs/install.md) (what an installation consists of, the support table,
  the checks afterwards), the shared preparation ([docs/install-prepare.md](docs/install-prepare.md)), and one
  page each for [Debian and Ubuntu](docs/install-debian-ubuntu.md), [Arch Linux](docs/install-arch.md),
  [NixOS](docs/install-nixos.md) and [installing from source](docs/install-from-source.md).
- The test plan and results, research notes on how the device behaves under Windows and where Linux differs, and
  the measurement tools in [`tools/`](tools/README.md). The website shows the documents with their footnotes in a
  side panel.

### Known issues

- Supported distributions: NixOS, Ubuntu 26.04 and Debian 13; Arch works on the device, best effort. Debian testing
  and Ubuntu 26.10 are built and install-tested in containers only.
- A linux-surface kernel release that `sp9-dkms` does not list runs the stock modules; hold the kernel until it is
  listed ([docs/install.md](docs/install.md#kernel-updates)).
- The packaged route was run on the device once each on Ubuntu 26.04, Debian 13 (with `sp9-libcamera` and
  PipeWire 1.6 from trixie-backports) and Arch (mkinitcpio unified kernel image, systemd-boot), all with Secure Boot
  off ([docs/test-results.md](docs/test-results.md#ubuntu-2604-packaged-route)). Secure Boot with the DKMS key
  enrolled, installing from the signed repositories and the source route are untested on the device.
- Ubuntu 24.04 is not a supported target: it works without cameras (no `sp9-libcamera` for it; its libcamera 0.2
  finds no camera), and WirePlumber 0.4 ignores the speaker and microphone rules, so the speaker protection filter
  is not the default sink ([its table](docs/test-results.md#ubuntu-2404-packaged-route)).
- Debian and Ubuntu: dkms recommends a distribution kernel's headers, which `linux-headers-surface` does not
  provide, so with recommends apt installs headers nothing uses. Section 1 of
  [docs/install-debian-ubuntu.md](docs/install-debian-ubuntu.md) says how to avoid or remove them.
- The pen keeps one bond per host address, and all systems on the device share the Bluetooth controller: coupling
  the pen on another installed system (Windows or another Linux) breaks this system's bond, which the helper does
  not detect. Put the pen in the cover and run `sp9-ctl pen couple --force`
  ([docs/helper.md](docs/helper.md), the section on the pen).
- The helper's guard for a coupling link that comes up through the pen's private address is tested with fakes
  only ([docs/research/gaps/pen-vendor-exchange.md](docs/research/gaps/pen-vendor-exchange.md) section 2.5).
  Patch 0023's refusal of a pairing without the OOB flag is verified on the device.
- Most local kernel patches are marked unverified in [PROVENANCE.md](kernel/PROVENANCE.md); the IR camera has no
  driver; tests that need a person or a second device are open ([docs/test-results.md](docs/test-results.md)).

[Unreleased]: https://github.com/joonhoekim/surface-pro-9-linux/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/joonhoekim/surface-pro-9-linux/releases/tag/v0.1.0
