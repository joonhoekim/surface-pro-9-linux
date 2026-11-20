# Other distributions

What a Surface Pro 9 (Intel, SKU `Surface_Pro_9_2038`) needs on a distribution other than NixOS, using the
same sources as the NixOS module.

**This path is untested.** The device this repository was developed on runs NixOS, and the NixOS path
([install-nixos.md](install-nixos.md)) is the verified one. The kernel build script has never been run to
completion, and neither `install.sh` has been run on the device. Everything below is untested unless it says
otherwise. What is verified is the content: the same patches, the same platform files and the same helper
binaries are what the NixOS system uses.

General installation (partitioning, Secure Boot, dual boot) is covered by the linux-surface wiki
([Installation and Setup](https://github.com/linux-surface/linux-surface/wiki/Installation-and-Setup)); the
device-specific preparation in [install-nixos.md](install-nixos.md) steps 1 and 2 applies to any distribution.

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

- **Verified**: the patches apply cleanly in this order (v7.3.4, linux-surface's 47 commits, then 0001–0017
  with `git am`; 0018–0026 were exported from commits on the tree of the booted kernel, where 0023, 0025 and
  0026 were first built and loaded as modules; a NixOS-built kernel with all 26 has been booted).
- **Untested**: the build itself, the packages, and booting their kernel. The configuration starts from the
  Ubuntu mainline config that linux-surface uses for its Debian packages, not from the nixpkgs configuration
  the verified kernel was built with: the options the fragments request are the same, the rest of the driver
  selection (built-in or module) differs.
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
  buttons), `cmdline` (`i915.enable_psr=1 pci=hpiosize=0`), `speakers` (PipeWire filter chain). Optional, with
  `--with`: `hda-pins`, `audio-legacy`, `hibernate-reboot`; none of them is needed on this
  device with the current firmware.
- With this repository's kernel, leave `cmdline` out: both parameters were measured as unnecessary on it, and
  `i915.enable_psr=1` taints the kernel. With any other kernel keep it.
- The script refuses to run on another SKU, calls no package manager, and names missing dependencies. You
  need: `iptsd` (from the linux-surface package repository), PipeWire 1.0 or later, WirePlumber 0.5 or later,
  and swh-plugins (`swh-plugins` on Debian and Arch, `ladspa-swh-plugins` on Fedora).

## 3. Settings the NixOS module makes that you must reproduce by hand

`install.sh` does not cover everything the NixOS module does.

| NixOS module | By hand |
|---|---|
| Kernel (`kernel.enable`) | Section 1 |
| Firmware (`hardware.enableRedistributableFirmware`) | The distribution's `linux-firmware` package: Wi-Fi and Bluetooth (AX211), i915, IPU6 |
| `pinctrl_tigerlake` in the initrd | `install.sh` component `initramfs` |
| `kernelParams.*` | `install.sh` component `cmdline`; not with this kernel |
| `touch.enable`, `touch.daemon = "iptsd"` | The `iptsd` package and service, plus `install.sh` component `iptsd` (the preset and the libinput quirk for the pen's tip threshold). The pen keys of the preset need iptsd 3.1.0 built with the patches of [`userspace/iptsd`](../userspace/iptsd/README.md) (`patch -p1`, in the order of their numbers); a stock iptsd ignores those keys and reads only the touch thresholds |
| `touch.daemon = "digitizerd"` | Not covered by either `install.sh`. Build `sp9-digitizerd` in `helper/` (`cargo build --release -p sp9-digitizerd`), install the binary, [`sp9-digitizerd.service`](../helper/data/sp9-digitizerd.service) and the D-Bus policy [`io.github.joonhoekim.Sp9Digitizer1.conf`](../helper/data/io.github.joonhoekim.Sp9Digitizer1.conf) by hand (replace `@BINDIR@` in the unit with the directory of the binary), load `uinput`, and stop and disable iptsd: only one of the two daemons may run ([digitizerd.md](digitizerd.md)). Untested outside NixOS |
| `sensors.enable` | `install.sh` component `sensors` (the udev rule for the accelerometer) and the distribution's `iio-sensor-proxy` package |
| `camera.libcamera` | Build libcamera 0.8.0 with the patches of [`userspace/libcamera`](../userspace/libcamera/README.md) and build PipeWire's libcamera plugin against it (PipeWire 1.6 needs the two small source adjustments that [`nix/pipewire-libcamera.nix`](../nix/pipewire-libcamera.nix) makes). Untested outside NixOS |
| `speakers.enable`: filter chain and raw sink name | `install.sh` component `speakers` |
| `speakers.enable`: raw sink at 100 % | Not installed by `install.sh`. Run [`pipewire/sp9-speaker-raw-volume`](../userspace/platform/pipewire/sp9-speaker-raw-volume) after every WirePlumber start (a systemd user unit of type `oneshot`, `After=` and `WantedBy=wireplumber.service`), or run `wpctl set-volume` on the sink `sp9_speaker_raw` once. Without it the raw sink starts at 40 %, which caps the output at −23.9 dB whatever the volume control shows |
| `microphone.enable` | Copy [`60-sp9-mic.conf`](../userspace/platform/wireplumber/wireplumber.conf.d/60-sp9-mic.conf) to `/usr/share/wireplumber/wireplumber.conf.d/`. Without it the first-use gain is +60 dB and clips |
| `audio.echoCancel` | Copy [`61-sp9-echo-cancel.conf`](../userspace/platform/pipewire/pipewire.conf.d/61-sp9-echo-cancel.conf) to `/usr/share/pipewire/pipewire.conf.d/` |
| `diagnostics.enable` | Install the tools the collector calls: libcamera tools (`cam`), v4l-utils, alsa-utils, alsa-tools, pulseaudio-utils (`pactl`), libinput tools, hid-tools, evtest, drm_info, i2c-tools, intel-gpu-tools, acpica-tools, lm-sensors, pciutils, usbutils. The collector skips what is missing |
| `helper.enable` | Section 4 |
| `pen.enable`, `pen.connectService` | Section 5 |

The cameras need libcamera with the simple pipeline and its software ISP, and WirePlumber's libcamera monitor
for PipeWire applications (on by default in WirePlumber). With an unpatched libcamera (0.7.2 and 0.8.0 were
measured) frames arrive, but the processed image has purple lines at a fixed spacing, the exposure is off or
swings, and the rear camera has no focus control; unpatched 0.8.0 delivers no frames at all on this device,
because it selects a packed CSI-2 format that fails here ([research/gaps/camera.md](research/gaps/camera.md)
section 6). The patches in `userspace/libcamera` fix these.

## 4. Device helper

Optional. Needs Rust 1.82 or later, systemd, D-Bus and polkit; the settings app additionally needs the GTK
4.12 and libadwaita 1.5 development files.

```sh
cd helper
cargo build --release
cargo build --release -p sp9-settings     # optional: the settings app
sudo ./install.sh                         # /usr/local, enables and starts sp9-helperd.service
sp9-ctl status
sudo ./install.sh --uninstall
```

`install.sh` loads `surface_aggregator_cdev` now and at boot; without that module smart charging and the fan
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

The collector is read-only and works on any distribution. Hardware expectations are in
[hardware-status.md](hardware-status.md).

To go back: boot the previous kernel from the boot loader's menu, remove the kernel packages
(`sudo apt remove linux-image-7.3.4-sp9 linux-headers-7.3.4-sp9`), and run both `install.sh --uninstall`.
