# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html): one version for the whole repository, tagged
`vX.Y.Z` ([docs/releasing.md](docs/releasing.md)). The kernel and libcamera keep their upstream versions; what
a release fixes is the set of patches on them.

## [Unreleased]

First release candidate, planned as 0.1.0. Everything below is new. The hardware status is in
[README.md](README.md) and [docs/hardware-status.md](docs/hardware-status.md); a part is listed as working only
where it was measured or observed on the device. Of the 20 parts in the README's table, 16 work, 3 work with a
caveat (speakers at full volume, heat under sustained load, standby) and the IR camera does not work.

### Added

- **Kernel**: stable v7.3.4 + linux-surface's 7.3 series + 26 patches in [`kernel/patches`](kernel/README.md)
  (6 upstream backports, 4 from a pending linux-surface pull request, 16 local), with a config fragment and
  [PROVENANCE.md](kernel/PROVENANCE.md). They cover the cameras (IPU6 bridge, OV5693, OV13858, DW9719 focus
  motor, INT3472 clock), USB-C through the Surface UCSI HID transport, the charger's adapter rating, the SAM RTC
  node and fan floor, the Type Cover's Fn usages, mic-mute key and keyboard backlight, the touch controller's
  feature responses, PSR2 off on this panel, and the pen's LE legacy OOB pairing (refused when the peer does
  not answer with the OOB flag, instead of falling back to Just Works) and reconnection.
- **NixOS module** `nixosModules.default` (`hardware.surface-pro-9.*`) and the flake packages `kernel`,
  `surface-pen-tools`, `sp9-helper`, `sp9-digitizerd`, `libcamera`, `pipewire`, `wireplumber`
  ([docs/nixos.md](docs/nixos.md)).
- **Platform files** for other distributions with `install.sh`: iptsd preset, libinput quirk, sensor udev rule,
  initramfs module lists, kernel command line, PSR1 through i915 debugfs (no tainting kernel parameter), speaker
  filter chain and microphone settings, optional codec pin patch ([userspace/platform](userspace/platform/README.md)).
- **iptsd patches** 0001–0017 for the Slim Pen 2's position, tilt, contact, pressure and filtering
  ([userspace/iptsd](userspace/iptsd/README.md)).
- **libcamera patches** 0001–0010 for libcamera 0.8.0: frames on this device, no purple lines, a settling
  exposure, rear autofocus ([userspace/libcamera](userspace/libcamera/README.md)).
- **sp9-digitizerd**, a touch and pen daemon for this digitizer derived from iptsd, which keeps its input
  devices across suspend ([docs/digitizerd.md](docs/digitizerd.md)).
- **Device helper** `sp9-helperd` with `sp9-ctl` and the settings app `sp9-settings`: power limits per profile,
  fan, battery, keyboard backlight, PSR policy, touch and pen settings, pen haptics, cover coupling of the pen,
  diagnostics ([docs/helper.md](docs/helper.md)). `helper/install.sh` installs it, and `sp9-digitizerd` with it,
  on other distributions.
- **Pen tools** `pen-haptic`, `surface-pen-connect`, `surface-pen-feedback` ([userspace/pen](userspace/pen/README.md)).
- **Documentation**: installation per distribution, test plan and results, research notes on
  how the device behaves under Windows and where Linux differs, and the measurement tools in
  [`tools/`](tools/README.md).
- **Installation pages per distribution**: [docs/install.md](docs/install.md) describes what any installation
  consists of, the support table and the checks afterwards, [docs/install-prepare.md](docs/install-prepare.md) the
  shared preparation (Windows, disk, UEFI); [Debian and Ubuntu](docs/install-debian-ubuntu.md),
  [Arch Linux](docs/install-arch.md), [NixOS](docs/install-nixos.md) and [from source](docs/install-from-source.md)
  follow as equal pages. The README is a short landing page. Asides, measurement details and long qualifiers are
  GFM footnotes, which the website also shows in a floating panel next to their marker. The Arch page now picks mkinitcpio, names `linux-firmware-intel`,
  `wireless-regdb`, `intel-ucode`, `sp9-dkms-bluetooth` and `sp9-cmdline`, and sets up unified kernel images for
  `sp9-cmdline`; both package pages say the signed repositories are not published yet.
- **Versioning**: one project version, `--version` on `sp9-ctl`, `sp9-helperd` and `sp9-digitizerd` with a
  `git describe` suffix on development builds.

### Known issues

- Supported distributions: NixOS, Ubuntu 26.04 and Debian 13; Arch works on the device, best effort. A linux-surface
  kernel release that `sp9-dkms` does not list runs the stock modules; hold the kernel until it is listed
  ([docs/install.md](docs/install.md#kernel-updates)). The packaged route was run on the device once on Ubuntu 26.04
  with Secure Boot off ([docs/test-results.md](docs/test-results.md#ubuntu-2604-packaged-route)), and once on
  Ubuntu 24.04, which works without cameras (no `sp9-libcamera` for it; its libcamera 0.2 finds no camera) and is
  not a supported target ([its table](docs/test-results.md#ubuntu-2404-packaged-route)), and once on Debian 13
  with `sp9-libcamera` and PipeWire 1.6 from trixie-backports
  ([its table](docs/test-results.md#debian-13-packaged-route)), and once on Arch with mkinitcpio's
  unified kernel image and systemd-boot ([its table](docs/test-results.md#arch-linux-packaged-route)). Debian
  testing and Ubuntu 26.10 are built and install-tested in containers only. Secure Boot with the DKMS key enrolled,
  the signed repositories and the source route are untested on the device.
- Debian and Ubuntu: dkms recommends a distribution kernel's headers, which `linux-headers-surface` does not
  provide, so with recommends on apt installs headers nothing uses (on Ubuntu 26.04 and 24.04).
  [docs/install-debian-ubuntu.md](docs/install-debian-ubuntu.md) section 1 says how to avoid or remove them.
- Ubuntu 24.04: WirePlumber 0.4 ignores the speaker and microphone rules, so the speaker protection filter is not
  the default sink (measured on the device).
- The pen keeps one bond per host address, and all systems on the device share the Bluetooth controller: coupling
  the pen on another installed system (Windows or another Linux) kills this system's bond, and the helper's probe
  did not recognise that (the link dropped without an encryption failure, `no-answer`). Put the pen in the cover
  and run `sp9-ctl pen couple --force` ([docs/helper.md](docs/helper.md), the section on the pen). On an
  installation with no bond for the pen, plain `sp9-ctl pen couple` couples it (once on Arch: `coupled-no-hid`,
  the Bluetooth HID not attached within 10 seconds; whether it attached later was not recorded).
- The device helper's guard for a coupling link that comes up through the pen's private address (refusing
  pairings that do not use the key, dropping the link) is tested with fakes only; on the device such a link
  did not come up ([docs/research/gaps/pen-vendor-exchange.md](docs/research/gaps/pen-vendor-exchange.md)
  section 2.5). Patch 0023's refusal of a pairing without the OOB flag while a TK is stored is verified on the
  device.
- Most local kernel patches are marked unverified in [PROVENANCE.md](kernel/PROVENANCE.md); the IR camera has no
  driver; tests that need a person or a second device are open ([docs/test-results.md](docs/test-results.md)).

[Unreleased]: https://github.com/joonhoekim/surface-pro-9-linux/commits/main
