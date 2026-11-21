# From source

This repository's full kernel and the two install scripts, for a distribution without packages here, or for this
repository's kernel instead of linux-surface's. Before this: [prepare the device](install-prepare.md); Secure Boot
off unless you sign the kernel. Overview and checks: [install.md](install.md).

**Unverified:** never followed on the device, and the kernel build script has not been run to completion. Reports
welcome ([install.md](install.md#report-what-you-see)).

## 1. Kernel

The kernel is stable v7.3.4 + linux-surface's 7.3 series + this repository's 26 patches
([kernel/README.md](../kernel/README.md)).

[`tools/linux/build-kernel.sh`](../tools/linux/build-kernel.sh) fetches the pinned sources, applies both
series with `git am`, merges and verifies the configuration, and builds on any x86-64 Linux host:

```sh
tools/linux/build-kernel.sh                  # linux-image and linux-headers .deb (Debian, Ubuntu)
tools/linux/build-kernel.sh --tarball        # boot/ + lib/modules/ as .tar.zst for other distributions
tools/linux/build-kernel.sh --prepare-only   # fetch, apply patches, verify the configuration; no build
```

Results go to `out/kernel/dist/` with a `SHA256SUMS`. The release string is `7.3.4-sp9`.

- **Verified**: the patches apply cleanly in this order.[^apply-order]
- **Untested**: the build itself, the packages, and booting their kernel. The configuration differs from the
  verified kernel's in the driver selection.[^config-base]
- **Secure Boot**: the result is unsigned unless you give the script a key. Either turn Secure Boot off or
  enrol your own MOK and sign (`--make-key`, `--sign-key`, `--sign-cert`); the procedure is in
  [kernel/README.md](../kernel/README.md).
- **Installing**: `sudo apt install ./linux-image-7.3.4-sp9_*.deb ./linux-headers-7.3.4-sp9_*.deb`, or for the
  tarball `sudo tar -xf … -C /`, `sudo depmod 7.3.4-sp9`, and an initramfs and boot entry made with the
  distribution's tools. [kernel/README.md](../kernel/README.md) also describes booting the new kernel once
  while keeping another kernel as the default.

Without this kernel (a distribution kernel, or linux-surface's packaged kernel) the platform files below
still apply, but the 26 patches are missing: see [kernel/PROVENANCE.md](../kernel/PROVENANCE.md) for what
each one changes. No such combination was tested.

[^apply-order]: v7.3.4, linux-surface's 47 commits, then 0001–0017 with `git am`; 0018–0026 were exported from
    commits on the tree of the booted kernel, where 0023, 0025 and 0026 were first built and loaded as modules; a
    NixOS-built kernel with all 26 has been booted.

[^config-base]: The configuration starts from the Ubuntu mainline config that linux-surface uses for its Debian
    packages, not from the nixpkgs configuration the verified kernel was built with: the options the fragments
    request are the same, the rest of the driver selection (built-in or module) differs.

## 2. Platform files

[`userspace/platform/install.sh`](../userspace/platform/install.sh) installs the configuration files that do
not depend on the distribution or the desktop. What each file fixes is in
[userspace/platform/README.md](../userspace/platform/README.md).

```sh
cd userspace/platform
./install.sh --dry-run                    # print what would change
sudo ./install.sh --without cmdline       # default components, without the kernel parameters
sudo ./install.sh --uninstall             # remove everything the script installed
```

- Default components: `iptsd` (touch preset), `initramfs` (`pinctrl_tigerlake` for the power and volume
  buttons), `cmdline` (`pci=hpiosize=0`), `speakers` (PipeWire filter chain), `sensors` (the accelerometer's
  udev rule), `psr` (PSR1 only on the panel through i915 debugfs, without a kernel parameter). Optional, with
  `--with`: `psr-param`, `hda-pins`, `audio-legacy`, `hibernate-reboot`; none of them is needed on this
  device with the current firmware.
- With this repository's kernel, leave `cmdline` out: the parameter was measured as unnecessary on it. `psr`
  does nothing there either (patch 0014 already gives PSR1).
- **PSR on another kernel.** A kernel without patch 0014 enables PSR2 on this panel, which Windows keeps off.
  `i915.enable_psr=1` limits it to PSR1 but is an unsafe i915 option: setting it taints the kernel (flag 64).
  The `psr` component gets the same limit without a taint: `sp9-psr1.service`, started by a udev rule when the
  i915 card appears, writes 3 (`FORCE_PSR1`) to `/sys/kernel/debug/dri/*/i915_edp_psr_debug`. That needs
  debugfs, and kernel lockdown makes debugfs read-only; most distributions turn lockdown on when Secure Boot is
  on. Under lockdown the unit only logs that, and
  `--with psr-param` adds `i915.enable_psr=1` instead, with the taint. Both are unverified on the device; check
  with `sudo cat /sys/kernel/debug/dri/0000:00:02.0/i915_edp_psr_status` (`PSR mode: PSR1 enabled`) and
  `cat /proc/sys/kernel/tainted`.
- The script refuses to run on another SKU, calls no package manager, and names missing dependencies. You
  need: `iptsd` (from the linux-surface package repository), PipeWire 1.0 or later, WirePlumber 0.5 or later,
  and swh-plugins (`swh-plugins` on Debian and Arch, `ladspa-swh-plugins` on Fedora).

## 3. Settings the NixOS module makes that you must reproduce by hand

`install.sh` does not cover everything the NixOS module does.

| NixOS module | By hand |
|---|---|
| Kernel (`kernel.enable`) | Section 1 |
| Firmware (`hardware.enableRedistributableFirmware`) | The distribution's firmware packages for Wi-Fi and Bluetooth (AX211), i915 and IPU6: `linux-firmware`; Debian: `firmware-iwlwifi` and `firmware-intel-graphics` from `non-free-firmware` ([install-debian-ubuntu.md](install-debian-ubuntu.md#8-what-the-packages-do-not-cover)); Arch: `linux-firmware-intel` and `wireless-regdb` ([install-arch.md](install-arch.md#1-linux-surfaces-repository-and-kernel)) |
| `pinctrl_tigerlake` in the initrd | `install.sh` component `initramfs` |
| `kernelParams.enable`, `kernelParams.hpiosize` | `install.sh` component `cmdline`; not with this kernel |
| `display.psr1` | `install.sh` component `psr`; not needed with this kernel |
| `kernelParams.psr` | `install.sh` component `psr-param` (taints the kernel); only where `psr` cannot write debugfs |
| `audio.codecPatch = "pins"`, `audio.legacy`, `hibernateReboot` | `install.sh --with hda-pins`, `audio-legacy`, `hibernate-reboot` |
| `touch.enable`, `touch.daemon = "iptsd"` | The `iptsd` package and service, plus `install.sh` component `iptsd` (the preset and the libinput quirk for the pen's tip threshold). The pen keys of the preset need iptsd 3.1.0 built with the patches of [`userspace/iptsd`](../userspace/iptsd/README.md) (`patch -p1`, in the order of their numbers); a stock iptsd ignores those keys and reads only the touch thresholds |
| `touch.daemon = "digitizerd"` | [`helper/install.sh`](../helper/install.sh) (section 4) installs and enables `sp9-digitizerd` and masks iptsd; `--keep-iptsd` installs it without enabling it (`digitizerd.package`). Only one of the two daemons may run ([digitizerd.md](digitizerd.md)). Untested outside NixOS |
| `sensors.enable` | `install.sh` component `sensors` (the udev rule for the accelerometer) and the distribution's `iio-sensor-proxy` package |
| `camera.libcamera` | Build libcamera 0.8.0 with the patches of [`userspace/libcamera`](../userspace/libcamera/README.md) and build PipeWire's libcamera plugin against it (PipeWire 1.6 needs the two small source adjustments that [`nix/pipewire-libcamera.nix`](../nix/pipewire-libcamera.nix) makes). Untested outside NixOS |
| `camera.mainsFrequency = 50` | In the tuning files that libcamera patch 0010 adds, replace `flickerPeriod: 8333` with `flickerPeriod: 10000` before building |
| `camera.pipewire = false` | Disable WirePlumber's libcamera monitor (`monitor.libcamera = disabled` in a WirePlumber drop-in) |
| `speakers.enable`: filter chain and raw sink name | `install.sh` component `speakers` |
| `speakers.enable`: raw sink at 100 % | Not installed by `install.sh`. Run [`pipewire/sp9-speaker-raw-volume`](../userspace/platform/pipewire/sp9-speaker-raw-volume) after every WirePlumber start,[^raw-volume] or run `wpctl set-volume` on the sink `sp9_speaker_raw` once. Without it the raw sink starts at 40 %, which caps the output at −23.9 dB whatever the volume control shows |
| `microphone.enable` | Copy [`60-sp9-mic.conf`](../userspace/platform/wireplumber/wireplumber.conf.d/60-sp9-mic.conf) to `/usr/share/wireplumber/wireplumber.conf.d/`. Without it the first-use gain is +60 dB and clips |
| `audio.echoCancel` | Copy [`61-sp9-echo-cancel.conf`](../userspace/platform/pipewire/pipewire.conf.d/61-sp9-echo-cancel.conf) to `/usr/share/pipewire/pipewire.conf.d/` |
| `diagnostics.enable` | Install the tools the collector calls.[^diag-tools] The collector skips what is missing |
| `helper.enable` | Section 4 |
| `pen.enable`, `pen.connectService`, `pen.feedbackService` | Section 5 |

[^raw-volume]: As a systemd user unit of type `oneshot`, `After=` and `WantedBy=wireplumber.service`. The script
    leaves the raw sink alone when the "Speakers" filter sink, the speaker protection, is missing.

[^diag-tools]: libcamera tools (`cam`), v4l-utils, alsa-utils, alsa-tools, pulseaudio-utils (`pactl`), libinput
    tools, hid-tools, evtest, drm_info, i2c-tools, intel-gpu-tools, acpica-tools, lm-sensors, pciutils, usbutils.

The cameras need libcamera with the simple pipeline and its software ISP, and WirePlumber's libcamera monitor
for PipeWire applications (on by default in WirePlumber). With an unpatched libcamera (measured)[^unpatched]
frames arrive, but the processed image has purple lines at a fixed spacing, the exposure is off or swings, and the
rear camera has no focus control. The patches in `userspace/libcamera` fix these.

[^unpatched]: 0.7.2 and 0.8.0 were measured. Unpatched 0.8.0 delivers no frames at all on this device, because it
    selects a packed CSI-2 format that fails here ([research/gaps/camera.md](research/gaps/camera.md) section 6).

## 4. Device helper

Optional. Needs Rust 1.89 or later, systemd, D-Bus and polkit; the settings app additionally needs the GTK
4.12 and libadwaita 1.5 development files.

```sh
cd helper
cargo build --release
cargo build --release -p sp9-settings     # optional: the settings app
sudo ./install.sh                         # /usr/local, enables and starts sp9-helperd and sp9-digitizerd, masks iptsd
sudo ./install.sh --keep-iptsd            # or: iptsd stays the touch and pen daemon
sp9-ctl status
sudo ./install.sh --uninstall             # also unmasks iptsd if install.sh masked it
```

By default `install.sh` also installs `sp9-digitizerd` in place of iptsd ([helper.md](helper.md#installation)).
It loads `surface_aggregator_cdev` now and at boot; without that module smart charging and the fan
request show as unavailable. Usage: [helper.md](helper.md). The helper's functions were checked on the device
under NixOS ([helper-reference.md](helper-reference.md)); this installer was not.

## 5. Pen tools

Optional prototypes: `pen-haptic`, `surface-pen-connect` and `surface-pen-feedback`
([userspace/pen/README.md](../userspace/pen/README.md)).
Python 3.10 or later; `surface-pen-connect` needs `dbus-next`. They run as root from a copy of the directory;
the README has the copy and unit commands. With the device helper installed (section 4) they are not needed
for everyday use: the helper does the inking haptics with the speed feedback the continuous waveforms need
(`sp9-ctl pen ink on`) and bonds a pen put into the cover's pen slot (`sp9-ctl pen auto-couple on`, kernel
patch 0023 required), and a pen bonded that way reconnects after its idle disconnect ([helper.md](helper.md),
the section on the pen). `surface-pen-feedback` remains as the standalone alternative for a system without the
helper; do not run it next to the helper's pen haptics. A pen paired by hand in pairing mode does not reconnect
after its idle disconnect, so `surface-pen-connect` has little effect for it.

## 6. Verify and go back

```sh
uname -r                                            # 7.3.4-sp9
sudo tools/linux/post-install-check.sh              # report in ~/sp9-check-<host>/, summary.txt first
```

The collector is read-only and works on any distribution; what it reports, and what to expect first:
[install.md](install.md#check-the-installation). Hardware expectations are in
[hardware-status.md](hardware-status.md).

To go back: boot the previous kernel from the boot loader's menu, remove the kernel packages
(`sudo apt remove linux-image-7.3.4-sp9 linux-headers-7.3.4-sp9`), and run both `install.sh --uninstall`.
