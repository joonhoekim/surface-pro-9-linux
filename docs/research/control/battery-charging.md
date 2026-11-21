# Battery and charging control: what Windows does and the same control on Linux

How Windows controls the battery and charging of this device, through which path (SAM request, ACPI, UEFI variable, HID) and
by sending what, and how the same control is done, or has to be done, on Linux (v7.3.4 + linux-surface 7.3 + `kernel/patches`).
In short: the charging algorithm is in the SAM firmware and the host only reads and flips a few switches; smart charging
already stops at about 80 % under Linux (measured), and what Linux lacks is the sysfs side of it (threshold and release).
The SAM node layout and the list of differences (G9 battery extras, G1 UCSI) are in [gaps/sam-platform](../gaps/sam-platform.md).

Notation:

- A SAM request is written as `TC/TID/IID CID` + payload. TC 0x01 = SAM, 0x02 = BAT. TID 1 = SAM. If the response flag (SNC) is 0 it
  is a write without response.
- The Windows side is the Surface battery stack: the Surface app, the Surface Integration driver on `MSHW0343` (which turns each
  request of the app into one SAM request, administrator only) and the Surface battery driver (which feeds the Windows battery
  class). What follows states the SAM requests and the behaviour, not how these components are built.
- "Surface SSDT" = the Surface-specific SSDT of this device's ACPI tables. It is not in this repository; dump it from your own device
  ([`dump-acpi.ps1`](../../../tools/windows/dump-acpi.ps1)) or see `https://github.com/linux-surface/acpidumps`, directory
  `surface_pro_9_intel`. `L/path:N` = path and line in the Linux v7.3.4 source tree.
- Evidence levels: **confirmed** (a query or measurement on this device, or the ACPI tables), **inferred** (judged from the
  surrounding behaviour, not observed). The few values read on Linux are in section 4; everything else is the Windows side.

Tools:

- [`uefi-vars.ps1`](../../../tools/windows/uefi-vars.ps1): list and values of UEFI variables (Administrator).
- [`battery-state.ps1`](../../../tools/windows/battery-state.ps1): WMI `root\wmi` battery classes; `-Watch` follows the state.

## Summary

- **There are two charging control paths.** Static information (BIX/BST/AC/adapter) is read by the Surface battery driver with BAT
  commands and handed to the battery class. "Surface-specific" functions such as smart charging, authentication and protection mode
  go from the Surface app (Administrator only) through the Surface Integration driver, each as a single SAM request. The host sets no
  charge current or voltage. The charging algorithm is held by the SAM firmware, and the host sends only reads and a few switches
  (the write requests are all in the tables below).
- **Smart charging (hold at 80%) = the SAM's BCL (Battery Charge Limit).** The state is `SAM 0x41` (u16): 0 while not holding
  and 0x00C0 while holding at 79 % (measured, section 4), so the helper reads bit 0x40 as engaged. What the bits mean is open.
  Disengaging is `SAM 0x43` (u32 result: 0 success, 1 countdown running, 2 not engaged).
  There is no request to switch it back on. The countdown is `SAM 0x42` (u32 seconds), and support is policy bit 0x10 of
  `SAM 0x3a` (confirmed).
- **At boot the OS declares battery charge limiting through ACPI.** `_OSC` CAP0 bit 19 (battery charge limiting) → `BCLE`=1
  (Surface SSDT, under the condition `SCAP`&0x400), which the host reads through `_DSM` fn6 of `MSHW0343`. `SCAP` = UEFI variable
  `VarSamCap` = 0x1F8F (confirmed: values). What the OS sends to the SAM after that is not covered here.
- **The status indication while charging is limited is `_BST` state bit 3 (0x08).** Windows shows "Battery Charge Limiting" from this
  bit of the BAT 0x03 response. Linux `surface_battery` does not look at this bit. ACPI `battery.c` treats the same bit as
  `Not charging` (L/drivers/acpi/battery.c:247).
- **The UEFI "Battery Limit" (50%) is in the UEFI variable `BatteryLimitEnable` ({41d9f172-a9e9-4f9f-81a9-133e2d5902d4}, 8 bytes,
  NV+BS+RT) and is 0 (off) on this device.** The OS sees it only through the SAM policy (`SAM 0x3a` bit 0x01) (inferred: not checked
  with the limit switched on).
- **#167 (linux-surface/kernel) has the right direction and differs in details.** TC and CID of 0x3a/0x41/0x43 are the same as on
  Windows. Differences: (1) it tests 0x41 with `== 1`, while this device answers 0x00C0 when holding, (2) it reads the authentication state
  with `SAM 0x1b`; the authentication state is byte 3 of the BAT 0x23 response (1.2), (3) it grows the property array even when smart charging is not supported, (4) it sends IID 1 of the battery node
  (Windows sends 0).
- **Functions the SP9 firmware does not answer**: for user charge mode (BAT 0x09), PSU general information (BAT 0x11) and
  cut-the-top (BAT 0x25) the request times out (Windows error 121). Predictive charging (BAT 0x26) answers but reports the feature as
  not supported (confirmed: queries on this device).

## 1. Path structure

```
Surface app (UI) ─(Administrator)─→ Surface Integration driver (MSHW0343) ─→ SSH → SAM   one SAM request per app request
                                                                             └ boot: _OSC/BCLE (_DSM fn6)
battery class ←── Surface battery driver ─→ SAM BAT 0x01…0x1a, events 0x15–0x1a, 0x53
UCSI class extension ←── Surface UCSI HID client ─HID feature/input reports 0x32·0x33─→ SAM HID IID 7 (UCSI PPM)
```

### 1.1 SAM requests of the Surface battery functions

| Function | SAM request | Payload/response |
|---|---|---|
| General information | BAT 0x22, response | State of charge, state of health, charge voltage and current, pack and cell voltages, temperature (1.2) |
| Extended information | BAT 0x23, response | Byte 1 = charge limit enabled, byte 3 = authentication state (0 unknown, 1 unauthenticated, 2 authenticated), protection status, discharge-rate level, temperatures, user charge mode, a command revision (1.2) |
| Battery protection state | BAT 0x24, response | Protection mode state (1.2) |
| Cut-the-top state | BAT 0x25, response | No answer on the SP9 |
| Protection policy | **SAM 0x3a**, response | u8 bits: 0x10 charge limit (BCL), seen on this device; 0x01 is taken to be the UEFI battery limit (inferred, not seen set) |
| BCL status | **SAM 0x41**, response | u16; 0 not holding, 0x00C0 holding at 79 % (section 4) |
| BCL countdown | **SAM 0x42**, response | u32 seconds |
| BCL release | **SAM 0x43**, response | u32: 0 disengaged, 1 countdown running (cannot disengage), 2 not engaged |
| Predictive charging | BAT 0x26 (state, response) | SP9: answers, all 0 (not supported) |
| PSU information | BAT 0x11, response | No answer on the SP9 |
| User charge mode | BAT 0x09 get | No answer on the SP9 |

Only the policy and BCL requests (0x3a, 0x41–0x43) are in the SAM TC (0x01); the rest are BAT (0x02). The IID is 0 (confirmed).

### 1.2 Values read on this device (under Windows, AC connected, charging)

Values that describe the individual battery (cycle count, capacity, state of health, cumulative counters) are left out.

| Request | Result |
|---|---|
| BAT 0x22 | SoC 98%, a state-of-health percentage, charge voltage 8700 mV, charge current 3069 mA, pack 8650 mV, current +575 mA, cells 4323/4325 mV (2S), temperature 42 (appears to be °C). The later fields are not decoded (one of them holds what looks like the full-charge capacity in mWh) |
| BAT 0x23 | Charge limit enabled 1, authentication 2 (authenticated), protection status 0, discharge-rate level 20, temperature maximum 43, average 42, user charge mode 0, command revision 0 |
| BAT 0x24 | Protection not disabled, status 2, exit threshold 50 %, time spent at high voltage non-zero |
| SAM 0x3a | 0x10 = charge limit only. No 0x01 (UEFI battery limit) = consistent with the UEFI variable being 0 |
| SAM 0x41/0x42 | BCL not engaged, countdown 0 |
| BAT 0x26 | All 0 (not supported) |
| BAT 0x25, 0x11, 0x09 | Error 121 (no answer from the SAM → taken as not supported by the SP9 firmware) |

WMI (`battery-state.ps1`): cycle count and full-charge capacity in mWh are reported; charging at 6.6 W (97%). `BatteryStaticData`
returns "Generic failure" (some queries are rejected), and the temperature is not reported.

### 1.3 SAM requests of the battery class path

Request header: `[TC=0x02][TID][IID][SNC][CID][len16]`.

| SAM | Meaning |
|---|---|
| BAT 0x01 | `_STA` |
| BAT 0x02 | `_BIX` (cycles, FCC, design capacity) |
| BAT 0x03 | `_BST`. state bit 1 discharging, 2 charging, 4 critical, **8 charge limiting** |
| BAT 0x04 (write u32) | `_BTP` alarm trip point |
| BAT 0x0d | `_PSR` AC connected |
| BAT 0x0f | Adapter rating in mW (the request linux-surface/kernel#184 uses; this repository's patch 0010) |
| BAT 0x10 | Minimum PSU power that can charge (inferred) |
| BAT 0x19 (write u8) | Protection mode off/on. When Windows sends it is not known |
| BAT 0x1a | Discharge rate level |
| BAT 0x53 | Two u16 values, inferred to be turbo power |

Events: 0x15 static information changed, 0x16 dynamic information, 0x17 PSU connection change,
**0x18 battery protection state changed**, 0x1a discharge rate level changed, 0x53 update of the 0x53 values. Linux
`surface_battery` ignores 0x18 and 0x53 as TODO (L/drivers/power/supply/surface_battery.c:370–383).

## 2. By control

| Control | Windows path | Values / trigger | Evidence | Same control on Linux | Open |
|---|---|---|---|---|---|
| Smart charging supported | app → `SAM 0x3a` | u8 policy, bit 0x10. Queried when the user opens the battery page | confirmed: 0x10 on this device | In the `surface_battery` probe, read `SAM 0x3a` and register `charge_control_end_threshold` if 0x10 is set (#167). Checking from user space: `/dev/surface/aggregator` `SSAM_CDEV_REQUEST` (tc 1, tid 1, iid 0, cid 0x3a, HAS_RESPONSE) | Whether the 0x01 bit is the same as the UEFI limit (switch it on in UEFI and check) |
| Smart charging state (holding at 80%?) | app → `SAM 0x41`; the battery class uses BST bit 0x08 | u16: 0 not holding, 0x00C0 holding at 79 % | confirmed (queries on this device) | Reading `charge_control_end_threshold` = 80 if engaged, otherwise 100. **The `== 1` comparison of #167 does not match the 0x00C0 read here**; bit 0x40 does. For the status, a patch that makes `surface_battery` report BST bit 0x08 as `POWER_SUPPLY_STATUS_NOT_CHARGING` | The meaning of each bit (0x40 and 0x80 set while holding). There is no request by which the SAM reports the value 80% itself |
| Smart charging disengage (once, up to 100%) | app → `SAM 0x43` | u32 result 0/1/2. When the user switches the app toggle off | confirmed | Writing 100 to `charge_control_end_threshold` → `SAM 0x43` (#167). Result 1 gives `-EBUSY`, 2 gives 0. This property fits better than `charge_behaviour` (there is no way back) | How long the disengagement lasts (#167 says "a few days"). Windows has no request to switch it back on either |
| Smart charging countdown | app → `SAM 0x42` | u32 seconds | confirmed | If needed, an extra sysfs property (read) | What it counts down (inferred: the period during which disengaging is not possible) |
| Telling the platform that the OS supports BCL | `_OSC` CAP0 bit19 → `BCLE`, read through `_DSM` fn6 of `MSHW0343` | BCLE = 1 | confirmed: Surface SSDT | Linux also declares `_OSC` bit19 (L/drivers/acpi/bus.c:484), so BCLE=1 | Whether the SAM holds at 80 % after a cold boot into Linux only |
| UEFI Battery Limit (50%) | UEFI setup → variable `BatteryLimitEnable` → (firmware) SAM | u64, 0 off | confirmed: the variable exists with value 0 (`uefi-vars.ps1`). inferred: reflected as SAM policy 0x01 | Read: `/sys/firmware/efi/efivars/BatteryLimitEnable-41d9f172-a9e9-4f9f-81a9-133e2d5902d4` (the first 4 bytes are the attributes). It is not written (use the UEFI menu) | How `SAM 0x3a`, `0x41` and the protection values change when it is on. Whether an OS write is possible given the RT attribute (variable policy) |
| Kiosk/protection mode (50% protection when connected for a long time) | The SAM's own logic. The state is BAT 0x24, switching off is BAT 0x19 (u8) | On this device status 2, exit threshold 50% | confirmed: values. `Surface.KioskMode.Scenario`=0 (DFCI setting) | A debug property that reads BAT 0x24. 0x19 is not sent because when to send it is unknown | The meaning of the status values, who sends 0x19 and when |
| User charge mode | app → BAT 0x09 | No answer on the SP9 | confirmed | Not applicable (not supported by the SP9 firmware) | |
| Predictive charging | app → BAT 0x26 | SP9: not supported | confirmed | Not applicable | |
| Charge current and voltage | none (the host does not set them) | Read only: BAT 0x22 charge voltage and current, BAT 0x23 actual charge voltage and current | confirmed (no write request exists) | The read-only properties (`constant_charge_current`, `constant_charge_voltage`) can be filled from BAT 0x22. No writes | Layout of the later part of 0x22 (1.2) |
| Adapter detection and rating | Battery driver: BAT 0x0d (`_PSR`), 0x0f (rating in mW), event 0x17 | At every AC change event | confirmed, 1.3 | Already there: `kernel/patches/0010` (charger `input_power_limit`) | What BAT 0x10 (minimum PSU power, inferred) reports; 0x11 gives no answer on the SP9 |
| Battery health and cycles | Battery driver: BAT 0x02 (BIX); app: BAT 0x22 (SoH%, cell voltages) | Cycle count and SoH% are reported | confirmed | `cycle_count` and `energy_full(_design)` already exist. SoH and cell voltages can be added by a patch from BAT 0x22 | |
| Battery authentication | The state is byte 3 of BAT 0x23 | 2 on this device (authenticated) | confirmed | The `authentic` property could be read from byte 3 of BAT 0x23; #167 reads `SAM 0x1b` instead | |
| Alarm trip point | Battery driver → BAT 0x04 | u32 mWh | confirmed | Already there (`alarm` property, `ssam_bat_set_btp`) | |
| USB-C PD and power role | Surface UCSI HID client ↔ SAM HID IID 7: feature 0x32 (49B: VERSION u16, reserved u16, CCI u32, CONTROL 8B, MESSAGE_IN 16B, MESSAGE_OUT 16B), input 0x32 (notification), feature 0x33 (number of connectors, UDC support). The commands are standard UCSI | | confirmed: HID caps (`hid-caps.ps1`) | `kernel/patches/0007–0008` (ucsi_surface_hid, #184) use the same reports. Power role and PDOs are in `/sys/class/typec`, `ucsi-source-psy-*` | Whether Windows sends `SET_PDR`/`SET_SINK_PATH` and the like (needs a UcmUcsiCx ETW `{569D11AA-…}` trace) |

## 3. Proposed order of Linux implementation

1. `surface_battery`: BST state bit 0x08 → `NOT_CHARGING`. No risk; meant for the case where the stop at 80% shows as "Charging 0 W".
   That symptom was not seen on this device: at the smart charging stop the status reads `Not charging` (section 4), so this
   item is a clean-up rather than a fix.
2. A corrected #167: test 0x41 for bit 0x40 (0x00C0 read while holding), authentication from BAT 0x23, property array length, IID 0.
   `charge_control_end_threshold` (read 80/100, write 100 = 0x43).
3. To decide after testing: whether `SAM 0x41` becomes engaged when the device was booted with Linux only. If not, find out what the
   OS has to send.
4. The debug reads (BAT 0x22/0x23/0x24) are tried from user space with `surface_aggregator_cdev` first, and only the values that are
   needed are raised to sysfs.

State of this list: none of the kernel changes 1–3 exists in this repository's patches. From user space, the device helper reads
`SAM 0x3a`, `0x41` and `0x42` through the cdev and can send the release `SAM 0x43` ([helper-reference.md](../../helper-reference.md)).

## 4. To check on Linux

- Compare `SAM 0x3a` and `0x41` right after a Linux cold boot and right after a Windows→Linux reboot.
- After switching `BatteryLimitEnable` on in UEFI, the change of `SAM 0x3a` bit 0x01 and of the protection state (BAT 0x24).
- Compare `input_power_limit` of a kernel with this repository's patch 0010 against the Windows adapter rating. Measured so far: 45 W with the charger used; the Windows value for the same charger was not recorded.

Read on Linux so far (status values from sysfs; SAM values through the device helper, which sends the read requests over
`/dev/surface/aggregator`, [helper-reference.md](../../helper-reference.md)):

- Smart charging engaged, UEFI limit off: charging stopped at 79 %. `BAT1/status` then reads `Not charging` (power 0), with
  single `Unknown` readings at the stop and a single `Discharging` reading after a resume (SAM-6). At the stop the SAM sent BAT
  event 0x18 (battery protection state changed) once; while charging it sent 0x16 every 5.7 s on average (SAM-8).
- On AC at 77 % with `BAT1/status` = `Not charging`: `SAM 0x3a` reports smart charging as supported and the raw u16 of
  `SAM 0x41` is 192 (0x00C0); the helper reads bit 0x40 of it as engaged. What each bit means is unresolved. Releasing it
  (`SAM 0x43`) has not been tried.
- `BatteryLimitEnable` reads 0 (off) from efivarfs.
- At 100 % on a charger `BAT1/status` reads `Charging`, not `Full` (SAM-6).

## 5. Open

- What the bits of `SAM 0x41` (0x40 and 0x80 set while holding) and the protection status values mean.
- The layout of the later fields of the BAT 0x22 response.
- When BAT 0x19 (protection mode off/on) is sent.
- The Windows UCSI command sequence (can be checked with UcmUcsiCx ETW).
