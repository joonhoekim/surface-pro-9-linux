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
| iptsd thresholds | `/etc/iptsd.d/zz-sp9-helper.conf` (`[Device] 045E:0C52`, `[Contacts] Activation/DeactivationThreshold`, `[Touchscreen] DisableOnStylus`). iptsd reads its configuration directory in name order, so this comes after the preset `surface-pro-9.conf`. On a change: a running sp9-digitizerd takes the values over D-Bus (`Touch.SetParams`) and is not restarted; otherwise `systemctl try-restart 'iptsd@*.service'` | `IptsdActivationThreshold`, `IptsdDeactivationThreshold`, `IptsdDisableOnStylus`; `SetIptsd(ddi)` | `touch` (yes) | iptsd `config-loader.hpp` (code). The right values have not been measured ([pen-touch.md](research/gaps/pen-touch.md) section 3) | Works (with iptsd): `iptsd 22 18 on` writes the drop-in, and iptsd restarts and reads that file; `iptsd preset` went back to the preset and restarted iptsd (with a version that removed the file; the file now always exists, "iptsd drop-in" below). Handing the values to sp9-digitizerd is unverified |
| Zero-force contact | `[DFT]` of the same drop-in `/etc/iptsd.d/zz-sp9-helper.conf`: `ZeroForceContact` (bool), `ZeroForceEnter` and `ZeroForceExit` (fractions of the reference), `ContactMinPressure` (fraction of the pressure range), `ZeroForceReference` (0 = learned). Read by iptsd with the patches of this repository and by sp9-digitizerd. On a change: a running sp9-digitizerd takes the values over D-Bus (`Pen.SetParams`); otherwise `systemctl try-restart 'iptsd@*.service'` | `Input.ZeroForceContact`, `ZeroForceEnter`, `ZeroForceExit`, `ContactMinPressure`, `ZeroForceReference`; `SetZeroForce(bdddd)` | `touch` (yes) | Captures of this device with the Slim Pen 2: the tip signal is at 92 % of the reference or above on the glass, at 65 % or below while lifted between letters, and at 9–30 % for a pen hovering 5 mm above the glass (measured) | With sp9-digitizerd: works, `sp9-ctl pen zero-force` changes the daemon's value while the process stays the same (measured, [test-results.md](test-results.md) TOUCH-1). With iptsd: unverified through the helper. Tested (fake: ranges, the text of the drop-in, D-Bus round trip) |
| Pen position prediction | `[Stylus] PredictionMs` of the same drop-in (ms, 0 = off). Only sp9-digitizerd reads it (iptsd has no such key). When the daemon is on the bus the value goes to it with `Pen.SetParams {PredictionMs}` and it is not restarted | `Input.PenPredictionMs`; `SetPenPrediction(d)` | `touch` (yes) | Values measured by replaying captures ([digitizerd.md](digitizerd.md) section 13) | Unverified (on the device), tested (fake: range, the text of the drop-in, D-Bus round trip, `helper.toml`) |
| Pen haptics | The pen's BLE hidraw (`045E:0C0F`, the Manual Trigger output of the Simple Haptic Controller inside the Stylus application). Start = the ordinal of the chosen waveform (`InkWaveform`: ink 9, pencil 10, marker 11, chisel-marker 12, brush 13, sparkle 15; looked up by the waveform usages 0x100B–0x1011 in the pen's waveform list, Feature 66, with these fixed ordinals when the list cannot be read), stop = ordinal 2 with intensity 0xFE, test and eraser click = ordinal 3 (click). The intensity byte is `Intensity` (`Strength` in % = intensity x 100 / 127). The eraser end (`BTN_TOOL_RUBBER`) gets one click at touch-down and no start or stop. Inking: once on every `BTN_TOUCH` edge of `/dev/input/event*` (name `IPTSD Virtual Stylus`) | `Pen.Available`, `InkFeedback`, `Intensity`, `Strength`, `InkWaveform`, `InkWaveforms`, `EraserClick`; `SetInkFeedback(b)`, `SetIntensity(u)`, `SetStrength(d)`, `SetInkWaveform(s)`, `SetEraserClick(b)`, `TestPulse()` | `pen` (yes) | Reports: byte-identical to the capture posted in linux-surface #1066 (test); inking timing and values: confirmed from a Windows capture ([pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md) section 5). Strength = the intensity byte of Output 65 (minimum 2, default 127, maximum 254), and no continuous waveform with the eraser end, only the click is felt: measured on Windows and Linux (the same note, 1.7 and 1.8). The waveforms applications use (of 230 continuous starts: brush 172, pencil 50, ink 3): Windows captures | Works: with `pen ink on` the pen vibrates while drawing and stops on lift (observed). Strength: 30 % is not felt and 200 % is clearly stronger than 100 % (observed). Eraser click: one click at touch-down, as intended (observed). Unverified, tested (fake): choosing the waveform (the default changed from ink to brush; looking the ordinal up in the list), `pen test` |
| Pen speed feedback | Feature 9 (63 B, FF00:C9) of the digitizer hidraw (`045E:0C52`): `8e a1` (DisplayStates, the 10-bit FastHostId), `8e a5` (pen feedback V06: sequence number, attributes `4c`/`4e` (contact = bit 1), FastHostId, protocol type, ZFI, process type, speed 0–63 or `ff`, valid bitmap). The speed comes from `ABS_X` and `ABS_Y` of the stylus evdev (in mm through the `EVIOCGABS` resolution), the range state from `BTN_TOOL_PEN` and `BTN_TOOL_RUBBER`. The FastHostId is in `/var/lib/sp9/pen-fast-host-id` | `SpeedFeedback`, `SpeedFullScale`, `DigitizerAvailable`; `SetSpeedFeedback(b)`, `SetSpeedFullScale(d)` (50–2000 mm/s, 0 = the default 300), `SpeedDeadZone`; `SetSpeedDeadZone(d)` (0–100 mm/s) | `pen` (yes) | Frame bytes, speed scale (63 = 300 mm/s, one step = 4.74 mm/s) and dead zone (about 15 mm/s): measured by reading the Windows touch processor's buffer (pen-vendor-exchange.md 1.8). Speed computation: static analysis. That the speed field switches the continuous waveforms on: measured and observed on Linux (section 1 there) | Works (with the earlier frames): the continuous waveform played with the helper alone (observed); after a resume the digitizer has lost the FastHostId (Feature 115 `fe ff`) and its hidraw node is new, and within 4 s of the pen approaching the helper had found the new node and sent again (measured). Unverified, tested (fake): whether the continuous waveform plays with the frames changed to match Windows (contact bit, no speed for hover and eraser, protocol and process type, valid bitmap) and the scale of 300 mm/s; the dead zone; the case of an iptsd restart alone. The `8e a1` right after a resume works: within 3 s of a resume without the pen, Feature 115 was back at the FastHostId (measured) |
| Cover coupling | The cover hidraw (`045E:09B0`, collection FFF4:0001): Feature 112 (bit 0010), Feature 86 (host address), Input 110 (pen ID, pen address, flags), Output 111 (pen ID, pen address, bit 0008, 16 B TK). The Bluetooth management socket (`HCI_CHANNEL_CONTROL`): Read Controller Index List, Read Controller Information, Add Remote OOB Data, Pair Device, Unpair Device, Cancel Pair Device, User Confirmation Negative Reply, the events Device Disconnected, Authentication Failed, New Long Term Key and User Confirmation Request. A raw HCI socket: LE Extended Create Connection (0x2043), its cancel (0x200E) and LE Set Address Resolution Enable (0x202D). BlueZ D-Bus: `Adapter1.Pairable`, `Device1.Paired`, `Bonded`, `Connected`, `Trusted` | `AutoCouple`, `CoverAvailable`, `CoupleState`, `CoupleResult`, `CoupledAddress`; `SetAutoCouple(b)`, `Couple()`, `CoupleForce()` | `pen-couple` (auth_admin_keep) | Procedure: measured on Linux (pen-vendor-exchange.md section 2.5); coupling address: static analysis, verified with this unit's values. Kernel patch 0023. The probe (key to the cover only, connection, encryption with the stored key): measured on Linux for a healthy bond. Why a bond must not be removed first: a failure on the device ("Cover coupling" below) | Works (measured with the helper): `pen couple` on a pen without a bond → `coupled`: at the first attempt 12 times out of 12 after the bond was removed while the pen was connected (about 11 s), and at the first attempt 6 times out of 6 with the pen left on the controller's resolving list (bond removed after a disconnect). The version that did not switch address resolution off managed 0 out of 2 in that state (Resolved Random link, Confirm value failed). `pen couple` on a bonded pen that is disconnected in the cover → probe → `connected` (2.2 s); the bond is unchanged and the pen's HID attaches. The same automatically. `pen couple` on a connected pen → `already-coupled`, and `--force` does not touch it either. Pairable is as it was after a coupling. Unverified, tested (fake: the order of `coupling::couple`, the decisions): the new order in automatic coupling (when the pen is put in), replacing a dead bond and `stale-bond` and `no-answer` of the probe, `CoupleForce()` on a pen without a bond, the path where address resolution cannot be switched off (a scan in progress), bluetoothd not answering, the refusal under privacy, the growing interval after failures, `unsupported-kernel` |
| Diagnostics | `post-install-check.sh OUTDIR` (read only, masked), `tar -czf` | `Diagnostics.LastReport`; `Run() → s` | `diagnostics` (auth_admin_keep) | `tools/linux/post-install-check.sh` | Works: `check-*.tar.gz`, 180 kB, is created and is owned by the caller |
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
| `…/Input` | `KeyboardBacklightAvailable` b, `KeyboardBacklightBackend` s (`led:<name>`, `hidraw:<node>`, "" when there is none), `KeyboardBacklight` i (−1), `KeyboardBacklightLevels` au (the values `SetKeyboardBacklight` takes: 0..`max_brightness` for the LED, 0 3 6 12 for hidraw), `TouchOffWhenDisplayOff` b, `TouchInhibited` b, `IptsdActivationThreshold` d, `IptsdDeactivationThreshold` d (−1 = preset), `IptsdDisableOnStylus` i (−1 preset, 0, 1), `ZeroForceContact` b, `ZeroForceEnter` d (%), `ZeroForceExit` d (%), `ContactMinPressure` d (%), `ZeroForceReference` d (0 = learned by the touch daemon), `PenPredictionMs` d (0 = off) | `SetKeyboardBacklight(u)`, `SetTouchOffWhenDisplayOff(b)`, `SetIptsd(d act, d deact, i disable_on_stylus)` (negative = preset), `SetZeroForce(b on, d enter, d exit, d floor, d reference)` (percentages; 1 ≤ exit < enter ≤ 150, floor 0–50, reference ≥ 0; outside that `InvalidArgs`), `SetPenPrediction(d ms)` (0–30) |
| `…/Pen` | `Available` b, `InkFeedback` b (pen haptics as a whole), `Intensity` u, `Strength` d (% = intensity x 100 / 127), `InkWaveform` s, `InkWaveforms` as (`ink`, `pencil`, `marker`, `chisel-marker`, `brush`, `sparkle`), `EraserClick` b, `SpeedFeedback` b, `SpeedFullScale` d (mm/s, the value in use), `SpeedDeadZone` d (mm/s, 0 = none), `DigitizerAvailable` b, `AutoCouple` b, `CoverAvailable` b, `CoupleState` s (`idle`, `waiting-for-pen`, `coupling`), `CoupleResult` s (`coupled`, `coupled-no-hid`, `already-coupled`, `connected`, `no-answer`, `stale-bond`, `no-cover`, `no-pen`, `unsupported-kernel`, `bluetooth-off`, `failed: …`, "" when there is none), `CoupledAddress` s (the coupling address of the pen the cover announced last, with only the last byte shown) | `SetInkFeedback(b)`, `SetIntensity(u)` (2–254), `SetStrength(d)` (%; intensity = `round(127 x % / 100)` clamped to 2–254; a negative value is `InvalidArgs`), `SetInkWaveform(s)` (a name outside the list is `InvalidArgs`), `SetEraserClick(b)`, `TestPulse()`, `SetSpeedFeedback(b)`, `SetSpeedFullScale(d)` (50–2000, 0 = default), `SetSpeedDeadZone(d)` (0–100), `SetAutoCouple(b)`, `Couple()` (returns at once; progress is in `CoupleState` and `CoupleResult`. Couples a pen without a bond, reports `already-coupled` for a connected one, probes a bonded pen that is not connected. `NotSupported` on the session bus and with `--fake`), `CoupleForce()` (replaces the bond of a pen that is not connected, without a probe) |
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

### iptsd drop-in

One file, `/etc/iptsd.d/zz-sp9-helper.conf`, holds the iptsd thresholds (`SetIptsd`), zero-force contact
(`SetZeroForce`) and the pen prediction (`SetPenPrediction`) (`input::Iptsd::render`). iptsd reads its
configuration directory in name order, so the file comes after the preset.

- After `[Device]` (045E:0C52) come `[Touchscreen]` and `[Contacts]`, only for thresholds that differ from the
  preset, then always `[Stylus] PredictionMs` and the five keys of `[DFT]`. With zero-force contact off the file
  still has `ZeroForceContact = false` and the other values: whatever the preset contains, this file states the
  current values. So the file always exists (earlier versions removed it when nothing differed from the preset).
- Percentages are written as fractions (85 % → `0.85`). `ZeroForceExp` and `ZeroForceMinSamples` are not
  written.
- The file is written only when its text changes. The helper then looks for sp9-digitizerd on the system bus
  (the owner of `io.github.joonhoekim.Sp9Digitizer1`): when it is there, the helper calls `SetParams` on its
  `Pen` interface (the zero-force values and `PredictionMs`) and on its `Touch` interface (the thresholds and
  `DisableOnStylus`, with the built-in values where the helper sets none), and nothing is restarted: a restart
  would replace the daemon's input devices ([digitizerd.md](digitizerd.md) section 6). When it is not there,
  iptsd is restarted, and only if the file changed.
  At start-up the helper brings the file in line with the settings as well: the first start of a version that
  adds keys to the file restarts iptsd once.
- An iptsd without the patches does not read keys it does not know: its `config-loader.hpp` fetches the keys it
  knows by name from the `INIReader` (`this->get(ini, "DFT", …)`).
- When iptsd restarts, the stylus evdev disappears and comes back. The pen thread leaves its inner loop on the
  read error, writes a stop for the running waveform, takes the contact back from the digitizer and then looks
  for the new node (daemon test `leaving_a_stroke_cleans_up`).

Example (the defaults plus thresholds of 22/18):

```ini
# Written by sp9-helperd (sp9-ctl iptsd, sp9-ctl pen zero-force, sp9-ctl pen predict). Loaded after the platform preset.
[Device]
Vendor = 0x045E
Product = 0x0C52

[Contacts]
ActivationThreshold = 22
DeactivationThreshold = 18

[Stylus]
PredictionMs = 0

[DFT]
ZeroForceContact = true
ZeroForceEnter = 0.85
ZeroForceExit = 0.75
ContactMinPressure = 0.03
ZeroForceReference = 0
```

### Inking waveform

- `pen.ink_waveform` (default `brush`). Name → waveform usage (page 0x0E): ink 0x100B, pencil 0x100C, marker
  0x100D, chisel-marker 0x100E, brush 0x100F, sparkle 0x1011. The eraser waveform (0x1010) cannot be chosen.
- The ordinal is looked up in the pen's waveform list: in the Feature report (66 on this pen), the Waveform
  List collection (000E:0010) holds a waveform usage in each of the ordinal slots 3–22. The list is read once
  when the name of the pen node changes (not for every stroke).
  When it cannot be read, or the waveform is not in it, the helper uses the ordinals of the list Windows read
  from this pen (9, 10, 11, 12, 13, 15).
- A settings file without the key gets `brush` (the waveform used to be ink always). A name outside the list in
  the file changes only that value to `brush`, with a warning (the other settings are read as they are).

### Pen speed feedback

One thread (`penwatch.rs`) reads the stylus evdev and does both the inking haptics and the speed feedback. With
`pen.ink_feedback` off it does not open the evdev either. It starts after the bus name has been acquired.
What to send is decided by `stylus::Tracker` (core, no I/O); the thread only reads and writes. The items under
"The thread" have passed unit tests and FIFO tests only (unverified on the device).

The thread:

- **Decisions at the end of a frame.** Keys (`BTN_TOUCH`, `BTN_TOOL_PEN`, `BTN_TOOL_RUBBER`) and positions are
  collected within a frame, and the inking haptics and the feedback frame are decided at `SYN_REPORT`. iptsd
  emits `BTN_TOUCH` before the tool key (iptsd's `stylus.hpp`): deciding on single events would treat an eraser
  end that touches down as it comes into range as the tip.
- **Time.** `EVIOCSCLOCKID` switches the event timestamps to `CLOCK_MONOTONIC`, and those are used (the time of
  the read when that fails). Several frames in one `read()` do not get the same time.
- **Held-back frames.** A state change that falls inside the minimum interval of 7 ms is not dropped: the thread
  waits for `Feeder::wait_s` only (the poll timeout is shortened by that much) and sends it. A lift right after
  a touch-down does not wait for the next event.
- **Standing still, and the age of an estimate.** With the tip in contact and no new position for 60 ms,
  `frame()` discards the estimate and sends 0 (checked on every frame, not on a timeout: iptsd emits `ABS_MISC`
  in every frame, so no timeout occurs in range). A sample after a gap of more than 32 ms discards the earlier
  estimate (a fast hover followed by a stop and a touch-down does not send the old speed).
- **Tracking regardless of the setting.** Range, contact and position are tracked with `speed_feedback` off as
  well; the setting only decides whether to send. Switching it off during contact sends one frame without a
  speed.
- **Resynchronising.** Right after the node is opened and after `SYN_DROPPED`, the key state is read with
  `EVIOCGKEY` and taken over (a missed `BTN_TOUCH` 0 does not leave a missing stop or a speed while hovering).
  The pen's waveform list is read outside strokes, when the pen node is seen (no BLE read during a stroke: if
  there is no list then, the fixed ordinals are used).
- **On leaving.** When the stylus node goes away, when pen haptics are switched off and on SIGTERM, the thread
  writes a stop for the running waveform and, when the last frame sent was a contact (or its delivery is
  uncertain), a hover frame. SIGTERM is taken by the main loop, which tells the thread and waits up to 2
  seconds.
- **The digitizer node.** The node that was found is kept open for writing (a handle is bound to its device;
  hidraw names are reused, so writing by name could send Feature 9 to another device after a resume). After a
  resume the handle is dropped and the node looked for again. The same when Feature 115 cannot be read.
  When a write or a read fails the node is looked for again at intervals that double from 2 to 60 seconds, and
  the same error is logged once. After a failed write the state is "delivery unknown", so that a hover frame is
  sent once even if the pen has left range in the meantime.
- **The pen node.** The pen that was found is remembered, and before a write the helper checks that the name
  still belongs to the pen (`HID_ID`); if not it looks again. sysfs is not scanned for every stroke.
- **No resolution.** When the resolution of the stylus axes cannot be read no speed is sent (contact frames
  with speed `ff` and valid `0x0352`: not a shape seen on Windows). One journal line.
- **One program only.** While it writes to the digitizer the thread holds an exclusive flock on
  `/run/lock/surface-pen-digitizer.lock` (`/run/lock` is in the unit's `ReadWritePaths`). When another program
  holds it the thread does not write and looks again every 10 seconds. When the file cannot be created it
  writes without a lock. `surface-pen-feedback` does not take this lock yet.
- The list of hidraw nodes is cached for 1.5 seconds (`hid::Cached`): `Available`, `DigitizerAvailable` and
  `CoverAvailable` of `Pen`, and the state comparison every 2 seconds, read the uevent and descriptor of every
  node once instead of each time.

The messages:

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
  `round(min(v / full_scale, 1) x 63)`. The default full scale is 300 mm/s (the Windows formula
  `min(v, 30) x 63 / 30` with v in 0.01 mm/ms; run over Raw Input recorded on Windows, this estimator gives the
  Windows values with ratio 1.000 at 300, and 0.855 at 353). When `pen.speed_dead_zone_mm_s` is above 0, an averaged speed below it gives the
  value 0 (the valid bit stays set). Windows sends 0 below about 15 mm/s.
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
  When a write fails the node is looked for again (after 2 seconds, doubling up to 60), and once found the
  helper starts over with `8e a1`.
  After sending it the helper reads Feature 115 of the digitizer (the FastHostId the firmware received) and
  writes a journal line when it differs.
- FastHostId: `/var/lib/sp9/pen-fast-host-id` → if missing, `/var/lib/surface-pen/fast-host-id` (that of
  `surface-pen-feedback`) is taken over and stored → if missing, a random number in 1–0x3FF.
- Run together with `surface-pen-feedback`, the two writers alternate and overwrite each other's speed value.
  Run only one.
- Settings: `pen.full_scale_mm_s` (300 when absent), `pen.speed_dead_zone_mm_s` (default 0). The old key `pen.speed_full_scale` is read but not used:
  the old `pen strength` wrote to it, so its value is not carried over; the helper writes
  `pen.speed_full_scale = N is no longer used …` to the journal at start-up and drops the key the next time it
  saves the file.

### Cover coupling

One thread (`couple.rs`) does it. With `pen.auto_couple` off and no manual request it opens neither the cover
node nor a socket. It runs only on the real device on the system bus.

1. When the cover node is found (a known cover, `cover::KNOWN_COVERS`: bus 0x19, `045E:09B0`, and a descriptor
   with FFF4:0001, Features 86 and 112, Input 110, Output 111; another HID device whose descriptor merely
   matches gets neither the host address nor a key), the helper reads the adapter index with Read Controller
   Index List on the management socket (the lowest one) and the host address with Read Controller Information,
   and writes Feature 112 = bit 0010 and Feature 86 = host address (LSB first, bit 0009 = 0). It writes them
   again after a resume and on a manual request (every write of 86 makes the cover announce the pen in its
   slot).
2. On Input 110 it computes the coupling address: the low 46 bits of `SHA1(16 B constant ‖ pen ID reversed)`
   with the two top bits set, XOR the host address (when bit 0012 is set; otherwise NOT).
3. `cover::on_seated` decides what to do (unit-tested):

   | State of the pen | Automatic | Request (`Couple()`) |
   |---|---|---|
   | Bonded, connected | `already-coupled` | `already-coupled` (also for `CoupleForce()`) |
   | bluetoothd does not answer | Nothing | `failed: bluetoothd does not answer` |
   | No bond | Couple. Skipped inside the interval that follows a failure | Couple |
   | Bonded, dropped for an encryption failure within the last 2 minutes | Remove the bond and couple. Skipped inside the interval that follows a failure | Remove the bond and couple |
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
   announcement takes the fourth row of the table); a disconnect for another reason gives `no-answer`; still
   `Device1.Connected` after 2 seconds, with `ServicesResolved` or the pen's hidraw present, gives `connected`;
   when the services do not come up within 10 seconds, `no-answer`. The helper does not start the encryption
   itself ("A bonded pen" below).
   The proof that a bond is dead is used for 2 minutes after the probe only (`DEAD_FRESH`). A pen put in later
   is probed again.
5. Coupling (`coupling::couple`; the order is in the core and unit-tested against a fake, and the helper
   supplies the operations it uses):
   1. The TK: the key last written to that pen with Output 111 and not used by a pairing
      (`cover::PendingKeys`: one a probe wrote, one a failed attempt wrote), otherwise 16 B from `getrandom`.
      When `Adapter1.Pairable` is off it is switched on, and back at the end (while it is off the pairing
      request goes out as "No bonding" and no keys arrive).
   2. Add Remote OOB Data (LE random, hash192 = TK, the rest 0). Invalid Parameters means `unsupported-kernel`.
      Then the Output 111 report is assembled and the raw HCI socket opened. **A failure up to this point
      leaves the bond as it is.**
   3. Only when a bond is being replaced: Unpair Device (disconnect = 0). The pen is not connected, so
      bluetoothd removes the keys and the device object.
   4. The key is handed over first: Output 111 is written (pen ID and address as received, bit 0008 as in the
      input, the TK), the helper waits 2.5 seconds **without connecting**, writes Feature 86 again, and when the
      cover announces the same pen (within 3 seconds; otherwise a failure) writes Output 111 again with the same
      TK. Then LE Set Address Resolution Enable = 0 (if it is accepted) and LE Extended Create Connection
      (random address, scan interval 22.5 ms and window 11.25 ms, connection interval 15 ms, supervision timeout
      1 s); when the connection attempt is over, address resolution is switched on again. When the link comes up
      through a resolved address (peer address type 2 or 3) there is no pairing on it: the TK is removed from
      the kernel and the next round follows. When a link already exists, the key is written once, without the
      hand-over and without a connection attempt.
      When there is no connection within 3 seconds the attempt is cancelled and the result of the cancel is read
      for up to 0.5 seconds (a connection made at the last moment counts). Without one: `no-answer`.
      On Command Disallowed (0x0C: the kernel happens to be making another connection) once more after 0.2
      seconds; Connection Already Exists (0x0B) counts as connected.
   5. A wait of 0.4 seconds. Then: if the pen's New Long Term Key has already arrived it is a success
      (bluetoothd's pairing) → Add Remote OOB Data again → Pair Device (LE random, NoInputNoOutput, up to 35
      seconds; Cancel Pair Device when there is no answer).
      - Success: wait 2 seconds for New Long Term Key. With it, done. Without it, again (right after a
        connection, before the kernel has created the security manager channel, Pair Device returns success
        without pairing).
      - Already Paired: a success when the key event arrives (2 seconds). When no key event was ever seen, an
        old key BlueZ does not show is left in the kernel: Unpair Device once and again. A second time is a
        failure.
      - Busy: another pairing (bluetoothd's) is running. Its key is awaited for 5 seconds; without it the round
        ends.
      - authentication failed: the pen refused. Once more on the same link after 1 second; a second refusal
        ends the round. When the pen answers without the OOB flag and the kernel raises a User Confirmation
        Request, the helper sends a Negative Reply at once and that attempt ends in authentication failed.
      - not connected: the link went down. On to the next round.
   6. When a round ends in failure: 20 seconds later Feature 86 is written again, and when the cover announces
      the same pen one more round runs from step 4 (handing the key over) with the same key (two rounds per
      request). No announcement and no link: failure. Both refused: `failed: the pen refused the pairing`.
   7. However it ends, the TK is removed from the kernel (Add Remote OOB Data with zeros). The remembered key
      is dropped only when the pairing succeeded.
6. When BlueZ does not show the bond (`Paired`, `Bonded`) within 5 seconds:
   `failed: … BlueZ does not show the bond`. When it does, `Trusted` is set and the helper waits up to 10
   seconds for the pen's HoG hidraw: `coupled` or `coupled-no-hid`.
7. For a pen that failed, Feature 86 is written again after 1, 2, 5 and then every 15 minutes
   (`cover::retry_after`) and the coupling is tried again. Automatic announcements inside that interval are
   skipped; a request goes ahead at once.

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
  moment the command fails with Command Disallowed (0x0C). The helper tries once more ("The price of raw HCI
  commands" below).
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
  Encryption Change successful, Connected in BlueZ, the pen's HID. The pen accepts the old LTK while holding
  the new key.
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
  announcement 1 minute later (2, 5 and 15 minutes after repeated failures). Before the removal the helper does
  what can fail without the pen (whether the kernel takes the TK, assembling the report, the raw socket).
  The handling of Already Paired after Pair Device (Unpair Device while connected) applies only to "no bond in
  BlueZ, a key left in the kernel, and no key event during this coupling": bluetoothd's device is then not
  paired and `device_set_unpaired()` does nothing.
- **The pen's connection is never dropped.** After the host drops the link (`bluetoothctl disconnect`) the pen
  does not advertise for more than 30 seconds, also when it is moved; after the pen has disconnected by itself
  for idling it is back within 3.5 seconds (measured). The helper makes no call that drops the link (Unpair
  Device with disconnect = 0, and only while not connected).
  A pen that has just been coupled does not answer a new Create Connection (measured), so a freshly coupled pen
  is left alone (60 seconds automatically, 15 seconds for a request).
- **The key is handed over first.** A coupling that connected right after writing the key was refused on the
  device: the pen answered without a key (no OOB flag) or made its Confirm with an earlier key, and it was the
  same when the same key was repeated on the same link and when the cover was asked again 3.5 seconds apart
  (measured).
  A procedure that wrote the key without a connection, asked the cover again 2.5 seconds later, wrote the same
  key again and then connected, coupled at the first attempt 3 times out of 3 (measured). The pen is taken to
  accept a key from the cover only while it is not connected, and slowly (inferred).
  The wait after the connection is 0.4 seconds: with 1 second bluetoothd's pairing starts first and consumes the
  single-use TK (measured: the Request of the second SMP exchange had no OOB flag). After a refusal there is
  one repetition only, and the next round comes 20 seconds later (repetitions a few seconds apart all failed,
  and an attempt 25 seconds later worked).
  A key written without a pairing (a probe, a failed attempt) stays in the pen. So the key written is remembered
  per pen (in memory only) and the same key is used until a pairing succeeds. A failed pairing may leave the
  link up (`smp_failure()` does not disconnect): when there is a link no new connection is made.
- **bluetoothd's pairing.** On a new link bluetoothd starts a pairing as well, about a second later (measured).
  With a TK in the kernel that pairing is legacy OOB too and can finish first. New Long Term Key reaches every
  management socket before the response to Pair Device, so that event is the proof of success, whoever paired.
  An Unpair Device on the Already Paired that follows would remove the bond just made and make bluetoothd drop
  the link (`device_set_unpaired()` → `device_request_disconnect()`: a link dropped by the host, after which
  the pen is silent for 30 seconds). So Unpair Device is used only when no key event was ever seen.
  A kernel with the current patch 0023 removes the TK once a pairing has used it, so it is added again before
  every Pair Device.
- **The proof of a dead bond.** Only Device Disconnected with reason 4 (`hci_encrypt_change_evt()` sets
  `HCI_CONN_AUTH_FAILURE` on PIN or Key Missing only). The Authentication Failed event of LE is a failed
  **pairing** (`smp_failure()` → `mgmt_auth_failed()`), and the one of a pairing bluetoothd started reaches the
  helper's socket too: it is not used as proof (only the one for that address during a probe is looked at).
  The proof has a time and is not used after 2 minutes. It is cleared when a new LTK of that pen arrives or the
  pen is seen connected.
- **bluetoothd not answering.** If a `Device1.Paired` that cannot be read were taken as "no bond", a key left in
  the kernel would make Pair Device answer Already Paired, and the Unpair would remove a good bond (a window
  `couple_restart_bluetooth` opens). So when `Adapter1.Powered` cannot be read either, the state is "unknown"
  and nothing is done. BlueZ calls are given up after 3 seconds.
- **Privacy.** When the adapter uses an RPA of its own (`Privacy`), the kernel computes the Confirm with that
  address and the pen with the public address the cover gave it. With that setting there is no coupling:
  `failed: the adapter uses private addresses…`.
- **A link through a resolved address.** While the pen is on the controller's resolving list (the kernel puts a
  bonded pen there while it is disconnected, and removing the bond does not take it off before the next scan),
  the Create Connection to the coupling address matches the pen's RPA advertising, and the link comes up with
  peer address type 3 (Resolved Random).
  On that link the kernel computes c1 with the identity address and type 3 (`le_conn_update_addr()` in
  `hci_event.c` puts the address and type of the event into `resp_addr` as they are and does not use
  `peer_rpa`; measured: the Confirm of the failed traces matches only with those values). The pen computes with
  the RPA and type 1, hence Confirm value failed.
  So address resolution is off while a coupling connects: with it off only the directed advertising from the
  static coupling address matches. A probe does not switch it off (a bonded pen is encrypted with the stored
  key on either link).
  The kernel does not know about the change, so its flag (`HCI_LL_RPA_RESOLUTION`) stays as it is. The helper
  switches resolution on again after the connection, and the kernel itself switches it off and on whenever it
  rewrites the list. While the controller is scanning or connecting the command is refused (0x0C), and the
  helper then connects with resolution on.
- **The price of raw HCI commands.** The Command Status of a Create Connection the kernel does not know about
  can complete a request the kernel has pending (a window of a few ms), and when the kernel makes another LE
  connection at that moment one of the two fails with 0x0C. The helper tries once more on 0x0C. A management
  command that connects without a scan would be the place to replace this.
- **Bluetooth switched off.** With the adapter off the result is `bluetooth-off` and the helper looks again
  every 3 seconds (one journal line). When it is switched off while the thread runs (New Settings), or the
  adapter goes away (Index Removed), the session ends and the helper looks again. An error for a missing
  adapter is logged once while it stays the same. Once the adapter is on and the host address has been written
  to the cover, `bluetooth-off` is cleared. That write makes the cover announce a pen in its slot, so a pen
  that was already there at boot goes through step 3 as well.
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
(StateDirectory), `/etc/iptsd.d` and `/run/lock` (when they exist). Address families and device access are not restricted
(hidraw, evdev, Bluetooth sockets). `install.sh` creates `/etc/iptsd.d`.
On NixOS with `/etc` as a read-only overlay the iptsd drop-in cannot be written.

## Tests

| Test | Covers | Runs on |
|---|---|---|
| `sp9-helper-core` unit tests | Keyboard backlight LED preference and key report, the table and ranges, the text of the iptsd drop-in and the ranges of zero-force contact, the sysfs backends (fake tree), HID descriptor parsing and report assembly (cover and pen, byte comparison with the #1066 capture), the pen feedback buffers (`8e a1`; `8e a5` byte for byte against frames read on Windows), the speed estimate (standing still, the age of an estimate), when to send (a speed only for tip contact, held-back frames, after a failed write, no resolution), taking over the FastHostId, stylus tracking (`stylus::Tracker`: decisions at the end of a frame, switching the setting, resynchronising, leaving), a hidraw handle not following the name and the cache of the node list, keeping a settings file that cannot be read, strength % ↔ intensity byte, the inking waveform (names and fixed ordinals, looking the ordinal up in the real pen descriptor and in the list Windows read), the dead zone, the eraser click decision, the order of a coupling (`coupling::couple`, against a fake: handing the key over before the connection and the 2.5 seconds, Pair Device 0.4 seconds after the connection, one repetition on the same link, the second round after 20 seconds, the TK before every Pair Device, Already Paired and Busy with the key event, the checks before Unpair, no connection attempt when a link exists, no pairing on a link through a resolved address), the classification of management events (only reason 4 proves a dead bond), choosing the remembered key (`PendingKeys`), known covers only, bluetoothd not answering, the interval after failures, the bytes of Index List, Cancel Pair Device, New Settings, New Long Term Key and User Confirmation, the decision for a seated pen (`on_seated`: connected, no bond, dead bond, probe, request and force) and the meaning of a probe result (`after_probe`), the old `speed_full_scale` setting, the cover reports (Features 86 and 112, Input 110, Output 111 with the real descriptor), the coupling address (the vectors of the Python tests) and SHA-1, the bytes of the management commands and of the HCI commands and events, SSAM requests and verdicts (`& 0x0440`, the 0x3a bits, CID, bytes and range of the fan request), the fan request state verdict (settle time, tolerance), the cdev struct size and ioctl number (40 B, `0xC028A501`), skin guard control, the policy step (following the profile, writing an EPP that something else replaced again, with its window, slow steps and failed writes, PSR, touch), reading and validating the settings and the example file, a settings file with unknown tables | Windows, Linux |
| `sp9-helperd` D-Bus tests | Object, property and method round trips, error names, saving the settings, the fan request's permission order, range and `FanRequestState`, withdrawing the fan request at start-up, keyboard (hidraw, LED) and pen reports, zero-force contact (the range of `SetZeroForce`, `[DFT]` in the drop-in), the pen prediction (range, drop-in, `helper.toml`), the pen settings (strength %, inking waveform, dead zone, eraser click, speed feedback, the full-scale range and default, automatic coupling) and the refusal of `Couple()` (on the session bus, and a request while a coupling runs), stylus events (a FIFO: tip and eraser end, two frames in one read, switching speed feedback off during contact, a stop request and the node going away, the digitizer lock) → digitizer Feature 9 and pen Output 65 (Linux), `.bad` for a settings file that cannot be read, the intensity range, the iptsd drop-in | Windows, Linux (TCP p2p) |
| `sp9-settings` unit tests | Property decoding (`a(ssdddy)`, `a(sd)`, `q`, `au`), language selection, every message ID in the source having a Korean translation | Windows (MSYS2 GTK), Linux |
| `cargo check --target x86_64-unknown-linux-gnu` | ioctl (cdev, `HIDIOCGFEATURE`), the evdev, hidraw and LED watcher threads | anywhere |

The `:` in `powercap` zone names and LED names cannot be used in Windows file names, so the RAPL and LED fake-tree
tests are skipped on Windows.

### Still to verify on Linux

What the table above leaves unverified. Confirmed: the six objects, per-profile PL and EPP, overrides, the fan
request, battery reading, writing the keyboard backlight, the PSR policy (AC), iptsd thresholds, ICC, diagnostics,
pen inking haptics and speed feedback (with the earlier frames), the pen strength and the eraser click, cover
coupling (a pen without a bond, and the probe of a bonded pen).

1. `turbostat` PkgWatt under the same load while switching profiles: whether the written PL takes effect on the
   actual power.
2. `sp9-ctl battery`: `Engaged` and the `SmartChargingStatus` value near 80 %, and the result of `release`.
3. Whether the level in the settings app follows a press of the backlight key.
4. After `sp9-ctl touch-off on`, switch the screen off (`DPMS`): whether touch is blocked, and whether it comes
   back when the screen is on.
5. `sp9-ctl pen test` (click). Whether the six waveforms of `pen waveform` feel different from each other, the
   default (brush), and whether the chosen waveform still plays after the pen has reconnected. A continuous
   vibration with the eraser end is not something to verify: there is none on Windows either
   ([pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md) 1.7).
6. `PsrStatus` after `sp9-ctl psr off psr1` and a switch to battery.
7. `sp9-ctl skin-guard on` + `stress-ng --cpu 12 -t 300`: compare `SkinTempC`, `Pl1W` and the surface temperature
   with the Windows table (power-thermal.md 3). The constants are settled according to the result.
8. `FanRequestState` = `not-honoured` with a request in place during `stress-ng --cpu 12` and after a suspend.
   Whether the fan stops after `systemctl restart sp9-helperd` with a request in place.
9. In `sp9-settings`, change the items on every page and check that they agree with `sp9-ctl status`.
10. Pen speed feedback: whether the vibration while drawing at the scale of 300 mm/s is similar to Windows;
    whether with `pen dead-zone 15` the vibration goes away in very slow strokes and comes back with `off`; no
    vibration when a continuous waveform is started with the pen hovering after the tip has lifted
    (`pen-haptic`; the "no speed" frame went out); and whether it still plays after
    `systemctl restart 'iptsd@*'`. The thread's handling of events (see "Pen speed feedback"): whether the vibration ends soon when a fast stroke
    stops with the tip held down; whether an eraser end brought straight down from outside range gives only the
    click; whether no vibration is left behind by `systemctl restart sp9-helperd` or
    `systemctl restart 'iptsd@*'` during a stroke; whether the `EVIOCGABS` resolution is read (no
    `no resolution` in the journal); whether `digitizer hidrawN` appears once, with the new name, after a
    resume.
11. Cover coupling: whether automatic coupling (`auto-couple on`, taking a pen without a bond out and putting
    it back) works like `pen couple`. After Windows has coupled the pen again: the probe gives `stale-bond` →
    `dropped the old bond` → `coupled` (if the probe gives `no-answer`: `pen couple --force`). Whether a second
    `pen couple` during a coupling is refused with `a coupling is in progress`. Whether the bond survives when
    the pen is put in during `systemctl restart bluetooth`. A coupling while a scan for other BLE devices runs
    (address resolution cannot be switched off). `unsupported-kernel` on a kernel without 0023.
12. Zero-force contact: with the patched iptsd, whether the values of `sp9-ctl pen zero-force` agree with
    `[DFT]` in `/etc/iptsd.d/zz-sp9-helper.conf`; whether iptsd restarts after `pen zero-force 60 45`, `off`
    and `pen floor 8` (and does not when the same value is given again) and handwriting changes accordingly;
    whether no vibration is left behind when it is changed during a stroke; whether the items in the settings
    app follow. With sp9-digitizerd: whether `pen predict` and `iptsd` take effect without a restart of the
    daemon (for `pen zero-force` this was measured: [test-results.md](test-results.md) TOUCH-1).
