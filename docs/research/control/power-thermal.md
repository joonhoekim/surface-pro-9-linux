# Power and thermal control: what Windows does and how to produce the same control on Linux

Which limits Windows changes, to which values and when (the CPU power limits, the SAM thermal profile and the fan), and how to
get the same control on Linux; the per-power-mode load measurements are the evidence that a control took effect. In short:
Windows sets PL1/PL2 per power mode through MMIO and cuts them on skin temperature; Linux has only the MSR path, which the device
helper writes per profile, and without its experimental skin guard the back runs hotter than on Windows under sustained load.

Notation:

- **Confirmed** = measured on this device, or read from its event log, registry, power tracker events or ACPI tables.
  **Inferred** = there is a basis, but it was not confirmed directly. **Not measured** = a value Windows uses for that mode
  that did not appear in any run here.
- "SMF" = the Surface power management framework in the Windows driver stack (the "Surface System Management Framework"
  devices of [sam-platform.md](../gaps/sam-platform.md) 1.1). It applies the power limits and the fan requests on Windows.
- `ETW` = event 1 (node id, class, name, value) of the provider `Microsoft-Surface-SurfacePowerTrackerCore`
  `{23AC50B3-680E-7AD3-A2B3-D5DBEF945870}`. It reports the inputs and outputs of SMF about once a second. Names such as
  `I_SLIDER` and `O_CPU_AVG` are the names these events carry.
  [`tools/windows/perf-modes.ps1`](../../../tools/windows/perf-modes.ps1) records it.
- Values in these events are in SMF's own units: power in mW, time in ms, temperature in 0.1 K (3181 = 45.0 °C), fan in rpm.
- VTS1 is the skin temperature sensor: SAM TMP instance 9, ACPI `\_SB.TZ09` (`_STR` "VTS1"), `I_VTS_SAM_9` in ETW, `temp9`
  of the Linux `surface_thermal` hwmon. `#1726` is linux-surface/linux-surface issue 1726.
- "PWR-9" and "SAM-5" with Windows figures (26.7 W, 13.1 W) are the runs of those tests of
  [test-plan.md](../../test-plan.md) on Windows; the Windows baseline results are not in this repository. The Linux results
  of the same IDs are in [test-results.md](../../test-results.md).

## Summary

- **The Windows power mode is a single Surface-specific power setting (the "slider state", 1–7), and that value has three
  effects.** (1) The SAM TMP profile (TC 0x03 CID 0x03) is set. (2) SMF takes the slider as an input and applies the
  PL1/PL2/tau and fan profile of that mode. (3) Windows PPM applies the per-overlay EPP and core parking values.
- **PL1/PL2 go in through MCHBAR MMIO (0xFEDC0000 + 0x59A0/0x59A4), not MSR 0x610.** MSR 0x610 stays at the firmware values
  35/60 W, tau 28 s, and the CPU follows the lower of the two. The evidence is that sustained power in Better Performance
  sticks to the SMF PL1 (28 W), at 28.1 W, and not to the MSR PL1 (35 W).
- **What the "constantly changing PL" is**: on top of the per-mode value, the skin temperature (VTS1) control, the battery
  discharge capability and the screen-off/standby state each lower the limits. On AC with the screen on, only the skin
  control moves; when VTS1 reaches the setpoint (47–48 °C) it cuts PL1 to 21–25.5 W (measured).
- **For the fan, Windows sets a profile (CID 0x0e) and a state (0x0f) in SAM FAN, and computes an rpm request** that rises
  linearly with CPU temperature (60 °C 3000 rpm → 85 °C 4600 rpm, ETW). Which SAM command carries the request on the wire is
  not identified; on Linux, CID 0x0b sets the fan speed and CID 0x0c does nothing (measured, section 1.4). The "stuck at
  ~3800 rpm after reboot" of linux-surface#1726 may be this rpm request left behind (inferred).
- The difference between the Windows runs of PWR-9 (26.7 W) and SAM-5 (13.1 W) appears to be not a power limit but **the
  load process being confined to the E-cores and the base clock by EcoQoS** (section 3.1).

## 1. Path of each control

### 1.1 Power mode → slider state

| Item | Content | Evidence | Confidence |
|---|---|---|---|
| Power setting | Subgroup `bfeba2e4-fdcc-41d9-93f4-3f6991c1f730` "Surface Overlay Power Settings Subgroup", setting `a4a0ab02-11f1-4584-bc52-cbb98eabdb0b`. Values: 1 Balanced AC, 2 Balanced DC, 3 High Performance AC, 4 High Performance DC, 5 Max Performance AC, 6 Max Performance DC | `HKLM\SYSTEM\CurrentControlSet\Control\Power\PowerSettings\bfeba2e4…\a4a0ab02…` | Confirmed |
| Default per overlay | Default Balanced scheme 1/2, `3af9b8d9` (Better Performance) 3/4, `ded574b5` (Best Performance) 5/6. `961cc777` (Best Power Efficiency) has no value and follows the default scheme's values (1/2) | `DefaultPowerSchemeValues` under the same key | Confirmed (registry; in the measurement the efficiency mode has `I_SLIDER` = 1) |
| Default AC mode | The AC overlay of this device is `3af9b8d9` (Better Performance). The DC overlay is empty (= Balanced) | `PowerGetEffectiveOverlayScheme`, `ActiveOverlayAcPowerScheme` | Confirmed |
| Battery saver | Slider state 7 | — | Not measured |
| SMF input | The effective slider state is an input of SMF. ETW `I_SLIDER` | ETW | Confirmed (the value changes with the mode switches, section 3) |

### 1.2 SAM thermal profile (TMP)

| Item | Content | Evidence | Confidence |
|---|---|---|---|
| SAM request | TMP (0x03) TID 1 IID 0 CID 0x03, payload `__le32` profile 1–4 | Linux `surface_platform_profile.c` | Confirmed |
| Mapping | Slider state → TMP profile: 1, 2 → 1; 3, 4 → 3; 5, 6 → 4; 7 → 2 | — | Not traced on the wire |
| When it is updated | Power mode change, AC/DC switch (whenever the slider state changes) | — | Inferred (timing) |
| Linux | `surface_platform_profile` sends the same request. `platform_profile` low-power → 2, balanced → 1, balanced-performance → 3, performance → 4. Same as the Windows mapping | `drivers/platform/surface/surface_platform_profile.c` (Linux v7.3.4) | Confirmed |

What the SAM firmware changes for a TMP profile cannot be seen from Windows. Windows changes PL and the fan directly through
SMF at the same moment, so changing only TMP on Linux leaves out the PL and fan effects (1.3, 1.4).

### 1.3 CPU power limits

The Windows event log entry "Cpu control update" records each limit SMF applies: Channel Index is the limit and the value is
in mW. The registers are Intel's: the package power limit in MCHBAR (0xFEDC0000 + 0x59A0/0x59A4, the same registers Linux
`intel-rapl-mmio` uses) and the RAPL MSRs.

| ch | ETW name | Register | Meaning | Values seen on this device | Same control on Linux |
|---|---|---|---|---|---|
| 0 | `O_CPU_AVG` | MCHBAR+0x59A0 | PL1 | AC 28 W (Best Performance 35), DC 13 W, standby 6 W. Under load 18.6–25.5 W through the skin control | `intel-rapl-mmio:0/constraint_0_power_limit_uw` (see the caveats below) |
| 1 | `O_CPU_MAX` | MCHBAR+0x59A4 | PL2 | AC 45 W, DC 35 W, standby 6 W | `intel-rapl-mmio:0/constraint_1_power_limit_uw` |
| 9 | `O_CPU_AVG_TIME` | MCHBAR+0x59A0 | PL1 tau | 28 s on AC | `intel-rapl-mmio:0/constraint_0_time_window_us` |
| 2 | `O_CPU_PEAK` | MSR 0x601 | PL4 (current limit) | AC 130 W, DC 95–130 W (varies with battery state) | `intel-rapl:0/constraint_2_power_limit_uw` (peak_power) |
| 3, 4 | `O_SYS_AVG`, `O_SYS_MAX` | MSR 0x65C | PSys PL1/PL2 | AC 55/118 W, DC 100/80–87 W | `intel-rapl:1` (psys) `constraint_0/1` |
| 7 | `O_SYS_PEAK` | MSR 0x615 | PSys maximum (inferred to be of the PL3 family) | AC 130 W, DC 70–85 W | No kernel interface (msr directly) |
| 17 | `O_BATT_MODE` | — | Battery mode indication | AC 0, DC 1 | — |

Verification state:

- **Confirmed (measured)**: MSR 0x610 = `0x004281E000DD8118` → PL1 35 W (enable, clamp), PL2 60 W, tau 28 s, not locked. It
  stays the same when the mode changes (section 3); Windows does not write it. At the same time SMF outputs PL1 28 W and
  sustained load power stays near 28 W. This means a path other than the MSR (the MMIO one) is the effective limit.
- MSR 0x1A2 (TCC offset) is 10 (TjMax 100 → 90 °C) and gets no update events.
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

| What | SAM request (FAN TID 1 IID 1) | Value | Evidence | Confidence |
|---|---|---|---|---|
| Fan profile | CID 0x0e, `u8` | Per mode: balanced 2, Better Performance 3, Best Performance 4. ETW `O_FAN1_PROFILE` | ETW (2, 3, 4 seen, sections 3 and 3.2) | Values 2–4 confirmed; battery saver not measured |
| Fan state | CID 0x0f, `u8` | 1 normal, 2 standby | linux-surface `surface_platform_profile` | Windows side inferred |
| rpm request | Not identified. Not CID 0x0c: that command does not move the fan (measured, below). CID 0x0b `u16` rpm is the command that sets a fan speed (measured, below); that Windows sends its request through it is unverified | 0 below an average CPU package temperature of about 60 °C; above it a linear rise from 3000 rpm at 60 °C to 4600 rpm at 85 °C, capped there. ETW `O_FAN1_ACTIVE_RPM` | ETW (`I_CPU_TEMP_AVG` 3517 → 4191 rpm) | Behaviour confirmed; SAM command unverified |

- linux-surface on Linux sends only the profile (0x0e, following `platform_profile`) and the state (0x0f, 2 on suspend).
- Patch 0013 of this repository sends 0 to CID 0x0b, based on the #1726 trace. The Linux measurement below agrees with that
  choice of command: 0x0b is the rpm command and 0x0c is not. After a load under Windows and a reboot into a kernel with the
  patch, the fan ran for the first two minutes while the device was still warm and then read 0 rpm (measured, SAM-4), so no
  floor stayed behind. The same round trip without the patch has not been done, so that the patch is what clears it is not
  shown by comparison.
- **Linux measurement (7.3.4, requests sent directly to FAN TID 1 IID 1 through `surface_aggregator_cdev`, idle at
  35–45 °C, `balanced`)**:
  - **CID 0x0b `u16` rpm sets the fan speed.** Sending 4000 to a stopped fan gives 3992 rpm in about 30 seconds; sending 0
    gives 0 rpm within a few seconds. Requests of 1500 / 3000 / 4600 / 5000 / 6000 / 7500 gave 1588 / 2977 / 4569 / 4988 /
    5958 / 7407 rpm (each after 22–25 seconds, including values still rising). Every request returns status 0.
  - **CID 0x0c does nothing.** It did not spin the fan either as `float32` (4000, 6000) or as `u16` (stopped fan, status 0).
    It is not the rpm request.
  - **While the SAM is driving the fan itself, 0x0b is ignored.** In `performance` during `stress-ng --cpu 12` (package
    85–89 °C, fan about 5950 rpm), 7000 and 2000 were each sent for 40 seconds and the rpm did not move from 5926–5971
    (status 0). It is neither a floor nor a ceiling; it is obeyed only when the SAM's own target is 0. So a low value cannot
    block cooling, and the only use is to spin the fan up in advance when idle or under light load. The case where 0x0b 5000
    produced no change while the fan was running at about 3000 rpm right after a build is the same situation. In that case
    the drop from 3000 → 1600 rpm right after 0x0c `u16` 5000 is a single observation and the cause is not known.
- No place was found on Windows to read the actual fan rpm (no `Win32_Fan`; ACPI `FAN0` has no `_FST` of its own and the
  value is not exposed to user mode). The rpm in ETW is the requested value.

### 1.5 Other controls

| What | Behaviour | Evidence |
|---|---|---|
| VTS1 offset | Windows sets an offset for VTS1 in SAM (TMP CID 0x24). On this device every boot logs "VTS offset configuration failed. Instance 9" | Event log |
| Standby state (ETW `I_MS_NOTIFY`) | On this device turning the screen off (`SC_MONITORPOWER` off) is immediately Modern Standby entry. Screen-off and standby are not separate; at the moment of entry `I_MS_NOTIFY` goes 4 → 2 and PL1 and PL2 go to 6 W | Kernel-Power 506/507, ETW |
| PPM (Windows power settings) | Per-overlay EPP (%, OEM provisioning `Prov*SettingIndex`): efficiency AC 60 / DC 72, default balanced 60 / 75, Better Performance 45 / 60, Best Performance 25 / 33. Same values for P-cores and E-cores (Class 1). As HWP EPP, x 255 / 100 = 153, 115, 64 | `PowerSettings\54533251…\36687f9e…6863`, `…6864` |

## 2. Per-mode limits

The limits Windows applies per slider state, before the skin temperature, battery and standby reductions, as the power
tracker reported them in the runs of section 3 (AC) and 3.2 (battery). Only measured values are listed.

| Slider | Meaning | PL1 | PL2 | tau | Fan profile | Skin setpoint and first PL1 |
|---|---|---|---|---|---|---|
| 1 | Balanced AC (default scheme; also Best Power Efficiency) | 28 W | 35 W | 28 s | 2 | Not measured |
| 2 | Balanced DC (also Best Power Efficiency) | 13 W | 35 W | 8 s | 2 | Not measured |
| 3 | Better Performance AC (AC default) | 28 W | 45 W | 28 s | 3 | 47 °C → 23 W |
| 4 | Better Performance DC | 28 W | 35 W | 28 s | 3 | Not measured |
| 5 | Best Performance AC | 35 W | 60 W | 28 s | 4 | 48 °C → 25.5 W |
| 6 | Best Performance DC | 30 W | 45 W | 28 s | 4 | Not measured |
| 7 | Battery saver | Not measured | | | | |

Reductions on top of these values:

- **Skin temperature (VTS1)**: when VTS1 reaches the setpoint, PL1 first drops to the value in the table and is then
  regulated (section 3: Best Performance 35 → 25.5 W at 48.0 °C, Better Performance 28 → 23 → 21.6 W near 47 °C). The
  setpoints of the other modes and any PL2 reduction on VTS1 were not seen in a run.
- **Standby**: PL1 and PL2 6 W while in Modern Standby (measured, section 3).
- PL4 and PSys follow the battery's discharge capability and the adapter size (the ranges in 1.3).

## 3. Windows measurement

With [`tools/windows/perf-modes.ps1`](../../../tools/windows/perf-modes.ps1), each mode was measured at 1-second intervals
through idle 60 s → 12-thread load 120 s (EcoQoS off) → cool-down 60 s (no other work, no standby entry). The only thing
changed was the power mode overlay, and at the end it was restored to its original value (`3af9b8d9`). Running the same
script with the charger unplugged writes the battery side as `battery-<mode>.md`.

| Mode | `I_SLIDER` | SMF PL1 / PL2 | Fan profile | Highest fan rpm request | Idle W | Peak load W | Sustained W (last 30 s) | Sustained effective clock MHz | Peak package °C | VTS1 at end of load |
|---|---|---|---|---|---|---|---|---|---|---|
| Best Power Efficiency (`961cc777`) | 1 | 28 / 35 W | 2 | 3238 | 3.1 | 20.5 | 18.8 | 2426 | 65 | 45.6 °C |
| Balanced (no overlay) | 1 | 28 / 35 W | 2 | 3238 | 3.3 | 20.0 | 19.1 | 2426 | 65 | 45.8 °C |
| Better Performance (`3af9b8d9`, AC default) | 3 | 28 / 45 W | 3 | 4358 | 3.7 | 35.5 | 28.1 | 2800 | 84 | 46.8 °C |
| Best Performance (`ded574b5`) | 5 | 35 / 60 W; at 110 s of load the skin control takes PL1 to 25.5 W | 4 | 4600 | 4.2 | 35.7 | 29.4 | 2794 | 97 | 48.0 °C |

What is confirmed:

- **Slider path**: `I_SLIDER` changes to 1 for efficiency and balanced, 3 for Better Performance and 5 for Best Performance,
  and at the same moment SMF changes PL2 (35 → 45 → 60 W), PL1 (28 → 28 → 35 W) and the fan profile (2 → 3 → 4). Best
  Power Efficiency is the same as Balanced on the Surface side (the same slider 1, the same EPP of AC 60, and the same
  measured values). The only difference between the two is the core parking setting of Windows PPM.
- **MMIO path**: MSR 0x610 is 35 / 60 W throughout all four modes. Sustained power in Better Performance is 28.1 W, sticking
  to the SMF PL1 (28 W). If the MSR PL1 (35 W) were the effective value, it should have gone up to 33–36 W as in Best
  Performance.
- **Skin temperature control**: in Best Performance, PL1 dropped 35 → 25.5 W at 110 s of load, when VTS1 (ETW
  `I_VTS_SAM_9`) reached 48.0 °C (3211). In a separate run with the device warmer (the part before a standby entry, so
  valid), Better Performance went 28 → 23 → 21.6 W near VTS1 47 °C, and the efficiency mode went down to 18.6 W. The
  setpoint of the efficiency and balanced modes was not pinned down.
- **Clock-side limit at slider 1**: in efficiency and balanced the SMF PL1 is 28 W but load power did not exceed 20 W
  (P-core multiplier 32, E-core 27; Better Performance has 37/31). It appears to be held not by PL but on the clock side
  (Windows PPM settings per slider state) (inferred). To get the same effect in Linux `balanced`, EPP or `scaling_max_freq`
  has to be used. Which one is an open question.
- **Fan rpm request**: 0 below an average CPU temperature of 60 °C, 3000–4600 rpm above it. Under Best Performance load it
  went over 85 °C and stuck to the 4600 cap.
- **PL2 burst**: peak load is 35.5 W in Better Performance and 35.7 W in Best Performance, short of PL2 (45 / 60 W). Even a
  short burst hits current or temperature (Best Performance 97 °C, throttling from 90 °C with TCC offset 10) before PL2
  (inferred; 0x64F could not be read).
- **Standby state**: in a separate run, another task turned the screen off and Modern Standby was entered immediately
  (Kernel-Power 506/507). On this device, screen off = standby entry. `I_MS_NOTIFY` changed 4 → 2 and PL1 and PL2 went down
  to 6 W, then came back on resume. The table above is from a run without a standby entry.

### 3.1 The difference between the Windows runs of PWR-9 (26.7 W) and SAM-5 (13.1 W)

Both ran on AC in Better Performance (slider 3, SMF PL1 28 W / PL2 45 W), per the event log (28/45 W after AC was connected,
before both tests). It is not a difference in power limits.

- SAM-5 had `% Processor Utility` 71 %, `% Processor Time` 70 %, `% Processor Performance` 100 % (pinned to the base clock).
  PWR-9 had 130 %. Only about 8 of the 12 threads running, with the clock held at the base value, is what it looks like when
  Windows EcoQoS (power throttling) is applied to the load process (inferred). The load process of that test was a
  windowless background process. `perf-modes.ps1` turns EcoQoS off for the load process (`-DefaultQos` leaves it on), so
  this difference does not recur.
- The fall of power from 32 → 22.5 W during the 300 s load of PWR-9 is the skin temperature control. At the same time the
  event log shows ch0 (PL1) updated 28 → 23 W.

### 3.2 Battery

The same script with the charger unplugged (Best Power Efficiency and Balanced report the same slider 2):

| Mode | `I_SLIDER` | SMF PL1 / PL2, tau | Fan profile | Highest fan rpm request | Idle W | Peak load W | Sustained W (last 30 s) | Sustained effective clock MHz | Peak package °C |
|---|---|---|---|---|---|---|---|---|---|
| Best Power Efficiency | 2 | 13 / 35 W, 8 s | 2 | 0 | 5.6 | 15.7 | 12.9 | 1988 | 55 |
| Balanced | 2 | 13 / 35 W, 8 s | 2 | 0 | 3.4 | 12.7 | 12.2 | 1872 | 54 |
| Better Performance | 4 | 28 / 35 W, 28 s | 3 | 4358 | 3.8 | 29.0 | 27.9 | 2675 | 86 |
| Best Performance | 6 | 30 / 45 W, 28 s | 4 | 4600 | 5.3 | 37.5 | 30.4 | 2818 | 95 |

On battery the limits did not move during the 120 s load (no skin control reached).

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
| Skin temperature protection | Lowering PL from `temp9` (VTS1) of the `surface_thermal` hwmon | User space: the helper's skin temperature guard (experimental, off by default) |

What exists. The user-space rows are implemented by the device helper `sp9-helperd` ([helper.md](../../helper.md); policy
details and verification status in [helper-reference.md](../../helper-reference.md)). The NixOS module has one option for it,
`hardware.surface-pro-9.helper.enable` (off by default), and no options for power limits. The settings are the helper's own
(`/etc/sp9/helper.toml`, changed with `sp9-ctl`):

- **Per-profile limits** (`follow_profile`, on by default; `sp9-ctl follow on|off`): when `platform_profile` or the power
  source changes, the helper writes PL1/PL2/tau of the table in 4.3 to RAPL and the EPP to cpufreq. `low-power` has no
  limits row (battery saver not measured): there the helper writes only the EPP and leaves RAPL as it is. Measured on the device:
  the written values read back from the kernel. Whether they take effect under load is unverified.
- **Overrides** (`sp9-ctl limits set <profile> ac|dc <PL1> <PL2> <tau> <EPP>`, `limits reset`): change one row of the table,
  within PL1 5–35 W, PL1 ≤ PL2 ≤ 60 W and tau 1–28 s (the upper bounds are the highest values measured on Windows; the
  floors are the helper's own).
- **Skin temperature guard** (`skin_guard`, experimental, **off by default**; `sp9-ctl skin-guard on`): lowers PL1 based on
  VTS1 in `balanced-performance` and `performance`, the two modes whose setpoint was measured (4.2). Unverified on the device.

### 4.2 Are static values enough

On AC with the screen on, the Windows per-mode values are constants, but the skin temperature control cut PL1 during load
(section 3). The parts that change are (1) PL4 and PSys with the battery discharge capability, (2) 6 W in standby / screen
off, and (3) the skin temperature control on VTS1. On Linux:

- (1) Leave PL4 and PSys at the firmware defaults. Linux does not separately protect the battery discharge limit, so voltage
  drop under heavy load on battery is something to look at on Linux (section 5).
- (2) During s2idle the CPU is in PC10, so PL is meaningless.
- (3) is the risk. Under a long-running load the skin control binds before the package PL1 (21.6–25.5 W at VTS1
  47–48 °C). If PL1 is left static at 28–35 W on Linux, VTS1 keeps rising past the setpoint and the surface gets hotter than
  on Windows (measured on Linux without the guard: under a ten-minute load in `performance` at a steady 29.9 W, `temp9`
  rises to 51 °C; test PWR-9). This is what the helper's skin temperature guard is for. It drops PL1 to 23 W at 47 °C in
  `balanced-performance` and to 25.5 W at 48 °C in `performance` (the measured values of section 2), then regulates it
  with proportional-integral control; PL2 is left alone. The other profiles get no guard until their setpoints are
  measured. A 1-second period is enough.

### 4.3 Example: `platform_profile` mapping

| `platform_profile` | Windows counterpart | TMP | Fan profile | PL1 / PL2 / tau | EPP (0–255) AC / battery |
|---|---|---|---|---|---|
| `low-power` | Battery saver (slider 7) | 2 | 1 | Not written (battery saver not measured) | 184 (72 %, the efficiency mode's battery value) |
| `balanced` | Balanced (slider 1/2) | 1 | 2 | AC 28 / 35 W / 28 s, battery 13 / 35 W / 8 s | 153 (60 %) / 191 (75 %) |
| `balanced-performance` | Better Performance (3/4, Windows AC default) | 3 | 3 | AC 28 / 45 W / 28 s, battery 28 / 35 W / 28 s | 115 (45 %) / 153 (60 %) |
| `performance` | Best Performance (5/6) | 4 | 4 | AC 35 / 60 W / 28 s, battery 30 / 45 W / 28 s | 64 (25 %) / 84 (33 %) |

- PL1/PL2/tau are the power tracker values of sections 3 and 3.2; EPP is the per-overlay value of 1.5.
- `balanced-performance` is the Windows AC default, so it is right to expose it. Desktops (`power-profiles-daemon`) use only
  three levels, so making `balanced-performance` the default AC profile is something a daemon or a udev rule has to set
  separately.
- "Maximum performance" is `performance` + the skin temperature guard. If only the MSR can be written, the MMIO value
  (firmware default) is the ceiling.
- The helper uses this table as it is. In `low-power` it writes only the EPP; PL1/PL2/tau keep whatever was in force
  before (at most the firmware's 35 / 60 W).

## 5. Open questions and what to measure on Linux

| Question | What to do on Linux |
|---|---|
| Is the processor thermal PCI device visible | `lspci -nn -s 00:04.0`; whether `ls /sys/class/powercap/` has `intel-rapl-mmio:0`. Measured: there is no `intel-rapl-mmio`, only `intel-rapl:0`; `lspci` was not run |
| Actual MCHBAR PL values (cold boot, right after rebooting from Windows) | Check the MCHBAR base with `setpci -s 00:00.0 48.l` (0xFEDC0000 expected), then `busybox devmem 0xfedc59a0 64` (`iomem=relaxed` may be needed) or `intel-rapl-mmio:0/constraint_*` |
| MSR-side values | `turbostat --quiet --show PkgWatt,PkgTmp,Bzy_MHz --interval 1`; `rdmsr -a 0x610 0x601 0x65c 0x615 0x64f`; `grep . /sys/class/powercap/intel-rapl:*/constraint_*`. Measured (idle, before the helper writes): package PL1 35 W / 27.98 s, PL2 60 W, peak 121 W, the same as the Windows MSR 0x610 values |
| Does a PL1 change take effect (MSR only) | PkgWatt while switching `constraint_0_power_limit_uw` between 20000000 / 35000000 during `stress-ng --cpu 12 -t 120`. Not measured |
| What the residual fan rpm is | With patch 0013 none stays after a Windows load (measured, SAM-4, section 1.4). Still to do: the same round trip on a kernel without the patch |
| Is the fan rpm request a floor or a target | Measured with CID 0x0b (section 1.4): neither a floor nor a ceiling; it is obeyed only when the SAM's own target is 0. CID 0x0c has no effect |
| Effect of the SAM TMP profile | Difference in fan rpm and temperature when only `platform_profile` changes at the same PL and EPP |
| Which `surface_thermal` channel on Linux is VTS1 | By structure it is `temp9` (ACPI `TZ09` `_UID` 9, Windows `I_VTS_SAM_9`, the driver maps channel n to instance n). The labels are `I_RTS1–8` followed by `I_RTS1–3`, so it cannot be picked by name. Comparing the temperature at the same moment with Windows `\_SB.TZ09` is still to be done |
| EPP applied | `cat /sys/devices/system/cpu/cpufreq/policy*/energy_performance_preference`, `rdmsr -a 0x774`. Measured with the helper: the table's value (64, 153 and so on) reads back after a profile change |
| Windows values on battery | On Windows, unplug the charger and run `perf-modes.ps1` again (`battery-<mode>.md`) |
| Is heavy load on battery fine without PL4 and PSys | On battery in `performance` + `stress-ng`: the voltage in `upower -d`, PROCHOT in `dmesg`, whether power cuts out |

More to look at on Windows: a way to read the MCHBAR values directly (a signed read tool under HVCI), how the per-slider
clock limit of balanced is applied, and which SAM FAN command carries the rpm request (SSH trace).
