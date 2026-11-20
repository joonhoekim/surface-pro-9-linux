# Platform files

Configuration files that are needed when Linux is installed on this device, whatever the distribution and desktop, and
[`install.sh`](install.sh).
The structure follows `userspace/platform/` of the author's `tb323fu-linux` repository; only things with a basis in
`docs/research/gaps/` or the linux-surface wiki are included.
The status column is the result of the NixOS module ([docs/nixos.md](../../docs/nixos.md)) installing the same files on this device.
Installing through `install.sh` has not been run.

## Installation

```sh
sudo ./install.sh                         # default components
sudo ./install.sh --with hda-pins         # add optional components (several, separated by commas)
sudo ./install.sh --without cmdline       # leave out a default component
./install.sh --dry-run                    # only print what would change
sudo ./install.sh --uninstall             # remove everything this script installed
DESTDIR=/mnt/root ./install.sh            # install only the files into a mounted root
```

- It stops if the DMI `product_sku` is not `Surface_Pro_9_2038`. With `DESTDIR` (other than `/`, which counts as no `DESTDIR`) it does not touch the running system (no initramfs regeneration, no boot loader update, no kernel command line change), so the check is skipped.
- A file with the same content is not written again, and initramfs and grub.cfg are regenerated only when something changed. Running it several times gives the same result.
- It does not call a package manager. Missing dependencies are reported with the package names of the distribution family (Debian/Ubuntu, Fedora, Arch, from `/etc/os-release`).
- Static files go under `/usr` (`PREFIX`). Only the kernel command line and the mkinitcpio drop-in are written to `/etc`.

## Default components

| Component | File → installed location | What it fixes | Basis | Status |
|---|---|---|---|---|
| `iptsd` | `iptsd/surface-pro-9.conf` → `/usr/share/iptsd/` | iptsd has no preset for the SP9 (`045E:0C52`), so the code default thresholds 40/36/15 are used and weak touches and swipes are dropped. This sets 24/20/5 and discards touch while the pen is in proximity (first measure against palm input). Size and inversion come from the metadata and are not set | [pen-touch.md](../../docs/research/gaps/pen-touch.md) section 3 items 1 and 5, wiki "Erratic touchscreen behaviour" | The preset is read, and there were 0 ghost touches during 150 seconds without contact (measured). Dropped weak touches and swipes, and palm rejection, are unverified (TOUCH-1, TOUCH-2) |
| `iptsd` (with it) | `libinput/sp9-stylus.quirks` → `/usr/share/libinput/60-sp9-stylus.quirks` (NixOS: `/etc/libinput/local-overrides.quirks`) | libinput takes the pen tip as down only above 5 % of the pressure range and as up below 1 %. Feather-light strokes of the Slim Pen 2 have a pressure of 3–25 % (median 11 %), so the lightest stroke is lost entirely and strokes are cut at both ends. The quirk lowers the thresholds to 2 (down) and 1 (up) in device units. It takes effect when the compositor is restarted (log in again) | libinput's tablet pressure thresholds | Measured: a recorded input replayed into libinput gave tip contact for 6 of 7 strokes with the default thresholds and for 7 of 7 with 2/1 |
| `initramfs` | `initramfs/initramfs-tools/sp9-platform` → `/usr/share/initramfs-tools/modules.d/`, `initramfs/dracut/90-sp9-platform.conf` → `/usr/lib/dracut/dracut.conf.d/`, `initramfs/mkinitcpio/sp9-platform.conf` → `/etc/mkinitcpio.conf.d/` | If `soc_button_array` probes before `pinctrl_tigerlake`, the power and volume buttons are dead. This puts `pinctrl_tigerlake` into the initramfs. Only the files for the generators present in the target root are installed | wiki "Hardware power and volume buttons", [hardware.md](../../docs/hardware.md), the section on the status of the Surface Pro 9 in the linux-surface wiki | In the NixOS initrd `pinctrl_tigerlake` is loaded and the button input devices appear (measured). There is no test in which the buttons were pressed (SAM-3) |
| `cmdline` | `cmdline/sp9-platform.cmdline` → `/usr/share/sp9-platform/cmdline`, and applied as described below | `i915.enable_psr=1`: like Windows, switches off only PSR2 and keeps the PSR1 power saving (instead of the wiki's `=0`). `pci=hpiosize=0`: removes the I/O reservation of hotplug bridges, which prevents a GPE storm on lid close and TBT hotplug | [display-power.md](../../docs/research/gaps/display-power.md) section 3 items 1 and 7, wiki "Screen flickering", "ACPI interrupt storm" | With this repository's kernel neither is needed (measured): patch 0014 alone gives PSR1, and the 0x1800 range is reserved as a firmware resource, so there are no GPE errors on lid open and close without the parameter (option table in [nixos.md](../../docs/nixos.md)). The effect on distribution kernels is not verified (that PSR2 is the cause of the flicker is inferred) |
| `speakers` | `pipewire/pipewire.conf.d/60-sp9-speakers.conf` → `/usr/share/pipewire/pipewire.conf.d/`, `wireplumber/wireplumber.conf.d/60-sp9-speaker-raw.conf` → `/usr/share/wireplumber/wireplumber.conf.d/` | There is no smart amplifier, so Windows corrects and protects the speakers with Surface Render EFX. This creates a "Speakers" sink that does the same processing and makes it the default output. The WirePlumber rule fixes the name of the PCH HDA speaker sink to `sp9_speaker_raw` (the name differs between the SOF and legacy paths) so that the filter can attach, and lowers its priority | [audio.md](../../docs/research/gaps/audio.md) 1.5 "Surface Render EFX tuning", section 3 G4 | The filter sink comes up as the default output, and the 40 Hz and 3.7 kHz bands are reduced. Full-scale input at 100 Hz–1 kHz is unchanged (measured). How it sounds is unverified (AUD-2) |
| `sensors` | `udev/rules.d/61-sp9-sensors.rules` → `/usr/lib/udev/rules.d/` | The accelerometer of the SAM sensor hub reports only when its reading moved by more than its change sensitivity (0.05 g), and SAM HID has no request for an input report. On a device at rest the buffer stays empty and `in_accel_*_raw` returns 0 after 5 seconds; iio-sensor-proxy takes the empty buffer for a broken one and falls back to exactly those reads. The rule writes the sensitivity as 0, and the hub then reports at 8 Hz while the sensor is on. iio-sensor-proxy itself comes from the distribution's package | [sam-platform.md](../../docs/research/gaps/sam-platform.md) test 12 | With the NixOS module: the sensitivity is 0 after boot (measured), and the screen follows when the device is turned, in a compositor that reads iio-sensor-proxy (observed, SAM-7). With the rule put in place by hand the proxy reported the orientation (measured). After a suspend: unverified |

### How the kernel command line is applied

User settings are not overwritten. A parameter that is already present is not added, and a warning is printed if `i915.enable_psr` has a different value.

| Boot method | What is done | Removal |
|---|---|---|
| `grubby` present (Fedora) | `grubby --update-kernel=ALL --args=…` with the missing parameters only | `--remove-args` with only what was added (record: `/var/lib/sp9-platform/`) |
| `grub-mkconfig` reads `/etc/default/grub.d/*.cfg` (Debian/Ubuntu) | A line in `/etc/default/grub.d/sp9-platform.cfg` that appends to `GRUB_CMDLINE_LINUX_DEFAULT`, then `update-grub` | Delete the file, `update-grub` |
| `/etc/kernel/cmdline` exists or systemd-boot is installed (kernel-install, UKI) | Only the missing parameters are appended. If the file does not exist it is created from the current `/proc/cmdline` (without `BOOT_IMAGE=` and `initrd=`). Regenerating the entries (`kernel-install add …`) is only pointed out | Only what was appended is removed (record: `/var/lib/sp9-platform/`); a file that was created is deleted. The file as it was before the first change is kept as `/var/lib/sp9-platform/kernel-cmdline.orig` |
| Anything else (GRUB without grub.d, rEFInd, Limine and so on) | The values to add are only printed | — |

### Speaker processing

The order and values of the EFX chain of the Windows `SurfaceAPO_1284.json` (audio.md 1.5) are carried over to a PipeWire filter-chain. Left and right have the same values.

| Windows block | Here | Plugin |
|---|---|---|
| `MainEQ` | 2 stages of 2nd-order HPF at 50 Hz, peak −9 dB @ 3763 Hz Q 2.0, −3 dB @ 2502 Hz Q 1.5, +3.7 dB @ 10.5 kHz Q 0.7 | built-in `bq_highpass`, `bq_peaking` |
| `VolumeDepMBDRC4` | Crossover 100/600 Hz (Linkwitz-Riley 4th order; a 100 Hz all-pass in the high band keeps the sum flat). Band 1: +4 dB, then −6 dB 2:1, release 20 ms. Band 2: +4 dB, then −5 dB 1.7:1, release 10 ms. Bands 3 and 4 are pass-through on Windows as well (ratio 1), so the 2750 Hz split is omitted. The values are the same for every volume step, so there is no volume dependence | built-in `bq_lowpass`/`bq_highpass`/`bq_allpass`/`mixer`, LADSPA `sc4` (swh-plugins) |
| `OutputLimiter` | −1 dBFS limiter, release 10 ms (Windows 0 dBFS; 1 dB margin for inter-sample peaks) | LADSPA `fastLookaheadLimiter` (swh-plugins) |
| `VolumeDepLS`, `VolumeDepHS` | Not included. The filter cannot know the sink volume (G4 marks them as omittable) | — |
| `Crystal` (dynamic limiting at 160/240/455/644/844 Hz) | Not included. Neither the built-ins nor swh have a per-band dynamic EQ | — |

Volume control is taken by the "Speakers" filter sink (the default sink). `sp9_speaker_raw` behind it is the ALSA `Master` of the amplifier and has to be at 100 %:
WirePlumber starts a sink without a stored value at 40 % (`Master` −23.25 dB), and then the output is held at −23.9 dB even with the filter sink at 100 %.
[`pipewire/sp9-speaker-raw-volume`](pipewire/sp9-speaker-raw-volume) raises it to 100 % with `wpctl` every time WirePlumber starts, and only while the "Speakers" filter sink exists: without the swh plugins the filter is skipped, the raw sink is the unprotected speaker output, and its volume is left alone. The NixOS module runs it as the user unit
`sp9-speaker-raw-volume.service` (wanted by `wireplumber.service`). `install.sh` does not install it, so on other distributions set up the same unit yourself or run `wpctl set-volume` once.

[`pipewire/pipewire.conf.d/61-sp9-echo-cancel.conf`](pipewire/pipewire.conf.d/61-sp9-echo-cancel.conf) creates a source for calls, "Microphone (echo cancelled)" (`sp9_echo_cancel`).
It is a WebRTC AEC that uses the monitor of `sp9_speaker_raw` as the reference signal; it does not change the default source or the default sink, so it is selected in the application. When not in use it does not wake the microphone and the codec (measured: all nodes suspended).
The NixOS module (`audio.echoCancel`) installs it and `install.sh` does not (put the same file into `/usr/share/pipewire/pipewire.conf.d/`). Call quality is not verified.

The minimum attack of `sc4` is 1.5 ms (Windows 1 ms) and it has no hold. Without swh-plugins the module drops out with `nofail` and the original speaker sink stays as it is.

## Optional components (`--with`)

| Component | File → installed location | What it fixes | Basis | Why optional |
|---|---|---|---|---|
| `hda-pins` | `optional/firmware/sp9-alc274-pins.fw` → `/usr/lib/firmware/`, `optional/modprobe.d/sp9-alc274-pins.conf` → `/usr/lib/modprobe.d/sp9-alc274.conf` | The ALC274 (`10ec:0274`, codec SSID `10ec:1284`) has no fixup, so the pin configuration depends on the BIOS values. This writes the pin defaults of the Windows driver (0x12 DMIC, 0x1b speaker, the rest N/A) with `snd_hda_intel patch=` | audio.md 1.2, G2, G6 | The BIOS values of this device are the same as the Windows table, so it is not needed (`/sys/class/sound/hwC0D0/init_pin_configs`). Use it if a firmware change makes the values differ |
| `audio-legacy` | `optional/modprobe.d/sp9-audio-legacy.conf` → `/usr/lib/modprobe.d/` | `snd_intel_dspcfg dsp_driver=1` selects legacy HDA instead of SOF. The NHLT has no DMIC, so no function is lost | audio.md 2.1, G1 | This device comes up with legacy `snd_hda_intel` without this setting. It is needed only with a firmware on which SOF takes the codec |
| `hibernate-reboot` | `optional/sleep.conf.d/sp9-hibernate-reboot.conf` → `/usr/lib/systemd/sleep.conf.d/` | Reboot instead of power-off after the hibernation image is saved | wiki "Machine Power State", display-power.md section 3 item 9 | The wiki also only says "may be needed", and with Secure Boot lockdown hibernation itself is blocked |

The codec patch file contains the pin defaults that the Windows Realtek driver package (`ehdxsstmd3a4.inf`) defines for codec SSID `10ec1284`, vendor `10ec0274`. The kernel needs `CONFIG_SND_HDA_PATCH_LOADER=y` (enabled in the linux-surface Arch and Debian configurations).

## Dependencies

| Component | Needs |
|---|---|
| `iptsd` | iptsd (the `iptsd` package of the linux-surface repository). The pen keys of the preset take effect only in an iptsd built with the patches of [`userspace/iptsd`](../iptsd/README.md); a stock iptsd ignores them. libinput for the quirk file |
| `sensors` | udev; iio-sensor-proxy for automatic rotation and brightness |
| `speakers` | PipeWire 1.0 or later (built-in `bq_allpass`, `mixer`), WirePlumber 0.5 or later (`wireplumber.conf.d` SPA-JSON rules), swh-plugins (Debian/Arch `swh-plugins`, Fedora `ladspa-swh-plugins`) |
| `initramfs` | One of initramfs-tools, dracut, mkinitcpio (36 or later, `conf.d`) |
| `cmdline` | One of grubby, Debian-style GRUB, kernel-install. Otherwise by hand |
| `hda-*` | `CONFIG_SND_HDA_PATCH_LOADER`, the legacy HDA path (the default path on this device) |

## Not included

- **iptsd `ButtonMinMag`, `TiltDistance`**: not set. With the patches of `userspace/iptsd` the Slim Pen 2 goes through the MPP v2 keys of the preset instead (`Mpp2TiltDistance`, `ButtonToneRatio`, `Mpp2ButtonPurity`; pen-touch.md section 5).
- **libinput/hwdb rules**: the duplicate 0C52 input devices on the kernel side (pen-touch.md section 3 item 12) are something to check on the installed system, and no hwdb entry is added for the accelerometer: in the Windows test SAM-7 the axes came out in the HID sensor standard coordinates (X to the right, Y towards the top of the screen, Z out of the screen; Y≈−0.9 g in upright landscape, X≈+0.6 g at 90° clockwise, Z negative when laid flat), and iio-sensor-proxy takes these coordinates as they are, so the identity matrix should be right (inferred). If the orientation from `monitor-sensor` is wrong on Linux, `ACCEL_MOUNT_MATRIX` is determined then.
- **Switching PSR off on AC** (display-power.md section 3 item 2): the need is inferred, and it requires the debug interface.
- **`bt_link_mask=0`** (audio.md G7): to be added if the error log is confirmed.
- **Microphone processing** (audio.md G5): there is no phase inversion (measured). Beamforming and noise suppression are not included. Only the default gain is set, to +30 dB (`Capture` +30 dB, `Mic Boost` 0 dB), by
  [`wireplumber/wireplumber.conf.d/60-sp9-mic.conf`](wireplumber/wireplumber.conf.d/60-sp9-mic.conf): the WirePlumber default of 100 % is +60 dB, which clips.
  It applies only to accounts without a stored value. The NixOS module (`microphone.enable`) installs it and `install.sh` does not (put the same file into `/usr/share/wireplumber/wireplumber.conf.d/`).
