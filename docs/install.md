# Installing

Pick the page for your distribution; the steps around it are the same everywhere.

| Distribution | Page | Kernel and packages |
|---|---|---|
| NixOS | [NixOS](install-nixos.md) | This repository's kernel and NixOS module, from its flake |
| Ubuntu, Debian | [Debian and Ubuntu](install-debian-ubuntu.md) | linux-surface's kernel, this project's apt packages |
| Arch Linux | [Arch Linux](install-arch.md) | linux-surface's kernel, this project's pacman packages |
| Any other | [From source](install-from-source.md) | This repository's kernel, built locally, and the install scripts |

## The steps

1. **Prepare the device**: Windows, the disk, the UEFI settings ([install-prepare.md](install-prepare.md)). The
   same for every distribution.
2. **Install the distribution** with its own installer, into the free space, keeping the EFI system partition.
3. **Add the kernel and this project's parts**, as the distribution's page describes.
4. **Reboot once**: the kernel command line, the initramfs, PSR and the kernel modules take effect at boot.
5. **Check** the installation ([below](#check-the-installation)).

## Support

| Release | Support | On the device | Cameras (`sp9-libcamera`) |
|---|---|---|---|
| NixOS | Supported | In depth, internal disk, dual boot[^nixos] | yes (module) |
| Ubuntu 26.04 | Supported | Once[^once] | yes |
| Debian 13 | Supported | Once[^once] | yes, with trixie-backports (PipeWire 1.6) |
| Arch Linux | Works; best effort[^arch] | Once[^once] | yes |
| Ubuntu 24.04 | Works without cameras; not a target | Once[^once] | no: neither camera works[^ubuntu-2404] |
| Ubuntu 26.10, Debian testing | Best effort | Containers only | yes |
| Any other ([from source](install-from-source.md)) | Best effort | Never | by hand |

Every package is built for each packaged release. Reports for any row are welcome
([below](#report-what-you-see)).

[^nixos]: The device this project is developed on runs it; the measurements in
    [hardware-status.md](hardware-status.md) were made on it.

[^once]: One pass from an SD card in a USB card reader, Secure Boot off, packages built locally. Not covered:
    installing to the internal NVMe disk, dual boot, disk encryption, standby power with the NVMe disk. Results:
    [Ubuntu 26.04](test-results.md#ubuntu-2604-packaged-route), [Debian 13](test-results.md#debian-13-packaged-route),
    [Arch](test-results.md#arch-linux-packaged-route), [Ubuntu 24.04](test-results.md#ubuntu-2404-packaged-route).

[^arch]: A rolling release: `sp9-dkms` is built for exact linux-surface kernel releases, and linux-surface's Arch
    kernel moves to a new one often ([kernel updates](#kernel-updates)).

[^ubuntu-2404]: There is no `sp9-libcamera` for it, and its libcamera 0.2 finds no camera. Its WirePlumber 0.4
    ignores the speaker and microphone rules, so the speaker protection filter does not attach.

### What was verified

- **Containers:** every package builds in the distribution's official container,[^containers] passes lintian or
  namcap, installs, has its files checked, runs its daemons without the hardware, and is removed without leaving
  a file behind. The repositories are signed and read back by apt and pacman. `sp9-dkms` is built and installed
  against linux-surface's real kernel and header packages ([dkms/README.md](../dkms/README.md)), `sp9-libcamera`
  against each distribution's PipeWire except Ubuntu 24.04's
  ([packaging/libcamera/README.md](../packaging/libcamera/README.md)).
- **Device:** NixOS in depth ([hardware-status.md](hardware-status.md)); the packaged releases once each (table
  above).
- **Unverified:** Ubuntu 26.10 and Debian testing on the device; on any packaged release, Secure Boot with the DKMS
  key enrolled through MokManager, installing from the signed repositories, and moving to a new linux-surface
  kernel release. That the files behave there as under NixOS is inferred. The source route was never followed, and
  its kernel build script has not been run to completion.

[^containers]: Ubuntu 24.04, 26.04, 26.10, Debian 13, Debian testing and Arch. The checks cover units, rules,
    D-Bus, polkit, desktop and AppStream files; the helper runs with fake backends, the touch daemon waits for its
    device.

### Kernel updates

There is no schedule for moving to new kernels.

- **NixOS.** The flake pins its kernel (v7.3.4 with linux-surface's series and this repository's patches); updating
  nixpkgs does not change it. A new kernel comes with an update of this flake.
- **Debian, Ubuntu, Arch.** Each `sp9-dkms` release covers the linux-surface kernel releases it lists
  (`/usr/src/sp9-*/kernels`). For any other release DKMS skips the build without failing the kernel update
  (`BUILD_EXCLUSIVE_KERNEL` in the generated `dkms.conf`, [dkms/README.md](../dkms/README.md#how-the-sources-are-made);
  seen in containers), `sp9-ctl packages` says "not covered", and the kernel runs linux-surface's stock modules.
  To stay covered, hold linux-surface's kernel until an `sp9-dkms` update lists the new release
  ([Debian and Ubuntu](install-debian-ubuntu.md#4-kernel-modules-sp9-dkms-and-secure-boot),
  [Arch](install-arch.md#4-kernel-modules-sp9-dkms-and-secure-boot)).

What an uncovered kernel loses:[^stock-modules]

- the camera fixes: without patch 0006 the front camera is expected to give no frames, without 0011 the rear camera
  has no focus motor;
- USB-C port and charger information;
- the SAM RTC node and fan floor;
- the Type Cover's mic-mute key and backlight LED;
- the touch controller's feature-response fix;
- the pen's cover coupling (`sp9-dkms-bluetooth`).

[^stock-modules]: Read from the patches ([kernel/PROVENANCE.md](../kernel/PROVENANCE.md)), not run on the device:
    the packaged routes have only been run on covered kernels. Touch and pen are driven by linux-surface's own
    `ithc` and by `sp9-digitizerd` either way, and the helper sets the keyboard backlight through hidraw when the
    LED is missing; both are inferred. Under Secure Boot an uncovered kernel needs no DKMS key: nothing of
    `sp9-dkms` is installed for it, so the stock modules load.

## What an installation consists of

The same parts on every route; they differ in where each part comes from.

| Part | NixOS | Debian, Ubuntu, Arch (packages) | From source |
|---|---|---|---|
| **Kernel** | This repository's: v7.3.4 + linux-surface's 7.3 series + the 26 patches of [kernel/](../kernel/README.md), built on the device | linux-surface's, from its repository[^ls-kernel] | This repository's, built with `tools/linux/build-kernel.sh` |
| **Kernel patches as modules** (DKMS overlay) | Not needed: the patches are in the kernel | `sp9-dkms` and `sp9-dkms-bluetooth`, built by DKMS for each installed linux-surface kernel ([dkms/README.md](../dkms/README.md)) | Not needed |
| **Platform files** ([userspace/platform](../userspace/platform/README.md)) | Module options | `sp9-platform` | `userspace/platform/install.sh` |
| **Touch and pen daemon** | `touch.daemon`: `sp9-digitizerd` or iptsd with this repository's pen patches | `sp9-digitizerd`, which replaces iptsd | `helper/install.sh`, or iptsd built with the patches |
| **Device helper** and settings app ([helper.md](helper.md)) | `helper.enable` | `sp9-helper`, `sp9-settings` | `helper/install.sh` |
| **Camera stack**: libcamera 0.8.0 with [this repository's patches](../userspace/libcamera/README.md) | `camera.libcamera`, on by default | `sp9-libcamera`, experimental and opt-in | Built by hand |
| **Firmware** | `hardware.enableRedistributableFirmware`, set by the module | Ubuntu `linux-firmware`; Debian `non-free-firmware`; Arch `linux-firmware-intel` | The distribution's packages |
| **Kernel command line** | Nothing needed with this kernel[^cmdline] | `sp9-cmdline`: GRUB on Debian and Ubuntu, unified kernel images on Arch | Not needed with this kernel |
| **Updates** | `nix flake update`, then a rebuild | apt or pacman, with the distribution's updates | By hand |

[^ls-kernel]: It lacks the 26 patches of [kernel/](../kernel/README.md); the DKMS overlay brings the ones that
    can be built as modules. [kernel/PROVENANCE.md](../kernel/PROVENANCE.md) says what each patch changes.

[^cmdline]: `pci=hpiosize=0` and `i915.enable_psr=1` were measured as unnecessary on this repository's kernel. A
    distribution or linux-surface kernel needs `pci=hpiosize=0`; PSR1 comes from the platform files' debugfs
    write instead of the parameter where debugfs is writable.

Nothing of this project downloads or installs anything by itself: the package manager (or Nix) does.

### The packages

| Package | What it is | Pulled in by `sp9` |
|---|---|---|
| `sp9-platform` | The files of [userspace/platform](../userspace/platform/README.md)[^platform-files] | yes |
| `sp9-helper` | The device helper `sp9-helperd` and `sp9-ctl` ([helper.md](helper.md)); it starts only on this model | yes |
| `sp9-digitizerd` | The touch and pen daemon ([digitizerd.md](digitizerd.md)). It replaces iptsd: installing it removes iptsd | yes |
| `sp9-settings` | The settings app | recommended |
| `sp9-dkms` | The kernel patches as modules for the linux-surface kernel ([Debian, Ubuntu](install-debian-ubuntu.md#4-kernel-modules-sp9-dkms-and-secure-boot), [Arch](install-arch.md#4-kernel-modules-sp9-dkms-and-secure-boot)) | recommended |
| `sp9-dkms-bluetooth` | The Bluetooth core with the cover coupling patches; needed for `sp9-ctl pen couple` | no |
| `sp9-cmdline` | `pci=hpiosize=0` on the kernel command line[^sp9-cmdline] | recommended |
| `sp9-libcamera` | Patched libcamera for the cameras, experimental | no |

"Recommended" is apt's: installed with `sp9` unless recommends are off. On Arch these are optional dependencies,
which pacman does not install by itself; name them ([install-arch.md](install-arch.md#3-install-the-packages)). The
reasons for the split are in [packaging/README.md](../packaging/README.md).

[^platform-files]: iptsd preset and pen quirk, the accelerometer rule (with `iio-sensor-proxy`), PSR1 through
    debugfs, `pinctrl_tigerlake` in the initramfs, speaker protection, microphone gain, the echo-cancelled
    microphone.

[^sp9-cmdline]: On Debian and Ubuntu through GRUB (`/etc/default/grub.d`, then `update-grub`); on Arch through
    `/etc/cmdline.d`, for unified kernel images only ([install-arch.md](install-arch.md#kernel-command-line-sp9-cmdline));
    other boot loaders by hand ([userspace/platform/README.md](../userspace/platform/README.md)).

## Check the installation

```sh
uname -r                          # NixOS and the source route: 7.3.4 (7.3.4-sp9 from source); packages: contains "surface"
cat /proc/cmdline                 # pci=hpiosize=0 with sp9-cmdline; neither it nor i915.enable_psr with this repository's kernel
sudo cat /sys/kernel/debug/dri/0000:00:02.0/i915_edp_psr_status    # "PSR mode: PSR1 enabled"
cat /sys/bus/iio/devices/iio:device*/in_accel_hysteresis 2>/dev/null   # 0: the sensor rule applied
cam -l                            # both cameras, with the patched libcamera 0.8.0
sp9-ctl status                    # with the helper: every property the daemon exports; Device is surface-pro-9
sp9-ctl packages                  # packaged routes: versions, the sp9-dkms modules of the running kernel, the update command
```

The **post-install check** reads everything that can be checked unattended and writes one file per area plus
`summary.txt` into `~/sp9-check-<host>/`. It is read-only and works on any distribution.[^check-path]

```sh
sudo /usr/libexec/sp9-helper/post-install-check.sh     # Debian, Ubuntu (packages)
sudo /usr/lib/sp9-helper/post-install-check.sh         # Arch (packages)
sudo tools/linux/post-install-check.sh                 # NixOS, from source: from a clone of this repository
sp9-ctl diagnostics                                    # with the helper: the same collector, as a tar.gz report
less ~/sp9-check-*/summary.txt
```

Each line of the summary has a status: `MATCH` (as the research documents expect), `DIFF`, `MISSING`, `INFO`,
or `SKIP` (needs root or a tool that is not installed). `MATCH` is not "works": that is decided
by the tests in [test-plan.md](test-plan.md).

The collector masks MAC addresses, UUIDs, serial numbers and Wi-Fi network names, and leaves out the names of
paired Bluetooth devices other than the pen. The report still describes the machine (kernel log, ACPI tables,
firmware versions): read it before you share it.

[^check-path]: `sp9-helper` installs it; on NixOS and the source route it is run from a clone
    (`git clone https://github.com/joonhoekim/surface-pro-9-linux`).

## Known limits you will meet first

The full list is [hardware-status.md](hardware-status.md).

- **Pen.** Drawing works without Bluetooth. The Bluetooth features (tail button, battery level, haptics) need
  the pen to be paired.[^pen-pairing] With the helper, `sp9-ctl pen auto-couple on` bonds the pen when it is put
  into the cover's pen slot, as on Windows, and a pen bonded that way reconnects when it is picked up;
  `sp9-ctl pen ink on` gives the inking haptics, including the continuous waveforms ([helper.md](helper.md#pen)).
  Without the helper, the continuous haptics need `surface-pen-feedback`[^pen-feedback] and the cover coupling
  has to be done by hand ([research/gaps/pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md)).
- **Pen in drawing applications.** Writing, the barrel button and the eraser end work. After erasing, the
  application may keep showing the eraser cursor when the pen is taken away; it does nothing.
- **Cameras.** Both work with the patched libcamera, the rear one with autofocus; the front sensor always runs its
  full-resolution mode at about 28.6 fps. Colours are uncalibrated (low saturation). Ubuntu 24.04 has no camera
  stack that works.
- **IR camera.** No driver, so no face unlock.
- **Speakers.** The volume control you move is the "Speakers" filter sink, which the platform files make the
  default. The raw sink behind it is kept at 100 %; leave it there.
- **Microphones.** The platform files set the first-use gain to +30 dB because the PipeWire default (+60 dB)
  clips.[^mic-gain] For calls there is a separate source, "Microphone (echo cancelled)", which has to be selected
  in the application; call quality is unverified.
- **Rotation.** The screen follows the device in a compositor that reads iio-sensor-proxy (observed). Whether
  your desktop holds the orientation while the cover is attached is the desktop's business.
- **Touch.** Works in everyday use with `sp9-digitizerd` (observed); the test of light taps and swipes and of
  palm rejection has not been run.
- **Type Cover.** Keys, touchpad, the function row, and detaching and reattaching worked once, on Ubuntu 26.04
  (observed); the repeated detach test has no record.
- **Buttons, external monitor.** With the Type Cover detached (tablet use), the power and volume buttons work
  (observed). An external monitor shows a picture on either USB-C port (observed); daisy-chaining over DP MST and
  audio over the monitor are unverified.
- **Hibernation.** Not available with zram swap only, the layout tested here.
- **Heat.** Under sustained load the surface gets hotter than on Windows, which lowers the power limit at a
  lower skin temperature.
- **Firmware updates** come through Windows Update. A firmware update can put Windows Boot Manager first in the
  boot order again;[^boot-order] `efibootmgr -o` or the UEFI boot configuration restores it.

[^pen-pairing]: By hand: hold the tail button until the pen is in pairing mode and pair it with a
    `NoInputNoOutput` agent. The pen disconnects after about 80 s without use, and a pen paired this way does not
    reconnect; it has to be put in pairing mode again.

[^pen-feedback]: From [userspace/pen](../userspace/pen/README.md); on NixOS the option
    `hardware.surface-pro-9.pen.feedbackService`.

[^mic-gain]: It applies only to an account with no stored volume; on an account that already recorded at 100 %
    run `wpctl set-volume @DEFAULT_AUDIO_SOURCE@ 32%`.

[^boot-order]: Not observed on the test device.

## Report what you see

Results from a Surface Pro 9 move statements out of unverified; Debian testing and Ubuntu 26.10 have none yet.

- One test of [test-plan.md](test-plan.md): the
  [Test result](https://github.com/joonhoekim/surface-pro-9-linux/issues/new?template=test-result.yml) issue form.
- A wider "what works on my unit" report: the
  [Device reports](https://github.com/joonhoekim/surface-pro-9-linux/discussions/categories/device-reports)
  discussion category.

Include: the distribution and release; `uname -r`; `sp9-ctl packages`;[^packages-output] whether Secure Boot is on
and the DKMS key enrolled; which touch daemon runs (`sp9-digitizerd` or iptsd); for cameras whether `sp9-libcamera`
is installed; and what you measured or saw, as the test plan describes. The post-install check
([above](#check-the-installation)) collects most of it and masks addresses and serials; read its output before
posting all the same ([CONTRIBUTING.md](../CONTRIBUTING.md)).

[^packages-output]: Package versions, which `sp9-dkms` modules are loaded, the Secure Boot advice if any.
