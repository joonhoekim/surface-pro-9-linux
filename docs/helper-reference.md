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
| Pen haptics | The pen's BLE hidraw (`045E:0C0F`, the Manual Trigger output of the Simple Haptic Controller inside the Stylus application). Start = ordinal 9 (ink-continuous), stop = ordinal 2 with intensity 0xFE, test and eraser click = ordinal 3 (click). The intensity byte is `Intensity` (`Strength` in % = intensity x 100 / 127). The eraser end (`BTN_TOOL_RUBBER`) gets one click at touch-down and no start or stop. Inking: once on every `BTN_TOUCH` edge of `/dev/input/event*` (name `IPTSD Virtual Stylus`) | `Pen.Available`, `InkFeedback`, `Intensity`, `Strength`, `EraserClick`; `SetInkFeedback(b)`, `SetIntensity(u)`, `SetStrength(d)`, `SetEraserClick(b)`, `TestPulse()` | `pen` (yes) | Reports: byte-identical to the capture posted in linux-surface #1066 (test); inking timing and values: confirmed from a Windows capture ([pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md) section 5). Strength = the intensity byte of Output 65 (minimum 2, default 127, maximum 254), and no continuous waveform with the eraser end, only the click is felt: measured on Windows and Linux (the same note, 1.7 and 1.8) | Works: with `pen ink on` the pen vibrates while drawing and stops on lift (observed). Unverified, tested (fake): `Strength` (since it became the intensity byte), the eraser click (since the start command for the eraser end was dropped), `pen test` |
| Pen speed feedback | Feature 9 (63 B, FF00:C9) of the digitizer hidraw (`045E:0C52`): `8e a1` (DisplayStates, the 10-bit FastHostId), `8e a5` (pen feedback V06: sequence number, attributes `4c`/`4e` (contact = bit 1), FastHostId, protocol type, ZFI, process type, speed 0–63 or `ff`, valid bitmap). The speed comes from `ABS_X` and `ABS_Y` of the stylus evdev (in mm through the `EVIOCGABS` resolution), the range state from `BTN_TOOL_PEN` and `BTN_TOOL_RUBBER`. The FastHostId is in `/var/lib/sp9/pen-fast-host-id` | `SpeedFeedback`, `SpeedFullScale`, `DigitizerAvailable`; `SetSpeedFeedback(b)`, `SetSpeedFullScale(d)` (50–2000 mm/s, 0 = the default 353) | `pen` (yes) | Frame bytes and speed scale (one step = 5.6 mm/s): measured by reading the Windows touch processor's buffer (pen-vendor-exchange.md 1.8). Speed computation: static analysis. That the speed field switches the continuous waveforms on: measured and observed on Linux (section 1 there) | Works (with the earlier frames): the continuous waveform played with the helper alone (observed); after a resume the digitizer has lost the FastHostId (Feature 115 `fe ff`) and its hidraw node is new, and within 4 s of the pen approaching the helper had found the new node and sent again (measured). Unverified, tested (fake): whether the continuous waveform plays with the frames changed to match Windows (contact bit, no speed for hover and eraser, protocol and process type, valid bitmap) and the scale of 353 mm/s; the case of an iptsd restart alone. The `8e a1` right after a resume works: within 3 s of a resume without the pen, Feature 115 was back at the FastHostId (measured) |
| Cover coupling | The cover hidraw (`045E:09B0`, collection FFF4:0001): Feature 112 (bit 0010), Feature 86 (host address), Input 110 (pen ID, pen address, flags), Output 111 (pen ID, pen address, bit 0008, 16 B TK). The Bluetooth management socket (`HCI_CHANNEL_CONTROL`): Read Controller Information, Add Remote OOB Data, Pair Device, Unpair Device, the events Device Disconnected, Authentication Failed and New Long Term Key. A raw HCI socket: LE Extended Create Connection (0x2043) and its cancel (0x200E). BlueZ D-Bus: `Adapter1.Pairable`, `Device1.Paired`, `Bonded`, `Connected`, `Trusted` | `AutoCouple`, `CoverAvailable`, `CoupleState`, `CoupleResult`, `CoupledAddress`; `SetAutoCouple(b)`, `Couple()`, `CoupleForce()` | `pen-couple` (auth_admin_keep) | Procedure: measured on Linux (pen-vendor-exchange.md section 2.5); coupling address: static analysis, verified with this unit's values. Kernel patch 0023. The probe (key to the cover only, connection, encryption with the stored key): measured on Linux for a healthy bond. Why a bond must not be removed first: a failure on the device ("Cover coupling" below) | Works (pen without a bond): automatic coupling and `pen couple` both end in `coupled`: connection 0.4 s, pairing 0.5 s, Paired, Bonded and Trusted in BlueZ, and the pen's HID attached without restarting bluetoothd (measured). Works (pen with a bond): `pen couple` on a connected pen → `already-coupled`, nothing is touched. `pen couple` on a pen in the cover that a suspend disconnected → probe → `connected`; the bond (Paired, Bonded, Trusted) is unchanged and the pen's HID attaches. Automatic: restarting the helper in the same state, it probes after 6 s and reports `connected` after 10 s (measured). Right after a resume it does nothing by itself. Once, a `pen couple` right after a resume ended in `no-pen` (the cover did not announce the pen; running it again works). Unverified, tested (fake: the decisions): `no-answer` and `stale-bond` of the probe, removing a dead bond with Unpair Device and coupling (after Windows coupled again), `CoupleForce()`, the refusal of `pen couple` within 15 s of a coupling, `unsupported-kernel` on a kernel without the patch |
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
- UEFI writes, battery cut-off (BAT 0x46), switching BPM off (BAT 0x19): the evidence is insufficient or the
  action is hard to undo.
- The other messages of digitizer Feature 9 (power saving by display state and so on) and the LocalId and Ack
  fields of the pen feedback: these values belong to the identification procedure between pen and digitizer and
  are not made up.
- The pen's CFU exchange (Output 45) and the CouplingKey (Output 85): the first is a firmware offer, the second
  has never been used on this unit (pen-vendor-exchange.md).
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
| `…/Pen` | `Available` b, `InkFeedback` b (pen haptics as a whole), `Intensity` u, `Strength` d (% = intensity x 100 / 127), `EraserClick` b, `SpeedFeedback` b, `SpeedFullScale` d (mm/s, the value in use), `DigitizerAvailable` b, `AutoCouple` b, `CoverAvailable` b, `CoupleState` s (`idle`, `waiting-for-pen`, `coupling`), `CoupleResult` s (`coupled`, `coupled-no-hid`, `already-coupled`, `connected`, `no-answer`, `stale-bond`, `no-cover`, `no-pen`, `unsupported-kernel`, `bluetooth-off`, `failed: …`, "" when there is none), `CoupledAddress` s (the coupling address of the pen the cover announced last, with only the last byte shown) | `SetInkFeedback(b)`, `SetIntensity(u)` (1–255), `SetStrength(d)` (%; intensity = `round(127 x % / 100)` clamped to 2–254; a negative value is `InvalidArgs`), `SetEraserClick(b)`, `TestPulse()`, `SetSpeedFeedback(b)`, `SetSpeedFullScale(d)` (50–2000, 0 = default), `SetAutoCouple(b)`, `Couple()` (returns at once; progress is in `CoupleState` and `CoupleResult`. Couples a pen without a bond, reports `already-coupled` for a connected one, probes a bonded pen that is not connected. `NotSupported` on the session bus and with `--fake`), `CoupleForce()` (replaces the bond of a pen that is not connected, without a probe) |
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

### Pen speed feedback

One thread (`penwatch.rs`) reads the stylus evdev and does both the inking haptics and the speed feedback. With
`pen.ink_feedback` off it does not open the evdev either.

- Messages are 64 B with the report ID. `8e a1`: display state 1, hinge 0x190, the FastHostId (offset 41).
- `8e a5` equals the frame the Windows touch processor sends without a pen ID and LocalId (about half of
  Windows' own contact frames have this shape, and the pen vibrates with them). Offsets count from the report ID:

  | Offset | Field | Hover | Tip contact | Eraser contact |
  |---|---|---|---|---|
  | 3 | Sequence number | +1 on every message | | |
  | 4–8 | | `02 01 00 90 01` | same | same |
  | 9–36 | Pen ID, a 16-bit value (changes slowly on Windows) and others | 0 | 0 | 0 |
  | 37 | LocalId | `ff` | `ff` | `ff` |
  | 38 | Attributes (contact = bit 1; bit 0 is not used) | `4c` | `4e` | `4e` |
  | 40–41 | FastHostId | | | |
  | 42 | Protocol type | 1 | 1 | 1 |
  | 43 | ZFI | 3 | 1 | 3 |
  | 44 | Process type | 2 and 4 alternating per frame (parity of the sequence number) | | |
  | 45 | Speed | `ff` | 0–63 | `ff` |
  | 47–48 | Valid bitmap | `0x0352` | `0x0752` | `0x0352` |

  The unit tests compare the hover frame and a contact frame with speed 0 byte for byte.
- Speed: pairs of samples 14–32 ms apart, the last three estimates averaged with 0.5/0.33/0.17,
  `round(min(v / full_scale, 1) x 63)`. The default full scale is 353 mm/s (the Windows step of 5.6 mm/s x 63).
  With the tip in contact and no new position for 60 ms the pen is taken to stand still and 0 is sent.
- When it sends: when the state changes (hover, contact and its speed value, eraser contact), and once a second
  in range without a change. Not again within 7 ms (Windows sends on every frame, 134 a second).
  When the tip lifts a hover frame without a speed goes out, and when the pen leaves range while in contact one
  more hover frame is sent: no old speed stays in the digitizer. Windows goes on sending hover frames for about
  a second after the pen has left.
- `8e a1`: when the digitizer node is found (again); after a resume (`CLOCK_BOOTTIME` − `CLOCK_MONOTONIC` grew
  by more than a second), without waiting for the pen, and once more 3.8 seconds later, as Windows does after the
  display comes back (Windows raises a sequence number in the second one; where that field is is not known, so
  the helper sends the same content); and when the pen comes into range 30 seconds or more after the last one.
  When a write fails the node is looked for again every 2 seconds, and once found the helper starts over with
  `8e a1`.
  After sending it the helper reads Feature 115 of the digitizer (the FastHostId the firmware received) and
  writes a journal line when it differs.
- FastHostId: `/var/lib/sp9/pen-fast-host-id` → if missing, `/var/lib/surface-pen/fast-host-id` (that of
  `surface-pen-feedback`) is taken over and stored → if missing, a random number in 1–0x3FF.
- Run together with `surface-pen-feedback`, the two writers alternate and overwrite each other's speed value.
  Run only one.
- Setting: `pen.full_scale_mm_s` (353 when absent). The old key `pen.speed_full_scale` is read but not used:
  the old `pen strength` wrote to it, so its value is not carried over; the helper writes
  `pen.speed_full_scale = N is no longer used …` to the journal at start-up and drops the key the next time it
  saves the file.

### Cover coupling

One thread (`couple.rs`) does it. With `pen.auto_couple` off and no manual request it opens neither the cover
node nor a socket. It runs only on the real device on the system bus.

1. When the cover node is found (descriptor with FFF4:0001, Features 86 and 112, Input 110, Output 111; the
   digitizer `0C52`, which has the same collection, is excluded), the helper reads the host address with Read
   Controller Information on the management socket and writes Feature 112 = bit 0010 and Feature 86 = host
   address (LSB first, bit 0009 = 0). It writes them again after a resume and on a manual request (every write
   of 86 makes the cover announce the pen in its slot).
2. On Input 110 it computes the coupling address: the low 46 bits of `SHA1(16 B constant ‖ pen ID reversed)`
   with the two top bits set, XOR the host address (when bit 0012 is set; otherwise NOT).
3. `cover::on_seated` decides what to do (unit-tested):

   | State of the pen | Automatic | Request (`Couple()`) |
   |---|---|---|
   | Bonded, connected | `already-coupled` | `already-coupled` (also for `CoupleForce()`) |
   | No bond | Couple. Skipped when a coupling failed within the last 60 seconds | Couple |
   | Bonded, an encryption failure has been seen | Remove the bond and couple. Skipped when that failed within the last 60 seconds | Remove the bond and couple |
   | Bonded, not connected | Probe. Skipped when the pen was coupled or probed within the last 60 seconds, or right after a resume | Probe. `CoupleForce()` removes the bond and couples |

   The automatic probe is not immediate: after 6 seconds the helper looks at `Device1.Connected` again (when the
   pen has connected by itself: `already-coupled`), and otherwise writes Feature 86 again and probes when the
   cover announces the same pen within 3 seconds (if it does not, the pen was taken out).
   A request is refused with `failed: coupled a moment ago…` when that pen was coupled within the last 15
   seconds.
4. Probe: 16 B from `/dev/urandom` are written to the cover only, with Output 111 (not given to the kernel),
   and LE Extended Create Connection follows. No connection: `no-answer`. With a connection the helper watches
   the management socket: Device Disconnected with reason authentication failure, or Authentication Failed, for
   that address gives `stale-bond` (the address is remembered and Feature 86 written again: the next
   announcement takes the third row of the table); a disconnect for another reason gives `no-answer`; still
   `Device1.Connected` after 2 seconds gives `connected`. The helper does not start the encryption itself
   ("A bonded pen" below).
   Removing a bond (a dead one, or `CoupleForce()`): Unpair Device with disconnect = 0. The pen is not
   connected, so bluetoothd removes the keys and the device object. When `Adapter1.Pairable` is off it is
   switched on, and back at the end (while it is off the pairing request goes out as "No bonding" and no keys
   arrive).
5. 16 B from `/dev/urandom` as the TK: Add Remote OOB Data (LE random, hash192 = TK, the rest 0). Status Invalid
   Parameters means `unsupported-kernel`.
6. A raw HCI socket is opened and filtered, Output 111 is written (pen ID and address as received, bit 0008 as
   in the input, the TK), and LE Extended Create Connection follows at once (random address, scan interval
   22.5 ms and window 11.25 ms, connection interval 15 ms, supervision timeout 1 s). When there is no connection
   within 3 seconds it is cancelled and the attempt ends in `no-answer` (Feature 86 is written again 60 seconds
   later).
7. After a wait of 0.3 s, Pair Device (LE random, NoInputNoOutput), for up to 30 seconds. On Already Paired:
   Unpair Device and once more. The proof of a pairing is the pen's New Long Term Key event: when it does not
   arrive within 2 seconds the step is repeated, up to four times. Right after a connection comes up the kernel
   has not created the security manager channel yet, and Pair Device then returns success at once without
   pairing (measured: an attempt that ended that way has no key, and the Just Works pairing bluetoothd starts
   afterwards fails).
8. Whatever the outcome, the TK is removed (Add Remote OOB Data with zeros). When BlueZ shows the bond,
   `Trusted` is set. The helper waits up to 10 seconds for the pen's HoG hidraw: `coupled` or `coupled-no-hid`.

Design choices and their basis (kernel v7.3.4 `net/bluetooth`, BlueZ 5.87):

- **The connection is made with Create Connection on a raw HCI socket.** Every way of letting the kernel connect
  waits for a scan to report the device's advertisement first: `pair_device()` in `mgmt.c` and
  `l2cap_chan_connect()` in `l2cap_core.c` call `hci_connect_le_scan()` (pending list and passive scan), and
  `Device1.Connect` and `Adapter1.ConnectDevice` of BlueZ are L2CAP connections and so the same. But the pen's
  advertising after Output 111 (directed at the host, about 1 second) was never reported by this controller's
  scan (measured: 0 reports for the coupling address, 0 directed advertisements). At the same moment Create
  Connection connected within 0.1 seconds (measured). `ConnectDevice` is, besides, an experimental API
  (`bluetoothd --experimental`).
  The kernel creates the `hci_conn` from the completion event of a connection it did not start
  (`le_conn_complete_evt()` in `hci_event.c`) and emits Device Connected.
  The price: the kernel does not know about the attempt, so when it is making another LE connection at that
  moment the command fails with Command Disallowed (`failed: create connection: HCI status 0x0c`). Trying again
  works.
- **The pairing goes through the helper's management socket (Pair Device); bluetoothd is left alone.**
  bluetoothd follows without being touched: its listener takes the ATT channel (`connect_cb()` in
  `src/gatt-database.c` → `device_attach_att()`; `l2cap_core.c` notifies listening fixed channels of every new
  connection), and it receives the keys as management events, stores them and marks the device bonded
  (`new_long_term_key_callback()` in `src/adapter.c` → `device_set_bonded()`). When service discovery is done it
  accepts HoG (`hog_accept()` in `profiles/input/hog.c` → `bt_hog_attach()`) and the HoG profile's
  `auto_connect` arms auto-connect.
  The `btmon` record of a coupling test shows exactly that: 1.2 seconds after the connection bluetoothd sent Add
  Device (auto-connect), wrote the CCCDs and read the Report Map (measured). Using `Device1.Pair()` instead
  would gain nothing: bluetoothd sends the same Pair Device (`adapter_create_bonding()`), the default agent's IO
  capability gets mixed in, and an existing bond is refused with `AlreadyExists`. With a TK stored, patch 0023
  picks OOB whatever the IO capability.
- **When HoG does not attach.** In earlier tests HoG was once not attached right after the bonding (the cause
  was not separated; those tests restarted bluetoothd straight away). `Device1.Connect()` does nothing for an LE
  device that is connected (`dev_connect()` in `src/device.c`), so there is no D-Bus call that asks for the
  attachment.
  The helper waits 10 seconds for the hidraw and reports `coupled-no-hid` when there is none. On the device the
  hidraw appeared within 3 s of the bonding (measured).
  bluetoothd is restarted only with `pen.couple_restart_bluetooth` on (other Bluetooth devices drop for a
  moment).
- **A bonded pen: checked, not removed.** An earlier version took "bonded, and still not connected after 6
  seconds" to mean a stale bond and coupled after `RemoveDevice`. On the device a good bond was lost that way
  (measured): the pen was healthy and merely not connected, it did not answer the connection made after the
  bond was removed (`the pen did not answer the connection`), and was left without a bond. A manual coupling 16
  seconds later brought it back.
  "Not connected" is a normal state: a healthy pen asleep in the cover, and any pen after a resume from suspend
  (it does not connect by itself for 60 seconds: measured). The flags of Input 110 are the same for a bonded and
  an unbonded pen (`07`: measured).
  Connecting without a key cannot check the bond either: without Output 111 the pen does not accept the
  connection (measured: 4 seconds).
  So the key goes to the cover only and the helper connects. bluetoothd does the rest: its ATT listener takes
  the new link (`connect_cb()` in `src/gatt-database.c`), and for a bonded device it raises the security level
  (`device_attach_att()` in `src/device.c`: "Elevating security level since LTK is available"), which makes the
  kernel start encryption with the stored LTK. Measured: LE Start Encryption 0.1 seconds after the connection,
  Encryption Change successful, Connected in BlueZ, the pen's HID. The pen accepts the old LTK while holding a
  new key it never used.
  With a dead bond the controller reports PIN or Key Missing, and the kernel drops the link and emits Device
  Disconnected with reason authentication failure (4) (`hci_encrypt_change_evt()` in `hci_event.c` sets
  `HCI_CONN_AUTH_FAILURE` and disconnects). This case has not been seen yet.
  BlueZ only postpones auto-connect for that reason and keeps the bond (`auth_failures` in `src/device.c`).
- **The old bond cannot be kept until the new key has arrived.** Three things prevent it. With a key in the
  kernel, Pair Device is refused with Already Paired (`hci_bdaddr_is_paired()` in `pair_device()`, `mgmt.c`).
  With a key present, bluetoothd starts encryption with the old key on connection as described above, and on
  failure the kernel drops that link.
  And Unpair Device on a connected device makes bluetoothd, on Device Unpaired, disconnect it
  (`unpaired_callback()` in `src/adapter.c` → `device_set_unpaired()` in `src/device.c`:
  `device_request_disconnect()` when connected, otherwise `btd_adapter_remove_device()`).
  So the order is: a bond that is proven dead is removed with Unpair Device while the pen is not connected, and
  then the pen is coupled. Only `CoupleForce()` removes without proof. `RemoveDevice` is not used (Unpair Device
  removes bluetoothd's device object as well).
  When the pen does not answer after the removal it is a pen without a bond, and coupling is tried again at the
  announcement 60 seconds later.
  The handling of Already Paired after Pair Device (Unpair Device while connected) remains only for "no bond in
  BlueZ but a key left in the kernel": bluetoothd's device is then not paired and `device_set_unpaired()` does
  nothing.
- **The pen's connection is never dropped.** After the host drops the link (`bluetoothctl disconnect`) the pen
  does not advertise for more than 30 seconds, also when it is moved; after the pen has disconnected by itself
  for idling it is back within 3.5 seconds (measured). The helper makes no call that drops the link (Unpair
  Device with disconnect = 0, and only while not connected).
  A pen that has just been coupled does not answer a new Create Connection (measured), so a freshly coupled pen
  is left alone (60 seconds automatically, 15 seconds for a request).
- **The key a probe leaves behind.** After a healthy probe the pen holds a TK that was never used. It is taken
  to be harmless (inferred: in earlier experiments coupling still worked after keys had been written several
  times without pairing).
- **Bluetooth switched off.** With the adapter off the result is `bluetooth-off` and the helper looks again
  every 3 seconds (one journal line). Once it is on and the host address has been written to the cover,
  `bluetooth-off` is cleared. That write makes the cover announce a pen in its slot, so a pen that was already
  there at boot goes through step 3 as well.
- **Kernel requirement.** A kernel without patch 0023 answers Invalid Parameters to Add Remote OOB Data for an
  LE address when the 192-bit values are not zero (`add_remote_oob_data()` in `mgmt.c`: "Enforce zero-valued
  192-bit parameters as long as legacy SMP OOB isn't implemented").
  The helper reports that as `unsupported-kernel` and writes no key to the cover. It does not probe at start-up
  (probing would need a management command).

Addresses and keys: D-Bus and the journal get only the last byte of an address (`cover::mask_addr`). Outside the
process the TK goes only to the kernel and to the cover.

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
(StateDirectory) and `/etc/iptsd.d` (when it exists). Address families and device access are not restricted
(hidraw, evdev, Bluetooth sockets). `install.sh` creates `/etc/iptsd.d`.
On NixOS with `/etc` as a read-only overlay the iptsd drop-in cannot be written.

## Tests

| Test | Covers | Runs on |
|---|---|---|
| `sp9-helper-core` unit tests | Keyboard backlight LED preference and key report, the table and ranges, the sysfs backends (fake tree), HID descriptor parsing and report assembly (cover and pen, byte comparison with the #1066 capture), the pen feedback buffers (`8e a1`; `8e a5` byte for byte against frames read on Windows), the speed estimate, when to send (a speed only for tip contact), taking over the FastHostId, strength % ↔ intensity byte, the eraser click decision, the decision for a seated pen (`on_seated`: connected, no bond, dead bond, probe, request and force) and the meaning of a probe result (`after_probe`), the old `speed_full_scale` setting, the cover reports (Features 86 and 112, Input 110, Output 111 with the real descriptor), the coupling address (the vectors of the Python tests) and SHA-1, the bytes of the management commands and of the HCI commands and events, SSAM requests and verdicts (`& 0x0440`, the 0x3a bits, CID, bytes and range of the fan request), the fan request state verdict (settle time, tolerance), the cdev struct size and ioctl number (40 B, `0xC028A501`), skin guard control, the policy step (following the profile, writing an EPP that something else replaced again, with its window, slow steps and failed writes, PSR, touch), reading and validating the settings and the example file, a settings file with unknown tables | Windows, Linux |
| `sp9-helperd` D-Bus tests | Object, property and method round trips, error names, saving the settings, the fan request's permission order, range and `FanRequestState`, withdrawing the fan request at start-up, keyboard (hidraw, LED) and pen reports, the pen settings (strength %, eraser click, speed feedback, the full-scale range and default, automatic coupling) and the refusal of `Couple()`, stylus events (a FIFO: tip and eraser end) → digitizer Feature 9 and pen Output 65 (Linux), the iptsd drop-in | Windows, Linux (TCP p2p) |
| `sp9-settings` unit tests | Property decoding (`a(ssdddy)`, `a(sd)`, `q`, `au`), language selection, every message ID in the source having a Korean translation | Windows (MSYS2 GTK), Linux |
| `cargo check --target x86_64-unknown-linux-gnu` | ioctl (cdev, `HIDIOCGFEATURE`), the evdev, hidraw and LED watcher threads | anywhere |

The `:` in `powercap` zone names and LED names cannot be used in Windows file names, so the RAPL and LED fake-tree
tests are skipped on Windows.

### Still to verify on Linux

What the table above leaves unverified. Confirmed: the six objects, per-profile PL and EPP, overrides, the fan
request, battery reading, writing the keyboard backlight, the PSR policy (AC), iptsd thresholds, ICC, diagnostics,
pen inking haptics and speed feedback (with the earlier frames), cover coupling (a pen without a bond, and the
probe of a bonded pen).

1. `turbostat` PkgWatt under the same load while switching profiles: whether the written PL takes effect on the
   actual power.
2. `sp9-ctl battery`: `Engaged` and the `SmartChargingStatus` value near 80 %, and the result of `release`.
3. Whether the level in the settings app follows a press of the backlight key.
4. After `sp9-ctl touch-off on`, switch the screen off (`DPMS`): whether touch is blocked, and whether it comes
   back when the screen is on.
5. `sp9-ctl pen test` (click). The difference in the strength of the vibration while drawing at
   `pen strength 2`, `100` and `200`. One click when the eraser end touches down, nothing while rubbing and on
   lift, nothing at all with `pen eraser-click off`. A continuous vibration with the eraser end is not
   something to verify: there is none on Windows either ([pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md) 1.7).
6. `PsrStatus` after `sp9-ctl psr off psr1` and a switch to battery.
7. `sp9-ctl skin-guard on` + `stress-ng --cpu 12 -t 300`: compare `SkinTempC`, `Pl1W` and the surface temperature
   with the Windows table (power-thermal.md 3). The constants are settled according to the result.
8. `FanRequestState` = `not-honoured` with a request in place during `stress-ng --cpu 12` and after a suspend.
   Whether the fan stops after `systemctl restart sp9-helperd` with a request in place.
9. In `sp9-settings`, change the items on every page and check that they agree with `sp9-ctl status`.
10. Pen speed feedback: whether the continuous vibration plays as before with the changed frames and the scale
    of 353 mm/s; no vibration when a continuous waveform is started with the pen hovering after the tip has
    lifted (`pen-haptic`; the "no speed" frame went out); and whether it still plays after
    `systemctl restart 'iptsd@*'`.
11. Cover coupling: after Windows has coupled the pen again, `sp9-ctl pen couple` (or automatic coupling) with
    the pen in the cover → the probe gives `stale-bond`, and the bond is then replaced, ending in `coupled`. If
    the probe only gives `no-answer`: `pen couple --force`. Whether `pen couple` within 15 s of a coupling is
    refused. `unsupported-kernel` on a kernel without 0023.
