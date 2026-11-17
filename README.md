# surface-pro-9-linux

Linux support for one device: the Microsoft Surface Pro 9 with an Intel CPU, SKU `Surface_Pro_9_2038`
(tested unit: i5-1235U, 16 GB, 256 GB). The documents abbreviate it as SP9.

- **Scope.** This SKU only. Not the Surface Pro 9 5G (ARM), and not any other Surface device: those belong to
  [linux-surface](https://github.com/linux-surface/linux-surface).
- **Relation to linux-surface.** This is a device-specific layer on top of that project: the kernel is stable
  v7.3.4 + linux-surface's 7.3 patch series + the 23 patches in [`kernel/patches`](kernel/README.md). Around the
  kernel there is a NixOS module, platform files for other distributions, a device helper daemon, pen tools,
  research notes and test results. This repository is not part of linux-surface and is not endorsed by
  linux-surface, Microsoft, Intel or Realtek. Issues about other devices, or about linux-surface's own patches
  and packages, go to linux-surface.
- **State.** NixOS runs on the device with this kernel, dual-booted with Windows 11. Of the 49 tests defined in
  [docs/test-plan.md](docs/test-plan.md): **pass 4, partial 20, fail 1, unverified 24**
  ([docs/test-results.md](docs/test-results.md); the last row of each ID is its current state).
  "Partial" means the part that can be measured was measured and the rest is open. "Unverified" mostly means
  a test that needs a person to touch, look or listen (touch gestures, pen pressure and tilt, buttons, cover
  detach, speaker listening), or a second device (USB-C monitor, Bluetooth headset or mouse), and a few things
  nobody has checked yet (one hour of standby on battery, shutdown, the fan floor left over from Windows).

"Works" is used only for something measured (logs, numbers) or observed by a person. A driver that probed is
not "works".

## Where to start

| You want to | Read |
|---|---|
| Install NixOS on this device | [docs/install-nixos.md](docs/install-nixos.md) |
| Use another distribution | [docs/install-other.md](docs/install-other.md) (this path is untested) |
| Review the kernel patches | [kernel/README.md](kernel/README.md), [kernel/PROVENANCE.md](kernel/PROVENANCE.md) |
| See what each piece of hardware does, in detail | [docs/hardware-status.md](docs/hardware-status.md) |
| Read how Windows drives the hardware and where Linux differs | [docs/research/gaps/](docs/research/gaps/README.md), [docs/research/control/](docs/research/control/) |

## Hardware status

Kernel 7.3.4 with patches 0001–0022, NixOS, firmware UEFI 24.103.143 and SAM 6.603.139 (patch 0023 has run only
as a module loaded into that kernel, for the pen results). The long form, with
the measurements, is [docs/hardware-status.md](docs/hardware-status.md). Test IDs refer to
[docs/test-results.md](docs/test-results.md).

| Hardware | Verdict | In short | Source |
|---|---|---|---|
| Display | Works, with a caveat | Runs PSR1 with no kernel parameter (measured). No flicker noticed in use (observed; not the 10-minute AC/battery test). 120 Hz switching and the lowest brightness steps are unverified | DSP-1 partial, DSP-2 unverified, DSP-3 partial |
| Touch | Unverified | iptsd loads the preset, no ghost touches in 150 s without contact, no ithc errors after 17 suspend cycles (measured). Taps, swipes, palm rejection: nobody has tested them. Wake by touch is not expected to work (inferred) | TOUCH-1, 2, 4 unverified, TOUCH-3 partial, PEN-8 partial |
| Pen (Slim Pen 2) | Partial | Strokes draw (observed); tail button sends three distinct keys (measured); manual pairing works (2/2). Continuous haptics play once `surface-pen-feedback` feeds the pen's speed back to the digitizer (observed). Bonding through the cover's pen slot, and reconnection of a pen bonded that way after its idle disconnect (3.5 s), were measured with patch 0023 loaded as a module; the procedure is run by hand with tools, nothing does it automatically yet, and a pen bonded in pairing mode still does not reconnect. Unverified: pressure, tilt, side button, eraser | PEN-5 partial, PEN-6 pass, PEN-9 partial, PEN-1–4, 7 unverified; [userspace/pen](userspace/pen/README.md), [gaps/pen-vendor-exchange.md](docs/research/gaps/pen-vendor-exchange.md) |
| Front camera | Works | Frames at 640x480, 720p and 1080p requests (measured: 1280x720 120/120 frames, 28.6 fps). The sensor's binned 1296x972 mode does not work and is not offered. Mirroring, privacy LED, image quality against Windows: unverified | CAM-2 pass, CAM-3 fail, CAM-5, 6 unverified |
| Rear camera | Works, focus unverified | Frames at 1080p and 720p (measured). Focus values are written to the lens driver; whether the image focuses is unverified. No autofocus through libcamera | CAM-1 partial, CAM-4 partial |
| IR camera | Does not work | No driver | [gaps/camera.md](docs/research/gaps/camera.md) section 4.1 |
| Speakers | Works, listening unverified | Left and right each produce sound (measured by recording). A PipeWire filter chain is the default sink. Stereo placement and how it sounds are unverified | AUD-1 partial, AUD-2 partial |
| Microphones | Works, calls unverified | Signal on both channels, same polarity (measured). Default gain is set to +30 dB because +60 dB clips. No call test | AUD-3 partial, AUD-4 unverified |
| Type Cover | Unverified; backlight works | Four SAM HID nodes exist. Backlight levels 0–3 reach the LED, off at 0 and on at 3 (observed). Keys, touchpad and detach have no test record | SAM-1 unverified; [docs/helper-reference.md](docs/helper-reference.md) |
| Buttons, tablet mode | Unverified | Input devices exist and their static values match the device state. Not operated | SAM-2, SAM-3 unverified |
| Lid | Works | Close → open on battery: 4/4 suspend and resume, no GPE errors (measured + observed). Not done while charging | PWR-2 partial |
| Ambient light and orientation sensors | Partial | Seven iio devices; samples arrive in buffer mode (measured). Single sysfs reads take over 5 s and return 0. Rotation and covering the sensor are unverified | DSP-4 partial, SAM-7 partial |
| Battery and charger | Partial | Charge, power, cycle count, health and the adapter rating are readable (measured). Plugging and unplugging, and the 80 % limit display, are unverified | SAM-6 partial |
| USB-C | Partial | Two `typec` ports and the partner appear; port and charger PDOs read correctly (measured). Monitor, hotplug and monitor audio are unverified | [PROVENANCE](kernel/PROVENANCE.md) 0019; DSP-5, AUD-6 unverified |
| Fan and temperatures | Works (reading) | 0 rpm idle, 4745–5960 rpm under load, 11 temperature channels (measured). Whether the fan floor left by Windows is cleared is unverified | SAM-5, PWR-9 partial; SAM-4 unverified |
| Performance profiles | Works, with a caveat | With the kernel alone only `low-power` changes power (6 W against 33 W for the other three, measured). With the helper each profile gets its own PL1/PL2/EPP (values written: measured; load comparison: not done). Under a 10-minute load the skin sensor reaches 51 °C; Windows lowers PL1 at 47–48 °C | SAM-5 partial, PWR-9 partial |
| Suspend (s2idle) | Works, standby power unverified | 10/10 suspend and resume by RTC, RTC wake 17/17, 3/3 after audio playback (measured). Residency accrues only in S0i2.0. Power button, one hour on battery and shutdown are unverified. Hibernation is not available: swap is zram only | PWR-1 partial, PWR-3 pass, PWR-6 pass, PWR-4, 5, 7, 8 unverified |
| Wi-Fi | Works | 5 GHz, 80 MHz link, no beacon loss in 854 s, reconnects 3.3 s after resume (measured). Throughput against Windows is unverified | NET-1 partial |
| Bluetooth | Partial | The pen pairs and connects. Mouse, earphones and audio are unverified | NET-2 unverified, AUD-5 unverified |
| Device helper | Works, partly unverified | Start-up, per-profile power limits, fan request, battery, keyboard backlight, PSR policy, iptsd thresholds, ICC, diagnostics confirmed on the device. Pen haptics through the helper, the settings app's individual controls, skin temperature guard and touch-off are unverified | [docs/helper-reference.md](docs/helper-reference.md) |

## Repository layout

| Path | Contents |
|---|---|
| [kernel/](kernel/README.md) | The patch series (v7.3.4 + linux-surface 7.3 + 23 patches), the config fragment, and where each patch comes from ([PROVENANCE.md](kernel/PROVENANCE.md)): 6 upstream backports, 4 from a pending linux-surface pull request, 13 local |
| [flake.nix](flake.nix), [nix/](nix/module.nix) | The NixOS module (`nixosModules.default`, options `hardware.surface-pro-9.*`) and the packages `kernel`, `surface-pen-tools`, `sp9-helper`. Reference: [docs/nixos.md](docs/nixos.md) |
| [userspace/platform/](userspace/platform/README.md) | Platform files for any distribution (iptsd preset, speaker filter chain, initramfs and kernel command line, optional codec pin patch) and `install.sh` |
| [userspace/pen/](userspace/pen/README.md) | Slim Pen 2 tool prototypes: `pen-haptic`, `surface-pen-connect`, `surface-pen-feedback`, and what was seen on the device |
| [helper/](docs/helper.md) | The device helper `sp9-helperd` and `sp9-ctl` (D-Bus) and the GTK settings app `sp9-settings`. Reference: [docs/helper-reference.md](docs/helper-reference.md) |
| [tools/](tools/README.md) | Collection and measurement tools for Linux and Windows |
| [docs/install-nixos.md](docs/install-nixos.md), [docs/install-other.md](docs/install-other.md) | Installation guides |
| [docs/hardware-status.md](docs/hardware-status.md) | Per-hardware status in detail |
| [docs/test-plan.md](docs/test-plan.md), [docs/test-results.md](docs/test-results.md) | The 49 test definitions and one result row per test run |
| [docs/research/gaps/](docs/research/gaps/README.md) | Linux against the Windows baseline, by area: pen and touch (and the pen's haptics, coupling and reconnection), cameras, SAM and platform, audio, display and power |
| [docs/research/control/](docs/research/control/) | How Windows controls each function and how to issue the same control on Linux: power and thermal, standby, display, battery and charging, input, Type Cover, camera |
| [docs/hardware.md](docs/hardware.md), [docs/reference/](docs/reference/) | Hardware inventory and firmware versions read on Windows, and the device lists behind it |

## Licence

Licensing is per file and follows the REUSE convention. [LICENSE](LICENSE) has the overview,
[REUSE.toml](REUSE.toml) and the SPDX headers in the files are authoritative, the licence texts are in
[LICENSES/](LICENSES/), and attributions and trademarks are in [NOTICE](NOTICE).

| Path | Licence |
|---|---|
| `kernel/patches/`, `kernel/config/` | GPL-2.0-only. The patches are changes to the Linux kernel and keep their authors and sign-offs |
| `helper/` | GPL-3.0-or-later |
| `userspace/`, `tools/`, `nix/`, `flake.nix` | MIT |
| Documentation (every `*.md` file and `docs/`) | CC-BY-SA-4.0; code and commands quoted in the documents may also be used under MIT |

## Warning

This repository changes the kernel, drivers that talk to the device's firmware (the Surface Aggregator Module,
the touch controller, the cameras' power control), and CPU power limits, on one specific device. Seventeen of the
23 patches are not in mainline Linux (4 from a pending linux-surface pull request, 13 local), and most of the
local ones are marked unverified in [PROVENANCE.md](kernel/PROVENANCE.md). Some helper
functions are experimental and off by default (fan request, skin temperature guard). Use it at your own risk;
there is no warranty. Keep Windows installed: firmware updates are delivered only through Windows.

This repository was written with the help of an LLM under the author's direction; statements are marked by
evidence level (measured, observed, inferred, unverified).
