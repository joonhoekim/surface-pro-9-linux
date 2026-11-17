# Device helper (sp9-helperd)

A system service and a command-line tool for the device-specific settings of the Surface Pro 9 (Intel) that a
desktop does not handle. It exports over **D-Bus**: per-profile power limits, battery and smart charging, the PSR
policy and the panel ICC profile, the Type Cover backlight, touch and iptsd settings, pen haptics, and a diagnostics
report. The structure follows the author's Open Device Helper for another device, the TB323FU (public repository
`tb323fu-linux`), and it uses the same bus name.
The code is in [`helper/`](../helper/); the kernel interfaces, the D-Bus API, the polkit actions and the
verification status of each feature are in [helper-reference.md](helper-reference.md).

It runs on this device under NixOS. The status of each feature is the status column of the reference.
The evidence for each feature is in `docs/research/control/*.md`, and only what was confirmed on the device is
marked "works" in the status column.

| Part | Status |
|---|---|
| `sp9-helperd` (system D-Bus service), `sp9-ctl` (CLI) | Works on the device: start-up, per-profile PL and EPP and overrides, fan reading and fan request, battery and smart-charging reading, keyboard backlight, PSR policy, iptsd thresholds, ICC, diagnostics. Unverified: pen haptics (not checked with a pen connected), the skin temperature guard, touch off while the screen is off (only switching the setting on and off was seen), smart-charging release |
| `sp9-settings` (GTK4/libadwaita settings app) | The window opens on the device (observed). Operating the individual items is unverified |

What it does not do:

- **Booting does not depend on the helper.** The kernel command line, the initramfs, the iptsd preset and the
  speaker processing belong to the [platform files](../userspace/platform/README.md) and the NixOS module.
- It adds no drivers. It uses only interfaces the kernel already provides (sysfs, debugfs, efivarfs, hidraw,
  evdev) and the SSAM character device (`/dev/surface/aggregator`).
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

`install.sh` loads `surface_aggregator_cdev` now and adds it to `modules-load.d` so that it is loaded at boot.
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
sp9-ctl pen ink on                     # inking haptics (start when the tip touches, stop when it lifts)
sp9-ctl pen intensity 160 && sp9-ctl pen test
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
| iptsd thresholds | Writes `/etc/iptsd.d/zz-sp9-helper.conf` (read after the preset) and restarts iptsd | preset |
| Pen haptics | Report 65 on the pen's BLE hidraw. Inking haptics send one start and one stop report on `BTN_TOUCH` of iptsd's virtual stylus | off |
| Diagnostics | Runs `post-install-check.sh` (read only; masks MAC addresses, UUIDs and serials) and packs it as `/var/lib/sp9/diagnostics/check-<time>.tar.gz` | — |

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
while the pen is disconnected (it disconnects by itself when idle: after about 52 s in the Windows trace, about 80 s as measured on Linux).

## Permissions

The daemon runs as root and asks polkit on every call. Everyday settings (profile, power limits, skin temperature
guard, smart-charging release, ICC, keyboard backlight, touch and iptsd, pen, diagnostics) are allowed without a
password in an active local session. The experimental fan request, the PSR policy (it writes to debugfs) and
reloading the settings ask for administrator authentication (kept for the session).
The actions are listed in `helper/data/io.github.joonhoekim.opendevicehelper.sp9.policy` and can be changed in
`/etc/polkit-1/rules.d/`.

## Settings and state

- The settings are in `/etc/sp9/helper.toml` (`helper/data/helper.toml.example` shows the defaults). The daemon
  writes the file when a setting changes through D-Bus; after editing it by hand run `sp9-ctl reload`.
  A value outside its range is dropped, the default is used and the journal records it. Tables and keys this
  version does not know are ignored without a warning and disappear the next time the daemon writes the file.
- State is in `/var/lib/sp9` (diagnostics reports, the ICC profile). The fan request and the keyboard backlight
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
| Pen | Inking haptics, intensity, test |
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
