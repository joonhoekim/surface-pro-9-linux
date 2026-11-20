# Device helper (sp9-helperd)

A system service and a command-line tool for the device-specific settings of the Surface Pro 9 (Intel) that a
desktop does not handle. It exports over **D-Bus**: per-profile power limits, battery and smart charging, the PSR
policy and the panel ICC profile, the Type Cover backlight, touch and iptsd settings, the pen (haptics, speed feedback to the digitizer, coupling
through the cover), and a diagnostics report. The structure follows the author's Open Device Helper for another device, the TB323FU (public repository
`tb323fu-linux`), and it uses the same bus name.
The code is in [`helper/`](../helper/); the kernel interfaces, the D-Bus API, the polkit actions and the
verification status of each feature are in [helper-reference.md](helper-reference.md).

It runs on this device under NixOS. The status of each feature is the status column of the reference.
The evidence for each feature is in `docs/research/control/*.md`, and only what was confirmed on the device is
marked "works" in the status column.

| Part | Status |
|---|---|
| `sp9-helperd` (system D-Bus service), `sp9-ctl` (CLI) | Works on the device: start-up, per-profile PL and EPP and overrides, fan reading and fan request, battery and smart-charging reading, keyboard backlight, PSR policy, iptsd thresholds, ICC, diagnostics, pen haptics and speed feedback (vibration while drawing, stop on lift: observed), cover coupling (automatic and manual, up to the pen's HID without restarting bluetoothd; a bonded pen that is not connected is probed and comes back with its bond kept: measured). The strength (30 % is not felt and 200 % is clearly stronger than 100 %: observed) and the eraser click (observed). Unverified: the pen parts changed afterwards (`8e a5` frames as Windows sends them, the choice of inking waveform (default brush), the speed scale of 300 mm/s, the dead zone, replacing a dead bond after Windows coupled the pen: "Pen" below), zero-force contact through the helper with iptsd (with `sp9-digitizerd` a changed value reaches the daemon without a restart: measured), pen position prediction through the helper, the skin temperature guard, touch off while the screen is off (only switching the setting on and off was seen), smart-charging release |
| `sp9-settings` (GTK4/libadwaita settings app) | The window opens on the device (observed). Operating the individual items is unverified |

What it does not do:

- **Booting does not depend on the helper.** The kernel command line, the initramfs, the iptsd preset and the
  speaker processing belong to the [platform files](../userspace/platform/README.md) and the NixOS module.
- It adds no drivers. It uses only interfaces the kernel already provides (sysfs, debugfs, efivarfs, hidraw,
  evdev) and the SSAM character device (`/dev/surface/aggregator`). Only the cover coupling uses more: the
  Bluetooth management socket, a raw HCI socket and the D-Bus interface of BlueZ.
- It does not touch what the desktop owns (refresh rate and VRR, brightness, automatic brightness, applying colour
  management). It only creates the ICC file; assigning it is the desktop's job.
  Continuing a touchpad edge scroll is the compositor's job too: only the compositor knows the scroll direction
  setting, so a wheel event produced outside it disagrees with that setting (the Windows measurements are in
  [research/control/typecover.md](research/control/typecover.md) section 6.1).
- UEFI variables are only read (`BatteryLimitEnable`, the panel calibration).

## Installation

Building needs Rust 1.82 or later. The daemon and the CLI build without GTK (`sp9-settings` is not a default
member of the workspace).

```sh
cd helper
cargo build --release
cargo build --release -p sp9-settings   # settings app (development files of GTK 4.12 and libadwaita 1.5 or later). When built, install.sh installs it too
sudo ./install.sh              # /usr/local + /etc/systemd/system; enables and starts sp9-helperd.service
sp9-ctl status
sudo ./install.sh --uninstall  # the settings in /etc/sp9, the reports in /var/lib/sp9 and the iptsd drop-in stay
PREFIX=/usr DESTDIR=$pkgdir ./install.sh   # packaging: places files only, does not touch the running system
```

Without `DESTDIR`, `install.sh` runs only as root on SKU `Surface_Pro_9_2038`, and the unit does not start on
another machine (`ConditionFirmware=`): the power limits and SAM requests are this device's. `install.sh` loads `surface_aggregator_cdev` now and adds it to `modules-load.d` so that it is loaded at boot.
Without this module smart charging and the fan request show as unavailable. It also installs
`tools/linux/post-install-check.sh` for diagnostics and `tools/sdc-blob.py` (python3) for the ICC profile.

On NixOS it is one module option ([nixos.md](nixos.md)).

```nix
hardware.surface-pro-9.helper.enable = true;
```

## Usage

```sh
sp9-ctl status                         # the properties of every object
sp9-ctl profile balanced-performance   # platform_profile (SAM thermal profile and fan profile) + that profile's PL and EPP
sp9-ctl limits                         # the table in force (W, s, EPP) and the actual values per RAPL zone
sp9-ctl limits set performance ac 30 50 28 64   # override the table (PL1 5–35 W, PL1 ≤ PL2 ≤ 60 W, tau 1–28 s)
sp9-ctl limits reset performance ac
sp9-ctl follow off                     # do not write RAPL and EPP when the profile changes
sp9-ctl skin-guard on                  # experimental: lower PL1 and PL2 by the skin temperature (VTS1 = surface_thermal temp9)
sp9-ctl fan                            # surface_fan rpm, SAM temperatures, the fan request and whether the fan follows it (FanRequestState)
sp9-ctl fan allow on && sp9-ctl fan request 3500   # experimental: SAM FAN 0x0b rpm request (1600–7500); release withdraws it
sp9-ctl battery                        # status, health, cycles, adapter rating, smart charging, UEFI battery limit
sp9-ctl battery release                # release smart charging (charge to 100 % this once)
sp9-ctl psr off psr1                   # PSR off on AC, PSR1 on battery (as Windows does)
sp9-ctl icc                            # /var/lib/sp9/display/panel-native.icc
sp9-ctl kbd 6                          # keyboard backlight (`sp9-ctl kbd` shows the levels: hidraw 0 3 6 12, LED 0..3)
sp9-ctl touch-off on                   # block touchscreen input while the screen is off
sp9-ctl iptsd 22 18 on                 # iptsd thresholds on top of the preset; iptsd preset reverts
sp9-ctl pen status                     # the pen settings and, when sp9-digitizerd runs, its live state (range, contact, tool, tip signal, reference) and counters
sp9-ctl pen predict 7.5                # sp9-digitizerd reports the pen where it will be 7.5 ms from now (0–30; off switches it off; digitizerd.md section 13)
sp9-ctl pen zero-force                 # the current zero-force contact values
sp9-ctl pen zero-force off             # switch it off (on switches it on)
sp9-ctl pen zero-force 60 45           # contact begins at 60 %, ends below 45 % (1 ≤ exit < enter ≤ 150); also switches it on
sp9-ctl pen floor 4                    # the pressure reported in contact without force (0–50 % of the range)
sp9-ctl pen zero-force reference learn # the reference signal: learn (the touch daemon learns it) or a number
sp9-ctl pen ink on                     # pen haptics as a whole: inking haptics (start when the tip touches, stop when it lifts) and speed feedback
sp9-ctl pen strength 150               # vibration strength in %. 100 = the Windows default (intensity byte 127), about 2–200 (intensity byte 2–254)
sp9-ctl pen intensity 160 && sp9-ctl pen test   # the same setting as the intensity byte (2–254); one click
sp9-ctl pen waveform pencil            # the waveform while drawing: ink pencil marker chisel-marker brush sparkle (default brush); without an argument, the current one and the list
sp9-ctl pen eraser-click off           # no click when the eraser end touches down
sp9-ctl pen speed off                  # do not send the pen state to the digitizer (no continuous vibration)
sp9-ctl pen full-scale 250             # advanced: the pen speed that gives the largest speed value (mm/s, default 300 = the Windows scale); default reverts
sp9-ctl pen dead-zone 15               # send speeds below this (mm/s) as speed 0 (Windows: about 15); off is the default
sp9-ctl pen auto-couple on             # couple a pen when it is put into the cover's pen slot (administrator authentication)
sp9-ctl pen couple                     # look at the pen in the cover now: couple it if it has no bond, otherwise wake and connect it, and replace only a dead bond (administrator authentication). --force replaces without checking
sp9-ctl diagnostics                    # path of the post-install check report tar.gz (owned by the caller)
sp9-ctl --json status
```

## Features

| Feature | What it does | Default |
|---|---|---|
| Performance profile | When `platform_profile` changes, the kernel sends the SAM thermal profile and the fan profile. At the same moment the helper writes PL1/PL2/tau from the Windows SMF table to RAPL (MSR `intel-rapl:0`, and MMIO `intel-rapl-mmio:0` when present) and the per-overlay EPP to `energy_performance_preference`. It writes again when AC/battery changes. For 5 seconds after writing the EPP it writes it again if something else changes it | on |
| Power limit overrides | Change the table per profile and power source. The range is limited to the node range of the SMF policy | none |
| Skin temperature guard | When VTS1 exceeds the profile's setpoint (45–48 °C), PL1 is lowered by proportional-integral control, and at 49/50/51/52 °C PL2 is capped at 35/30/20/15 W | **experimental, off** |
| Fan | Reads the `surface_fan` rpm. The SAM FAN CID 0x0b (`u16` rpm) request is available only after it has been allowed explicitly: 1600–7500 rpm, or 0 (withdraw). It is followed only while the SAM is not driving the fan itself ("Fan request" below) | **experimental, off** |
| Battery | Status, capacity, cycles, health against the design capacity, adapter rating (`input_power_limit`, kernel patch 0010), UEFI Battery Limit (read only) | — |
| Smart charging | Reads SAM 0x3a (support), 0x41 (engaged, `& 0x0440`), 0x42 (countdown); 0x43 releases. Windows has no command to switch it back on either. The firmware engages it again by itself | — |
| PSR policy | Per power source: `default` (untouched), `off`, `psr1`. Written to debugfs `i915_edp_psr_debug`. The boot parameter `i915.enable_psr=1` belongs to the platform files | `default` |
| Panel ICC | Makes an ICC profile of the panel's native gamut from the factory calibration in UEFI `Type_1`/`Type_2`. Assign it with colord (`colormgr`) or in the desktop settings | — |
| Keyboard backlight | When the kernel provides a `*::kbd_backlight` LED, the helper writes its `brightness` (UPower and the desktop keys use the same). Otherwise it writes the level to hidraw Output 213 (`000C:007B`), reads the supported levels from Feature 213 and the current level from Feature 214. When the backlight key is pressed (the firmware changes the level), the helper sees it through the LED's `brightness_hw_changed` or hidraw Input 213 and sends a change signal, so the settings app follows | — |
| Touch off while the screen is off | Sets `inhibited` on iptsd's virtual touchscreen while the eDP connector is off | off |
| iptsd thresholds | Writes `/etc/iptsd.d/zz-sp9-helper.conf` (read after the preset). When `sp9-digitizerd` is running, the helper hands it the same values over D-Bus: they apply at once and the daemon is not restarted ([digitizerd.md](digitizerd.md) section 6). Otherwise it restarts iptsd | preset |
| Zero-force contact | The pen counts as touching when its tip signal is strong enough, even when it reports no pressure (the iptsd patches of this repository and `sp9-digitizerd`; the counterpart of the Windows ZFI). Written to `[DFT]` of the same drop-in | on; contact begins at 85 %, ends below 75 %; pressure floor 3 %; the reference is learned |
| Pen position prediction | `sp9-digitizerd` reports the pen where it will be a few milliseconds from now. Written to `[Stylus] PredictionMs` of the same drop-in; iptsd does not have this key | off (0 ms) |
| Pen haptics | Report 65 on the pen's BLE hidraw. Inking haptics send one start and one stop report on `BTN_TOUCH` of iptsd's virtual stylus. The waveform can be chosen (default brush), and the strength (%) is the intensity byte of the start report (the same as the Windows "tactile signals" strength). This is the switch for pen haptics as a whole | off |
| Eraser click | Writes one click (ordinal 3) when the eraser end touches the screen. The eraser end plays no continuous waveform (the same on Windows). Only while pen haptics are on | on |
| Pen speed feedback | Sends the FastHostId (`8e a1`) and the pen state (`8e a5`: hover, tip contact with its speed, eraser contact) to Feature 9 of the digitizer (THC), with the bytes Windows sends. The pen plays its continuous waveforms only while the digitizer has a speed. Only while pen haptics are on | on (nothing is sent while pen haptics are off) |
| Cover coupling | When the pen is put into the Type Cover's pen slot, hands a key over through the cover and bonds with LE legacy OOB pairing. Needs kernel patch 0023 | off |
| Diagnostics | Runs `post-install-check.sh` (read only; masks MAC addresses, UUIDs, serials and Wi-Fi network names) and packs it as `/var/lib/sp9/diagnostics/check-<time>.tar.gz`, readable only by root and the caller; the five newest are kept | — |

The desktop power mode (power-profiles-daemon) uses only three levels, so `balanced-performance` (the Windows
default on AC) is chosen with `sp9-ctl profile`.
The helper follows `platform_profile` also when power-profiles-daemon changes it, so the RAPL limits are the same
whoever changes the profile (power-profiles-daemon does not write RAPL).
Both write the EPP. power-profiles-daemon sees a profile change it did not make and 1–2 seconds later writes its
own EPP name (`performance`, `balance_performance` and so on), which replaces the value the helper wrote first.
The helper therefore writes the table's value again when the value changes within 5 seconds of its own write. With
`FollowProfile` and `EppEnabled` on, the table's EPP stays whichever way the profile is changed:
`sp9-ctl profile`, `powerprofilesctl set` or sysfs. To use power-profiles-daemon's EPP: `sp9-ctl epp off`.

Fan request. A request can only spin a fan the SAM has stopped. While the SAM drives the fan itself (under load) it
ignores higher and lower values alike, so a request can neither reduce cooling nor add to it under load.
On resume from suspend the kernel (`surface_platform_profile`) clears the request, and the helper does not send it
again. When the helper restarts it withdraws the request at start-up.
The SAM always answers that it accepted a request, so from 30 seconds after sending the helper compares `FanRpm`
with the request and reports the result as `FanRequestState`:
`settling` (waiting), `honoured` (within 10 %), `not-honoured` (the SAM is driving the fan, or the request was
cleared), `unknown` (the rpm could not be read), `none`.
The range 1600–7500 rpm is the span measured on this device (asking for 1500 gives 1588 rpm, asking for 7500 gives
7407 rpm; nothing outside it was measured). The Windows fan curve uses 3000–4600 rpm.

Pen haptics requested by applications (planned). On Windows, when an application calls the pen haptics API,
`dwm.exe` writes report 65 to the pen (Bluetooth trace: single bursts of 1–3 writes and groups of continuous writes
about 6 ms apart; that capture truncates the values. A later capture has them: one start and one stop per stroke,
[pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md) section 5).
All the pen receives is an ordinal, an intensity, a repeat count and a cutoff, so the same expression is possible
on Linux. What is missing is a channel for applications to ask, so the plan is an effect method on the helper's
D-Bus interface (`PlayEffect(kind)`, a table from effect name to write sequence) that applications call.
To check on Linux: whether writes 6 ms apart go out at the BlueZ connection interval, and what to do with a request
while the pen is disconnected (it disconnects by itself when idle: after about 52 s in the Windows trace, after 50–80 s as measured on Linux).

## Pen

The basis and the measurements are in [research/gaps/pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md).
Inking haptics, speed feedback and cover coupling have worked on the device with the helper (the status column
of [helper-reference.md](helper-reference.md)). The strength and the eraser click were confirmed on the device as
well (observed). The choice of inking waveform, the speed scale of 300 mm/s and the dead zone **have not been run
on the device**; the status of every other part is in the status column of the reference.

**Strength.** The Windows "tactile signals" strength is one byte: the intensity of the start report written to
the pen (Output 65; minimum 2, default 127, maximum 254). The helper does the same: `pen strength P` sets the
intensity byte to `round(127 x P / 100)`, clamped to 2–254 (100 % = 127, about 2–200 %). `pen intensity N` sets
the same value as the byte (2–254; anything else is refused, and a value in the settings file is clamped to that
range). The speed scale has nothing to do with the strength. The Windows setting is a continuous slider and its
intensity is the percentage x 2.54 (2, 104, 127 and 254 were seen): the same mapping.
On the device 30 % is not felt, and 200 % is clearly stronger than 100 % (observed).

**Inking waveform.** The continuous waveform that starts when the tip touches is chosen with `pen waveform NAME`:
`ink`, `pencil`, `marker`, `chisel-marker`, `brush`, `sparkle`. The default is `brush`.
Windows applications pick a waveform per tool and hardly use ink: of the 230 continuous starts in three captures,
172 are brush (OneNote), 50 pencil (Journal) and 3 ink. At the same speed and strength brush felt strongest
(observed: ink, brush and pencil compared).
The ordinal is looked up by name in the pen's waveform list (Feature 66, read once per pen node); when the list
cannot be read the helper uses this pen's fixed ordinals (9, 10, 11, 12, 13, 15). The eraser waveform cannot be
chosen (the eraser end only clicks).
A settings file without the `ink_waveform` key gets the default `brush`. The waveform used to be ink always, so the
vibration feels different; `sp9-ctl pen waveform ink` brings the earlier one back.

**Speed feedback.** With `pen ink on` the helper computes the pen's speed from the positions of iptsd's virtual
stylus (two samples 14–32 ms apart, the last three values averaged with 0.5/0.33/0.17), quantises it to 0–63 and
sends it to the digitizer in the `8e a5` frame the Windows touch processor sends without a pen ID. A speed is
given **only while the tip is in contact**: while hovering and while the eraser end is used the frame says "no
speed" (`ff`), and when the tip lifts a frame without a speed goes out at once, so that no old speed stays in
the digitizer.
It sends when the state changes (hover, contact and its speed value, eraser contact) and once a second. Windows
sends on every frame (134 a second).
`8e a1` (the FastHostId) is sent when the digitizer node is seen (again), right after a resume from suspend and
once more 3.8 seconds later (as Windows does after the display comes back), and when the pen comes into range
30 seconds or more after the last one.
The speed scale (full scale: the speed that gives the largest value, 63) is 300 mm/s by default: the Windows
formula is `min(v, 30) x 63 / 30` with v in 0.01 mm/ms, and the helper's estimator run over Raw Input recorded on
Windows gives the Windows values exactly at 300 (measured).
`pen full-scale N` changes the scale, but it is not a strength setting.
Dead zone: Windows sends speed 0 below about 15 mm/s. The helper does not by default; `pen dead-zone 15`
(0–100 mm/s) switches it on: when the averaged speed is below it, the value 0 is sent (as a valid speed).
The FastHostId is kept in `/var/lib/sp9/pen-fast-host-id`. When that is missing the helper takes over
`/var/lib/surface-pen/fast-host-id`, which `surface-pen-feedback` used, and when that is missing too it makes one
up at random.

**The eraser end.** No continuous waveform plays with the eraser end (the same on Windows; seven combinations of
frames and waveforms were tried). Only the click waveform is felt, so the helper writes one click when the eraser
end touches down (`pen eraser-click`, on by default). Nothing is written while rubbing or on lift.

**Settings files from before.** The old `pen strength` changed the speed scale (`speed_full_scale`). That key is
no longer used: whatever its value, the speed scale becomes 300 mm/s, the helper writes one journal line at
start-up, and the key disappears the next time the settings are saved.
A value set with the old `pen full-scale` used the same key and is dropped with it (the two cannot be told
apart). Set it again with `pen full-scale N` if needed (new key `full_scale_mm_s`). `intensity` stays.

**Zero-force contact.** In small, light handwriting some strokes pass without the pen reporting any pressure, and
they are lost. With the iptsd patches of this repository ([userspace/iptsd](../userspace/iptsd/README.md)) the
pen counts as touching, pressure or not, when its tip signal reaches a set fraction of the signal it has while
writing with force (the reference).
The helper writes the setting to `[DFT]` of the drop-in that also holds the iptsd thresholds
(`/etc/iptsd.d/zz-sp9-helper.conf`). A running `sp9-digitizerd` takes the values over D-Bus without a restart;
otherwise iptsd is restarted, and only when the file changed. The commands are `sp9-ctl pen zero-force …` and
`pen floor` (in the settings app: "Write Without Pressure" on the pen page).

- Contact begins at the enter value (`ZeroForceEnter`) and ends below the exit value (`ZeroForceExit`). Both are
  percentages of the reference, with 1 ≤ exit < enter ≤ 150.
- The pressure floor (`ContactMinPressure`) is the pressure reported while in contact without force (0–50 % of
  the range).
- The reference (`ZeroForceReference`): with `learn` the touch daemon learns it after each start from strokes
  written with force; a number fixes it.
- The defaults are 85 / 75 / 3 % and learn. In captures of this device with the Slim Pen 2 the tip signal was at
  92 % of the reference or above with the pen on the glass, and at 65 % or below while it was lifted between
  letters (measured). Lower thresholds join strokes: at 50/35, 8 seconds of small, faint handwriting came out as
  4 strokes, where the pen reported force in 23. The floor of 3 % matches the smallest pressure Windows reports
  (37/1024).
  The signal of a pen hovering 5 mm above the glass was measured at 9–30 % of the reference, so with low values
  a pen moved very close to the glass can draw. Raise the values then.
- An iptsd without the patches does not read these keys (its `config-loader.hpp` looks up the keys it knows by
  name). In the drop-in they do no harm and have no effect.
- When a change restarts iptsd, touch and pen stop for a moment. If a stroke was in progress, the helper's pen
  thread sees the stylus node go away, stops the running waveform and takes the contact back from the digitizer
  (the same path as in "Handling of stylus events" below).
- With `sp9-digitizerd`, a value changed with `sp9-ctl pen zero-force` reaches the running daemon without a
  restart (measured, [test-results.md](test-results.md) TOUCH-1). With iptsd, the path through the helper
  **has not been run on the device**.

**Pen position prediction.** `sp9-ctl pen predict MS` (0–30 ms, `off` = 0) sets how far ahead `sp9-digitizerd`
reports the pen position ([digitizerd.md](digitizerd.md) section 13). The value is written to
`[Stylus] PredictionMs` of the same drop-in and handed to a running `sp9-digitizerd` over D-Bus without a
restart. iptsd has no such key and ignores it. Unverified on the device.

**Handling of stylus events.** The helper decides at the end of a frame (SYN_REPORT), not on single events:
iptsd emits `BTN_TOUCH` before the tool key, so only the complete frame tells which end touched.
With the tip in contact and no new position for 60 ms, speed 0 is sent even while events keep arriving (iptsd
emits a timestamp event in every frame). Estimates have an age: a sample that follows a gap of more than 32 ms
discards the earlier estimate.
Times are the event timestamps (monotonic clock), and a state change held back by the minimum interval (7 ms)
goes out when that time has passed. When the kernel has dropped events (SYN_DROPPED), and when the node is
opened, the key state is read again (`EVIOCGKEY`).
When the stylus node goes away (an iptsd restart, a suspend), when pen haptics are switched off and when the
helper stops (SIGTERM), it writes a stop for the running waveform and takes the contact back from the digitizer
(a frame without a speed).
Switching speed feedback off during a stroke sends one frame that takes the contact back at that point. When the
stylus axes have no resolution no speed is sent (one journal line).
The digitizer node is kept open for writing: hidraw numbers are reused (hidraw7 → hidraw0 after a resume), so
remembering the name alone could write to another device. When writes keep failing the interval grows from 2 to
60 seconds and the same error is logged once.
What this paragraph describes **has not been run on the device** (unit tests and FIFO tests only).

**Do not run it together with `surface-pen-feedback`.** Both write the same Feature 9 and overwrite each other's
speed value. The helper does not write to the digitizer with `pen ink off` or `pen speed off`.
With `helper.enable` the NixOS module does not start the unit of `pen.feedbackService` at boot and prints a
warning.
While it writes to the digitizer the helper holds an exclusive flock on `/run/lock/surface-pen-digitizer.lock`,
and when another program holds it the helper does not write (one journal line; it looks again every 10
seconds). Inking haptics go out regardless.
`surface-pen-feedback` does not take this lock yet, so the lock does not guard against it. A second instance of
the helper ends before it writes to any device when it does not get the bus name.

**Cover coupling.** With `pen auto-couple on` the helper keeps the cover's pen bonding node open (`045E:09B0`,
FFF4:0001) and writes Features 112 and 86 (the host address) when the node appears, after a resume and on a manual
request.
When the cover announces a pen with Input 110, the helper computes the coupling address. When BlueZ has no bond
for that address (or encryption with that bond has failed before) it couples at once.
A bonded pen that is connected is left alone. A bonded pen that is not connected is **checked, not removed**
(the probe, below). The coupling procedure:
switch Pairable on → a 16 B key (the TK: the key last written to that pen and not yet used by a pairing,
otherwise random bytes) to the kernel (MGMT Add Remote OOB Data) → the same 16 B to Output 111 of the cover → LE
Extended Create Connection on a raw HCI socket → MGMT Pair Device, wait for the pen's LTK → remove the TK, set
Trusted → wait up to 10 seconds for the pen's HID node. When the pen does not connect the attempt ends in
`no-answer` and the cover is asked again 1 minute later (2, 5 and 15 minutes after repeated failures). When the
pen's key arrived but BlueZ does not show the bond, the result is `failed`.

**The key is handed over first.** The pen takes a key from the cover only while it is not connected, and that
takes longer than the interval between writing the key and connecting (inferred. Measured: a coupling that
connected right after writing the key was refused, also when the same key was repeated, and a coupling that
handed the key over first succeeded at the first attempt 3 times out of 3;
[pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md) 2.8). So a coupling goes like this:

- The key goes to the kernel and is written to the cover (Output 111). The helper waits 2.5 seconds **without
  connecting**.
- It asks the cover again (Feature 86), and when the cover announces the same pen it writes the same key again
  and connects.
- Pair Device follows 0.4 seconds after the connection. The key is given to the kernel again before every Pair
  Device (a kernel with the current patch 0023 removes the key once a pairing has used it).
- After a refusal it tries once more on the same link, 1 second later. If that fails too the round ends, and 20
  seconds later one more round starts from the beginning (handing the key over). Two rounds per request.
- bluetoothd starts a pairing of its own on the new link as well. Whoever paired, the pen's LTK arriving is the
  success. The bond is not removed when Pair Device then answers Busy or Already Paired.
- For every pen (by coupling address) the helper remembers the key it last wrote to the cover until a pairing
  succeeds (in memory only) and uses that key. This includes a key written by a probe. The hand-over step is
  done also when a key is remembered.
- When everything is refused: `failed: the pen refused the pairing`. For a pen that failed the cover is asked
  again after 1, 2, 5 and then every 15 minutes (at once when the pen is taken out and put back, or on
  `pen couple`).

With this order the helper coupled a pen without a bond (measured): after the bond had been removed while the pen
was connected, 12 times out of 12 at the first attempt, `coupled` after about 11 seconds. It worked with a new
key and with a key written earlier and not used by a pairing. A coupling with the same key 35 seconds after a
pairing that was made to fail on purpose worked too.

**No pairing on a link that came up through a resolved address.** The repeated failures of an earlier version on
the device were caused by the link, not by the key (measured;
[pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md) 2.9).
When the pen is still on the controller's resolving list (a bonded pen was disconnected at some point and its
bond was removed afterwards), the connection made to the coupling address matches the pen's reconnection
advertising from its RPA, and on that link the kernel computes the Confirm value with the identity address and
disagrees with the pen (Confirm value failed).
The helper switches the controller's address resolution off for the Create Connection of a coupling (LE Set
Address Resolution Enable on the raw HCI socket; switched on again afterwards). With that state reproduced the
pen coupled at the first attempt 6 times out of 6 (0 out of 2 without it).
If the link still comes up through a resolved address (the controller was scanning and resolution could not be
switched off), the helper does not pair on it, logs `connected through the pen's private address…` and goes on
to the next round. When every round ends that way: `failed: the pen only answers from its private address…`.
Confirmed with the helper (measured): the probe of a bonded pen → `connected` (2.2 seconds, bond kept, the pen's
HID), `pen couple` on a connected pen → `already-coupled`, `--force` on a connected pen → nothing touched, and
Pairable left as it was.

**The probe: a bonded pen that is not connected.** This is a common state: a healthy pen sleeps in the cover,
and after a resume from suspend it does not connect by itself (measured: 60 seconds).
"Not connected" is therefore no reason to drop a bond. An earlier version did so and removed a good bond on the
device (measured): it removed the bond of a pen that was not connected after 6 seconds and started a coupling,
the pen did not answer the connection, and it was left without a bond.
Now the helper waits 6 seconds (the pen may connect by itself), asks the cover again, and if the pen is still
there writes a new key to the cover only (Output 111; the pen accepts a connection only after it received a
key: measured) and connects. That key is not given to the kernel, and nothing is removed or paired. The key
stays in the pen and is used by the next pairing, so the helper remembers it ("The key is handed over first"
above).
Once connected, bluetoothd encrypts with the stored key by itself (measured: LE Start Encryption 0.1 seconds
after the connection, success, the pen's HID attaches; the pen accepts the old key while holding the new one).

| Probe outcome | Meaning | What happens |
|---|---|---|
| Connected, still connected 2 seconds later, and the services are up (`ServicesResolved` in BlueZ, or the pen's HID node) | The bond is healthy | `connected`. Nothing changed except that a sleeping pen is connected again |
| Connected, then dropped for an encryption failure (Device Disconnected, reason authentication failure) | The bond is dead (Windows coupled the pen since) | `stale-bond`. The cover is asked again, and at that announcement the dead bond is removed (MGMT Unpair Device) and the pen coupled |
| No connection | Unknown | `no-answer`. The bond stays. No automatic probe again within 60 seconds |

- The dead-bond case (encryption refused) has not been seen on the device yet. The healthy case was measured,
  with the helper: by request and automatically the result was `connected`, with the bond kept.
- The key a probe writes stays in the pen, and the pen uses it in the next pairing (measured). The earlier
  assumption that it is harmless was wrong; this is why the helper remembers the key and uses it for the next
  coupling.
- Right after a resume the helper does nothing by itself. `sp9-ctl pen couple` probes then as well and
  reconnects the pen. Once, right after a resume, it ended in `no-pen` (the cover had not announced the pen);
  running it again works.
- When a bond is replaced the old bond is removed before the attempt (the kernel requires it). What can fail
  without the pen (whether the kernel takes the key, assembling the report, the raw socket) is done first, and
  the bond is removed after that. The only proof that a bond is dead is a probe that was dropped for an
  encryption failure (within the last 2 minutes): a failed pairing is no proof.
- When bluetoothd does not answer (it is restarting), whether there is a bond is unknown, and the helper does
  nothing. When the adapter uses privacy (an RPA of its own) the coupling is refused. Only the known cover is
  used (`045E:09B0`, bus 0x19). When both rounds fail the pen is left without a bond and the result is
  `failed: …`; at the next announcement the coupling is tried again with the same key.
- `sp9-ctl pen couple`: `already-coupled` for a connected pen (nothing is touched), coupling for a pen without a
  bond, a probe for a bonded pen that is not connected. `--force` removes the bond of a pen that is not connected
  without a probe and couples: when the pen does not answer it is left without a bond (the cover is asked again
  1 minute later and coupling is tried). It is for a stale bond whose probe only gives `no-answer`.

The outcome is `CoupleResult`: `coupled`, `coupled-no-hid` (bonded, BlueZ has not attached the HID yet),
`already-coupled`, `connected`, `no-answer`, `stale-bond` (a coupling follows), `no-cover`, `no-pen`,
`unsupported-kernel` (the kernel lacks patch 0023), `bluetooth-off`, `failed: reason`. Only the last byte of an
address is shown, and the journal has neither whole addresses nor keys. The reasons for the design are in
[helper-reference.md](helper-reference.md#cover-coupling).

- Dual boot: when Windows couples the pen again with the same host address, the pen's keys for this host change,
  the IRK included. Linux then does not recognise the pen's advertising and does not connect, so an encryption
  failure does not happen by itself and the old bond stays in BlueZ (measured). The probe provokes that failure
  on purpose to tell. A bond whose encryption failure was seen outside a probe is also replaced the next time
  the pen is put in.
- A pen that has just been coupled is not touched automatically for 60 seconds, and `pen couple` is refused
  within 15 seconds of a coupling (`failed: coupled a moment ago…`): a freshly coupled pen does not answer a new
  connection (measured).
- **The helper never drops the pen's connection and does not use `RemoveDevice`.** After the host drops the link
  (`bluetoothctl disconnect`) the pen does not advertise for more than 30 seconds, also when it is moved. After
  the pen has disconnected by itself for idling it is back within 3.5 seconds (measured).
  Only a bond that is proven dead (and `--force`) is removed, and only while the pen is not connected.
- When Bluetooth is not on yet at boot, `CoupleResult` is `bluetooth-off` for a moment and is cleared once the
  adapter is on and the host address has been written to the cover. A pen that was already in the slot is
  announced by the cover at that point.
- A pen whose coupling failed is tried again by itself only after 1, 2, 5 and then every 15 minutes.
- When the result stays `coupled-no-hid`, see whether the HID attaches the next time the pen reconnects. If not:
  `sudo systemctl restart bluetooth`, or `[pen] couple_restart_bluetooth = true` in `helper.toml` (the helper
  then restarts it in that case only). On the device the HID attached without a restart.

## Permissions

The daemon runs as root and asks polkit on every call. Everyday settings (profile, power limits, skin temperature
guard, smart-charging release, ICC, keyboard backlight, touch and iptsd, pen haptics) are allowed
without a password in an active local session. The experimental fan request, the PSR policy (it writes to
debugfs), the diagnostics report (it holds the kernel log, debugfs and the ACPI tables, which only root may read), pen coupling (switching automatic coupling on, and `pen couple`: they create a Bluetooth bond and replace a
dead one) and reloading the settings ask for administrator authentication (kept for the session).
The actions are listed in `helper/data/io.github.joonhoekim.opendevicehelper.sp9.policy` and can be changed in
`/etc/polkit-1/rules.d/`.

## Settings and state

- When the settings file cannot be read as a whole (a syntax error, a value of the wrong type) the helper runs
  with the defaults, and the first time a setting is changed afterwards it keeps the original file as
  `helper.toml.bad` before overwriting it. An unknown inking waveform name changes only that value to the
  default (brush); the rest is read as it is.
- The settings are in `/etc/sp9/helper.toml` (`helper/data/helper.toml.example` shows the defaults). The daemon
  writes the file when a setting changes through D-Bus; after editing it by hand run `sp9-ctl reload`.
  A value outside its range is dropped, the default is used and the journal records it. Tables and keys this
  version does not know are ignored without a warning and disappear the next time the daemon writes the file.
- State is in `/var/lib/sp9` (diagnostics reports, the ICC profile, the pen's FastHostId). The fan request and the keyboard backlight
  level are not stored (the firmware holds them). The helper withdraws the fan request at start-up (when requests
  are allowed).
- The daemon runs its policy once a second. It writes only when the wanted value differs from the value it wrote
  last, so when another tool changes RAPL between profile changes, that stays until the next profile or power
  source change.
  The same goes for the EPP, except that a change within 5 seconds of the write is reverted; an EPP set by hand
  therefore stays from 5 seconds after a profile or power source change until the next one.
  After a gap of more than 10 seconds (a suspend) it writes everything again.

## Front-ends

Every front-end uses D-Bus only. Each object has the standard `org.freedesktop.DBus.Properties` (`GetAll`;
`PropertiesChanged` only invalidates, without values) and methods; every write is a method. Failures arrive under
the standard names (`org.freedesktop.DBus.Error.AccessDenied`, `InvalidArgs`, `NotSupported`, `Failed`).
`Device` (`surface-pro-9`) and `Features` (the names of the exported objects) on the root object decide which
pages to show.

### Settings app (sp9-settings)

A GTK4/libadwaita app that does what `sp9-ctl` does. It stands alone and is not tied to the settings of any
desktop. It has the same structure as `tb323fu-settings` of the TB323FU helper: one page per object (the page of
an object that is not in `Features` is hidden), values are read with `GetAll`, and a change calls the method with
the interactive-authorization flag (the polkit dialog appears outside the app).
It reads again immediately on the helper's `PropertiesChanged` and on a change of the bus name owner, and
otherwise every 10 seconds. Failures are shown as toasts.

| Page | Content |
|---|---|
| Performance | Profile selection, power source, switches for applying the profile's limits and EPP, current and target PL1, PL2, tau and EPP, editing one row of the table (within the range) and resetting it, the surface temperature guard (confirmation when switching on), the temperature list, fan rpm, the fan request (warning confirmation when allowing; the current request row shows whether the fan follows) |
| Battery | Status, charge, power, health, cycles, adapter rating, smart charging state and the one-time release, UEFI battery limit (read only, with how to change it in the UEFI menu) |
| Display | PSR policy per power source, the boot value, the driver state, creating the ICC profile and how to assign it in GNOME, KDE and colord |
| Keyboard and touch | Keyboard backlight level (follows changes made with the key), ignoring touch while the screen is off, iptsd sensitivity presets (device preset 24/20, 20/16, 32/28, iptsd default 40/36) and ignoring touch near the pen |
| Pen | Inking haptics, strength (%), texture (the inking waveform), eraser click, test, sending the pen speed, ignoring very slow movement (the dead zone of 15 mm/s), writing without pressure (the switch, enter and exit %, the lowest pressure, the reference signal shown), automatic pairing through the cover (confirmation when switching on), the last pairing result, pair now |
| Diagnostics | Exporting a report, the path of the last report and opening its folder |
| About | Device, model, helper and app versions, feature list, reloading the settings file |

- Strings are English message IDs wrapped in `tr`/`trf`/`n_` and translated through a per-language table
  (`src/i18n/ko.rs`). When the locale is not English (`en*`) the app is Korean (`SP9_SETTINGS_LANG=en|ko`
  overrides).
  A test checks that every message ID in the source has a Korean translation.
- `--page NAME` (performance, battery, display, input, pen, diagnostics, about), `--maximized`, `--session`
  (attaches to `sp9-helperd --session --fake` on the session bus).
- When another device's helper holds the same bus name (its `Device` differs), the app says so.
- The app ID and the names of the icon and the desktop file are
  `io.github.joonhoekim.OpenDeviceHelper.SurfacePro9` (`helper/data/`).

## Tests

```sh
cd helper
cargo test                                                   # unit tests on fake backends + D-Bus round trips (TCP p2p, polkit skipped)
cargo test -p sp9-settings                                   # settings app: property decoding, missing translations (needs the GTK development files)
cargo check --target x86_64-unknown-linux-gnu --workspace    # includes the Linux-only code (ioctl, evdev)
sp9-helperd --session --no-polkit --fake                     # a fake device on the session bus, for front-end work
```

On a Windows host zbus builds only with the `stable-x86_64-pc-windows-gnu` toolchain and MinGW (`dlltool`, `as`)
in PATH.
For the settings app, install `mingw-w64-ucrt-x86_64-{gtk4,libadwaita,pkgconf,gcc}` from MSYS2 UCRT64, put
`C:\msys64\ucrt64\bin` at the front of PATH and set `PKG_CONFIG_PATH` to `C:\msys64\ucrt64\lib\pkgconfig`; then
`cargo clippy -p sp9-settings` and `cargo test -p sp9-settings` work and the app starts (there is no D-Bus, so it
shows the "helper service is not running" screen).
`SP9_SYSFS_ROOT` redirects every device path to a fake tree, `SP9_CONFIG` the settings file, and `SP9_STATE_DIR`,
`SP9_DATA_DIR` and `SP9_PYTHON` the state and script locations and the Python interpreter.
The order of checks to run on the device is in
[helper-reference.md](helper-reference.md#still-to-verify-on-linux).
