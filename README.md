# surface-pro-9-linux

Linux on the Microsoft Surface Pro 9 with an Intel CPU (SKU `Surface_Pro_9_2038`, SP9 in these documents): kernel
patches, packages and a NixOS module on top of [linux-surface](https://github.com/linux-surface/linux-surface).[^scope]
Website: <https://joonhoekim.github.io/surface-pro-9-linux/>.

**Status: maintenance.** Kept working on the author's device; no schedule for new kernels. Contributions welcome
([CONTRIBUTING.md](CONTRIBUTING.md)).

Keeping Windows installed is recommended: Surface firmware updates come through Windows Update.

[^scope]: Only this SKU: not the Surface Pro 9 5G (ARM) or other Surface devices, which belong to linux-surface, as
    do issues about linux-surface's own patches and packages. Not part of, or endorsed by, linux-surface,
    Microsoft, Intel or Realtek.

## Hardware status

Measured on NixOS with this repository's kernel.[^conditions] Details: [docs/hardware-status.md](docs/hardware-status.md);
test IDs: [docs/test-results.md](docs/test-results.md).

[^conditions]: One unit (i5-1235U, 16 GB, 256 GB); kernel 7.3.4 with patches 0001–0026, firmware UEFI 24.103.143
    and SAM 6.603.139. Most results were measured on kernels with 0001–0022; the later patches matter only for the
    pen's cover coupling and reconnection (0023, 0024) and for the gain range of the camera sensors (0025, 0026).
    Unverified or untested means there is no record yet, not that the part fails.

| Hardware | Verdict | In short | Source |
|---|---|---|---|
| Display | Works[^display-open] | Runs PSR1 with no kernel parameter (measured). No flicker noticed in use (observed).[^display] 60↔120 Hz switching works, with the screen dark for about 1.1 s each time (measured). Brightness steps 0 and 1 leave the screen dark (observed) | DSP-1 partial, DSP-2 pass, DSP-3 partial |
| Touch | Works[^touch-open] | Touch works with `sp9-digitizerd` in daily use in Xournal++ and Rnote (observed). No ghost touches and no ithc errors with iptsd (measured)[^touch] | TOUCH-1 partial, TOUCH-3 partial, TOUCH-2, 4 unverified, PEN-8 partial; [docs/digitizerd.md](docs/digitizerd.md) |
| Pen (Slim Pen 2) | Works[^pen-open] | Writing, the barrel button and the eraser end work in a drawing application (observed).[^pen-daemons] Position and tilt match the Windows touch processor (measured by replay).[^pen-replay] Tail button sends three distinct keys (measured); manual pairing works (2/2). With the device helper: continuous haptics while drawing (observed), cover-slot bonding (measured, 12/12) and reconnection after the idle disconnect (measured); the battery level is read.[^pen-helper] The eraser end plays one click, no continuous waveform, as on Windows | PEN-3, 5, 6, 9 pass, PEN-1, 4, 8 partial, PEN-2, 7 unverified; [userspace/pen](userspace/pen/README.md), [gaps/pen-vendor-exchange.md](docs/research/gaps/pen-vendor-exchange.md) |
| Front camera | Works | Frames at 640x480, 720p and 1080p requests (measured).[^front-frames] No purple lines and the exposure settles (measured; observed in a camera application).[^front-patches] While a camera streams, the privacy LED on its side lights (observed, front and rear). The sensor's binned 1296x972 mode does not work and is not offered. Mirroring and image quality against Windows: unverified; colours are uncalibrated | CAM-2 pass, CAM-7 pass, CAM-5 pass, CAM-3 fail, CAM-6 unverified |
| Rear camera | Works | Frames at 4K, 1080p and 720p (measured); the exposure settles.[^rear-patches] The image is upright and sharp, and autofocus focuses (observed).[^rear-af] The scale between lens position and distance is not measured; colours are uncalibrated | CAM-1 pass, CAM-4 pass |
| IR camera | Does not work: no Linux driver for the VD55G0 sensor (Windows uses it for Hello face sign-in) | The mainline `vd55g1` driver does not list this sensor, and with no driver to power it the sensor does not answer | [gaps/camera.md](docs/research/gaps/camera.md) section 4.1 and item 8 |
| Speakers | Works; the speaker filter does not limit the 100 Hz–1 kHz range[^speaker-limit] | Left and right each produce sound (measured by recording). A PipeWire filter chain built from this device's measurements (high-pass, a cut at the 3.7 kHz distortion peak, limiter) is the default sink[^speaker-listen] | AUD-1 partial, AUD-2 partial; [gaps/audio.md](docs/research/gaps/audio.md) G4 |
| Microphones | Works | Signal on both channels, same polarity (measured). Default gain is set to +30 dB because +60 dB clips (measured). An echo-cancelled source for calls is available, not the default (measured)[^mic-calls] | AUD-3 partial, AUD-4 partial |
| Type Cover | Works | Keys, touchpad and the function row work. Over 6 detach and attach cycles the keyboard and touchpad went away and came back each time (measured), and worked after each attach (observed).[^cover-ubuntu] Backlight levels 0–3 reach the LED (observed)[^cover-backlight][^cover-open] | SAM-1 pass; [docs/helper-reference.md](docs/helper-reference.md) |
| Buttons, tablet mode (cover detached) | Works | With the Type Cover detached, the power and volume buttons work (observed). The power button wakes the device from s2idle (5/5, measured).[^tablet-mode] The tablet mode switch reads 1 with the cover detached or folded back behind the screen and 0 with it attached and open, and a compositor reads it through libinput (measured) | SAM-3 partial, SAM-2 pass |
| Lid | Works | Close → open on battery: 4/4 suspend and resume, no GPE errors (measured + observed). While charging: 1/1, the screen off as the cover closes and on when it opens (measured + observed), with one SAM serial parser line at that resume and no further errors | PWR-2 partial |
| Ambient light and orientation sensors | Works | The screen follows the device (observed).[^rotation] The light sensor reports about 81 lux in room light and drops from about 600 lux to below 1 lux when covered (measured)[^light-polls][^auto-brightness] | SAM-7 pass, DSP-4 pass |
| Battery and charger | Works | Charge, power, cycle count, health and the adapter rating are readable (measured); the battery charges from the 45 W adapter on either USB-C port, and plugging and unplugging is followed every time (6 cycles, measured)[^battery] | SAM-6 pass |
| USB-C | Works | An external display works on both USB-C ports (observed).[^usbc-monitor] Charging works, and the port and charger PDOs read correctly (measured)[^usbc-open] | [PROVENANCE](kernel/PROVENANCE.md) 0019; DSP-5 partial, AUD-6 unverified |
| Fan and temperatures | Works (reading) | 0 rpm idle, 4745–5960 rpm under load, 11 temperature channels (measured). No fan floor is left behind after Windows (measured)[^fan-floor] | SAM-4 pass; SAM-5, PWR-9 partial |
| Performance profiles | Works; under sustained load the back runs hotter than on Windows | With the kernel alone only `low-power` changes power (measured).[^profiles-kernel] With the helper each profile gets its own PL1/PL2/EPP.[^profiles-helper] Under a 10-minute load the skin sensor reaches 51 °C; Windows lowers PL1 at 47–48 °C[^skin-guard] | SAM-5 partial, PWR-9 partial |
| Suspend (s2idle) | Works; standby drain not measured, and only the shallower S0i2.0 state is reached | 10/10 suspend and resume by RTC, RTC wake 17/17, 3/3 after audio playback (measured). Residency accrues only in S0i2.0; S0i3.0 stays 0. Windows spends 97–99 % of standby in its deepest idle state at 49–52 mW. With the screen off outside suspend the package is in PC10 20 % of the time (measured). Waking with the power button 5/5 (measured). One hour on battery and shutdown are unverified[^hibernation] | PWR-1 partial, PWR-3 pass, PWR-5 pass, PWR-6 pass, PWR-4, 7, 8 unverified |
| Wi-Fi | Works | 5 GHz, 80 MHz link, no beacon loss in 854 s, reconnects 3.3 s after resume (measured). Throughput against Windows is unverified | NET-1 partial |
| Bluetooth | Works | The pen pairs, bonds through the cover and reconnects (measured, observed); Bluetooth also worked in the Ubuntu, Debian and Arch runs (observed)[^bt-open] | PEN-5, 6 pass; NET-2, AUD-5 unverified |
| Device helper | Works | Most functions confirmed on the device[^helper-confirmed][^helper-open] | [docs/helper-reference.md](docs/helper-reference.md) |

[^display-open]: The brightness keys and flicker while adjusting (DSP-3) are untested.

[^display]: Not the 10-minute AC/battery test.

[^touch-open]: Untested: counted light taps and swipes against the iptsd defaults (TOUCH-1), palm rejection
    (TOUCH-2), and touch after every resume (TOUCH-3). Wake by touch is not expected to work (inferred, TOUCH-4);
    whether Windows wakes by touch is not known.

[^touch]: 150 s without contact, no ghost touches; no ithc errors after 17 suspend cycles.

[^pen-open]: Tilt against real angles is unverified (PEN-2). After erasing, the drawing application may keep
    showing the eraser cursor; that cursor is the application's.

[^pen-daemons]: With iptsd and this repository's iptsd patches or with `sp9-digitizerd`.

[^pen-replay]: Against the Windows touch processor on the same raw captures: position within 0.04–0.05 mm and tilt
    within 2° in the median.

[^pen-helper]: The helper feeds the pen's speed back to the digitizer for the continuous haptics. Bonding a pen
    put into the cover's pen slot uses kernel patch 0023, and needs the pen coupled through the cover once (on
    Windows, with the bond imported, or earlier through the helper) or a user-supplied coupling prefix; pairing
    mode (hold the tail button) always works.

[^front-frames]: 1280x720: 120/120 frames, 28.6 fps.

[^front-patches]: With the patched libcamera 0.8.0 and patch 0025.

[^rear-patches]: With patch 0026 and the patched libcamera.

[^rear-af]: The patched libcamera's contrast autofocus.

[^speaker-limit]: The filter leaves 100 Hz–1 kHz unchanged; a full-scale 100 Hz tone has 72 % THD on the raw
    speaker sink (measured, [gaps/audio.md](docs/research/gaps/audio.md) G4).

[^speaker-listen]: Stereo placement and how the filter sounds are unverified: no listening test with the current filter yet.

[^mic-calls]: Its name is "Microphone (echo cancelled)" and it is chosen per application. With the speakers
    playing speech it removes the echo down to its noise floor, about 60 dB below the raw echo (measured). Speech
    from the near end during playback and a real call are untested (AUD-4). Windows also applies beamforming to
    the two microphones; Linux does not.

[^cover-ubuntu]: Also seen once on Ubuntu 26.04 with `sp9-dkms`. Four SAM HID nodes exist.

[^cover-backlight]: Off at 0 and on at 3.

[^cover-open]: The backlight key was not tried in the detach cycles (SAM-1). Continuing a two-finger scroll at the touchpad
    edge, as Windows does, is left to the compositor.

[^tablet-mode]: The Surface Pro 9 has no 360° fold: tablet use means the keyboard detached. Presses after each of
    five reboots (SAM-3) are unverified. A compositor that takes the power key may turn the screen off on a press
    instead of suspending; that is the desktop's setting (PWR-1).

[^rotation]: With `sensors.enable` (the default), in a compositor that reads iio-sensor-proxy. The udev rule sets
    the accelerometer's change sensitivity to 0, without which a device at rest reads (0, 0, 0) (measured).

[^light-polls]: 2 of 35 sysfs polls gave a value. The others read 0 because the sensor hub reports only when a
    reading changes, so a poll of a device at rest returns 0; iio-sensor-proxy takes the value from the hub's
    reports.

[^auto-brightness]: Automatic brightness is unverified.

[^battery]: The 80 % charge limit is a UEFI setting that applies to Windows and Linux alike; the helper only reads
    it. With smart charging engaged, charging stopped at 79 % and the status then reads `Not charging` (measured). At 100 % on the charger the status reads `Charging`, not `Full` (cosmetic). Plugged in at the smart charging stop
    the status reads `Unknown` for about a second and then `Not charging` (SAM-6).

[^usbc-monitor]: One portable monitor with a built-in hub (model E1730A) showed no picture, on Windows as well,
    so the cause is that monitor or its set-up. On Linux i915 trained the link at every rate tried (RBR x4, HBR
    x2, HBR2 x1 and x4) with the monitor in SST mode.

[^usbc-open]: Unverified: daisy-chained monitors over DP MST, USB devices through the ports, audio over the monitor
    (AUD-6), GPE counters during hotplug (DSP-5).

[^fan-floor]: After a load under Windows and a reboot the fan ran at about 3000 rpm while warm and stopped at
    38.8 °C. Not compared with a kernel without patch 0013.

[^profiles-kernel]: 6 W against 33 W for the other three.

[^profiles-helper]: Values written: measured; load comparison: not done.

[^skin-guard]: The helper's skin temperature guard, which lowers the limits by that sensor, is experimental and
    off by default.

[^hibernation]: Hibernation is a configuration choice: it needs disk swap, and the tested set-up has zram only, so
    it was not tested (PWR-8).

[^bt-open]: Mouse, earphones and Bluetooth audio are unverified (NET-2, AUD-5).

[^helper-confirmed]: Confirmed on the device: start-up, per-profile power limits, fan request, battery, keyboard
    backlight, PSR policy, iptsd thresholds, ICC, diagnostics, pen haptics with speed feedback and strength, the
    eraser click, cover coupling, and pen settings handed to `sp9-digitizerd` without a restart.

[^helper-open]: Unverified: automatic coupling with the current order, replacing a dead bond, the settings app's
    individual controls, the skin temperature guard and touch-off. Each function's state is in
    [docs/helper-reference.md](docs/helper-reference.md).


## Install

Start with [preparing the device](docs/install-prepare.md), then follow the page for your distribution.
[docs/install.md](docs/install.md) has the support details, what an installation consists of and the checks
afterwards.

| Distribution | Guide | Support |
|---|---|---|
| NixOS | [install-nixos.md](docs/install-nixos.md) | Supported |
| Ubuntu 26.04, Debian 13 | [install-debian-ubuntu.md](docs/install-debian-ubuntu.md) | Supported[^repos] |
| Arch Linux | [install-arch.md](docs/install-arch.md) | Works; best effort[^repos] |
| Ubuntu 24.04 | [install-debian-ubuntu.md](docs/install-debian-ubuntu.md) | Works without cameras; not a target |
| Ubuntu 26.10, Debian testing | [install-debian-ubuntu.md](docs/install-debian-ubuntu.md) | Best effort; tested in containers only |
| Any other | [install-from-source.md](docs/install-from-source.md) | Unverified |

A linux-surface kernel release newer than `sp9-dkms` runs without this project's kernel patches until `sp9-dkms`
covers it ([kernel updates](docs/install.md#kernel-updates)).

[^repos]: The apt and pacman repositories are published with the first release; until then, build the packages
    locally.

## Where details live

| You want to | Read |
|---|---|
| Install | [docs/install.md](docs/install.md), then the page for your distribution |
| See what each piece of hardware does, in detail | [docs/hardware-status.md](docs/hardware-status.md); tests in [docs/test-plan.md](docs/test-plan.md) and [docs/test-results.md](docs/test-results.md) |
| Use the device helper, the settings app or the touch and pen daemon | [docs/helper.md](docs/helper.md), [docs/helper-reference.md](docs/helper-reference.md), [docs/digitizerd.md](docs/digitizerd.md) |
| Configure the NixOS module | [docs/nixos.md](docs/nixos.md) |
| Build or change the packages | [packaging/README.md](packaging/README.md), [dkms/README.md](dkms/README.md) |
| Review the kernel patches | [kernel/README.md](kernel/README.md), [kernel/PROVENANCE.md](kernel/PROVENANCE.md) |
| Read how Windows drives the hardware and where Linux differs | [docs/research/gaps/](docs/research/gaps/README.md), [docs/research/control/](docs/research/control/) |
| See what changed between versions | [CHANGELOG.md](CHANGELOG.md), [docs/releasing.md](docs/releasing.md) |
| Contribute a test result or a change | [CONTRIBUTING.md](CONTRIBUTING.md) |

### Repository layout

| Path | Contents |
|---|---|
| [kernel/](kernel/README.md) | The patch series, the config fragment, and where each patch comes from ([PROVENANCE.md](kernel/PROVENANCE.md))[^patch-origins] |
| [dkms/](dkms/README.md) | The kernel patches that can be built as modules, as DKMS packages for linux-surface's kernel |
| [packaging/](packaging/README.md) | Debian, Ubuntu and Arch packages, their CI build, install test and signed repositories[^package-list] |
| [flake.nix](flake.nix), [nix/](nix/module.nix) | The NixOS module (`nixosModules.default`, options `hardware.surface-pro-9.*`) and the flake packages ([docs/nixos.md](docs/nixos.md))[^flake-packages] |
| [userspace/platform/](userspace/platform/README.md) | Platform files for any distribution and `install.sh`[^platform] |
| [userspace/iptsd/](userspace/iptsd/README.md) | Patches for iptsd's pen processing (position, tilt, contact, pressure, filtering), turned on by the preset |
| [userspace/libcamera/](userspace/libcamera/README.md) | Patches for libcamera 0.8.0 (simple pipeline and software ISP: purple lines, exposure, rear autofocus) |
| [userspace/pen/](userspace/pen/README.md) | Slim Pen 2 tool prototypes: `pen-haptic`, `surface-pen-connect`, `surface-pen-feedback`, and what was seen on the device |
| [helper/](docs/helper.md) | The device helper `sp9-helperd` and `sp9-ctl` (D-Bus), the GTK settings app `sp9-settings`, and the touch and pen daemon `sp9-digitizerd` ([docs/digitizerd.md](docs/digitizerd.md)), derived from iptsd |
| [tools/](tools/README.md) | Collection and measurement tools for Linux and Windows |
| [docs/](docs/install.md) | Installation, hardware status, tests, the helper, and the research: [gaps/](docs/research/gaps/README.md) (Linux against the Windows baseline, by area) and [control/](docs/research/control/) (how Windows controls each function and how to do the same on Linux) |
| [docs/hardware.md](docs/hardware.md), [docs/reference/](docs/reference/) | Hardware inventory and firmware versions read on Windows, and the device lists behind it |

[^patch-origins]: v7.3.4 + linux-surface 7.3 + 26 patches: 6 upstream backports, 4 from a pending linux-surface
    pull request, 16 local.

[^package-list]: `sp9`, `sp9-helper`, `sp9-settings`, `sp9-digitizerd`, `sp9-platform`, `sp9-cmdline`, keyrings,
    the experimental `sp9-libcamera`.

[^flake-packages]: `kernel`, `surface-pen-tools`, `sp9-helper`, `sp9-digitizerd`, `libcamera`, `pipewire`,
    `wireplumber`.

[^platform]: iptsd preset, speaker filter chain, initramfs and kernel command line, optional codec pin patch.

## Licence

Licensing is per file and follows the REUSE convention. [LICENSE](LICENSE) has the overview,
[REUSE.toml](REUSE.toml) and the SPDX headers in the files are authoritative, the licence texts are in
[LICENSES/](LICENSES/), and attributions and trademarks are in [NOTICE](NOTICE).

| Path | Licence |
|---|---|
| `kernel/patches/`, `kernel/config/` | GPL-2.0-only. The patches are changes to the Linux kernel and keep their authors and sign-offs |
| `helper/` | GPL-3.0-or-later. `sp9-digitizer` and `sp9-digitizerd` are derived from iptsd (GPL-2.0-or-later) |
| `userspace/iptsd/` patches | GPL-2.0-or-later, as iptsd |
| `userspace/libcamera/` patches | The licence of each libcamera file they change: LGPL-2.1-or-later, BSD-2-Clause (one shader, 0002), CC0-1.0 (build and tuning files) |
| `userspace/` (everything else), `tools/`, `nix/`, `packaging/`, `flake.nix` | MIT |
| Documentation (every `*.md` file and `docs/`) | CC-BY-SA-4.0; code and commands quoted in the documents may also be used under MIT |


The code and documents were written with the help of an LLM under the author's direction.
