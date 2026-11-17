# Power and thermal control: the Windows control paths and how to produce the same control on Linux

This document describes **through what, with which request, and when** Windows changes the CPU power limits (PL), the SAM
thermal profile and the fan, and how to produce the same control on Linux. The per-power-mode load measurements are used only
as evidence that a control actually took effect.

Notation:

- **Confirmed** = confirmed on this device by measurement, log or binary. **Inferred** = there is a basis, but it was not
  confirmed directly.
- Windows components and versions: the CPU client is `SurfaceIhvCpuSmfClient.sys` 1.174.34.0 (the loaded version), the fan
  and thermal client is `SurfaceSmfClient.sys` 8.127.139.0, the user-mode service is `SurfaceService.exe`.
- "Policy blob" = the SMF policy data (section 2) decoded to text with
  [`tools/windows/perf/smf_blob.py`](../../../tools/windows/perf/smf_blob.py). The blob is not in this repository; a reader
  reads it from their own device.
- `ETW` = event 1 (node id, class, name, value) of the provider `Microsoft-Surface-SurfacePowerTrackerCore`
  `{23AC50B3-680E-7AD3-A2B3-D5DBEF945870}`. It emits every input and output of the SMF node graph about once a second.
  [`tools/windows/perf-modes.ps1`](../../../tools/windows/perf-modes.ps1) records it.
- Node values are in SMF's own units: power in mW, time in ms, temperature in 0.1 K (3181 = 45.0 °C), fan in rpm.
- `HDR` = Microsoft's names for the SAM command IDs, from the metadata of the Surface app (defined in the notation of
  [sam-platform.md](../gaps/sam-platform.md)). `#1726` is linux-surface/linux-surface issue 1726.
- "PWR-9" and "SAM-5" with Windows figures (26.7 W, 13.1 W) are the runs of those tests of
  [test-plan.md](../../test-plan.md) on Windows; the Windows baseline results are not in this repository. The Linux results
  of the same IDs are in [test-results.md](../../test-results.md).

## Summary

- **The Windows power mode is changed through a single Surface-specific power setting (the "slider state", 1–7), and that
  value fans out three ways.** (1) SurfaceService sends the SAM TMP profile (TC 0x03 CID 0x03). (2) The SMF core takes the
  slider as an input and selects the PL1/PL2/tau and fan profile tables. (3) Windows PPM writes the per-overlay EPP and core
  parking values.
- **PL1/PL2 go in through MCHBAR MMIO (0xFEDC0000 + 0x59A0/0x59A4), not MSR 0x610.** MSR 0x610 stays at the firmware values
  35/60 W, tau 28 s, and the CPU follows the lower of the two. The evidence is that sustained power in Better Performance
  sticks to the SMF PL1 (28 W), at 28.1 W, and not to the MSR PL1 (35 W).
- **SMF reads its policy from the UEFI variable `SMF_DATA_BLOB`.** It can be read on Linux through `efivarfs` too, and the
  per-slider PL table, the skin temperature PID and the fan curve are in it as plain-text CSV. Linux can use this table as it
  is.
- **What the "constantly changing PL" is**: on top of the slider table value, the skin temperature (SAM sensor 509 = VTS1)
  PID, a sensor step table, the battery discharge capability and the screen-off/standby state are combined with `min()`. On
  AC with the screen on, only the skin PID moves; when VTS1 reaches the setpoint (47–48 °C) it cuts PL1 to 21–25.5 W
  (measured).
- **For the fan, the SMF fan client sends a profile (CID 0x0e) and a state (0x0f) to SAM FAN, and computes an rpm request.**
  The rpm request is a linear function of CPU temperature (60 °C 3000 rpm → 85 °C 4600 rpm). Which SAM command carries it
  is not identified on the Windows side; on Linux, CID 0x0b sets the fan speed and CID 0x0c does nothing (measured, section
  1.4). The "stuck at ~3800 rpm after reboot" of linux-surface#1726 may be this rpm request left behind (inferred).
- The difference between the Windows runs of PWR-9 (26.7 W) and SAM-5 (13.1 W) appears to be not a power limit but **the load process being
  confined to the E-cores and the base clock by EcoQoS** (section 3.1).

## 1. Path of each control

### 1.1 Power mode → slider state

| Item | Content | Evidence | Confidence |
|---|---|---|---|
| Power setting | Subgroup `bfeba2e4-fdcc-41d9-93f4-3f6991c1f730` "Surface Overlay Power Settings Subgroup", setting `a4a0ab02-11f1-4584-bc52-cbb98eabdb0b`. Values: 1 Balanced AC, 2 Balanced DC, 3 High Performance AC, 4 High Performance DC, 5 Max Performance AC, 6 Max Performance DC | `HKLM\SYSTEM\CurrentControlSet\Control\Power\PowerSettings\bfeba2e4…\a4a0ab02…` | Confirmed |
| Default per overlay | Default Balanced scheme 1/2, `3af9b8d9` (Better Performance) 3/4, `ded574b5` (Best Performance) 5/6. `961cc777` (Best Power Efficiency) has no value and follows the default scheme's values (1/2) | `DefaultPowerSchemeValues` under the same key | Confirmed (registry; in the measurement the efficiency mode has `I_SLIDER` = 1) |
| Default AC mode | The AC overlay of this device is `3af9b8d9` (Better Performance). The DC overlay is empty (= Balanced) | `PowerGetEffectiveOverlayScheme`, `ActiveOverlayAcPowerScheme` | Confirmed |
| Consumer | SurfaceService (user mode) holds this setting and the overlay GUIDs and handles power-overlay changes. The kernel drivers do not have them. State 7 = battery saver mode | SurfaceService | Confirmed (strings and code) |
| SMF input | SurfaceService passes the effective slider state to the SMF input `2DD6960F…,1`. ETW `I_SLIDER` | Policy blob, ETW | Confirmed (the ETW value changes with the mode switches, section 3) |

### 1.2 SAM thermal profile (TMP)

| Item | Content | Evidence | Confidence |
|---|---|---|---|
| Sender | SurfaceService. Slider state → TMP profile table: 1, 2 → 1; 3, 4 → 3; 5, 6 → 4; 7 → 2 (8–11 → 5–8, not used on this device) | SurfaceService code | Confirmed (code) |
| Transport | `DeviceIoControl(0x04001C48)` on the device interface `{99BBC337-05D6-4819-B648-39589FEBB93E}` (inferred to be the user-mode channel of the SSH driver). Buffer: byte0 TC = 0x03, byte1 = 0x01 (TID), byte4 CID = 0x03, byte5 `u16` length = 4, byte7 `__le32` profile | SurfaceService code | Confirmed (code); who owns the interface is inferred |
| SAM request | TMP (0x03) TID 1 IID 0 CID 0x03, payload `__le32` 1–4 | Above; Linux `surface_platform_profile.c` | Confirmed |
| When it is updated | Power mode change, AC/DC switch (whenever the slider state changes) | SurfaceService's power-overlay change handler | Inferred (timing) |
| Linux | `surface_platform_profile` sends the same request. `platform_profile` low-power → 2, balanced → 1, balanced-performance → 3, performance → 4. Same as the Windows mapping | `drivers/platform/surface/surface_platform_profile.c` (Linux v7.3.4) | Confirmed |

What the SAM firmware changes for a TMP profile cannot be seen from Windows. Windows changes PL and the fan directly through
SMF at the same moment, so changing only TMP on Linux leaves out the PL and fan effects (1.3, 1.4).

### 1.3 CPU power limits

SurfaceIhvCpuSmfClient writes SMF output nodes to hardware registers. It has a register table for Alder Lake (0x13 entries
of 0x40 bytes: OR mask, field mask, bit position, kind 0 = MMIO / 1 = MSR / 2 = software, address, SMF node id, class GUID).
The MCHBAR base 0xFEDC0000 is hard-coded; the driver maps it and does 64-bit read-modify-write. MSRs are read and written
directly. Values are converted as `mW x 8 / 1000` (units of 1/8 W), and times from ms to the RAPL Y/Z encoding. In the event
log entry "Cpu control update", Channel Index is the index into this table and ChannelInstance is the SMF node id.

| ch | Node (ETW name) | Register | Meaning | Values seen on this device | Same control on Linux |
|---|---|---|---|---|---|
| 0 | 51 `O_CPU_AVG` | MCHBAR+0x59A0 [14:0], OR 0x18000 (enable+clamp) | PL1 | Table value AC 28 W (Best Performance 35), DC 13 W, standby 6 W. Under load 18.6–25.5 W through the skin PID | `intel-rapl-mmio:0/constraint_0_power_limit_uw` (see the caveats below) |
| 1 | 52 `O_CPU_MAX` | MCHBAR+0x59A4 [14:0] | PL2 | AC 45 W, DC 35 W, standby 6 W | `intel-rapl-mmio:0/constraint_1_power_limit_uw` |
| 9 | 61 `O_CPU_AVG_TIME` | MCHBAR+0x59A0 [23:17] | PL1 tau | 28 s (AC, DC high performance), 8 s (DC balanced), 1 s (battery saver) | `intel-rapl-mmio:0/constraint_0_time_window_us` |
| 2 | 53 `O_CPU_PEAK` | MSR 0x601 [12:0] | PL4 (current limit) | AC 130 W, DC 95–130 W (varies with battery state) | `intel-rapl:0/constraint_2_power_limit_uw` (peak_power) |
| 3, 4 | 54 `O_SYS_AVG`, 55 `O_SYS_MAX` | MSR 0x65C [14:0], [46:32] | PSys PL1/PL2 | AC 55/118 W, DC 100/80–87 W | `intel-rapl:1` (psys) `constraint_0/1` |
| 7 | 59 `O_SYS_PEAK` | MSR 0x615 [12:0] | PSys maximum (inferred to be of the PL3 family) | AC 130 W, DC 70–85 W | No kernel interface (msr directly) |
| 5, 6, 10 | 56, 57, 62 | MSR 0x610 PL1/PL2/tau | Not used on this device (value 0, no updates) | — | — |
| 12, 13 | 64, 11 | MSR 0x1A2 [6:0], [29:24] | TCC offset | No update events. Current offset 10 (TjMax 100 → 90 °C) | Keep the current value |
| 8, 14, 16 | 1, 2, 4 | MCHBAR+0x5B10 [7:0], [15:8], [31] | Unverified | — | — |
| 17 | 70 `O_BATT_MODE` | No register (software) | Battery mode indication | AC 0, DC 1 | — |

Verification state:

- **Confirmed (measured)**: MSR 0x610 = `0x004281E000DD8118` → PL1 35 W (enable, clamp), PL2 60 W, tau 28 s, not locked. It
  stays the same when the mode changes (section 3). At the same time SMF outputs PL1 28 W and sustained load power stays near
  28 W. This means a path other than the MSR (the MMIO one) is the effective limit.
- **Confirmed (code)**: MMIO addresses and bits, MSR numbers, unit conversion (from the driver's register table).
- **Could not be read**: the MCHBAR values themselves, MSR 0x601/0x65C/0x615/0x64F/0x774 (HWP_REQUEST). The read path,
  LibreHardwareMonitor + PawnIO, returns 0 for these MSRs (inferred: outside the module's allow list). HVCI is on, so no
  other ring-0 tool was used.

Linux caveats:

- `intel-rapl-mmio` is created by `processor_thermal_device_pci`, which binds to the processor thermal device PCI `00:04.0`
  (8086:461D). It uses the same register 0x59A0
  (`drivers/thermal/intel/int340x_thermal/processor_thermal_rapl.c:87`, Linux v7.3.4). **The Windows PnP list has no
  8086:461D** (and no Intel DTT device). If the firmware hides it, `intel-rapl-mmio` does not appear on Linux either
  (inferred). Measured on Linux 7.3.4: `/sys/class/powercap` has no `intel-rapl-mmio`, only `intel-rapl:0`. Whether that is
  because `00:04.0` is hidden was not checked with `lspci -nn -s 00:04.0`.
- In that case the MCHBAR PL stays at the value the firmware put in at boot. The effective PL is the lower of MSR and MMIO,
  so through the MSR (`intel-rapl:0`) Linux can **only lower** it. Whether the firmware puts the same 35/60 W into MMIO as
  into the MSR is not known. Whether, right after rebooting from Windows, the value Windows wrote last (6 W if just before
  entering standby) remains is not known either. Read MCHBAR directly on Linux to find out (section 5).
- If it has to be raised through MMIO, the options are (a) a small kernel module that writes MCHBAR 0x59A0/0x59A4, or an
  extension of `surface_platform_profile`, or (b) the boot parameter `iomem=relaxed` plus a tool that writes `/dev/mem`.
  (a) is better.

### 1.4 Fan

SAM requests are to FAN, TID 1, IID 1.

| Node | SAM request (FAN TID 1 IID 1) | Value | Evidence | Confidence |
|---|---|---|---|---|
| 201 `O_FAN1_PROFILE` | CID 0x0e, `u8` | Slider table (node 220): 1, 2 → 2; 3, 4 → 3; 5, 6 → 4; 7 → 1; 8 → 4. ETW value = 3 (AC Better Performance) | Policy blob, ETW; the Windows fan client issues this request for three nodes (201–203) | Table and ETW confirmed; node → CID mapping inferred |
| 301 `O_FAN_CS_REQUEST` | CID 0x0f, `u8` | 1 (normal). Node 260 = standby state (`I_MS_NOTIFY`) table | Windows fan client | Inferred (mapping) |
| 101 `O_FAN1_ACTIVE_RPM` | Not identified. Not CID 0x0c: that command does not move the fan (measured, below). CID 0x0b `u16` rpm is the command that sets a fan speed (measured, below); that Windows sends this node through it is unverified | `clamp(6.4 x T − 18318.4, 3000, 4600)`, T = average CPU package temperature (0.1 K, node 112, based on MSR 0x1B1). 60 °C → 3000 rpm, 85 °C → 4600 rpm. 0 when the hysteresis node 158 (3331–3631) is off | Policy blob, ETW (`I_CPU_TEMP_AVG` 3517 → 4191 rpm, matches the formula) | Formula and ETW confirmed; SAM command unverified |
| 111 `O_FAN1_SPL_MAX` | Not identified. Not CID 0x0b: that command sets the fan speed (measured, below) | From node 234 (power supply state table). Observed 0 | The Windows fan client issues CID 0x0b (`u16`) from three places; reading those as nodes 111–113 is contradicted by the Linux measurement | Unverified (mapping) |

- linux-surface on Linux sends only the profile (0x0e, following `platform_profile`) and the state (0x0f, 2 on suspend).
- Patch 0013 of this repository sends 0 to CID 0x0b, based on the HDR name (`SetMinFanRpm` = 0x0b) and the #1726 trace. The
  Linux measurement below agrees with that choice of command: 0x0b is the rpm command and 0x0c is not. Whether 0x0b 0 clears
  the residual rpm of #1726 after a Windows → Linux reboot is still to be measured (section 5).
- **Linux measurement (7.3.4, requests sent directly to FAN TID 1 IID 1 through `surface_aggregator_cdev`, idle at
  35–45 °C, `balanced`)**:
  - **CID 0x0b `u16` rpm sets the fan speed.** Sending 4000 to a stopped fan gives 3992 rpm in about 30 seconds; sending 0
    gives 0 rpm within a few seconds. Requests of 1500 / 3000 / 4600 / 5000 / 6000 / 7500 gave 1588 / 2977 / 4569 / 4988 /
    5958 / 7407 rpm (each after 22–25 seconds, including values still rising). Every request returns status 0. This matches
    the HDR name `SetMinFanRpm`; 0x0b is not the SPL_MAX node's command.
  - **CID 0x0c does nothing.** It did not spin the fan either as `float32` (4000, 6000) or as `u16` (stopped fan, status 0).
    It is not the rpm request.
  - **While the SAM is driving the fan itself, 0x0b is ignored.** In `performance` during `stress-ng --cpu 12` (package
    85–89 °C, fan about 5950 rpm), 7000 and 2000 were each sent for 40 seconds and the rpm did not move from 5926–5971
    (status 0). It is neither a floor nor a ceiling; it is obeyed only when the SAM's own target is 0. So a low value cannot
    block cooling, and the only use is to spin the fan up in advance when idle or under light load. The case where 0x0b 5000
    produced no change while the fan was running at about 3000 rpm right after a build is the same situation. In that case
    the drop from 3000 → 1600 rpm right after 0x0c `u16` 5000 is a single observation and the cause is not known.
- No place was found on Windows to read the actual fan rpm (no `Win32_Fan`; ACPI `FAN0` `_FST` is filled in by the PEP but
  not exposed to user mode). The rpm in ETW is the requested value.

### 1.5 Other SMF outputs

| Node | What it does | Evidence |
|---|---|---|
| 62 `O_WTMF_PARK` | Core parking limit in % (Windows PPM). `min(node 53, node 236)`. Node 53: 50 % when sensor 509 ≥ 52.0 °C (hysteresis 3251–3291) | Policy blob |
| 509 `O_SEN9_OFFSET` | VTS1 offset (SAM TMP CID 0x24, `u8 0` + `float` offset). On this device, every boot logs "VTS offset configuration failed. Instance 9" | Policy blob, Windows thermal client, event log |
| 110DF2A4,1–3 `O_HOT`/`O_CRT`/`O_MINPWR2` | Emergency outputs that turn on at sensor 509 ≥ 54.9–55.0 °C, sensor 505 ≥ 56.9 °C and so on (inferred) | Policy blob |
| Standby state (`I_MS_NOTIFY`) | On this device turning the screen off (`SC_MONITORPOWER` off) is immediately Modern Standby entry. Screen-off and standby are not separate; at the moment of entry `I_MS_NOTIFY` goes 4 → 2 and PL1 and PL2 go to 6 W | Kernel-Power 506/507, ETW |
| PPM (Windows power settings) | Per-overlay EPP (%, OEM provisioning `Prov*SettingIndex`): efficiency AC 60 / DC 72, default balanced 60 / 75, Better Performance 45 / 60, Best Performance 25 / 33. Same values for P-cores and E-cores (Class 1). As HWP EPP, x 255 / 100 = 153, 115, 64 | `PowerSettings\54533251…\36687f9e…6863`, `…6864` |
| SurfaceService PPM table | SurfaceService configures PPM settings per slider state, with a separate battery saver case, from what it calls an "MSR table". How it is delivered is unverified | Names in SurfaceService |

## 2. Policy data: `SMF_DATA_BLOB`

- At boot the SMF core (`SurfaceSystemManagementFrameworkDriver.sys`) reads the UEFI variable `SMF_DATA_BLOB` (vendor
  `{A3D05FA9-DB5E-490C-956E-A57384A4B70B}`, attributes NV+BS+RT) with `ExGetFirmwareEnvironmentVariable` and decompresses
  it with LZNT1. Size 7006 bytes, header 20 bytes (`0x5d000031`, product 0x5d, board 1, format 5, compressed size 0x1b4a);
  decompressed it is 21237 bytes of CSV. Because it has the runtime attribute, it can be read on Linux as
  `/sys/firmware/efi/efivars/SMF_DATA_BLOB-a3d05fa9-db5e-490c-956e-a57384a4b70b` (inferred; the path form follows the
  efivarfs convention).
- Reading and decoding: [`tools/windows/perf/smf-blob.ps1`](../../../tools/windows/perf/smf-blob.ps1) (Windows,
  administrator) → [`smf_blob.py`](../../../tools/windows/perf/smf_blob.py) (pure-Python LZNT1, works on Linux too).
- Structure: the first part defines the input and output nodes (`class GUID, node id, minimum, maximum, …`), then come the
  computation nodes (`1, stage, …, kind GUID, node index, number of inputs, …, inputs, output, parameters`), and at the end a
  telemetry list. Every output is either `FB805E77…` (a virtual node, VN) or a hardware class (`0B7B5350` = CPU power,
  `CF2F6076` = time, `B620798F` = fan, `9BBB8910` = PPM, `A79674B4` = sensor offset).
- Computation node kinds (first 8 digits of the GUID, count): `3AFD73EE` table lookup (26), `83F47D50` PID (18), `026CCFCC`
  constant / pass-through (13), `0AE20A40` minimum (12), `FD5F6C7E` limiter / filter (7), `2DB3B9EE` hysteresis (5),
  `5974888D` linear function (5), `013CEB02` select (3), `9E2A7FED` filter (2). The kind names are inferred from the shape of
  the parameters.

How PL1 and PL2 are determined (read from the policy blob; the interpretation is inferred):

| Output | Formula | Inputs |
|---|---|---|
| PL1 (node 51) | `min(VN1, VN2, VN17, VN37, VN148, VN230)`, range 5.0–35 W, default 28 W | VN2 = slider table `1..10 → 28, 13, 28, 28, 35, 30, 10, 35, 28, 28 W`. VN1 = skin temperature PID (VTS1 setpoint 45.0–48.0 °C, output cap 28–35 W). VN37 = input `5D0EE597` (2 states) → 35 / 15 W. VN17 = filter based on battery current. VN148 = limiter based on sensor 509. VN230 = standby state (`I_MS_NOTIFY` 1–5 → 12, 6, 6, 100, 12 W; 4 = screen on) |
| PL2 (node 52) | `min(VN21, VN92, VN98)`, range 4–61 W | VN21 = slider table `1..10 → 35, 35, 45, 35, 60, 45, 11, 60, 30, 30 W`. VN92 = standby state table (12, 6, 9, 100, 12 W). VN98 = sensor 509 step table: 35 W at 49.0 °C and above, 30 W at 50 °C, 20 W at 51 °C, 15 W at 52 °C (rising 3221/3231/3241/3248, falling 3231/3241/3248/3254) |
| PL1 tau (node 61) | Slider table 2 → 8 s, 3–6 → 28 s, 7 → 1 s, otherwise 28 s | Slider state |
| PL4 (node 53) | VN23 ← VN237 (limiter), cap 119–130 W | Battery maximum discharge, `I_IPL_TOP` |
| PSys PL1/PL2 (54/55) | VN11 (adapter size table: below / at or above 45 W → 65 / 100 W, default 55 W) / VN196 (selects between a battery-based linear function 68.5–86.5 W and 118 W) | `I_PSU_SIZE`, battery |

Slider state columns (1–7) and Windows modes:

| Slider | Meaning | PL1 | PL2 | tau | Fan profile | TMP profile | Skin PID setpoint |
|---|---|---|---|---|---|---|---|
| 1 | Balanced AC (default scheme) | 28 W | 35 W | 28 s | 2 | 1 | 46.0 °C |
| 2 | Balanced DC | 13 W | 35 W | 8 s | 2 | 1 | 46.0 °C |
| 3 | High Performance AC ("Better Performance", AC default) | 28 W | 45 W | 28 s | 3 | 3 | 47.0 °C (matches the measurement) |
| 4 | High Performance DC | 28 W | 35 W | 28 s | 3 | 3 | 47.0 °C |
| 5 | Max Performance AC ("Best Performance") | 35 W | 60 W | 28 s | 4 | 4 | 48.0 °C (matches the measurement) |
| 6 | Max Performance DC | 30 W | 45 W | 28 s | 4 | 4 | 48.0 °C |
| 7 | Battery saver | 10 W | 11 W | 1 s | 1 | 2 | 45.0 °C |

The PL, tau, fan profile and TMP columns are values read from the tables and the code (confirmed: code and blob; actual
application is confirmed in section 3 for the AC side only). The skin PID setpoint is inferred by reading indexes 1–4 of the
blob's PID rows as the same grade as the fan profile (battery saver 1, balanced 2, Better Performance 3, Best Performance 4).
Indexes 3 and 4 match the measurement (section 3). In the same rows, 20500/22000/23000/25500 appear to be the value PL1 first
drops to when the PID steps in, and 28003/30003/35003 the output cap.

## 3. Windows measurement (AC)

With [`tools/windows/perf-modes.ps1`](../../../tools/windows/perf-modes.ps1), each mode was measured at 1-second intervals
through idle 60 s → 12-thread load 120 s (EcoQoS off) → cool-down 60 s (no other work, no standby entry). The only thing
changed was the power mode overlay, and at the end it was restored to its original value (`3af9b8d9`). Running the same
script with the charger unplugged writes the battery side as `battery-<mode>.md`.

| Mode | `I_SLIDER` | SMF PL1 / PL2 | Fan profile | Highest fan rpm request | Idle W | Peak load W | Sustained W (last 30 s) | Sustained effective clock MHz | Peak package °C | VTS1 at end of load |
|---|---|---|---|---|---|---|---|---|---|---|
| Best Power Efficiency (`961cc777`) | 1 | 28 / 35 W | 2 | 3238 | 3.1 | 20.5 | 18.8 | 2426 | 65 | 45.6 °C |
| Balanced (no overlay) | 1 | 28 / 35 W | 2 | 3238 | 3.3 | 20.0 | 19.1 | 2426 | 65 | 45.8 °C |
| Better Performance (`3af9b8d9`, AC default) | 3 | 28 / 45 W | 3 | 4358 | 3.7 | 35.5 | 28.1 | 2800 | 84 | 46.8 °C |
| Best Performance (`ded574b5`) | 5 | 35 / 60 W; at 110 s of load the PID takes PL1 to 25.5 W | 4 | 4600 | 4.2 | 35.7 | 29.4 | 2794 | 97 | 48.0 °C |

What is confirmed:

- **Slider path**: `I_SLIDER` changes to 1 for efficiency and balanced, 3 for Better Performance and 5 for Best Performance,
  and at the same moment SMF changes PL2 (35 → 45 → 60 W), the PL1 table value (28 → 28 → 35 W) and the fan profile
  (2 → 3 → 4). Best Power Efficiency is the same as Balanced on the Surface side (the same slider 1, the same EPP of AC 60,
  and the same measured values). The only difference between the two is the core parking setting of Windows PPM.
- **MMIO path**: MSR 0x610 is 35 / 60 W throughout all four modes. Sustained power in Better Performance is 28.1 W, sticking
  to the SMF PL1 (28 W). If the MSR PL1 (35 W) were the effective value, it should have gone up to 33–36 W as in Best
  Performance.
- **Skin temperature PID**: in Best Performance, PL1 dropped 35 → 25.5 W at 110 s of load, when VTS1 (sensor 509, ETW
  `I_VTS_SAM_9`) reached 48.0 °C (3211). In a separate run with the device warmer (the part before a standby entry, so
  valid), Better Performance went 28 → 23 → 21.6 W near VTS1 47 °C, and the efficiency mode went down to 18.6 W. The first
  values it drops to, 25.5 W and 23 W, and the threshold 48.0 °C equal the values at indexes 4 and 3 of the blob's PID rows
  (25500/3211, 23000/3201). So the PID row index is taken to be selected not by the slider but by the same grade as the fan
  profile (1 battery saver … 4 Best Performance) (inferred, table in section 2).
- **Clock-side limit at slider 1**: in efficiency and balanced the SMF PL1 is 28 W but load power did not exceed 20 W
  (P-core multiplier 32, E-core 27; Better Performance has 37/31). It appears to be held not by PL but on the clock side
  (Windows PPM, or the "MSR table" SurfaceService writes per slider state, section 1.5) (inferred). To get the same effect
  in Linux `balanced`, EPP or `scaling_max_freq` has to be used. Which one is an open question.
- **Fan rpm request**: 0 below an average CPU temperature of 60 °C, 3000–4600 rpm above it. Under Best Performance load it
  went over 85 °C and stuck to the 4600 cap.
- **PL2 burst**: peak load is 35.5 W in Better Performance and 35.7 W in Best Performance, short of PL2 (45 / 60 W). Even a
  short burst hits current or temperature (Best Performance 97 °C, throttling from 90 °C with TCC offset 10) before PL2
  (inferred; 0x64F could not be read).
- **Standby state**: in a separate run, another task turned the screen off and Modern Standby was entered immediately
  (Kernel-Power 506/507). On this device, screen off = standby entry. `I_MS_NOTIFY` changed 4 → 2 and PL1 and PL2 went down
  to 6 W (VN230, VN92), then came back on resume. The table above is from a run without a standby entry.

### 3.1 The difference between the Windows runs of PWR-9 (26.7 W) and SAM-5 (13.1 W)

Both ran on AC in Better Performance (slider 3, SMF PL1 28 W / PL2 45 W), per the event log (28/45 W after AC was connected,
before both tests). It is not a difference in power limits.

- SAM-5 had `% Processor Utility` 71 %, `% Processor Time` 70 %, `% Processor Performance` 100 % (pinned to the base clock).
  PWR-9 had 130 %. Only about 8 of the 12 threads running, with the clock held at the base value, is what it looks like when
  Windows EcoQoS (power throttling) is applied to the load process (inferred). The load process of that test was a
  windowless background process. `perf-modes.ps1` turns EcoQoS off for the load process (`-DefaultQos` leaves it on), so
  this difference does not recur.
- The fall of power from 32 → 22.5 W during the 300 s load of PWR-9 is the skin PID. At the same time the event log shows
  ch0 (PL1) updated 28 → 23 W.

## 4. Linux design

### 4.1 Who does what

| Control | Linux path | Where it lives |
|---|---|---|
| SAM TMP profile | `surface_platform_profile` (existing) | Kernel (linux-surface) |
| Fan profile and state | `surface_platform_profile` (existing, 0x0e and 0x0f) | Kernel |
| Clearing the fan rpm request | CID 0x0b 0 (patch 0013). CID 0x0c has no effect (measured, section 1.4) | Kernel patch 0013 |
| PL1/PL2/tau (MSR) | `/sys/class/powercap/intel-rapl:0/constraint_{0,1}_power_limit_uw`, `constraint_0_time_window_us` | User space: the device helper, on every profile or power source change |
| PL1/PL2/tau (MMIO) | The same files under `intel-rapl-mmio:0` if it exists (the helper writes them too). This device has none (measured). Raising the limit above the firmware's MMIO value would take a small driver that writes MCHBAR 0x59A0/0x59A4 | Kernel (if the need is confirmed); nothing exists |
| PL4 | `intel-rapl:0/constraint_2_power_limit_uw` | Not written: the firmware value is kept |
| PSys | `intel-rapl:1/constraint_{0,1}_power_limit_uw` | Not written: the firmware values are kept |
| EPP | `/sys/devices/system/cpu/cpufreq/policy*/energy_performance_preference` (intel_pstate active) | User space: the device helper |
| Skin temperature protection | Lowering PL from `temp9` (sensor 9 = VTS1) of the `surface_thermal` hwmon | User space: the helper's skin temperature guard (experimental, off by default) |

What exists. The user-space rows are implemented by the device helper `sp9-helperd` ([helper.md](../../helper.md); policy
details and verification status in [helper-reference.md](../../helper-reference.md)). The NixOS module has one option for it,
`hardware.surface-pro-9.helper.enable` (off by default), and no options for power limits. The settings are the helper's own
(`/etc/sp9/helper.toml`, changed with `sp9-ctl`):

- **Per-profile limits** (`follow_profile`, on by default; `sp9-ctl follow on|off`): when `platform_profile` or the power
  source changes, the helper writes PL1/PL2/tau of the table in 4.3 to RAPL and the EPP to cpufreq. Measured on the device:
  the written values read back from the kernel. Whether they take effect under load is unverified.
- **Overrides** (`sp9-ctl limits set <profile> ac|dc <PL1> <PL2> <tau> <EPP>`, `limits reset`): change one row of the table,
  within PL1 5–35 W, PL1 ≤ PL2 ≤ 60 W and tau 1–28 s.
- **Skin temperature guard** (`skin_guard`, experimental, **off by default**; `sp9-ctl skin-guard on`): lowers PL1 and PL2
  based on sensor 9 (VTS1). A simplification of Windows' VN98 and skin PID (4.2). Unverified on the device.

### 4.2 Are static values enough

On AC with the screen on, the Windows PL table values are constants, but the skin PID cut PL1 during load (section 3). The
parts that change are (1) PL4 and PSys with the battery discharge capability, (2) 6 W in standby / screen off, and (3) the
skin temperature PID and the sensor 509 step table. On Linux:

- (1) Leave PL4 and PSys at the firmware defaults. Linux does not separately protect the battery discharge limit, so voltage
  drop under heavy load on battery is something to look at on Linux (section 5).
- (2) During s2idle the CPU is in PC10, so PL is meaningless.
- (3) is the risk. Under a long-running load the skin PID binds before the package PL1 (21.6–25.5 W at VTS1 47–48 °C). If
  PL1 is left static at 28–35 W on Linux, VTS1 keeps rising past the setpoint and the surface gets hotter than on Windows
  (measured on Linux without the guard: under a ten-minute load in `performance` at a steady 29.9 W, `temp9` rises to 51 °C;
  test PWR-9). This is what the helper's skin temperature guard is for, in every profile including `performance`. It lowers
  PL2 to 35 W when VTS1 exceeds 49 °C and to 30/20/15 W at 50/51/52 °C (Windows' VN98 as it is), and drops PL1 to 20.5–25.5 W when
  the setpoint (45–48 °C per profile, table in section 2) is exceeded, then regulates it with proportional-integral control.
  A 1-second period is enough.

### 4.3 Example: `platform_profile` mapping

| `platform_profile` | Windows counterpart | TMP | Fan profile | PL1 / PL2 / tau | EPP (0–255) |
|---|---|---|---|---|---|
| `low-power` | Battery saver (slider 7) | 2 | 1 | 10 W / 11 W / 1 s (as on Windows) or relaxed 13 / 35 W / 8 s (DC balanced) | 184 (72 %) |
| `balanced` | Balanced (slider 1/2) | 1 | 2 | AC 28 / 35 W / 28 s, DC 13 / 35 W / 8 s | 153 (60 %) / 191 (75 %) |
| `balanced-performance` | Better Performance (3/4, Windows AC default) | 3 | 3 | 28 / 45 W / 28 s (DC 28 / 35) | 115 (45 %) |
| `performance` | Best Performance (5/6) | 4 | 4 | 35 / 60 W / 28 s (DC 30 / 45) | 64 (25 %) |

- `balanced-performance` is the Windows AC default, so it is right to expose it. Desktops (`power-profiles-daemon`) use only
  three levels, so making `balanced-performance` the default AC profile is something a daemon or a udev rule has to set
  separately.
- "Maximum power saving" is `low-power` + the values above; "maximum performance" is `performance` + the skin temperature
  guard. If only the MSR can be written, the MMIO value (firmware default) is the ceiling.
- The helper uses this table as it is; for `low-power` it takes the Windows values (10 W / 11 W / 1 s) on AC and on battery.

## 5. Open questions and what to measure on Linux

| Question | What to do on Linux |
|---|---|
| Is the processor thermal PCI device visible | `lspci -nn -s 00:04.0`; whether `ls /sys/class/powercap/` has `intel-rapl-mmio:0`. Measured: there is no `intel-rapl-mmio`, only `intel-rapl:0`; `lspci` was not run |
| Actual MCHBAR PL values (cold boot, right after rebooting from Windows) | Check the MCHBAR base with `setpci -s 00:00.0 48.l` (0xFEDC0000 expected), then `busybox devmem 0xfedc59a0 64` (`iomem=relaxed` may be needed) or `intel-rapl-mmio:0/constraint_*` |
| MSR-side values | `turbostat --quiet --show PkgWatt,PkgTmp,Bzy_MHz --interval 1`; `rdmsr -a 0x610 0x601 0x65c 0x615 0x64f`; `grep . /sys/class/powercap/intel-rapl:*/constraint_*`. Measured (idle, before the helper writes): package PL1 35 W / 27.98 s, PL2 60 W, peak 121 W, the same as the Windows MSR 0x610 values |
| Does a PL1 change take effect (MSR only) | PkgWatt while switching `constraint_0_power_limit_uw` between 20000000 / 35000000 during `stress-ng --cpu 12 -t 120`. Not measured |
| What the residual fan rpm is | Reboot after a Windows load → surface_fan rpm in `sensors` → send FAN CID 0x0b `00 00` through `surface_aggregator_cdev` and watch the rpm. CID 0x0c has no effect (measured, section 1.4) |
| Is the fan rpm request a floor or a target | Measured with CID 0x0b (section 1.4): neither a floor nor a ceiling; it is obeyed only when the SAM's own target is 0. CID 0x0c has no effect |
| Effect of the SAM TMP profile | Difference in fan rpm and temperature when only `platform_profile` changes at the same PL and EPP |
| Which `surface_thermal` channel on Linux is sensor 509 = VTS1 | By structure it is `temp9` (ACPI `TZ09` `_UID` 9, Windows `I_VTS_SAM_9`, the driver maps channel n to instance n). The labels are `I_RTS1–8` followed by `I_RTS1–3`, so it cannot be picked by name. Comparing the temperature at the same moment with Windows `\_SB.TZ09` is still to be done |
| Is `SMF_DATA_BLOB` readable | `ls /sys/firmware/efi/efivars/SMF_DATA_BLOB-*`; `python3 tools/windows/perf/smf_blob.py <file> out.txt` |
| EPP applied | `cat /sys/devices/system/cpu/cpufreq/policy*/energy_performance_preference`, `rdmsr -a 0x774`. Measured with the helper: the table's value (64, 153 and so on) reads back after a profile change |
| Windows values on battery | On Windows, unplug the charger and run `perf-modes.ps1` again (`battery-<mode>.md`) |
| Is heavy load on battery fine without PL4 and PSys | On battery in `performance` + `stress-ng`: the voltage in `upower -d`, PROCHOT in `dmesg`, whether power cuts out |

More to look at on Windows: a way to read the MCHBAR values directly (a signed read tool under HVCI), how the SurfaceService
PPM "MSR table" is delivered, the meaning of MCHBAR+0x5B10, and which SAM FAN commands nodes 101 and 111 map to (SSH WPP
trace).
