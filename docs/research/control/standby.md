# Modern Standby entry and exit control: what Windows does and the same control on Linux

This document describes which component sends what, over which path, when Windows enters and leaves Modern Standby (S0ix) on
this device, and how the same control is done, or has to be done, on Linux (v7.3.4 + linux-surface 7.3). The background and
the baseline power figures are in [display-power](../gaps/display-power.md) (section 1.4 and difference 6); the differences
on the SAM side are in [sam-platform](../gaps/sam-platform.md) (G5–G8).

Notation: ACPI references name the object in the DSDT or in the Surface SSDT. The ACPI tables are not in this repository; a
reader dumps them from their own device (`tools/windows/dump-acpi.ps1`, or `acpidump` on Linux) or takes them from
`https://github.com/linux-surface/acpidumps` (directory `surface_pro_9_intel`). Linux source references are to v7.3.4:
`s2idle.c:N` = `drivers/acpi/x86/s2idle.c`, `controller.c:N` and `core.c:N` = `drivers/platform/surface/aggregator/`.
`LS/0007:N` = line of linux-surface `patches/7.3/0007-surface-sam.patch`. `HDR` = Microsoft's names for the SAM command IDs, from
the metadata of the Surface app (notation of [sam-platform](../gaps/sam-platform.md)). "PWR-4" with Windows figures is the run of
that test on Windows; the Windows baseline results are not in this repository. Evidence levels are **confirmed** (confirmed from
ASL, binary or log) and **inferred** (based on names or documentation, not confirmed by trace). Of the Windows-side
sequence, only what is in section 8 was seen in a trace (screen off → 506 → PDC phases → exit, PEP constraints); there is no
trace of the order of LPS0 `_DSM`, SAPD and SSH. On Windows standby cannot be ended automatically (section 9), so repeated
measurements are done on Linux (section 10).

## Summary

- **On the LPS0 `_DSM` side, Linux already does everything the firmware does.** In this DSDT the Microsoft functions 2–9 are
  nothing but `Notify(\_SB.SAPD, 0xC0+n)`, and what actually touches hardware is the Intel functions 5/6 (`GUAM`: CPU power
  limit and TCSS `DPOF`). Linux `s2idle.c` calls both the Intel functions (rev 1) and the Microsoft functions (rev 0).
- **What is missing is the receiver of the Notify.** On Windows, `SurfaceAcpiPlatformExtensionDriver` binds to `MSHW0299`
  (SAPD), receives the Notify and **passes it on to SMF (Surface System Management Framework)** (`SmfMsNotification`). There
  is no evidence that SAPD itself sends SAM commands. The SMF clients (fan, CPU, thermal) are taken to issue SAM commands
  according to the standby state (inferred). linux-surface's fan stop during sleep (FAN CID 0x0f = HDR `SetFanMsRequest`)
  corresponds to the Linux version of this path (correspondence by name, inferred).
- **The SAM sleep commands are issued by the SSH driver.** On a change of display state (`GUID_CONSOLE_DISPLAY_STATE`) it
  sends display off/on (SAM CID 0x15/0x16), and on D0 exit/entry of the SSH device it sends UART sleep enter/exit (Linux
  names D0-exit/entry, CID 0x33/0x34; the correspondence is inferred). Linux sends the same four commands from the PM
  callbacks (prepare/suspend/resume/complete). The commands are the same; only the timing differs.
- **The PEP constraints come from the DSDT `PEPD` Intel function 1 (`DEVY`).** Linux `s2idle.c` reads this table and only
  logs it (`check_lps0_constraints`). The entries to note are **`\_SB.SSH` D3** (the SAM UART hub has to be in D3 for
  DRIPS), NVMe (`PEG0.PEGP`) **D0+F1** (not D3), HDA and HECI D0+F1, and D3 for the rest: LPSS, THC, IPU, xHCI, GFX, CNVi.
- **What actually blocked DRIPS on Windows was CNVi (Wi-Fi/BT) activity** (sleepstudy SoC blockers). The device blockers
  GFX0, THC0, UA00, SSH and NVMe were all around 0%.

## 1. Components and channels

| Component | Windows driver | What it does | Evidence |
|---|---|---|---|
| `\_SB.PEPD` `INT33A1`/`PNP0D80` | `acpi.sys` (Microsoft GUID), `intelpep.sys` (Intel GUID) | LPS0 `_DSM`. Microsoft GUID `11e00d56…` function mask `0x3FF`, Intel GUID `c4eb40a0…` mask `0x7F`, and also `57a6512e…` function 1 (LPM requirement registers, 0xC0 bytes) | DSDT `\_SB.PEPD`. `acpi.sys` contains only the Microsoft GUID bytes and `intelpep.sys` only the Intel ones (binary search; `57a6512e` is in neither) |
| `\_SB.SAPD` `MSHW0299` | `SurfaceAcpiPlatformExtensionDriver.sys` 3.40.137.0, boot start, `LoadOrderGroup = Core Platform Extensions` | DMF module `AcpiNotification` → `SmfMsNotification` → `SystemManagementFrameworkInterface_MsNotification`. Also `AcpiPepDevice`/`AcpiPepDeviceFan` (injects `\_SB.FAN0` `_FST` through the PEP, via SAM). It checks the SMBIOS model string (`Surface Pro` and others) | Names in the driver. ETW `Microsoft-Surface-SurfaceAcpiPlatformExtensionDriver` `{ABA759C2-…}` (defines only an error event for a failed Notify, carrying the Notify code; the Operational log is empty) |
| `\_SB.SSH` `MSHW0084` | `SurfaceSerialHubDriver.sys` | `PoRegisterPowerSettingCallback(GUID_CONSOLE_DISPLAY_STATE)` → `SendSamSleepCommand` `DisplayOff`/`DisplayOn`. `EvtDeviceD0Exit`/`D0Entry` → `SendSamSleepCommand` `UartSleepEnter`/`UartSleepExit`. It handles SAM requests internally during connected standby | Names in the driver |
| SSH `_DSM` d5e383e1… | Read by the driver above | fn6 UART idle with screen on 200 ms, fn7 with screen off 150 ms, `_S0W` = 3 | Surface SSDT, `\_SB.SSH` |
| SMF core and clients | `SurfaceSystemManagementFrameworkDriver`, `SurfaceSmfClient`, `SurfaceIhvCpuSmfClient` | The receiving side of the MS notification passed on by SAPD. They change the fan, thermals and CPU power limits through SAM and MSRs | sam-platform, section 1.5. No standby-related names were found in the SMF binaries |
| Kernel power manager, PDC | `ntoskrnl` | Screen off → state phases (PDC phase) → DRIPS. Entry 506, exit 507 (System log) | Event log: 506 "entering Modern Standby", 507 "exiting … Reason: Lid" |

## 2. LPS0 `_DSM`: what this DSDT does in each function

| GUID / function | Name (MS documentation, Linux `s2idle.c:45-52`) | Behaviour in this DSDT |
|---|---|---|
| Microsoft 0 | Supported functions | `0x3FF` (0–9) |
| Microsoft 1 | Device constraints (unused) | Does nothing (condition `Arg2 > 1`) |
| Microsoft 2 | (reserved) | `Notify(SAPD, 0xC2)` |
| Microsoft 3 / 4 | Screen off / on | `Notify(SAPD, 0xC3)` / `0xC4` |
| Microsoft 5 / 6 | LPS0 entry / exit | `Notify(SAPD, 0xC5)` / `0xC6` |
| Microsoft 7 / 8 | Modern Standby (sleep) entry / exit | `Notify(SAPD, 0xC7)` / `0xC8` |
| Microsoft 9 | Intent to turn the display on (Turn On Display) | `Notify(SAPD, 0xC9)` |
| Intel 0 | Supported functions | `0x7F` (0–6) |
| Intel 1 | Device constraints | Fills in the root port constraints with `RPPC` and returns `DEVY` (120 entries) with entries switched on and off by NVS flags (section 3) |
| Intel 2 | Crash dump device | Empty package `BCCD` |
| Intel 3 / 4 | Screen off / on | Port 80 debug codes only (`D8XH`: 3 → `C5 00`, 4 → `C5 AB` when `S0ID == 1`) |
| Intel 5 | LPS0 entry | `GUAM(1)`: port 80 `E0`; if `PSCP`, update the `_PPC` limit (`SPNT`/`BPNT`); if `PLCS`, `SPL1` (lower PL1); `UAMS = !AC`; `P_CS` (no `PAUD.PUAM` → no-op). If `SCME` of the TBT DMA (`TDM0/1`) is 0, IOM register `DPOF = 1` (turns off the TCSS display path; inferred from the bit name). There is no `XHCI.PSLI` |
| Intel 6 | LPS0 exit | `GUAM(0)`: restore `_PPC` and PL1 (`RPL1`), `DPOF = 0` |
| `57a6512e…` 1 | LPM requirement registers | Returns the 0xC0 bytes (`L0IX`) at the `LPMR` address. The Windows drivers do not contain this GUID |

All of this is read from `\_SB.PEPD._DSM` and `GUAM` in the DSDT; `DPOF` is in the TCSS SSDT.

`_INI` of `PEPD` sets `PEPC = 0x200000000` (bit 33). This is what makes the storage root port constraint in section 3
D0+F1.

## 3. PEP device constraints (`DEVY` of Intel function 1)

The form is `{path, enabled, {0, {0xFF, minimum D-state[, 0x81]}}}`. `0x81` is read as the D0:F1 constraint that
`intelpep.sys` names in its root port constraint messages (D0 + component state F1, inferred). Only the entries that are
statically enabled and those enabled conditionally inside `_DSM` are listed. The conditions are NVS values and could not be
read from Windows.

| Device | Constraint | Condition | On this device (per PnP) | Check on Linux |
|---|---|---|---|---|
| `GFX0` | D3 | Always | iGPU | i915 runtime PM, DC6/DC9 counts in `i915_dmc_info` |
| `UA00` (SAM UART) / `UA01`, `UA02` | D3 | UART mode, `UM00 == 1` and so on | UA00 = SSH connection | serdev (8250_dw/LPSS) runtime PM. SSH holds it open, so whether it reaches D3 must be checked |
| **`\_SB.SSH`** | **D3** | Always | SAM hub (`_S0W` 3) | On Linux SSH is an ACPI device but a serdev driver, so there is no ACPI D-state transition. Section 4 |
| `I2C0`–`I2C7`, `SPI0`–`SPI2` | D3 | Controller mode flags | Some | `/sys/bus/pci/devices/*/power/runtime_status` |
| `XHCI` | D3 | Device present | Present | xhci runtime PM, USB device autosuspend |
| `HDAS` | D0+F1 | Device present | Present (SOF/HDA) | Linux may take it down to D3 (deeper) |
| `HECI` | D0+F1 | Always | Present | mei runtime PM |
| `IPU0` | D3 | `IPTP != 0` | Camera IPU6 | Runtime PM of the ipu6 driver. If no driver binds, it stays in D0 and blocks |
| `THC0`, `THC1` | D3 | Device present | THC0 = touch and pen (ITHC) | D3 handling in the `ithc` driver |
| `GLAN`, `GNA0`, `VMD0`, `PUF0/1`, `PEMC`, `PSDC` | D3 | The respective flag | Mostly absent | — |
| `CNVW` (Wi-Fi) | D3 | Device present → `_DSM` enables it | AX211 | iwlwifi D3 in s2idle |
| `PEG0.PEGP` (NVMe, CPU PCIe) | D0+F1 (`PEPC` bit 33 → `GUCP` = 1 → `PKD0`) | `GRPT` = 1 (AHCI, NVMe) | Present | Linux nvme stays in D0 in s2idle and goes down with APST / power states. Check root port L1.2 |
| `TXHC`, `TXDC`, `TRP0`–`3`, `TDM0/1` | D3 | `THCE`, `TDCE`, `TRE0-3`, `DME0/1` | TCSS present | thunderbolt/xhci runtime PM, D3cold when no TBT device is connected |
| BT USB (`8087:0AC9`), `SAT0`, `RP01`–`RP28`, `PRxx` | — | Disabled (or not applicable) | — | — |

Linux `s2idle.c` reads function 1 with rev 1 (`drivers/acpi/x86/s2idle.c:196-303`), compares only the minimum D-state and
ignores the F-state (`0x81`). With `check_lps0_constraints` enabled, the "LPI: Constraint not met" log lets the table above
be checked directly on Linux.

## 4. Step-by-step table

| Step | Windows path | Values and order | Evidence | Same control on Linux | Open |
|---|---|---|---|---|---|
| 1. Screen off | Kernel power manager → `GUID_CONSOLE_DISPLAY_STATE` callback → SSH driver `SendSamSleepCommand` `DisplayOff` | SAM TC 0x01 (SAM), TID 1, IID 0, **CID 0x15**. UART idle time goes 200 → 150 ms (`_DSM` fn7) | SSH driver names (confirmed); the CID is a correspondence by name with Linux `controller.c:1826-1831` (inferred) | `ssam_serial_hub_pm_prepare` → `ssam_ctrl_notif_display_off` (`core.c:413-431`). But it is sent only on suspend. It is absent in the idle state with only the screen off (sam-platform G8) | Whether Windows sends 0x15 also with only the screen off (not in standby); the meaning of the 0x15 response byte |
| 2. LPS0 screen-off notification | `acpi.sys` → `PEPD._DSM` Microsoft fn3, `intelpep.sys` → Intel fn3 (inferred) | MS fn3 → `Notify(SAPD, 0xC3)`. Intel fn3 → port 80 only | Section 2 (confirmed). Which driver calls Intel fn3–6 is inferred from where the GUID is present | `acpi_s2idle_prepare_late_lps0` calls Intel fn3, then MS fn3 (`s2idle.c:549-557`). No hardware effect → the same | Whether Windows calls MS fn3 when the screen goes off (before standby) or on standby entry |
| 3. SAPD → SMF | `SurfaceAcpiPlatformExtensionDriver` `AcpiNotification` (receives Notify 0xC2–0xC9) → `SmfMsNotification` → SMF interface | Inferred to pass the Notify code on as it is. The SMF clients change the fan (`SetFanMsRequest` 0x0f is the candidate) and the CPU power limits | Driver names (confirmed); what is passed on and the SAM commands are inferred | No receiving driver. Current counterpart: linux-surface `surface_platform_profile` sends FAN CID 0x0f = 2 (standby) on suspend and 1 on resume (LS/0007:271-343). Proposal: a small ACPI driver that binds to `MSHW0299` (`surface_sapd`), first to record the 0xC2–0xC9 Notify and, if needed, to forward them to an SSAM client | What SMF actually sends to SAM for each MS notification (needs an SSH trace). Whether any code other than 0xC7/0xC8 is meaningful |
| 4. Modern Standby (sleep) entry | `acpi.sys` → MS fn7 | `Notify(SAPD, 0xC7)`. Per the documentation, once, when the activity phase after screen off ends and the sleep phase begins | Section 2, kernel event 506 (confirmed). The timing is per the MS documentation (inferred) | `s2idle.c:564-570`: MS fn7, then MS fn5. Linux calls 3, 7 and 5 in one go | The fn3 → fn7 interval on Windows; whether fn7 repeats for each DRIPS |
| 5. Devices to D3 | PoFx takes devices down according to the PEP constraints. SSH: D0Exit → `UartSleepEnter` | Table in section 3. SSH (`_S0W` 3) has a D3 constraint, so D0Exit must happen before DRIPS | `DEVY[0x6B]` `\_SB.SSH` D3 (confirmed), SSH driver names (confirmed); UartSleepEnter = CID 0x33 is inferred | `ssam_serial_hub_pm_suspend`: `d0_exit` (CID 0x33) → arm the wake IRQ (GPIO 0xAE) → stop the controller (`core.c:451-479`). The order is the same as on Windows | Whether UartSleepEnter and D0-exit are the same command (SSH trace) |
| 6. LPS0 entry | `intelpep.sys` (inferred) → Intel fn5, `acpi.sys` → MS fn5 | Intel fn5 → `GUAM(1)` (lower the CPU power limit, `DPOF = 1`). MS fn5 → `Notify(SAPD, 0xC5)` | Section 2 (confirmed) | `s2idle.c:564-574`: MS fn7, MS fn5, Intel fn5. The same ASL runs | The order of MS fn5 / Intel fn5 on Windows; whether it repeats for each DRIPS |
| 7. DRIPS | The PMC enters SLP_S0. HW DRIPS 99.8%, 49–52 mW | LPIT: SLP_S0 counter MMIO `0xFE00193C` (8197 Hz) | display-power section 1.4, the Windows run of PWR-4 (confirmed) | `intel_pmc_core`: `slp_s0_residency_usec`, `substate_residencies`, `ltr_show`. On this device `57a6512e` fn1 (LPM requirement registers) is understood to be read by the pmc_core TGL/ADL path (unverified) | Measured on Linux (AC, RTC wake, PWR-1): `slp_s0` 27.86–28.35 s of 30.8–31.2 s per cycle, all of it in S0i2.0. One hour on battery (PWR-4) is unverified |
| 8. SAM events during DRIPS | SSH takes the wake GPIO, handles it as an internal request and wakes the whole system only when needed | — | SSH driver names (confirmed) | Not implemented: `ssam_irq_handle` only logs, and draining the events is a TODO (`controller.c:2672-2700`). At present the system wakes whenever a SAM IRQ arrives | The command that drains events (HDR `Sam/GetPendingTransaction=0x17` is the candidate). sam-platform G7 |
| 9. LPS0 exit | Intel fn6, MS fn6 | `GUAM(0)` (restore the power limit), `DPOF = 0`, `Notify(SAPD, 0xC6)` | Section 2 | `acpi_s2idle_restore_early_lps0`: Intel fn6 → MS fn6 (`s2idle.c:609-617`) | — |
| 10. Intent to turn the display on | MS fn9 | `Notify(SAPD, 0xC9)` | Section 2 | `s2idle.c:619`: fn9 after MS fn6, then fn8 | The order of fn9 and fn8 on Windows |
| 11. Sleep exit | MS fn8 → SAPD → SMF | `Notify(SAPD, 0xC8)`. Exit latency about 285 ms | display-power section 1.4 | `s2idle.c:622`. linux-surface sets FAN 0x0f = 1 on resume | — |
| 12. Devices to D0, SSH D0Entry | SSH `UartSleepExit` | CID 0x34 (inferred) | SSH driver names | `ssam_serial_hub_pm_resume`: release the IRQ → `d0_entry` (`core.c:481-503`) | — |
| 13. Screen on | MS fn4, Intel fn4, SSH `DisplayOn` (CID 0x16) | `Notify(SAPD, 0xC4)`, port 80 | Section 2, SSH driver names | `s2idle.c:627-634` MS fn4 → Intel fn4. `pm_complete` → `display_on` (`core.c:433-448`) | — |

## 5. What blocked DRIPS (Windows sleepstudy, aggregate of 5 battery and AC sessions)

Names and ratios only.

| Kind | Item | Maximum active ratio | Linux counterpart |
|---|---|---|---|
| SoC (PMC) | `NoHwDrips.PMC_CNVi_Bluetooth_ACTIVITY` | 30% (1 session) | btusb/btintel autosuspend; whether BT scanning and connections (pen, keyboard) drop during standby |
| SoC (PMC) | `NoHwDrips.PMC_CNVi_WiFi_ACTIVITY` | 26% | iwlwifi WoWLAN off/on, NetworkManager standby policy. Windows does "Network Connected" standby |
| SoC (PMC) | `NoHwDrips.PMC_THERMAL_SENSOR_ON_OFF_ACTIVITY` | 0% | — |
| Device (Fx) | `GFX0`, `THC0`, `UA00`, `\_SB.SSH`, `PEG0.PEGP` (NVMe) | 0% | The checks in the table in section 3 |
| Activity | Audio, lock screen, WU and so on | 1–2% | User space |

Distribution of DRIPS interval length: 40% in 4–16 minute intervals, 53% in 16 minutes or longer (for the long standby
sessions). There were no PEP pre-vetoes (`PepPreVetoes`).

## 6. Order of work on Linux

1. **Measure first.** Enable `check_lps0_constraints` and do one s2idle → the "Constraint not met" list among the table in
   section 3, `slp_s0_residency_usec`, `substate_residencies`, `ltr_show`. Whether the SSH (UA00) path fails to reach D3 is
   the first thing to look at. Done with `tools/linux/s2idle-trace.sh` (section 10; test PWR-1 in
   [test-results.md](../../test-results.md)): 10/10 cycles on AC entered and resumed with 0 unmet LPI constraints, and
   `slp_s0` accrued in every cycle (S0i2.0 only; S0i3.0 stays 0). Steps 2–5 have not been done.
2. **Observe the SAPD Notify.** With a small driver that only does `acpi_install_notify_handler` on `MSHW0299` (or by
   tracing `Notify` with `acpi.debug_layer`/`debug_level`), see whether 0xC3, 0xC7, 0xC5, 0xC6, 0xC9, 0xC8, 0xC4 arrive in
   that order on s2idle. At present they are dropped at the "No handler for notify" level.
3. **Decide on the SMF counterpart.** If the Windows trace (section 7) shows commands going to SAM after SAPD → SMF other
   than FAN 0x0f, the driver of step 2 sends them as SSAM requests. If not, the linux-surface fan patch is enough.
4. **Drain SAM wake events (G7).** If the system resumes fully for every SAM event (battery, temperature) during DRIPS, the
   residency is eaten away. Implement the `controller.c` TODO.
5. **CNVi.** It was a blocker on Windows too, so decide a standby policy for Linux (Wi-Fi off or no WoWLAN, BT
   autosuspend).

## 7. Windows trace

`tools/windows/standby-trace.ps1` (administrator): starts a logman ETW session, requests capture state (rundown) from
Kernel-Pep, Kernel-Acpi, PDC and Intel PEP, then sends `SC_MONITORPOWER` 2 to the shell window to enter Modern Standby. By
default **a person presses the power button once after N minutes** and the script waits for 507 (section 9). `-TryTimerWake`
is an experimental path that tries the timer wake of `standby-wake.ps1`, and waits for a person if 507 does not come.
"Allow wake timers" (RTCWAKE) is changed and restored only on this path. At the end `standby-summary.py` decodes the ETL
with TDH (`etwdecode.cs`) and produces `summary.md`.

A WPR custom profile (GUIDs in place of provider names) does not record these providers: during a 10-minute standby only
PDC, SleepStudy and UserModePowerService were recorded, and Kernel-Power, Kernel-Acpi, Kernel-Pep, Intel PEP and the Surface
providers had 0 events (237 events in a 13.6 MB ETL). Enabling the same GUIDs with logman gives, in 5 seconds awake,
Kernel-Power 4205, Kernel-Acpi 704, Kernel-Pep 124, Intel PEP 52 and Surface PowerTrackerCore 994 events. For Kernel-Power
the Diagnostic, Performance, Idle and TimerResolution keywords have to be left out (otherwise about 4000 events per second,
mostly 557 timer resolution).

| Provider | What to look for |
|---|---|
| `Microsoft-Windows-Kernel-Acpi` | `MsStateNotification` (event 35, `Promoted`, `NextState`: inferred to be the LPS0 `_DSM` notification), `AmlMethodTrace` (event 7, name filter `PEPD`, `SAPD`, `_DSM`), GPE, fan state |
| `Microsoft-Windows-Kernel-Pep` | `PepDeviceRundown` (per-device platform state → D-state dependency = the constraints actually applied), `PepPlatformIdleTransition` |
| `IntelPepTraceLoggingProvider` `{9B82887E-…}` | `DeviceConstraints`, `StandbySessionSummary`, `HwDripsTime100ns` and so on (by name). The GUID was read from the provider metadata in the binary |
| `Microsoft-Windows-PDC`, `Kernel-Power` (including PoFx device D-states), `SleepStudy`, `UserModePowerService` | Screen off, phase entry/exit, 506/507/566, activators |
| Surface: SAPD, SSH, SMF Core, SMF Client, SMF CPU Client, PowerState, PowerTrackerCore, HotPlug, HidMiniDriver, Battery | SAPD Notify errors, SSH sleep commands, SMF reactions |
| `Intel-iaLPSS2-UART2`, `IntelTHCBase` | D-states of UA00 and THC |

## 8. Entry and exit sequence (measured)

One run: AC connected, entry by screen off (`SC_MONITORPOWER` broadcast), exit by a person pressing the power button 10
minutes 36 seconds later (`standby-trace.ps1`, captured with the WPR profile). The only providers recorded are PDC,
SleepStudy and UserModePowerService, so LPS0 `_DSM`, SAPD, SSH and device D-states are not in this run (section 7). Times
are relative to the moment `SC_MONITORPOWER` was sent.

| Time | Event | Source |
|---|---|---|
| 0 | `SC_MONITORPOWER` 2 broadcast | Script log |
| +0.88 s | UserModePowerService `SmartUserPresenceSuspendResume` Suspend=false | ETW |
| +0.91 s | 2 PDC activators `PowerEvent / PowerOriginEvent:MonitorDisplayOff` active (`wpncore.dll`) | ETW |
| +0.92 s | Kernel-Power 566 (session transition, reason SleepButton) → **506 "entering Modern Standby, SC_MONITORPOWER"** → SleepStudy session start | System log, ETW |
| +1.8–2.0 s | PDC `USO Worker` active 0.2 s | ETW |
| +2.4 s | The 2 `MonitorDisplayOff` activators released (each held about 1.5 s) | ETW |
| +1 min 48 s | The only PDC activity during standby: `WNS` (push channel keepalive, `pdcReasonPNGSent`/`PacketReceived`) 0.27 s | ETW |
| +10 min 36.78 s | `NCSI` connectivity check active (0.3 s before 507; start of exit = return to the resiliency phase) | ETW |
| +10 min 37.01 s | `SmartUserPresenceSuspendResume` Suspend=true, PDC `MonitorDisplayOn` active | ETW |
| +10 min 37.09 s | **507 "exiting Modern Standby, Power Button"**, 566 (reason Unknown) | System log, ETW |
| +10 min 37.87 s | **506 entered again (SC_MONITORPOWER)**: the broadcast left in the message queue of a window that was frozen during standby was processed after resume. Exit by keyboard 4 s later | System log |

The same session in sleepstudy:

| Item | Value |
|---|---|
| Length | 641.1 s, entry reason 0x101 (Transition To Sleep), exit reason 0x1 (Power Button) |
| SW DRIPS / **HW DRIPS** | 97.9% / **97.8%** |
| Phases | Resiliency 99.1%, Low Power 0.8% (5.4 s), Resiliency Notification 0% |
| Top blockers | Audio Active 2% (audio activity type; a browser `TimerForAudioPlayback` activator 10 s before entry), PDC Low Power Phase 1%. Device, SoC and processor blockers and PEP pre-vetoes 0 |
| Distribution of DRIPS interval length | 27% in the 2-minute range, 70% in the 4-minute range |
| Energy | On AC, so battery drain 0 mWh. RAPL package average 65 mW (41.8 J), PP0 (cores) 24 mW, PP1 (GPU) 0 mW |
| Exit latency | 278 ms (resiliency 47 ms, turning the display on 229 ms) |

The constraints actually applied, seen with logman from the capture state while awake (dry run) (Kernel-Pep
`PepDeviceRundown`; the same values for platform idle states 0 and 1):

- **D3**: `GFX0`, `UA00`, `I2C0`, `I2C2`, `I2C3`, `I2C6`, `XHCI`, `TXHC`, `TDM1`, `CNVW`, `IPU0`, `THC0`, `GNA0`,
  **`\_SB.SSH`**.
- **D0**: `HDAS`, `HECI`, `PEG0.PEGP` (NVMe), `TRP2`, `TRP3`, 12 CPUs. This agrees with the reading of `DEVY` in section 3
  (SSH D3; NVMe, HDA, HECI D0+F1). Intel PEP reports NVMe with L1 substates 0xF (both L1.1 and L1.2), not D3cold
  (`PCIeEndPointConstraintInfo`).
- Even while awake, `UA00._PS0` → `UA00._PS3` 1.3 s later repeats (a Kernel-Acpi `FrequentAmlMethod` target). Windows takes
  the SAM UART controller down to D3 every time it is idle. This is the reference to compare the runtime PM of the Linux
  serdev/LPSS UART against.

### 8.1 Entry and exit by the cover (lid) (measured, on battery)

A record of closing the cover over the screen and opening it about 27 seconds later with `standby-trace.ps1 -ByLid` (on
battery; with the logman capture the Kernel-Acpi, Pep, PDC and Intel PEP events were all recorded).

| Time (relative to entry) | What |
|---|---|
| −0.85 s | `\_GPE._L52` runs (4 µs) → `\_SB.LID0` button notification (Kernel-Acpi ButtonNotification, EventMask 0xA0004). GPE 0x52 is the lid |
| 0 | Kernel-Power 506 "Reason: Lid" |
| +0.006 s | `\_SB.PEPD._DSM` call (LPS0 notification) |
| +0.01 – 0.15 s | PDC phases 2→6 passed in succession |
| +5.5 s | PDC phase 7→8 (the deepest phase) |
| During standby | `\_SB.FAN0._DSM` once, no other activators |
| +26.6 s | 507 "Reason: Lid" (cover opened) |

Linux counterpart: the lid is the same path, GPE 0x52 → `LID0` (`PNP0C0D`). The Linux `button` driver receives `LID0` and
logind `HandleLidSwitch=suspend` enters s2idle. To wake by the lid, GPE 0x52 has to be enabled as a wake GPE, which
linux-surface `surface_gpe` does for the SP9 SKU (sam-platform G10; the same GPE as the GPE storm of display-power
differences 7 and 10). Check on Linux: `cat /proc/acpi/wakeup`, `cat /sys/firmware/acpi/interrupts/gpe52`, and whether
`gpe52` increases by about 2 per cycle after closing and opening the cover 5 times (thousands if it is a storm).

The Surface driver providers (SAPD, SSH, SMF Core, HotPlug, HidMini, Battery) have 0 events in this record too. Either the
GUIDs are wrong or they are WPP providers that cannot be enabled through the manifest path (inferred). SMF CPU and
PowerTrackerCore do come in.

## 9. Why automatic wake does not work on Windows

| Attempt | Result | Evidence |
|---|---|---|
| `NtPowerInformation(PhysicalPowerButtonPress = 90)` (administrator, SeShutdownPrivilege) | `0xC000000D` (STATUS_INVALID_PARAMETER) on entry. For exit it was placed as the last step of the service below, but that step was never reached | standby-trace run log |
| SYSTEM scheduled task with WakeToRun, 10 minutes later | The task runs on time but no 507 appears. That run also left standby only when a person pressed the power button 47 s after the scheduled time | 507 reason Power Button |
| Resume timer in a service (`standby-wake.cs`: `SetWaitableTimerEx` fResume, 90 s) → `PowerSetRequest` → `SendInput` from a user task → power button path | The timer was signalled only after 181 s, and `PowerSetRequest(DisplayRequired)` failed (only SystemRequired succeeded). The service's 8 s wait after that took **14 minutes** and ended only after a person woke the device with the keyboard. The input injection and power button steps did not run | Wake log, 507 reason Input Keyboard |
| Keyboard input injected from the user session | 9 s after entry it did cause an exit (507 Input Keyboard). In a longer standby it did not work because the injecting process was frozen | System log |

Interpretation (inferred; agrees with Microsoft's Modern Standby documentation): an exit from Modern Standby that turns the
screen on happens only on user-presence input (power button, lid, keyboard/touch/pen HID, face recognition). Wake timers and
WakeToRun only open the resiliency (maintenance) phase with the screen off, and during it the desktop processes and the
scheduled task processes are kept frozen by DAM. A session 0 service cannot issue a display request, and when the short CPU
time it got is over the system goes back to DRIPS. So the Windows-side trace is taken only by having a person press the
power button (the default of `standby-trace.ps1`), and repeated and long measurements are done in Linux s2idle, which wakes
itself by RTC.

## 10. Automatic measurement on Linux (`tools/linux/s2idle-trace.sh`)

Run it as, for example, `sudo tools/linux/s2idle-trace.sh -n 5 -s 300`. Unattended, it repeats N seconds of s2idle with
`rtcwake -m freeze -s N` followed by waking on the RTC alarm, leaves the following for each cycle in
`~/sp9-s2idle-<host>-<time>/cycleK/` and collects one line per cycle in `summary.tsv`. The settings it changes (`pm_test`,
`pm_debug_messages`, `pm_print_times`, dynamic debug, ftrace) are restored at the end.

| Collected | Windows counterpart | What to look at |
|---|---|---|
| `pmc_core/slp_s0_residency_usec`, `substate_residencies`, `package_cstate_show`, `ltr_show`, `substate_status_registers`, before and after | HW DRIPS (97.8%), sleepstudy SoC blockers | Ratio of SLP_S0 to elapsed time. If 0, the blocking IP from `substate_status_registers` and `ltr_show` |
| `low_power_idle_system_residency_us`, `low_power_idle_cpu_residency_us` | LPIT SLP_S0 counter (the source of Windows' HwLowPowerStateTime) | Whether it agrees with the pmc_core value |
| dmesg with `pm_debug_messages` (cut out by cycle boundary markers) | Steps 2, 4, 6, 9–13 of section 4 | The order of LPS0 `_DSM` calls, "LPI: Constraint not met" (compare with the table in section 3 and the measured constraints in section 8) |
| ftrace `power:device_pm_callback_*`, `power:suspend_resume`, `power:wakeup_source_*`, `surface_aggregator:*` | SSH `SendSamSleepCommand`, SMF | SAM requests (display off/on 0x15/0x16, D0 exit/entry 0x33/0x34, fan 0x0f) and SAM events during standby |
| `suspend_stats` (`last_hw_sleep`), `pm_wakeup_irq`, change in `wakeup_sources` | Exit reason, sleepstudy activity | Wakes other than RTC (SAM IRQ = G7, lid, network) |
| Battery `energy_now` (or charge × voltage) before and after → mW | sleepstudy drain (battery sessions 49–52 mW) | Meaningless on AC. Long cycles (≥ 10 minutes) on battery |
| `-t devices` / `platform` (`/sys/power/pm_test`) | — | Runs only up to the driver callbacks without actually sleeping, to narrow down where a failure is |
| `--turbostat` | RAPL 65 mW (AC standby) | SYS%LPI, Pkg%pc10, PkgWatt over the whole cycle |

Criterion: if every cycle wakes only by RTC (`pm_wakeup_irq` is RTC/ACPI SCI) and SLP_S0 is 90% or more of the elapsed time,
it is considered the same level as Windows. Below that, find the blocking device from the dmesg constraint list and
`substate_status_registers` and fix it in the order of section 6.

## 11. Open

- The actual call order and intervals on Windows (fn3 → fn7 → fn5/fn6 repeated → fn9/fn8 → fn4). First confirm by trace
  whether Kernel-Acpi event 35 carries this.
- Who calls Intel fn3–6 (`intelpep.sys`, inferred). `acpi.sys` has only the Microsoft GUID.
- The SAM commands after SAPD → SMF. If they do not show up in ETW, the SSH UART has to be watched with a logic analyser.
- The correspondences `UartSleepEnter/Exit` = CID 0x33/0x34 and `DisplayOff/On` = 0x15/0x16 (at present by name only).
- The reading `0x81` = D0+F1 and which state corresponds to F1 on Linux (inferred: HDA, HECI, L1 substates of the NVMe root
  port).
- The actual values of the NVS flags (`UM00`, `IM0x`, `THCE`, `TRE0-3`, `DME0/1`, `IPTP`). Check on Linux with `acpidbg` or
  the `check_lps0_constraints` log.
- One complete standby captured with the logman session (Kernel-Acpi `_DSM` and SAPD Notify, SSH sleep commands, the order
  of device D-states). A person has to press the power button (section 9).
- While awake, the Type-C PCIe root ports `TRP2/3` are D0 in the Kernel-Pep table, but the Intel PEP root port constraint
  array has D3 values. Check the values during standby and the format of the array.
