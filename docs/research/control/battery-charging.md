# Battery and charging control: what Windows does and the same control on Linux

This document describes which Windows component controls the battery and charging of this device, through which path (SAM request,
ACPI, UEFI variable, HID) and by sending what, and how the same control is done, or has to be done, on Linux
(v7.3.4 + linux-surface 7.3 + `kernel/patches`).
The SAM node layout and the list of differences (G9 battery extras, G1 UCSI) are in [gaps/sam-platform](../gaps/sam-platform.md) and
are not repeated here.

Notation:

- A SAM request is written as `TC/TID/IID CID` + payload. TC 0x01 = SAM, 0x02 = BAT. TID 1 = SAM. If the response flag (SNC) is 0 it
  is a write without response.
- Facts about Windows drivers name the driver they come from, without positions inside the file. `SID` =
  `SurfaceIntegrationDriver.sys` (`MSHW0343`, 93.52.12.0), `SB` = `SurfaceBattery.sys` (2.117.139.0), `BatteryDm` =
  `Services\BatteryDm.exe` of the Surface app proxy (`Microsoft.SurfaceAppProxy` 5.102.139.0).
- "Surface SSDT" = the Surface-specific SSDT of this device's ACPI tables. It is not in this repository; dump it from your own device
  ([`dump-acpi.ps1`](../../../tools/windows/dump-acpi.ps1)) or see `https://github.com/linux-surface/acpidumps`, directory
  `surface_pro_9_intel`. `L/path:N` = path and line in the Linux v7.3.4 source tree.
- Evidence levels: **confirmed** (confirmed by driver and application analysis or by a query), **inferred** (judged from names and
  structure, behaviour not observed). The few values read on Linux are in section 4; everything else is the Windows side.

Tools:

- [`surface-battery-query.ps1`](../../../tools/windows/surface-battery-query.ps1): sends only the read commands through the same IOCTL
  as the app and decodes the values (Administrator).
- [`uefi-vars.ps1`](../../../tools/windows/uefi-vars.ps1): list and values of UEFI variables (Administrator).
- [`battery-state.ps1`](../../../tools/windows/battery-state.ps1): WMI `root\wmi` battery classes; `-Watch` follows the state.

## Summary

- **There are two charging control paths.** Static information (BIX/BST/AC/adapter) is read by `SB` with BAT commands and handed to the
  battery class. "Surface-specific" functions such as smart charging, authentication and protection mode go app → IOCTL `0x225C00` of
  `SID` (Administrator only) → translated into a single SAM request. No command was found by which the Windows kernel itself sets charge
  current or voltage. The charging algorithm is held by the SAM firmware, and the host sends only reads and a few switches (confirmed:
  every write command found is in the tables below).
- **Smart charging (hold at 80%) = the SAM's BCL (Battery Charge Limit).** The state is `SAM 0x41` (u16); **the engaged test is
  `value & 0x0440 != 0`** (per `SID`). Disengaging is `SAM 0x43` (u32 result: 0 success, 1 countdown running, 2 not engaged). There is
  no command to switch it back on (the app refuses to set its smart-charging-enabled property to true).
  The countdown is `SAM 0x42` (u32 seconds), and support is policy bit 0x10 of `SAM 0x3a` (confirmed).
- **At boot the SID tells the SAM that "the OS knows BCL".** ACPI `_OSC` CAP0 bit 19 (battery charge limiting) → `BCLE`=1 (Surface
  SSDT, under the condition `SCAP`&0x400) → `SID` reads BCLE through `_DSM` fn6, and if the u16 response to `SAM 0x2c` (payload
  `00 02 02 00 00`) contains 0x400 it sends `SAM 0x2f {BCLE?0x10:0, 0x10}`. `SCAP` = UEFI variable `VarSamCap` = 0x1F8F (confirmed:
  values; inferred: the exact meaning of 0x2c/0x2f).
- **The status indication while charging is limited is `_BST` state bit 3 (0x08).** `SB` shows "Battery Charge Limiting" from this bit
  of the BAT 0x03 response. Linux `surface_battery` does not look at this bit. ACPI `battery.c` treats the same bit as `Not charging`
  (L/drivers/acpi/battery.c:247).
- **The UEFI "Battery Limit" (50%) is in the UEFI variable `BatteryLimitEnable` ({41d9f172-a9e9-4f9f-81a9-133e2d5902d4}, 8 bytes,
  NV+BS+RT) and is 0 (off) on this device.** The Windows drivers do not read this variable. The OS knows it only through the SAM policy
  (`SAM 0x3a` bit 0x01 `BatteryLimit`) (inferred: not checked with the limit switched on).
- **#167 (linux-surface/kernel) has the right direction and differs in details.** TC and CID of 0x3a/0x41/0x43 are the same as on
  Windows. Differences: (1) it tests 0x41 with `== 1` (Windows uses `& 0x440`), (2) it reads the authentication state with `SAM 0x1b`
  (on Windows 0x1b is in the **BAT** TC and is the command that *runs* authentication; the authentication *state* is
  `authenticationStatus` in the BAT 0x23 response), (3) it grows the property array even when smart charging is not supported, (4) it
  sends IID 1 of the battery node (Windows sends 0).
- **Functions the SP9 firmware does not answer**: for user charge mode (BAT 0x09/0x0a), PSU general information (BAT 0x11) and
  cut-the-top (BAT 0x25) the IOCTL ends with 121 (timeout). Predictive charging (BAT 0x26) answers but `featureSupported`=0
  (confirmed: queries on this device).

## 1. Path structure

```
Surface app (UI) ─gRPC→ SurfaceBroker ─→ BatteryDm.exe ─IOCTL 0x225C00─→ SID(MSHW0343) ─SSH IOCTL 0x4001c48─→ SSH → SAM
                                                                             └ boot: _OSC/BCLE → SAM 0x2c, 0x2f
battery class (battc) ←── SurfaceBattery.sys ─(DMF SamCommunicationViaSsh)─→ SAM BAT 0x01…0x1a, events 0x15–0x1a, 0x53
UcmUcsiCx ←── SurfaceUcmUcsiHidClient.sys ─HID feature/input reports 0x32·0x33─→ SAM HID IID 7 (UCSI PPM)
```

- `BatteryDm` first opens the interface `{31076642-1572-4fbb-a5dd-4b879b515476}` ("SBCD") and, if that does not exist,
  `{54f596c1-3b0d-40f2-97a4-fcbcfd253d3b}` ("SID"). No driver on this device has the SBCD GUID. On the SP9 it is the SID path.
- The IOCTL code is `CTL_CODE(0x22, 0x700, METHOD_BUFFERED, FILE_READ_ACCESS)` = `0x225C00`. In the SID's IOCTL table this code is
  Administrator only.
- Packet: `[length, 0x03, 0x00(CommandGroup), command, 0x00(instance), payload…]`. The SID checks length ≥5 and version byte ≥1, and
  then takes either the SAM path or an ACPI method path (`BSDN`/`BAUT`/`BPMN`/`BPMC`…), depending on a mode value in its device
  context. The ACPI tables of this device do not have those methods, so it is the SAM path.

### 1.1 App command → SAM request (the SID's translation)

| App command (`BatteryDevice.BatteryCommand`) | SAM request | Payload/response | Where the app calls it |
|---|---|---|---|
| 0 `Get_Bix` | none (`STATUS_NOT_SUPPORTED`) | | |
| 1 `Get_ExtInfoGeneralStandard` | BAT 0x22, response | Response in 1.2 below | |
| 2 `Get_ExtInfoGeneralCustom` | BAT 0x23, response | Response in 1.2 below. Authentication state, `cmdRevision` (gray-out support) | `GetBatteryAuthenticated`, `IsGrayOutSupportedInFw` |
| 3 `Get_ExtInfoBatteryProtection` | BAT 0x24, response | BPM structure | |
| 4 `Get_ExtInfoCutTheTop` | BAT 0x25, response | CTT structure. No answer on the SP9 | |
| 5 `Get_Protection_Policy` | **SAM 0x3a**, response | u8 bits: 0x01 BatteryLimit, 0x02 ThermalOverride, 0x04 DisplayOverride, 0x08 CutTheTop, 0x10 BatteryChargeLimit | `GetSmartChargingSupported` |
| 6 `Get_Consolidated_Bcl_Status` | **SAM 0x41**, response | The SAM returns a u16; the SID converts it with `& 0x440 → 1/0` and returns that | `GetSmartChargingEnabled` |
| 7 `Get_Bcl_Countdown` | **SAM 0x42**, response | u32 seconds | same |
| 8 `Force_Bcl_Disengage` | **SAM 0x43**, response | u32: 0 disengaged, 1 countdown running (cannot disengage), 2 not engaged | `SetSmartCharging(false)` |
| 9 `AuthenticateBattery` | BAT 0x1b, response | u8 result: 0 success, 1 unauthorised, 2 failed | `SetBatteryAuthenticated(true)` |
| 10 `BatteryShutdown` | BAT 0x46, no response | u32 `ShutdownType` (0 MAC, 1 Emergency) | `SetBatteryShutdown(true)` |
| 11 `Get_ExtInfoPredictiveChargeControl` | BAT 0x26, response | PCC structure | |
| 12 `Set_PccEnable` | BAT 0x65, no response | u8 | `SetPredictiveChargingEnabled` |
| 13 `Set_PccPredictions` | BAT 0x64, no response | 25 bytes | |
| 14 `Get_PsuGeneralInfo` | BAT 0x11, response | Adapter required and actual capability (1.2) | `GetSlowChargingSupported` |
| 15 `Set_User_Charge_Mode` | BAT 0x0a, no response | `[mode u8, settings 4B]`: 0 Adaptive, 1 UserLimit (settings[0] = %), 2 UnlimitedWithTimeout (settings = u32 seconds) | `SetUserChargeMode` |
| 16 `Get_User_Charge_Mode` | BAT 0x09, response | `ModesSupported u32, CurrentMode u8, PreviousMode u8, ModeSettings[24]` | `GetUserChargeMode` |

Only commands 5–8 switch the TC to SAM (0x01). The rest are BAT (0x02). The IID is the instance of the packet (the app always uses 0)
(confirmed).

### 1.2 Values read on this device (`surface-battery-query.ps1`, AC connected, charging)

Values that describe the individual battery (cycle count, capacity, state of health, cumulative counters) are left out.

| Command | Result |
|---|---|
| 1 GeneralStandard | SoC 98%, a state-of-health percentage, charge voltage 8700 mV, charge current 3069 mA, pack 8650 mV, current +575 mA, cells 4323/4325 mV (2S), temperature 42 (appears to be °C). The later fields appear misaligned with the app's structure (the full-charge capacity in mWh sits in the `temperatureVtsMax` position) |
| 2 GeneralCustom | `batteryChargeLimitEnable`=1, `authenticationStatus`=2 (Authenticated), `protectionStatus`=0, `dischargeRateEwmaLvl`=20, temperature maximum 43, average 42, `userChargeMode`=0, `cmdRevision`=0 |
| 3 BatteryProtection (BPM) | `bpmDisable`=0, `bpmStatus`=2, `bpmExitRsocThreshold_pct`=50, `bpmHvtTime_sec` (time spent at high voltage) non-zero |
| 5 ProtectionPolicy | 0x10 = BatteryChargeLimit only. No BatteryLimit (0x01) = consistent with the UEFI variable being 0 |
| 6/7 | BCL not engaged, countdown 0 |
| 11 PCC | all 0 (`featureSupported`=0) |
| 4, 14, 16 | error 121 (no answer from the SAM → taken as not supported by the SP9 firmware) |

WMI (`battery-state.ps1`): cycle count and full-charge capacity in mWh are reported; charging at 6.6 W (97%). `BatteryStaticData`
returns "Generic failure" (SB rejects some queries), and the temperature is not reported.

### 1.3 SAM requests sent by SurfaceBattery.sys

Request header: `[TC=0x02][TID][IID][SNC][CID][len16]`.

| SAM | Function (name in the driver's log messages) | Meaning |
|---|---|---|
| BAT 0x01 | `McuReadSta` | `_STA` |
| BAT 0x02 | `McuReadBix` | `_BIX` (cycles, FCC, design capacity) |
| BAT 0x03 | `McuReadBst` | `_BST`. state bit 1 discharging, 2 charging, 4 critical, **8 charge limiting** (`UpdateBst.Bcl`) |
| BAT 0x04 (write u32) | `SetBtp` | `_BTP` alarm trip point |
| BAT 0x0d | `McuReadCharger` | `_PSR` AC connected |
| BAT 0x0f | `McuReadCharger…Artg` | Adapter rating in mW (the request linux-surface/kernel#184 uses; this repository's patch 0010) |
| BAT 0x10 | `UpdateMinPsuChargePower` | Minimum PSU power that can charge (inferred: from the name) |
| BAT 0x19 (write u8) | `McuSetBpmDisable` | BPM off/on. The trigger that calls it was not found |
| BAT 0x1a | `McuReadDischargeRateEwmaLvl` | Discharge rate EWMA level |
| BAT 0x53 | `McuReadTp` | Two "Tp" values (u16). Inferred to be turbo power |

Events (`ProcessResponse`): 0x15 static information changed, 0x16 dynamic information, 0x17 PSU connection change,
**0x18 battery protection state changed**, 0x1a EWMA changed, 0x53 Tp update. Linux `surface_battery` ignores 0x18 and 0x53 as TODO
(L/drivers/power/supply/surface_battery.c:370–383).

## 2. By control

| Control | Windows path | Values / trigger | Evidence | Same control on Linux | Open |
|---|---|---|---|---|---|
| Smart charging supported | app → SID → `SAM 0x3a` | u8 policy, bit 0x10. The app queries when the user enters the page (5 retries, 100 ms apart) | confirmed: `SID`, the app's `BatteryLogic::GetSmartChargingSupported`. 0x10 on this device | In the `surface_battery` probe, read `SAM 0x3a` and register `charge_control_end_threshold` if 0x10 is set (#167). Checking from user space: `/dev/surface/aggregator` `SSAM_CDEV_REQUEST` (tc 1, tid 1, iid 0, cid 0x3a, HAS_RESPONSE) | Whether the 0x01 BatteryLimit bit is the same as the UEFI limit (switch it on in UEFI and check) |
| Smart charging state (holding at 80%?) | app → SID → `SAM 0x41`; the battery class uses BST bit 0x08 | u16, engaged = `& 0x0440` | confirmed: `SID`; `SB` | Reading `charge_control_end_threshold` = 80 if engaged, otherwise 100. **The `== 1` comparison of #167 has to be changed to `& 0x440`**. For the status, a patch that makes `surface_battery` report BST bit 0x08 as `POWER_SUPPLY_STATUS_NOT_CHARGING` | The meaning of each of the bits 0x40 and 0x400. There is no command by which the SAM reports the value 80% itself |
| Smart charging disengage (once, up to 100%) | app → SID → `SAM 0x43` | u32 result 0/1/2. When the user switches the app toggle off | confirmed: `SID`, `BatteryLogic::SetSmartCharging` | Writing 100 to `charge_control_end_threshold` → `SAM 0x43` (#167). Result 1 gives `-EBUSY`, 2 gives 0. This property fits better than `charge_behaviour` (there is no way back) | How long the disengagement lasts (#167 says "a few days"). Windows has no command to switch it back on either |
| Smart charging countdown | app → SID → `SAM 0x42` | u32 seconds | confirmed | If needed, an extra sysfs property (read) | What it counts down (inferred: the period during which disengaging is not possible) |
| Telling the SAM that the OS supports BCL | `_OSC` CAP0 bit19 → `BCLE`; at SID start `SAM 0x2c` → `SAM 0x2f {v, 0x10}` | v = BCLE ? 0x10 : 0. At every driver start | confirmed: Surface SSDT, `SID`; inferred: 0x2f sets SAM feature bits | Linux also declares `_OSC` bit19 (L/drivers/acpi/bus.c:484), so BCLE=1. The same notification would be: in the `surface_battery` (or platform hub) probe, check `SAM 0x2c` (payload `00 02 02 00 00`, response 3B) and then send `SAM 0x2f {0x10, 0x10}` | Whether the SAM enables BCL when only Linux was booted (0x2f not sent). The names of 0x2c/0x2f (not checked against the Surface app's header assembly) |
| UEFI Battery Limit (50%) | UEFI setup → variable `BatteryLimitEnable` → (firmware) SAM | u64, 0 off. The Windows drivers do not use it | confirmed: the variable exists with value 0 (`uefi-vars.ps1`). inferred: reflected as SAM policy 0x01 | Read: `/sys/firmware/efi/efivars/BatteryLimitEnable-41d9f172-a9e9-4f9f-81a9-133e2d5902d4` (the first 4 bytes are the attributes). It is not written (use the UEFI menu) | How `SAM 0x3a`, `0x41` and the BPM values change when it is on. Whether an OS write is possible given the RT attribute (variable policy) |
| Kiosk/BPM (50% protection when connected for a long time) | The SAM's own logic. The state is BAT 0x24, switching off is BAT 0x19 (u8) | On this device `bpmStatus`=2, exit threshold 50% | confirmed: commands and values. `Surface.KioskMode.Scenario`=0 (DFCI setting) | A debug property that reads BAT 0x24. 0x19 is not sent because its trigger is unknown | The meaning of the `bpmStatus` values, who sends 0x19 and when |
| User charge mode (limit %, time-limited release) | app → SID → BAT 0x0a / 0x09 | Table in 1.1 | confirmed: commands. No answer on the SP9 | Not applicable (not supported by the SP9 firmware). On newer models, a write to `charge_control_end_threshold` would map to BAT 0x0a `{1, %}` | |
| Predictive charging (PCC) | app → SID → BAT 0x26/0x65/0x64 | SP9 `featureSupported`=0 | confirmed | Not applicable | |
| Charge current and voltage | none (the host does not set them) | Read only: BAT 0x22 `chargingVoltage/Current`, 0x23 `actualCharging…` | confirmed (no write command exists) | The read-only properties (`constant_charge_current`, `constant_charge_voltage`) can be filled from BAT 0x22. No writes | Field alignment of the later part of 0x22 (1.2) |
| Adapter detection and rating | SB: BAT 0x0d (`_PSR`), 0x0f (Artg mW), event 0x17 | At every AC change event | confirmed: `SB`, 1.3 | Already there: `kernel/patches/0010` (charger `input_power_limit`) | The difference between BAT 0x10 (minimum PSU power) and 0x11 (PSU information, no answer on the SP9) |
| Battery health and cycles | SB: BAT 0x02 (BIX); app: BAT 0x22 (SoH%, cell voltages) | Cycle count and SoH% are reported | confirmed | `cycle_count` and `energy_full(_design)` already exist. SoH and cell voltages can be added by a patch from BAT 0x22 | |
| Battery authentication | app → SID → BAT 0x1b (run); the state is BAT 0x23 `authenticationStatus` (0 Unknown, 1 Unauthenticated, 2 Authenticated) | 2 on this device | confirmed | The `authentic` property is read from byte 3 of BAT 0x23. **The `SAM 0x1b` read of #167 is a different command** (it collides with `Sam/GetWakeAlarmStatus=0x1b` of the Surface app's header assembly) | |
| Battery cut-off (shipping mode) | app → SID → BAT 0x46 `{u32 type}` | No response | confirmed | Not sent (recovery is inferred to need a charger to be connected) | |
| Alarm trip point | SB → BAT 0x04 | u32 mWh | confirmed | Already there (`alarm` property, `ssam_bat_set_btp`) | |
| USB-C PD and power role | `SurfaceUcmUcsiHidClient` ↔ SAM HID IID 7: feature 0x32 (49B: VERSION u16, reserved u16, CCI u32, CONTROL 8B, MESSAGE_IN 16B, MESSAGE_OUT 16B), input 0x32 (notification), feature 0x33 (number of connectors, UDC support). The commands are standard UCSI (UcmUcsiCx) | | confirmed: HID caps (`hid-caps.ps1`), driver names `UcsiDataSend/Receive` | `kernel/patches/0007–0008` (ucsi_surface_hid, #184) use the same reports. Power role and PDOs are in `/sys/class/typec`, `ucsi-source-psy-*` | Whether Windows sends `SET_PDR`/`SET_SINK_PATH` and the like (needs a UcmUcsiCx ETW `{569D11AA-…}` trace) |

## 3. Proposed order of Linux implementation

1. `surface_battery`: BST state bit 0x08 → `NOT_CHARGING`. No risk; removes the problem that the stop at 80% shows as "Charging 0 W".
   (In the one reading taken on this device with the limit engaged, on AC at 77 %, the status was `Not charging`, so that symptom was
   not seen; section 4.)
2. A corrected #167: test 0x41 with `& 0x440`, authentication from BAT 0x23, property array length, IID 0.
   `charge_control_end_threshold` (read 80/100, write 100 = 0x43).
3. To decide after testing: whether `SAM 0x41` becomes engaged when the device was booted with Linux only. If not, add the `SAM 0x2c`
   check + `SAM 0x2f {0x10, 0x10}` to the probe.
4. The debug reads (BAT 0x22/0x23/0x24) are tried from user space with `surface_aggregator_cdev` first, and only the values that are
   needed are raised to sysfs.

State of this list: none of the kernel changes 1–3 exists in this repository's patches. From user space, the device helper reads
`SAM 0x3a`, `0x41` and `0x42` through the cdev and can send the release `SAM 0x43` ([helper-reference.md](../../helper-reference.md)).

## 4. To check on Linux

- `cat /sys/class/power_supply/BAT1/status` near 80% (when smart charging is engaged), and the raw u16 of `SAM 0x41` through the cdev.
- Compare `SAM 0x3a` and `0x41` right after a Linux cold boot and right after a Windows→Linux reboot (the effect of the 0x2f
  notification).
- Read `BatteryLimitEnable` from `efivars`; after switching it on in UEFI, the change of `SAM 0x3a` bit 0x01 and of BPM (BAT 0x24).
- Compare `input_power_limit` of a kernel with this repository's patch 0010 against the Windows Artg. Measured so far: 45 W with the charger used; the Windows value for the same charger was not recorded.

Read on Linux so far (through the device helper, which sends the read commands over `/dev/surface/aggregator`;
[helper-reference.md](../../helper-reference.md)):

- On AC at 77 % with `BAT1/status` = `Not charging`: `SAM 0x3a` reports smart charging as supported, the engaged test
  (`& 0x0440`) is true, and the raw u16 of `SAM 0x41` is 192 (0x00C0). What each bit means is unresolved. Releasing it
  (`SAM 0x43`) has not been tried.
- `BatteryLimitEnable` reads 0 (off) from efivarfs.
- At 100 % on a charger `BAT1/status` reads `Charging`, not `Full` (test SAM-6). The status near 80 % was not recorded.

## 5. Open

- The names of `SAM 0x2c`/`0x2f`, of the 0x40 and 0x400 bits of `0x41`, and of the `bpmStatus` values. They would be in the `Sam/` enums
  of `SurfaceCommonHeadersDotNet.dll` of the Surface app (`Microsoft.SurfaceHub`), which was not available for this analysis.
- The actual layout of the later fields of the BAT 0x22 response (misaligned with the app's structure).
- The trigger that calls `McuSetBpmDisable` (BAT 0x19).
- The Windows UCSI command sequence (can be checked with UcmUcsiCx ETW).
