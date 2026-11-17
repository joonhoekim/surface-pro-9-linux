# Device helper: reference

The developer-side document of the [device helper](helper.md). For each feature it gives the kernel interface, the
D-Bus properties and methods, the polkit action, the evidence and the verification status, and it covers the whole
D-Bus API, the policy details, packaging and tests. The code is in [`helper/`](../helper/) (`sp9-helper-core`,
`sp9-helperd`, `sp9-ctl`, the settings app `sp9-settings`).

Notation:

- Evidence levels are those of the research documents. **Confirmed** = confirmed by Windows measurement, by the
  Windows drivers or by code; **inferred** = circumstantial evidence.
- Status is for Linux. **Works** = on this device the value the helper reports matches the kernel's, or the value
  it writes can be read back from the kernel, or a person confirmed it.
  **Unverified** = not run on the device. **Tested (fake)** = the same code passes unit tests on a fake backend or
  a fake sysfs tree.
- D-Bus objects are `/io/github/joonhoekim/OpenDeviceHelper1/<object>` and interfaces are
  `io.github.joonhoekim.OpenDeviceHelper1.<object>`.
  polkit action names are prefixed with `io.github.joonhoekim.opendevicehelper.sp9.`.
- `power-thermal.md`, `battery-charging.md`, `display.md`, `input.md` and `typecover.md` are in
  `docs/research/control/`; `sam-platform.md`, `display-power.md` and `pen-touch.md` are in `docs/research/gaps/`.

## Features

| Feature | Kernel interface | D-Bus | polkit (default for an active local user) | Evidence | Status |
|---|---|---|---|---|---|
| Performance profile | `/sys/firmware/acpi/platform_profile`, `platform_profile_choices` (surface_platform_profile: SAM TMP 0x03, FAN 0x0e) | `Performance.Profile`, `ProfileChoices`; `SetProfile(s)` | `performance` (yes) | [power-thermal.md](research/control/power-thermal.md) 1.2, 4.3. Same as the Windows mapping: confirmed | Works: switching through the four profiles with `sp9-ctl profile` makes that profile's PL and EPP readable from the kernel |
| Per-profile PL1/PL2/tau | `constraint_N_{power_limit_uw,time_window_us}` of `/sys/class/powercap/intel-rapl:0` and `intel-rapl-mmio:0` (when present). The constraints whose `constraint_N_name` is `long_term`/`short_term` are used. Package zones only; PSys and PL4 are not touched | `Pl1W`, `Pl2W`, `TauS` (the lowest over the zones), `RaplZones`, `Target*`, `LimitTable`, `FollowProfile`; `SetFollowProfile(b)` | `performance` (yes) | Table values: confirmed from the SMF policy data and code; that they are applied on AC is confirmed by measurement. Whether the MSR alone takes effect on Linux is unverified (power-thermal.md 1.3, the note on Linux) | Works: at start-up and on a profile change (sysfs, `sp9-ctl`, `powerprofilesctl`) the table's values can be read from `intel-rapl:0`, and with `follow off` they stay as they were. This device has no `intel-rapl-mmio`. Whether the written limits take effect under load (PkgWatt) is unverified |
| Power limit overrides | as above | `SetLimits(ssdddy)`, `ResetLimits(ss)` | `performance-limits` (yes) | Range: SMF node 51 (PL1 5–35 W); PL2 up to the 60 W maximum of the slider table; tau 1–28 s (the slider values 1/8/28 s) | Works: `limits set` writes 20/30 W, 10 s and EPP 128, and `reset` returns to the table values. PL1 50 W is rejected as out of range |
| EPP | `/sys/devices/system/cpu/cpufreq/policy*/energy_performance_preference` (intel_pstate active, numbers 0–255) | `Epp`, `TargetEpp`, `EppEnabled`; `SetEppEnabled(b)` | `performance` (yes) | Per-overlay EPP: confirmed from the registry (power-thermal.md 1.5). Whether the EPP can stand in for the clock limit of slider 1 is an open question (power-thermal.md 3) | Works: after a profile change the table's value (64, 153 and so on) is read, and a value power-profiles-daemon wrote over it is written again in the next cycle, so the table's value stays on all three paths (sysfs, `sp9-ctl`, `powerprofilesctl`) |
| Skin temperature guard | `temp9_input` of the `surface_thermal` (or `surface_temp`) hwmon (`skin_sensor = "temp9"`; channel number = SAM TMP instance number). It cannot be selected by label: the firmware gives channels 9–11 the same labels `I_RTS1`–`I_RTS3` as channels 1–3. The result goes to RAPL as above | `SkinGuard`, `SkinGuardActive`, `SkinSensor`, `SkinTempC`, `Temperatures`; `SetSkinGuard(b)` | `skin-guard` (yes) | Setpoints, first values and PL2 steps: confirmed from the SMF policy data; which grade each setpoint belongs to is inferred. The proportional and integral constants are not Windows values. VTS1 = SAM TMP instance 9 = `temp9`: matched from three items on the Windows side (ACPI `TZ09` with `_UID` 9 and `_STR` "VTS1", the SMF input name `I_VTS_SAM_9`, and a driver message about VTS offset configuration that names instance 9) and from `surface_temp.c`, where channel n is instance n. Not compared with a Windows reading taken at the same time | **Experimental (off by default)**, unverified, tested (fake). Sensor selection (`SkinSensor` temp9) and the temperature reading work |
| Fan rpm reading | `fan1_input` of the `surface_fan` hwmon | `FanRpm` | — | [sam-platform.md](research/gaps/sam-platform.md) 2.1 (the `surface_fan` node) | Works: `FanRpm` equals the hwmon value |
| Fan rpm request | `/dev/surface/aggregator` `SSAM_CDEV_REQUEST`: FAN TC 0x05 TID 1 IID 1 CID 0x0b, `u16` LE rpm, no response. 0 = withdraw (hands the fan back to the SAM). Whether the fan follows is judged from `surface_fan` `fan1_input` | `FanControlAllowed`, `FanRpmRequest`, `FanRequestState`; `SetFanControlAllowed(b)`, `RequestFanRpm(d)` (1600–7500 or 0) | `fan-control` (auth_admin_keep) | Linux measurement ([power-thermal.md](research/control/power-thermal.md) 1.4, the Linux measurement): 0x0b with a `u16` sets the fan speed, and 0x0c does nothing as `float32` or as `u16`. It is followed only while the SAM's own fan target is 0 | **Experimental (works only after being allowed)**, works: a request of 4000 to a stopped fan gives 3244 / 3894 / 3992 rpm at 10 / 20 / 35 s with the state going `settling` → `honoured`; changing it to 2500 gives 2512 rpm after 25 s; withdrawing gives 0 rpm within 12 s. A request of 4000 while the SAM was driving the fan itself (about 3850 rpm) also came out as `honoured`. The `not-honoured` verdict (under load, after suspend) and the withdrawal at restart are unverified, tested (fake) |
| Battery state | `/sys/class/power_supply/<type=Battery>`: `status`, `capacity`, `cycle_count`, `energy_full`, `energy_full_design`, `energy_now`, `power_now`, `voltage_now` | `Battery.Present`, `Status`, `Capacity`, `CycleCount`, `HealthPercent`, `EnergyFullWh`, `EnergyFullDesignWh`, `PowerW`, `VoltageV` | — | surface_battery attributes (code) | Works: capacity, cycles and health equal sysfs |
| Adapter | `<type=Mains>/online`, `input_power_limit` (µW, kernel patch 0010) | `AcOnline`, `AdapterW` | — | [battery-charging.md](research/control/battery-charging.md) 2, the part on adapter detection and rating | Works: `AdapterW` equals sysfs |
| Smart charging state | cdev: SAM TC 0x01 TID 1 IID 0 CID 0x3a (u8 policy), 0x41 (u16, engaged = `& 0x0440`), 0x42 (u32 seconds). Cached for 5 seconds | `SmartChargingAvailable`, `SmartChargingSupported`, `SmartChargingEngaged`, `SmartChargingStatus`, `SmartChargingCountdownS` | — | Commands and the engaged test: confirmed (from the Windows driver). What each of the bits 0x40 and 0x400 means is unresolved | Works, the meaning of the value is unverified: on AC at 77 % with `Not charging`, Supported, Available and Engaged are true and Status is 192 |
| Smart charging release | cdev: SAM CID 0x43 → u32 (0 released, 1 counting down, 2 not engaged) | `ReleaseSmartCharging() → s` | `smart-charging` (yes) | Confirmed. How long a release lasts, and whether it engages when only Linux is booted, are unverified | Unverified, tested (fake) |
| UEFI Battery Limit | efivarfs `BatteryLimitEnable-41d9f172-a9e9-4f9f-81a9-133e2d5902d4` (the first 4 bytes are attributes, then a u64). Read only | `UefiBatteryLimit` (on/off/unknown) | — | Variable and value confirmed (Windows) | Reading works (`off`). The value in the on state is unverified |
| PSR policy | debugfs `/sys/kernel/debug/dri/*/i915_edp_psr_debug` (0x00 default, 0x01 off, 0x03 force PSR1); the state is `i915_edp_psr_status`; the boot value is `/sys/module/i915/parameters/enable_psr` | `Display.EnablePsr`, `PsrStatus`, `PsrAc`, `PsrDc`, `PsrWritten`; `SetPsrPolicy(ss)` | `display-psr` (auth_admin_keep) | Windows `PsrDisableInAc=1` and `PSR2Disable=1` confirmed. That it needs to be off on AC is inferred ([display-power.md](research/gaps/display-power.md) section 3, item 2) | Works: after administrator authentication, `psr off psr1` gives `PSR mode: disabled` on AC in the next cycle, and reverting with `default default` gives `PSR1 enabled`. The switch on battery is unverified |
| Panel ICC | efivarfs `Type_1-0c3b397f-…` (measurement), `Type_2-…` (configuration) → `python3 -I sdc-blob.py … --icc` | `CalibrationAvailable`, `IccPath`; `GenerateIcc() → s` | `display-icc` (yes) | [display.md](research/control/display.md) section 4, `tools/sdc-blob.py` | Works: `panel-native.icc`, 2168 B, is created. The colour with the profile assigned is unverified |
| Panel on | `/sys/class/drm/*-eDP-*/dpms` | `PanelOn` | — | DRM sysfs | Unverified, tested (fake tree) |
| Keyboard backlight | The LED class device `/sys/class/leds/*::kbd_backlight` when there is one (`brightness`, `max_brightness`; key changes by polling `brightness_hw_changed`). Otherwise hidraw: the node whose descriptor has Output `000C:007B` (Type Cover keyboard Col05). The level goes to Output 213; Feature 213 (`000C:0517` x4) = supported levels, Feature 214 = current level, `000C:0515` in Input 213 = the backlight key (on receiving it Feature 214 is read again). The report layout is computed from the descriptor | `Input.KeyboardBacklightAvailable`, `KeyboardBacklightBackend`, `KeyboardBacklight`, `KeyboardBacklightLevels`; `SetKeyboardBacklight(u)`. `PropertiesChanged` when the key changes the level | `keyboard-backlight` (yes) | Reports and values confirmed (HID caps, Feature values `00 03 06 0C`, `06`). On a key press the firmware changes the level itself and reports it through In 213 (typecover.md 6.1). The LED comes from kernel patch 0017 (`surface::kbd_backlight` of `hid-surface`, maximum 3) | Works (LED backend `led:surface::kbd_backlight`): writing 0/1/2/3 gives the same LED `brightness`, off at 0 and on at 3 (observed). Following a press of the backlight key and the hidraw backend are unverified, tested (fake) |
| Touch off while the screen is off | `/sys/class/input/input*/inhibited` (name `IPTSD Virtual Touchscreen …`); the condition is eDP `dpms` = Off | `TouchOffWhenDisplayOff`, `TouchInhibited`; `SetTouchOffWhenDisplayOff(b)` | `touch` (yes) | Windows `SystemInputSuppressionEnabled=1` confirmed. Linux has no such policy (input.md 4, the part on touch while the screen is off). Power does not drop (Feature 9 is not used) | Partial: the setting switches on and off. Whether `inhibited` is set when the screen goes off is unverified, tested (fake) |
| iptsd thresholds | `/etc/iptsd.d/zz-sp9-helper.conf` (`[Device] 045E:0C52`, `[Contacts] Activation/DeactivationThreshold`, `[Touchscreen] DisableOnStylus`). iptsd reads its configuration directory in name order, so this comes after the preset `surface-pro-9.conf`. On a change: `systemctl try-restart 'iptsd@*.service'` | `IptsdActivationThreshold`, `IptsdDeactivationThreshold`, `IptsdDisableOnStylus`; `SetIptsd(ddi)` | `touch` (yes) | iptsd `config-loader.hpp` (code). The right values have not been measured ([pen-touch.md](research/gaps/pen-touch.md) section 3) | Works: `iptsd 22 18 on` writes the drop-in, and iptsd restarts and reads that file. `iptsd preset` removes the file and restarts iptsd |
| Pen haptics | The pen's BLE hidraw (`045E:0C0F`, the Manual Trigger output of the Simple Haptic Controller inside the Stylus application). Start = ordinal 9 (ink-continuous), stop = ordinal 2 with intensity 0xFE, test = ordinal 3 (click). Inking: once on every `BTN_TOUCH` edge of `/dev/input/event*` (name `IPTSD Virtual Stylus`) | `Pen.Available`, `InkFeedback`, `Intensity`; `SetInkFeedback(b)`, `SetIntensity(u)`, `TestPulse()` | `pen` (yes) | Reports: byte-identical to the capture posted in linux-surface #1066 (test); inking timing: confirmed by Windows ETW; that the continuous waveform lasts until the stop is inferred ([userspace/pen](../userspace/pen/README.md)) | Unverified, tested (fake): not checked with a pen connected (`Available` false). In a test that wrote to the pen's hidraw directly, ordinal 3 (click) was felt and ordinal 9 (ink-continuous) was not felt through Manual Trigger ([userspace/pen](../userspace/pen/README.md), the part on what was seen on the device) |
| Diagnostics | `post-install-check.sh OUTDIR` (read only, masked), `tar -czf` | `Diagnostics.LastReport`; `Run() → s` | `diagnostics` (yes) | `tools/linux/post-install-check.sh` | Works: `check-*.tar.gz`, 180 kB, is created and is owned by the caller |
| Reloading the settings | `/etc/sp9/helper.toml` | root `Reload() → as` (list of problems) | `reload` (auth_admin_keep) | — | Tested (fake tree: reading and validation) |

Left out:

- Writing MCHBAR 0x59A0/0x59A4 directly (`/dev/mem`): without `intel-rapl-mmio` that is kernel work
  (power-thermal.md 1.3). Through the MSR the helper only **lowers** limits.
- PL4 and PSys: left at the firmware values (power-thermal.md 4.2).
- FAN CID 0x0e and 0x0f: the kernel sends them (`surface_platform_profile`, patch 0013). The same driver also sends
  0x0b = 0 at probe and on resume, so the helper's fan request does not survive a suspend.
- Sending the fan request again: a request the fan does not follow is only reported through `FanRequestState`.
  While the SAM drives the fan, sending again changes nothing, and sending again after a resume would take back
  the fan the kernel just handed to the SAM.
- UEFI writes, battery cut-off (BAT 0x46), switching BPM off (BAT 0x19), writing touch Feature 9, pen auto-bonding
  Features 86/112: the evidence is insufficient or the action is hard to undo.
- Refresh rate, VRR, the brightness curve, applying colour: the desktop's job.

## D-Bus API

System bus name **`io.github.joonhoekim.OpenDeviceHelper1`**. It is the same name as the TB323FU helper's
(public repository `tb323fu-linux`), and one device has only one of them installed. `Device` on the root object
tells the devices apart.
Properties are read only (writes are methods); when a value changes, `PropertiesChanged` arrives within 2 seconds
with the list of invalidated properties. Methods allow interactive authorization.

| Object | Properties (type) | Methods |
|---|---|---|
| `/io/github/joonhoekim/OpenDeviceHelper1` (`…OpenDeviceHelper1`) | `Device` s, `Model` s (DMI SKU), `Version` s, `Features` as | `Reload() → as` |
| `…/Performance` | `Profile` s, `ProfileChoices` as, `OnAc` b, `FollowProfile` b, `EppEnabled` b, `Pl1W` d, `Pl2W` d, `TauS` d (−1 unknown), `RaplZones` as, `TargetPl1W` d, `TargetPl2W` d, `TargetTauS` d, `TargetEpp` i, `Epp` as, `LimitTable` a(ssdddy), `SkinGuard` b, `SkinGuardActive` b, `SkinSensor` s, `SkinTempC` d (NaN unknown), `Temperatures` a(sd), `FanRpm` i (−1), `FanControlAllowed` b, `FanRpmRequest` d (the request this daemon sent last, 0 none), `FanRequestState` s (`none`, `settling`, `honoured`, `not-honoured`, `unknown`) | `SetProfile(s)`, `SetFollowProfile(b)`, `SetEppEnabled(b)`, `SetLimits(s profile, s "ac"\|"dc", d pl1, d pl2, d tau, y epp)`, `ResetLimits(s, s)`, `SetSkinGuard(b)`, `SetFanControlAllowed(b)`, `RequestFanRpm(d)` (0 or 1600–7500, rounded to a whole number) |
| `…/Battery` | `Present` b, `Status` s, `Capacity` i, `CycleCount` i, `HealthPercent` d, `EnergyFullWh` d, `EnergyFullDesignWh` d, `PowerW` d, `VoltageV` d, `AcOnline` b, `AdapterW` d, `SmartChargingAvailable` b, `SmartChargingSupported` b, `SmartChargingEngaged` b, `SmartChargingStatus` q, `SmartChargingCountdownS` u, `UefiBatteryLimit` s | `ReleaseSmartCharging() → s` ("released", "counting-down", "not-engaged", "other:N") |
| `…/Display` | `EnablePsr` i (−2 unknown), `PsrStatus` s, `PsrAc` s, `PsrDc` s, `PsrWritten` i (−1 none), `PanelOn` b, `CalibrationAvailable` b, `IccPath` s | `SetPsrPolicy(s ac, s dc)` ("default", "off", "psr1"), `GenerateIcc() → s` |
| `…/Input` | `KeyboardBacklightAvailable` b, `KeyboardBacklightBackend` s (`led:<name>`, `hidraw:<node>`, "" when there is none), `KeyboardBacklight` i (−1), `KeyboardBacklightLevels` au (the values `SetKeyboardBacklight` takes: 0..`max_brightness` for the LED, 0 3 6 12 for hidraw), `TouchOffWhenDisplayOff` b, `TouchInhibited` b, `IptsdActivationThreshold` d, `IptsdDeactivationThreshold` d (−1 = preset), `IptsdDisableOnStylus` i (−1 preset, 0, 1) | `SetKeyboardBacklight(u)`, `SetTouchOffWhenDisplayOff(b)`, `SetIptsd(d act, d deact, i disable_on_stylus)` (negative = preset) |
| `…/Pen` | `Available` b, `InkFeedback` b, `Intensity` u | `SetInkFeedback(b)`, `SetIntensity(u)` (1–255), `TestPulse()` |
| `…/Diagnostics` | `LastReport` s | `Run() → s` |

An object is exported only when the device has the interface behind it: `Performance` (platform_profile or RAPL),
`Battery` (a battery or the cdev), `Display` (one of the i915 parameter, an eDP connector, the calibration).
`Input`, `Pen` and `Diagnostics` are always there (the cover and the pen attach later).

Errors: not authorized `AccessDenied`; out of range or unknown value `InvalidArgs`; no device or not allowed
`NotSupported`; write failed `Failed`.

## Policy details

### Profile table

The table of [power-thermal.md](research/control/power-thermal.md) 4.3 as is (`low-power` uses the battery saver
column on both AC and battery).

| `platform_profile` | AC PL1 / PL2 / tau | Battery PL1 / PL2 / tau | EPP AC / battery |
|---|---|---|---|
| `low-power` | 10 / 11 W / 1 s | same | 184 |
| `balanced` | 28 / 35 W / 28 s | 13 / 35 W / 8 s | 153 / 191 |
| `balanced-performance` | 28 / 45 W / 28 s | 28 / 35 W / 28 s | 115 |
| `performance` | 35 / 60 W / 28 s | 30 / 45 W / 28 s | 64 |

The policy step (once a second) writes RAPL only when `(profile, power source, wanted values)` differs from what it
wrote last, and writes the EPP when `(profile, power source)` changes.
For 5 seconds after writing the EPP (`engine::EPP_HOLD_S`) it reads it back on every step and, when it differs from
the value read right after the write, writes the table's value again (once per step; it stops when the window
ends).
The 5 seconds are the time in which the value may be replaced, not the time the helper spends looking. A step that
starts inside the window compares first and counts its own `dt` afterwards, so the step that ends the window still
looks once.
Counting first would wear the window down without a comparison whenever the steps are more than 1 second apart:
two steps of 2–3 seconds use up the 5 seconds, and the first of them comes before power-profiles-daemon writes (a
step reads the SAM sensors and the power supplies; a step delayed by 2 seconds was seen on the device).
When a write inside the window fails, the next step writes again.
The reason is that power-profiles-daemon watches both `platform_profile` and the power source and writes its own
EPP name. Measurement (cpu0 EPP, sampled every 0.4 seconds): after a change through sysfs or `sp9-ctl profile` the
helper's number is visible for 0.8–1.2 seconds and is then replaced by power-profiles-daemon's name; after a
change through `powerprofilesctl set`, power-profiles-daemon writes first and the helper's value stays.
The comparison is against "the value read right after the write" and not the table's number because the kernel
shows a value that has a name by that name (0 is `performance`), and the number `balance_performance` stands for
differs between CPU models.
A RAPL write puts the same values into every package zone (MSR, MMIO). The effective value is the lower of the two.

### Skin temperature guard (experimental)

- Setpoint and the PL1 it first drops to: `low-power` 45 °C / 20.5 W, `balanced` 46 / 22, `balanced-performance`
  47 / 23, `performance` 48 / 25.5.
- It engages when VTS1 ≥ setpoint. PL1 = first value + 1.5 W/°C x (setpoint − T) + 0.1 W/(°C·s) x
  ∫(setpoint − T)dt, clamped between 5 W and the table's PL1; the integral stops within the same range.
  It disengages when T < setpoint − 1 °C and PL1 is back at the table value.
- PL2 steps: 35/30/20/15 W at or above 49/50/51/52 °C; each step is released when the temperature is 0.5 °C below
  its threshold. PL2 never goes below PL1.
- Values are rounded to 0.1 W so that small fluctuations do not cause writes. Without the sensor the table values
  are used and the journal records it once.

### Fan request (experimental)

- **Command.** A `u16` rpm to FAN CID 0x0b. On this device requests of 1500 / 3000 / 4600 / 5000 / 6000 / 7500
  gave 1588 / 2977 / 4569 / 4988 / 5958 / 7407 rpm (idle, 22–25 seconds after each). 0 stops the fan within a few
  seconds and hands it back to the SAM.
- **Range 1600–7500.** The measured span as is. At the bottom the fan does not go below about 1600 (1500 → 1588);
  at the top nothing above 7500 was measured. Why it is not narrowed to the 3000–4600 of the Windows curve:
  a request is followed only while the SAM is not driving the fan, so a low value cannot block cooling, and
  spinning the fan up quietly in advance needs values below 3000.
- **When it is followed.** While the SAM drives the fan itself (about 5950 rpm during `stress-ng`), 7000 and 2000
  are both ignored. It is neither a lower nor an upper bound.
- **When it is cleared.** The kernel's `surface_platform_profile` (patch 0013) sends 0x0b = 0 at probe and on
  resume. The helper does not send the request again. At start-up, when `allow_rpm_request` is on, the helper
  sends 0 to withdraw a request an earlier instance left in the SAM (the SAM holds the request, so without this
  `FanRpmRequest` would be 0 while the fan runs at the old speed). Switching the permission off withdraws it too.
- **State verdict** (`thermal::fan_request_state`, on every policy step). The SAM returns status 0 for a request
  it ignores as well, so the helper looks at `FanRpm`. Within 30 seconds of sending (`FAN_SETTLE_S`: a stopped fan
  takes about 30 seconds to reach 3992 rpm on a request of 4000) the state is `settling`;
  after that it is `honoured` when `FanRpm` is within 10 % of the request (`FAN_TOLERANCE`: a settled value is
  within 2 %, an ignored request is off by 15 % or more), `not-honoured` otherwise, and `unknown` without
  `surface_fan`.
  The case where the SAM happens to drive the fan at the requested speed cannot be told apart from `honoured`. A
  request cleared by a resume shows as `not-honoured` because the fan stops (`FanRpmRequest` keeps the value that
  was sent).

### PSR

`default` writes nothing. When the helper has written at least once and the policy returns to `default`, it writes
0x00 (the driver default) once.
A write to `i915_edp_psr_debug` reconfigures PSR, so the helper writes only when the power source changes.

### Touch while the screen is off

When the policy is on and eDP `dpms` is `Off`, the helper sets `inhibited` to 1 on each iptsd virtual touchscreen
where it is 0, and remembers which. When the screen comes on or the policy is switched off, it sets only the
remembered ones back to 0.
When iptsd restarts and creates a new device, the next step picks the new device up.

## Packaging

| File | `install.sh` (PREFIX=/usr/local) | PREFIX=/usr | NixOS (`nix/helper.nix`) |
|---|---|---|---|
| `sp9-helperd` | `/usr/local/libexec/` | `/usr/libexec/` | `$out/libexec/` |
| `sp9-ctl` | `/usr/local/bin/` | `/usr/bin/` | `$out/bin/` |
| `post-install-check.sh`, `sdc-blob.py` | `…/libexec/sp9-helper/` | same | `$out/libexec/sp9-helper/` |
| `sp9-helperd.service` | `/etc/systemd/system/` | `/usr/lib/systemd/system/` | `systemd.packages` |
| D-Bus policy and activation | `/etc/dbus-1/system.d/`, `/usr/share/dbus-1/system-services/` | `/usr/share/dbus-1/…` | `services.dbus.packages` |
| polkit actions | `/usr/share/polkit-1/actions/` | same | `share/polkit-1/actions` of the package |
| `sp9-settings` (when built) | `/usr/local/bin/` | `/usr/bin/` | `$out/bin/` (`wrapGApp`) |
| The app's desktop file, icon and metainfo (`io.github.joonhoekim.OpenDeviceHelper.SurfacePro9`) | `/usr/local/share/{applications,icons/hicolor/scalable/apps,metainfo}/` | `/usr/share/…` | `$out/share/…` |
| Loading `surface_aggregator_cdev` | `/etc/modules-load.d/sp9-helper.conf` | `/usr/lib/modules-load.d/` | `boot.kernelModules` |

The daemon looks for the scripts in `sp9-helper/` next to its own executable (`SP9_DATA_DIR` overrides). The unit
has `ProtectSystem=strict` and writes only to `/sys`, `/etc/sp9` (ConfigurationDirectory), `/var/lib/sp9`
(StateDirectory) and `/etc/iptsd.d` (when it exists). `install.sh` creates `/etc/iptsd.d`.
On NixOS with `/etc` as a read-only overlay the iptsd drop-in cannot be written.

## Tests

| Test | Covers | Runs on |
|---|---|---|
| `sp9-helper-core` unit tests | Keyboard backlight LED preference and key report, the table and ranges, the sysfs backends (fake tree), HID descriptor parsing and report assembly (cover and pen, byte comparison with the #1066 capture), SSAM requests and verdicts (`& 0x0440`, the 0x3a bits, CID, bytes and range of the fan request), the fan request state verdict (settle time, tolerance), the cdev struct size and ioctl number (40 B, `0xC028A501`), skin guard control, the policy step (following the profile, writing an EPP that something else replaced again, with its window, slow steps and failed writes, PSR, touch), reading and validating the settings and the example file, a settings file with unknown tables | Windows, Linux |
| `sp9-helperd` D-Bus tests | Object, property and method round trips, error names, saving the settings, the fan request's permission order, range and `FanRequestState`, withdrawing the fan request at start-up, keyboard (hidraw, LED) and pen reports, the iptsd drop-in | Windows, Linux (TCP p2p) |
| `sp9-settings` unit tests | Property decoding (`a(ssdddy)`, `a(sd)`, `q`, `au`), language selection, every message ID in the source having a Korean translation | Windows (MSYS2 GTK), Linux |
| `cargo check --target x86_64-unknown-linux-gnu` | ioctl (cdev, `HIDIOCGFEATURE`), the evdev, hidraw and LED watcher threads | anywhere |

The `:` in `powercap` zone names and LED names cannot be used in Windows file names, so the RAPL and LED fake-tree
tests are skipped on Windows.

### Still to verify on Linux

What the table above leaves unverified. Confirmed: the six objects, per-profile PL and EPP, overrides, the fan
request, battery reading, writing the keyboard backlight, the PSR policy (AC), iptsd thresholds, ICC, diagnostics.

1. `turbostat` PkgWatt under the same load while switching profiles: whether the written PL takes effect on the
   actual power.
2. `sp9-ctl battery`: `Engaged` and the `SmartChargingStatus` value near 80 %, and the result of `release`.
3. Whether the level in the settings app follows a press of the backlight key.
4. After `sp9-ctl touch-off on`, switch the screen off (`DPMS`): whether touch is blocked, and whether it comes
   back when the screen is on.
5. With the pen connected, `sp9-ctl pen test`, then draw with `pen ink on`: whether start and stop are felt, and
   whether the vibration lasts through a long stroke. The continuous waveform needs the pen's speed at the
   digitizer, which `surface-pen-feedback` sends and the helper does not
   ([pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md) section 1). A pen paired in pairing mode
   disconnects after about 80 s without use and does not reconnect, so do this right after pairing.
6. `PsrStatus` after `sp9-ctl psr off psr1` and a switch to battery.
7. `sp9-ctl skin-guard on` + `stress-ng --cpu 12 -t 300`: compare `SkinTempC`, `Pl1W` and the surface temperature
   with the Windows table (power-thermal.md 3). The constants are settled according to the result.
8. `FanRequestState` = `not-honoured` with a request in place during `stress-ng --cpu 12` and after a suspend.
   Whether the fan stops after `systemctl restart sp9-helperd` with a request in place.
9. In `sp9-settings`, change the items on every page and check that they agree with `sp9-ctl status`.
