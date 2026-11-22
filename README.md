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
    The IR camera was measured with 0029 loaded as a module, then on booted kernels built with 0001–0029 and
    0001–0031.
    Unverified or untested means there is no record yet, not that the part fails.

| Hardware | Verdict | In short | Source |
|---|---|---|---|
| Display | Works[^display-open] | Runs PSR1 with no kernel parameter when the desktop's VRR is off; with VRR on, i915 keeps PSR off (measured). With PSR1 on battery, no flicker over 5 minutes each at 120 and 60 Hz (observed).[^display] 60↔120 Hz switching works, with the screen dark for about 1.1 s each time (measured). The brightness keys go down to 2 % in 20 steps with no flicker (observed) | DSP-1 pass, DSP-2 pass, DSP-3 pass |
| Touch | Works[^touch-open] | Touch works with `sp9-digitizerd` in daily use in Xournal++ and Rnote (observed). Light taps 10/10 and three-finger swipes 5/5 (1/10 and 0/5 with the iptsd defaults), no touch from a palm while the pen hovers, touch and pen back after every resume checked (measured). No ghost touches and no ithc errors with iptsd (measured)[^touch] | TOUCH-1, 2, 3, 4 pass, PEN-8 pass; [docs/digitizerd.md](docs/digitizerd.md) |
| Pen (Slim Pen 2) | Works[^pen-open] | Writing, the barrel button and the eraser end work in a drawing application (observed).[^pen-daemons] Position and tilt match the Windows touch processor (measured by replay);[^pen-replay] tilt held by hand reads the right direction and 39–48° at about 45° (measured). Tail button sends three distinct keys (measured); manual pairing works (2/2). With the device helper: continuous haptics while drawing (observed), cover-slot bonding (measured, 12/12) and reconnection after the idle disconnect (measured); the battery level is read.[^pen-helper] The eraser end plays one click, no continuous waveform, as on Windows | PEN-1 to 9 pass; [userspace/pen](userspace/pen/README.md), [gaps/pen-vendor-exchange.md](docs/research/gaps/pen-vendor-exchange.md) |
| Front camera | Works | Frames at 640x480, 720p and 1080p requests (measured).[^front-frames] No purple lines and the exposure settles (measured; observed in a camera application).[^front-patches] While a camera streams, the privacy LED on its side lights (observed, front and rear). The sensor's binned 1296x972 mode does not work and is not offered. Mirroring and image quality against Windows: unverified; colours are uncalibrated | CAM-2 pass, CAM-7 pass, CAM-5 pass, CAM-3 fail, CAM-6 unverified |
| Rear camera | Works | Frames at 4K, 1080p and 720p (measured); the exposure settles.[^rear-patches] The image is upright and sharp, and autofocus focuses (observed).[^rear-af] The scale between lens position and distance is not measured; colours are uncalibrated | CAM-1 pass, CAM-4 pass |
| IR camera | Works with this repository's kernel only[^ir-open] | Frames as a 10-bit monochrome camera (644×604) through libcamera, with no CSI-2 errors (measured). The IR illuminator lights a dark room (observed).[^ir-driver] Applications keep the front camera as the default[^ir-priority] | CAM-8 pass; [gaps/camera.md](docs/research/gaps/camera.md) difference 8 |
| Speakers | Works; the speaker filter does not limit the 100 Hz–1 kHz range[^speaker-limit] | Left and right each produce sound (measured by recording). A PipeWire filter chain built from this device's measurements (high-pass, a cut at the 3.7 kHz distortion peak, limiter) is the default sink. With it, music has no audible distortion at 25–100 % volume and left and right are correct (observed)[^speaker-listen] | AUD-1 pass, AUD-2 pass; [gaps/audio.md](docs/research/gaps/audio.md) G4 |
| Microphones | Works; the optional echo-cancelled source cuts the near-end voice while the speakers play | Signal on both channels, same polarity (measured). Default gain is set to +30 dB because +60 dB clips; speech from the front does not clip at that gain (measured). An echo-cancelled source for calls is available, not the default (measured)[^mic-calls] | AUD-3 partial, AUD-4 partial |
| Type Cover | Works | Keys, touchpad and the function row work. Over 6 detach and attach cycles the keyboard and touchpad went away and came back each time (measured), and worked after each attach (observed).[^cover-ubuntu] Backlight levels 0–3 reach the LED (observed)[^cover-backlight][^cover-open] | SAM-1 pass; [docs/helper-reference.md](docs/helper-reference.md) |
| Buttons, tablet mode (cover detached) | Works | With the Type Cover detached, the power and volume buttons work (observed). The power button wakes the device from s2idle (8/8), and the button devices were registered in all 24 boots checked (measured).[^tablet-mode] The tablet mode switch reads 1 with the cover detached or folded back behind the screen and 0 with it attached and open, and a compositor reads it through libinput (measured) | SAM-3 pass, SAM-2 pass |
| Lid | Works | Close → open on battery: 4/4 suspend and resume, no GPE errors (measured + observed). While charging: 5/5, the screen off as the cover closes and on when it opens (measured + observed), with one SAM serial parser line at each resume and no further errors. With an external monitor plugged in, logind ignores the lid (docked) | PWR-2 pass |
| Ambient light and orientation sensors | Works | The screen follows the device (observed).[^rotation] The light sensor reports about 81 lux in room light and drops from about 600 lux to below 1 lux when covered (measured)[^light-polls][^auto-brightness] | SAM-7 pass, DSP-4 pass |
| Battery and charger | Works | Charge, power, cycle count, health and the adapter rating are readable (measured); the battery charges from the 45 W adapter on either USB-C port, and plugging and unplugging is followed every time (6 cycles, measured)[^battery] | SAM-6 pass |
| USB-C | Works | An external display works on both USB-C ports (observed).[^usbc-monitor] Two screens over DP MST on either port, with no GPE errors on hotplug, and audio through the monitor work (measured + observed). Charging works, and the port and charger PDOs read correctly (measured)[^usbc-open] | [PROVENANCE](kernel/PROVENANCE.md) 0019; DSP-5 pass, AUD-6 pass |
| Fan and temperatures | Works (reading) | 0 rpm idle, 4745–5960 rpm under load, 11 temperature channels (measured). No fan floor is left behind after Windows (measured)[^fan-floor] | SAM-4 pass, SAM-5 pass, PWR-9 partial |
| Performance profiles | Works; under sustained load the back runs hotter than on Windows | With the kernel alone only `low-power` changes power (measured).[^profiles-kernel] With the helper each profile gets its own PL1/PL2/EPP.[^profiles-helper] Under a 10-minute load the skin sensor reaches 51 °C; Windows lowers PL1 at 47–48 °C[^skin-guard] | SAM-5 pass, PWR-9 partial |
| Suspend (s2idle) | Works; standby on battery draws about 190 mW, 2–4 times Windows, and only the shallower S0i2.0 state is reached | 10/10 suspend and resume by RTC on AC and 10/10 on battery, RTC wake 17/17, 3/3 after audio playback (measured). One hour on battery: about 190 mW, `slp_s0` residency 99.86 %; Windows: 52–102 mW at 99.8–99.9 % (measured). Residency accrues only in S0i2.0; S0i3.0 stays 0. Windows spends 97–99 % of standby in its deepest idle state. With the screen off outside suspend the package is in PC10 20 % of the time (measured). Waking with the power button 8/8 (measured). Shutdown and reboot complete (24 boots in the journal, measured + observed)[^hibernation] | PWR-1 pass, PWR-3 pass, PWR-4 pass, PWR-5 pass, PWR-6 pass, PWR-7 pass, PWR-8 unverified |
| Wi-Fi | Works | 5 GHz, 80 MHz link, no beacon loss in 854 s, reconnects 3.3 s after resume (measured). Throughput against Windows is unverified | NET-1 partial |
| Bluetooth | Works | The pen pairs, bonds through the cover and reconnects (measured, observed). A mouse works, and an earset plays over A2DP (AAC) and records over HFP (mSBC); both come back after each resume (measured + observed). Bluetooth also worked in the Ubuntu, Debian and Arch runs (observed)[^bt-open] | PEN-5, 6 pass; NET-2, AUD-5 pass |
| Device helper | Works | Most functions confirmed on the device[^helper-confirmed][^helper-open] | [docs/helper-reference.md](docs/helper-reference.md) |

[^display-open]: The colour of the ICC profile is untested, and whether PSR saves power on this panel is not
    resolved: screen-on idle measurements vary by more than the difference.

[^display]: With VRR off the panel self-refreshes on a static screen (87 % of samples at 60 Hz). On AC no flicker
    was noticed in use.

[^touch-open]: Touch does not wake the device from suspend, as expected (TOUCH-4); whether Windows wakes by
    touch is not known.

[^touch]: 150 s without contact, no ghost touches; no ithc errors after 17 suspend cycles.

[^pen-open]: The screen edges and the palm are unverified. After erasing, one drawing application (Rnote) may
    keep showing the eraser cursor although the daemon reports the eraser as gone; that is the application's and
    the desktop's cursor handling (Xournal++ shows no hover cursor at all).

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

[^ir-open]: Not in `sp9-dkms`, so not on the packaged routes. Face sign-in is tested, not provided: howdy 3.0.0
    without PAM recognised a face 2/2 with three enrolled models (one model was not enough) and rejected an empty
    or covered view; no spoofing test, not wired into PAM.

[^ir-driver]: ST's VD55G0 driver as adapted in a pending linux-surface pull request (patch 0028), with this
    repository's ipu-bridge entry, illuminator patch and IPU6 capture fix (0027, 0029–0031). 10-bit is the
    default; 8-bit output is offered too.

[^ir-priority]: PipeWire node priorities: front 800, rear 750, IR 700 (measured).

[^speaker-limit]: The filter leaves 100 Hz–1 kHz unchanged; a full-scale 100 Hz tone has 72 % THD on the raw
    speaker sink (measured, [gaps/audio.md](docs/research/gaps/audio.md) G4).

[^speaker-listen]: Switching the filter off and on at 100 % makes no large audible difference; it makes nothing
    worse (AUD-2).

[^mic-calls]: Its name is "Microphone (echo cancelled)" and it is chosen per application. With the speakers
    playing speech it removes the echo down to its noise floor, about 60 dB below the raw echo (measured), but
    speech from the near end during playback comes through almost entirely cut (observed, AUD-4). A real call is
    untested. Windows also applies beamforming to
    the two microphones; Linux does not.

[^cover-ubuntu]: Also seen once on Ubuntu 26.04 with `sp9-dkms`. Four SAM HID nodes exist.

[^cover-backlight]: Off at 0 and on at 3.

[^cover-open]: The backlight key was not tried in the detach cycles (SAM-1). Continuing a two-finger scroll at the touchpad
    edge, as Windows does, is left to the compositor.

[^tablet-mode]: The Surface Pro 9 has no 360° fold: tablet use means the keyboard detached. The buttons were not
    pressed after each reboot; instead the journal of 24 boots shows the button devices registered every time
    (SAM-3). A compositor that takes the power key may turn the screen off on a press instead of suspending;
    that is the desktop's setting (PWR-1).

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

[^usbc-open]: Unverified: USB devices through the ports.

[^fan-floor]: After a load under Windows and a reboot the fan ran at about 3000 rpm while warm and stopped at
    38.8 °C. Not compared with a kernel without patch 0013.

[^profiles-kernel]: 6 W against 33 W for the other three.

[^profiles-helper]: Values written: measured; load comparison: not done.

[^skin-guard]: The helper's skin temperature guard, which lowers the limits by that sensor, is experimental and
    off by default.

[^hibernation]: Hibernation is a configuration choice: it needs disk swap, and the tested set-up has zram only, so
    it was not tested (PWR-8).

[^bt-open]: Waking the device from suspend with a Bluetooth mouse does not work (observed).

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
| Ubuntu 26.04, Debian 13 | [install-debian-ubuntu.md](docs/install-debian-ubuntu.md) | Supported |
| Arch Linux | [install-arch.md](docs/install-arch.md) | Works; best effort |
| Ubuntu 24.04 | [install-debian-ubuntu.md](docs/install-debian-ubuntu.md) | Works without cameras; not a target |
| Ubuntu 26.10, Debian testing | [install-debian-ubuntu.md](docs/install-debian-ubuntu.md) | Best effort; tested in containers only |
| Any other | [install-from-source.md](docs/install-from-source.md) | Unverified |

A linux-surface kernel release newer than `sp9-dkms` runs without this project's kernel patches until `sp9-dkms`
covers it ([kernel updates](docs/install.md#kernel-updates)).

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

[^patch-origins]: On v7.3.4 + linux-surface 7.3: upstream backports (0001–0006), patches from pending linux-surface
    pull requests (0007–0010, and 0028, ST's VD55G0 driver), and this repository's own (the rest).

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
