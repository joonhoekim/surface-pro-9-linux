# Display, graphics and power management: how Linux differs from the Windows reference

This document takes the values found on Windows 11 (same device, all hardware attached) as the reference and lists what Linux
(v7.3.4 + the linux-surface 7.3 patches) misses or does differently. Linux-side sources are given as `file:line` in the v7.3.4 source
tree, iwlwifi included. Confidence is either **confirmed** (verified in code or data) or **inferred** (there is evidence, but it has not
been reproduced or measured on Linux on the device).

Measurements on Linux 7.3.4 are in the "Verification on Linux" column of section 3 and in section 4. Unless stated otherwise they were
taken on AC with the kernel command line `i915.enable_psr=1 pci=hpiosize=0`; values from a boot without the two parameters are marked as
such. During the measurements a terminal that kept updating was on screen and other measurements were running on the same device, so the
PSR and package C-state figures are not those of an idle static screen.

Notation: ACPI objects are named by table and path; unless a table is named they are in the DSDT, and an SSDT is named by its OEM
table ID. The tables are not in this repository: dump and decompile them on your own device with
[`tools/windows/dump-acpi.ps1`](../../../tools/windows/dump-acpi.ps1), or see <https://github.com/linux-surface/acpidumps> (directory
`surface_pro_9_intel`). `#N` is an issue of linux-surface/linux-surface. A patch named by file (`0011-surface-shutdown.patch`) is one
of linux-surface's `patches/7.3`; "patch 0014" is one of this repository's patches in `kernel/patches`. Test IDs (`DSP-n`, `PWR-n`,
`NET-n`, `SAM-n`) refer to [test-plan.md](../../test-plan.md) and [test-results.md](../../test-results.md).

## 1. Windows reference values

### 1.1 Panel and EDID

The EDID is 256 B including the extension block (sha256 `0555ee35…f168`). The file is not in this repository. On Windows it is the
registry value `Enum\DISPLAY\LGD0719\...\Device Parameters\EDID`; on Linux it is `/sys/class/drm/card*-eDP-1/edid`.

| Item | Value |
|---|---|
| Manufacturer/product | `LGD` `0x0719`, year 2021. Strings `LGD_MP2.3_`, `LP129WT252166` |
| Serial field (0x0C–0x0F) | Equal to the **TCON firmware version 161.33.3.35** (the revision of the `Surface TCON` device). It is not a per-unit value |
| Input | EDID 1.4, digital, DisplayPort, **8 bpc**, 27×18 cm, features `0x03` (continuous frequency on) |
| Block 0 DTD (preferred) | 2880×1920 **60.00 Hz**, 355.50 MHz, htotal 3000, vtotal 1975 |
| Range descriptor | `range limits only` (flags `0x01`), **vertical 30–120 Hz**, horizontal 240–240 kHz, maximum 720 MHz |
| Extension block 1 | **DisplayID 1.3**, one Type I detailed timing: 2880×1920 **120.00 Hz**, 715.20 MHz, htotal 2980, vtotal 2000 (hfp 8, hs 32, vfp 66, vs 8) |

The 60 Hz and 120 Hz timings have different htotal and vtotal (3000/1975 versus 2980/2000). They are not a pair that differs only in
pixel clock.

### 1.2 Intel graphics driver settings (Windows registry and INF)

Device: `PCI\VEN_8086&DEV_46A8&SUBSYS_00631414`, driver 32.0.101.7085 (`iigd_dch.inf`, section `iADLPD_w11_DS`).
A **Surface-specific extension INF**, `iigd_ext_zc.inf` (Class=Extension, matches only this SUBSYS), overrides the defaults.
The values were read from `HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-…}\0000`.

| Key | Base INF | Surface extension INF / actual value | Meaning (per the INF comments) |
|---|---|---|---|
| `PSR2Disable` | 0 | **1** | PSR2 off |
| `PsrDisableInAc` | absent | **1** | PSR off on AC power |
| `FeatureTestControl` | `0x1200` | **`0x9200`** | Comment "disable LACE". The rest (DPST, PSR, BLC, FBC and so on) is on |
| `DisplayPcFeatureControl` | `0x11C0000` | `0x11C0000` | EnduranceGaming, CdClkBy2Change, DC6v, DP Panel Replay off |
| `Psr2DrrsEnable` | 1 | 1 | Meaningless because PSR2 is off |
| `AdaptiveVsyncEnable` | 1 | 1 | Adaptive sync (VRR) allowed |
| `DpstEpsmWeight`, `PowerDpstAggressivenessLevel` | 100 | 100, 6 | DPST (content-based backlight reduction) in use |
| `eDPFecDisable`, `NoFastLinkTrainingForeDP` | 1, 1 | 1, 1 | eDP FEC off, normal link training every time |
| `MonitorFlickerProfile_800050_<EDID bytes 8–17>_60` | — | `30000, 60000, 16666, 16666` | Flicker profile for the 60 Hz mode of this panel (the key contains the leading part of the EDID). Reads as a range of 30.000–60.000 Hz (unit inferred) |

The current mode is 2880×1920 60 Hz (maximum 120 Hz; `docs/reference/display.txt`). Brightness: WMI `WmiMonitorBrightness` has
101 levels (0–100), currently 52. ACPI `\_SB.PC00.GFX0.DD1F._BCL` returns `\PBCL` (1–100, defaults 50/50) and `_BCM` hands over to the
OpRegion ASLE (`BCLP`, `ASLC=2`) (`_BCL` and `_BCM` are in the SSDT `IGFXSSDT`, `\PBCL` in the DSDT). The panel INF (`surfaceoempanel.inf`) gives
`LGD0719` only a 200% scale factor. Colour profiles (sRGB/Vivid, Dolby Vision components) are installed by
`surfaceoempanelcustomization.inf`. `SurfaceSmfDisplayClient` has no settings in its INF (a single SMF kernel driver).

### 1.3 VBT

The OpRegion cannot be read on Windows, so the values are from the `$VBT` blobs in the **Surface UEFI capsule**
(`surfaceuefi.inf_*\SurfaceUEFI_24.103.143_0.bin`, the same version as the installed UEFI). There are two, and the 23.102.143 capsule
contains the same two. The blobs are not in this repository: [`tools/vbt-extract.py`](../../../tools/vbt-extract.py) extracts them from
a capsule and prints the hashes, and on Linux the running VBT is `/sys/kernel/debug/dri/*/i915_vbt`. The running OpRegion carries the
first one (measured on Linux, section 4 item 1).

| Blob (in the order the tool finds them) | sha256 |
|---|---|
| vbt0 | `eb0062ebc10b52f6b9066a5cf598be23996077b51c32f7d8ed480317d9c51388` |
| vbt1 | `7049f7d814e53e2ac7bc508f1e4a40874da1aafa0f606e6a6e267510eaadc441` |

Both are `$VBT ALDERLAKE-P`, BDB 249, 8701 B with a valid checksum, and they differ only in block 253 (PRD table). There are no
per-unit values. Block 41 (LFP data pointers) is absent (Linux generates it with `generate_lfp_data_ptrs()`, `intel_bios.c:367`).
Main values (for `panel_type` = 2):

| Block | Value |
|---|---|
| 40 LFP options | `panel_type` 2, `panel_type2` 3, DRRS mode bits = 2 (**seamless**) |
| 42/58 LFP timings | Generic Intel reference values (panel 2 = 1024×768 60 Hz). The real panel timings come from the EDID |
| 44 LFP power | dpst 1 (aggressiveness 6), **psr 1**, drrs 1, dmrrs 1, adb 1, lace supported and on, **vrr_feature_enabled 1**, hobl 0, ALS on (0/10/80/300/1000 lux → 70/73/85/100/150%) |
| 9 PSR | full_link 0, aux wake 0, idle_frames 0, TP1 and TP2/3 wake-up selection 2 (**2500 µs**), PSR2 TP2/3 selection 2 (2500 µs) |
| 27 eDP | PPS t1_t3 200 ms, t8 1 ms, t9 200 ms, t10 50 ms, t11_t12 500 ms. color_depth 18 bpp (the EDID's 8 bpc takes precedence), fast link parameters 0 |
| 43 backlight | **PWM** (type 2), control method 2 = **display DDI PWM** (not DPCD AUX), 200 Hz, active high, brightness precision 16 bits, minimum 50/65535 |
| 2 child devices | Port A (DPA) eDP with 4 lanes, port B (DPB, inferred to be the Surface Connect DP), DPH and DPI (marked Type-C/TBT) |

### 1.4 Power states and the Modern Standby reference

- FADT: `Low Power S0 Idle` = 1, PM profile = Tablet, reset register I/O `0xCF9` value 6, PM1a event `0x1800`, PM1a control
  `0x1804`, GPE0 block `0x1860` (length 0x20) (FADT fields).
- `powercfg /a`: S0 Low Power Idle (Network Connected), Hibernate, Fast Startup. No S1–S3.
- LPIT has 2 entries: PKG C10 (MSR `0x632`, FFH) and the SLP_S0 residency counter (MMIO `0xFE00193C`, 0x2005 = 8197 Hz ≈ units of
  122 µs) (LPIT entries).
- LPS0 device `\_SB.PEPD` (`INT33A1`): Intel `_DSM` functions 0–6 (mask `0x7F`), Microsoft `_DSM` functions 0–9 (mask `0x3FF`).
  Microsoft functions 2 and above only do `Notify(\_SB.SAPD, 0xC0 + function number)` (`\_SB.PEPD._DSM`). `SAPD` is `MSHW0299`
  (defined in the SSDT `SSDTSURF`), and on Windows `SurfaceAcpiPlatformExtensionDriver.sys` binds to it. Going by the component names inside
  that driver, it **forwards the Modern Standby entry/exit notifications to SAM (EC) over SSH and manages the fan as a PEP device**
  (based on names only; the exact SAM commands are unverified).
- **sleepstudy reference** (battery, disconnected Modern Standby, collected with `tools/windows/collect-power.ps1`):

| Session | Length | SW DRIPS | HW DRIPS | Drain | Average | Exit reason |
|---|---|---|---|---|---|---|
| 1 | 36.89 h | 100.0% | **99.9%** | 1950 mWh | **52 mW** | Lid |
| 2 | 4.58 h | 99.9% | 99.8% | 470 mWh | 102 mW | Lid |
| 3 | 0.46 h | 98.6% | 98.6% | 350 mWh | 762 mW | Standby battery budget exceeded |
| 4 | 0.25 h | 97.9% | 97.8% | 80 mWh | 319 mW | Standby battery budget exceeded |

  Exit latency is about 285 ms. Settings: standby-to-hibernate battery budget 5%, hibernation enabled. The top standby blockers are the
  Serial IO UART (`UA00`, the SAM link), the Surface Serial Hub and NVMe, all in the 0% range.

### 1.5 Thermal and CPU power

Windows has **no** Intel DTT/DPTF device (`INTC1041`/`INT3400` kind). Thermal policy is handled by `MSHW0187` (Surface Thermal Policy),
11 `MSHW0188` devices (Thermal Zone Sensor, no `_TMP` or trip points; `\_SB.TZ01`–`TZ0B` in the SSDT `SSDTSURF`) and the SMF clients
(`SurfaceSmfAlderlakeClient` = `SurfaceIhvCpuSmfClient.sys`, `SurfaceSmfThermalClient`, `SurfaceSmfFanClient`). The CPU SSDT has no
`_CPC` (HWP is used). The actual PL1/PL2 values cannot be read on Windows without a kernel driver and were not determined.

### 1.6 Ambient light sensor

The ALS is in the `HID Sensor Collection V2` (`HID\VID_045E&PID_005D`) behind SAM and works (WinRT `LightSensor` reading 47 lux,
minimum report interval 750 ms). The power setting "Adaptive display" is off.

### 1.7 Wi-Fi AX211 / BT SAR

- ACPI `\_SB.PC00.CNVW` has `SPLC`, `WRDD`, `ECKV`, `PPAG` (rev 1), `WTAS` (rev 1), `WAIS`, `GPC`, `GLAI` and `_DSM` (Intel GUID
  `7266172c…` functions 0–4, `f21202bf…` functions 0–12). **`WRDS`, `EWRD` and `WGDS` (static SAR and geo
  tables) are absent.** There is no `BRDS` for BT either. Most values are NVS variables and could not be read on Windows.
- Of the UEFI variables iwlwifi looks at (GUIDs `92daaf2f…`, `e65d8884…`), **only `UefiCnvWlanPPAG` exists** (27 B, rev 1, mode 3).
  There are no WRDS/EWRD/WGDS/SGOM/WTAS variables.
- Windows runs a separate `Surface Radio Monitor` (`SurfaceSarManager.dll`, UMDF). Since there are no static SAR tables, it is inferred
  that this component feeds posture- and proximity-based dynamic SAR into the Intel driver.
- Wi-Fi driver 23.160.0.4 (`netwtw6e.inf`); no SAR-related values in the registry.

### 1.8 TCSS/USB4

- Windows uses the **Microsoft software connection manager** (`USB4(TM) Host Router (Microsoft)`, `PCI\USB4_MS_CM`, NHI `8086:466D`).
- **I/O port assignment** (`Win32_PnPAllocatedResource`): only the GPU's `0x2000-0x203F` and the root bridge windows. The TBT root
  ports `8086:461F` and `462F` have **no I/O window**.
- The root bridge I/O windows are `0x0000-0x0CF7` and `0x0D00-0xFFFF`. The `PNP0C02` device `\_SB.PRRE` (UID `PCHRESV`), which covers
  256 B at PMBS (`0x1800`), has `_STA = 0x08` (not present).
- The TBT root ports `TRP0`–`TRP3` have the `_DSD` property `HotPlugSupportInD3` (SSDT `TCSSSSDT`). DMAR flags `0x05`
  (interrupt remapping + **DMA protection platform opt-in**), a DMAR unit dedicated to the IGD and an RMRR (`0x44000000`).

## 2. State of Linux

| Area | Linux behaviour | Source |
|---|---|---|
| EDID 120 Hz mode | The DisplayID Type I timing is added as a mode | `drm_edid.c:6957` |
| VRR range | 30–120 Hz is used only with EDID 1.4, continuous frequency and `range limits only`. This panel satisfies all three | `drm_edid.c:6543-6601` |
| eDP VRR | Checks VBT `vrr_feature_enabled` (BDB ≥ 233), then needs DPCD `MSA_TIMING_PAR_IGNORED` | `intel_bios.c:1423`, `intel_vrr.c:37-75` |
| Fixed modes | The preferred mode (60 Hz) plus an alternate mode of the same resolution (120 Hz) | `intel_panel.c:154-162,342-360` |
| DRRS | The VBT says seamless, but a pair differing only in pixel clock is required. The 60/120 timings differ, so `DRRS_TYPE_NONE` | `intel_bios.c:835-856`, `intel_panel.c:144-151,220-234,535-537` |
| PSR | On eDP it follows VBT `psr` (block 44). If the sink supports PSR2 it enables up to PSR2 (selective update). There is a device quirk that disables only PSR2 | `intel_bios.c:1400`, `intel_psr.c:225-238,683`, `intel_quirks.c:97-100,264` |
| `enable_psr` | -1 default, 0 off, 1 up to PSR1, 2 up to PSR2 | `intel_display_params.c:118-121` |
| PSR per AC/DC | No distinction | No power-source branch in `intel_psr.c` |
| Panel Replay | eDP needs AUX-less ALPM and early transport | `intel_psr.c:590-628` |
| PSR wake-up time | VBT selection value 2 → 2500 µs | `intel_bios.c:1576-1667` |
| Backlight | VBT type 2 → `INTEL_BACKLIGHT_DISPLAY_DDI` (PWM). DPCD AUX backlight only for VBT type 5. Minimum: with 16-bit precision, `50/255` = 0 | `intel_bios.c:1036-1111`, `intel_backlight.c:1206-1230` |
| eDP bpp | When the EDID has a bpc (8), the VBT 18 bpp limit is not applied | `intel_dp.c:1666-1675` |
| DPST/LACE | **Not implemented** (only definitions in `intel_vbt_defs.h`) | Result of `grep -w dpst` |
| Device quirks | No i915/EDID quirk for `46a8:1414:0063` or `LGD 0x0719` | `intel_quirks.c`, `drm_edid.c` |
| s2idle | s2idle by default because of the FADT LPS0 flag. Both the Intel and the Microsoft `_DSM` are used (screen off/on 3·4, entry/exit 5·6, MS 7·8) | `acpi/x86/s2idle.c:41-51,441-500` |
| LPIT | The counter frequency field is used as is | `acpi/acpi_lpit.c:107` |
| `MSHW0299` (SAPD) | **No driver binds to it**. The Notify sent by the LPS0 `_DSM` is not handled | `grep MSHW0299` finds nothing |
| Fan standby state | linux-surface switches the SAM fan to standby on suspend entry (`platform_profile: add fan power state`) | `patches/7.3/0007-surface-sam.patch:255` |
| SAM devices | SP9 group: performance profile + fan, temperature sensors, fan speed, tablet switch, SAM HID sensors | `surface_aggregator_registry.c:373-389` |
| Lid wake | `LID0` has no `_PRW`. linux-surface `surface_gpe` sets GPE `0x52` as a wake source (not upstream) | DSDT `\_SB.LID0`, linux-surface `patches/7.3/0012-surface-gpe.patch` |
| Shutdown | linux-surface skips the PCI shutdown callbacks of TBT (`461e/461f/462f/466d`) and the GPU (`46a8`). Reason: otherwise the EFI ResetSystem(shutdown) that follows returns | `patches/7.3/0011-surface-shutdown.patch:6-14,78-82` |
| Hotplug I/O | 256 B of I/O reserved per hotplug bridge (bridge windows are aligned to 4 KiB). Changed with `pci=hpiosize=` | `pci/pci.c:107-111,6870` |
| ACPI PM I/O reservation | The FADT registers are reserved at `fs_initcall_sync` | `acpi/osl.c:115-147` |
| TBT root port D3 | `HotPlugSupportInD3` is honoured | `pci/pci-acpi.c:1048-1053` |
| USB4 | Native USB4 support is announced in `_OSC` | `acpi/bus.c:494-516` |
| iwlwifi SAR | Reads `WRDS`/`EWRD`/`WGDS`/`PPAG`/`WTAS`/`SPLC`/`WRDD`/`ECKV`/`GLAI`/`WBEM`/`DSBR` from ACPI and the same names from UEFI variables. Microsoft is on the DMI allow list for PPAG and TAS | iwlwifi `fw/acpi.h:15-26`, `fw/uefi.h:10-28`, `fw/regulatory.c:46-60,123-147` |

## 3. Differences

In the last column, the text before "Measured" is the check and the text after it is the result on this device.

| # | Difference | Evidence | Effect | How to close it | Confidence | Verification on Linux |
|---|---|---|---|---|---|---|
| 1 | **PSR2**: Windows disables PSR2 for this SUBSYS. Linux has VBT `psr=1` and enables up to PSR2 when the panel advertises it | `iigd_ext_zc.inf` `PSR2Disable=1` (1.2), `intel_psr.c:683` | The flicker of the wiki and #862. The wiki workaround (`enable_psr=0`) also gives up the PSR1 power saving. Linux loses FBC when it selects PSR1 (measured: `FBC disabled: PSR1 enabled (Wa_14016291713)`, display versions 12–13). Windows keeps FBC on (1.2). The size of the power effect is not measured | First step: `i915.enable_psr=1` (PSR1 only, as on Windows). Upstream fix: `{ 0x46a8, 0x1414, 0x0063, quirk_disable_psr2 }` in the `intel_quirks.c` table | Windows setting confirmed. That PSR2 is the cause of the flicker is inferred | `/sys/kernel/debug/dri/*/i915_edp_psr_status` (mode, whether PSR2, whether the sink supports PSR2); reproduce the flicker with the default, `enable_psr=1` and `=0`. Measured (patch 0014 + `i915.enable_psr=1`): `applying disable PSR2 quirk`, `PSR mode: PSR1 enabled`, the sink is PSR2 capable (`PSR = yes [0x03]`). SRDENT in 16 % of 600 samples at 10 Hz, 0 sink errors. The parameter is an unsafe i915 option, so the kernel is tainted on every boot that uses it (`Setting dangerous option enable_psr - tainting kernel`, taint flag 64). Booted without the parameter (only the quirk of patch 0014, measured): still `PSR mode: PSR1 enabled`, `tainted` 0, IDLE 295 and SRDENT 3 in 300 samples at 10 Hz while the screen updates, and over 60 s idle PkgWatt 1.6, Pkg%pc8 24–28, pc10 0.0. The parameter is not needed on this kernel. No flicker was noticed while using the device in that state (observed; this was not a test of a 10-minute static screen, typing and scrolling, separately on AC and battery). A boot with `=0` is unverified (DSP-1) |
| 2 | **PSR off on AC**: Windows turns PSR off on AC. Linux does not look at the power source | `PsrDisableInAc=1` | If there is flicker that occurs only on AC, only Linux shows it. The power effect is small because it is on AC | If needed, a udev rule that writes the disable value to `i915_edp_psr_debug` when AC is connected (a debugging interface) | Setting confirmed, need inferred | After difference 1, watch for flicker on AC and on battery. Measured: on AC, DPCD `0x170`=`05` (on Windows on AC it reads `00`), which means Linux enables PSR on AC as well. Flicker is unverified. The device helper can apply the split per power source (`sp9-ctl psr off psr1`; the AC side works, the battery side is unverified, [helper-reference.md](../../helper-reference.md)) |
| 3 | **Seamless 60↔120 Hz switching**: Windows has VBT seamless DRRS plus adaptive sync (`AdaptiveVsyncEnable=1`, panel flicker profile 30–60 Hz). On Linux the 60/120 timings differ, so DRRS is off and a 60↔120 switch is a full modeset | EDID (1.1), `intel_panel.c:144-151,220-234` | More power when fixed at 120 Hz, and the screen blinks on a switch (modeset) | Use VRR: with eDP VRR enabled in the compositor and the 120 Hz mode set, the rate is lowered within 30–120 Hz by stretching vblank (a substitute for DRRS). Another option is an EDID override that replaces the 60 Hz timing with one that has the 120 Hz htotal/vtotal and half the clock, which would revive DRRS, but whether the panel accepts it is unverified | Timing difference and DRRS off confirmed; that Windows switches by VRR is inferred | `i915_drrs_status`, connector property `vrr_capable` (`drm_info`/`modetest`), DPCD `0x007` bit 6 (MSA ignore), debugfs `eDP-1/vrr_range` (the range line is a drm debug message and is not in the default kernel log). Measured: both modes 2880×1920@60 (preferred, active) and @120 are exposed, `vrr_capable`=1, `vrr_range` 30–120, DPCD `0x007`=`0xc0` (bit 6), `0x107`=`0x80` (set at 60 Hz too, same as Windows), CRTC `VRR_ENABLED`=0, DRRS enabled no. Switching and enabling VRR are unverified (DSP-2) |
| 4 | **DPST**: Windows uses DPST (aggressiveness 6) and turns LACE off. Linux has no DPST | `FeatureTestControl=0x9200`, VBT block 44, no implementation in i915 | On battery the backlight power is higher than on Windows (size not measured). In image fidelity Linux is, if anything, the more faithful | No kernel-side means. Allow for this difference in comparative measurements | Confirmed (presence/absence); the size of the power difference is inferred | Compare the battery discharge rate on Windows and Linux at the same brightness with the same screen content |
| 5 | **SAPD (MSHW0299) notifications**: Windows receives the screen off/on and standby entry/exit Notify of the LPS0 `_DSM`, forwards them to SAM and manages the fan through PEP. Linux has no driver that receives them | 1.4, `grep MSHW0299` | Fan activity during standby (#1445) is partly covered by the linux-surface fan standby patch. Other states SAM should receive (screen off and so on) do not reach it. A standby power difference is possible | A small driver that binds to `MSHW0299` and receives the `0xC2-0xC9` Notify. The SAM commands to send have to be determined by capturing the SSH traffic on Windows or by analysing `SurfaceAcpiPlatformExtensionDriver.sys` | Notification path confirmed, effect inferred | Confirm with `acpi.debug_layer` that the SAPD Notify occurs; measure the SAM fan state and standby power before and after s2idle |
| 6 | **S0ix residency**: Windows HW DRIPS 99.9%, 52 mW average (36.9 h). The Linux wiki reports 0.01% runtime PC10 with the screen off | 1.4, wiki Surface-Pro-9 "S0ix" | The standby battery life may differ considerably | Isolate the cause: `intel_pmc_core` substates/LTR (`ltr_show`, `ltr_ignore`), NVMe APST, `ithc`, IPU6 (D3 when the camera is unused), TCSS (D3cold when no TBT device is attached), CNVi. Look at it together with difference 5 | Windows values confirmed, Linux cause inferred | `cat /sys/kernel/debug/pmc_core/{slp_s0_residency_usec,substate_residencies,ltr_show}`, `/sys/devices/system/cpu/cpuidle/low_power_idle_{cpu,system}_residency_us`, `turbostat`, s0ix-selftest-tool. Measured (baseline, 60 seconds idle with the screen on, other load present): Pkg%pc2 26.7, pc6 13.5, pc8 0.12, pc10 0.00. `pmc_core/substate_requirements` is 0 bytes, so that means of diagnosis is unavailable. Reading `ltr_show` logs `Invalid LTR scale factor 7` (the ns conversion of some IP cannot be trusted). s2idle (AC, woken by RTC, PWR-1): 10/10 entries and resumes, `slp_s0` 27.86–28.35 s of 30.8–31.2 s per cycle, 0 unmet LPI constraints. Residency accumulates only in S0i2.0; S0i3.0 is 0. One `intel_ipu6.psys.40: Failed to get runtime PM` warning per cycle. Standby power over 1 hour on battery (PWR-4) and PC10 when idle with the screen off (PWR-5) are unverified |
| 7 | **ACPI GPE storm**: Windows gives the TBT root ports no I/O window. Linux reserves I/O for hotplug bridges. `PCHRESV`, which would cover PMBS (`0x1800`) and GPE0 (`0x1860`), has `_STA=0x08`, and the FADT reservation happens at `fs_initcall_sync`. Even with `_STA=0x08`, 7.3.4 reserves the resources of `PCHRESV` (measured, last column) | 1.8, `pci.c:107`, `osl.c:147`, #1082 | Thousands of GPE errors and memory/CPU consumption on lid close (AC), TBT hotplug and lid wake | `pci=hpiosize=0` (the same state as Windows). The root fix is a quirk reserving `0x1000-0x1FFF` or a change in the order of PCI resource assignment | The workaround is confirmed in the issue. The mechanism "the bridge I/O window overlaps the PM/GPE registers" is inferred | Boot without the parameter and check in `/proc/ioports` whether a TBT root port window includes `1800-18ff`; the counts in `/sys/firmware/acpi/interrupts/`. Measured (boot with `pci=hpiosize=0`): `acpi PNP0C02:01: Reserved [io  0x1800-0x18fe]` is logged before the PCI bridge windows are assigned, and `/proc/ioports` shows the PM1a and GPE0 blocks under `1800-18fe : PNP0C02:01`. The I/O windows of the TBT root ports `00:07.0` and `00:07.3` are disabled, `sci`, `gpe_all` and `gpe52` are 0, 0 GPE error lines. Boot without the parameter (measured): `1800-18fe` is still reserved by `PNP0C02:01` and the I/O windows of the TBT root ports are placed outside it, at `0x3000-0x3fff` and `0x4000-0x4fff`. After boot, 3 s2idle cycles and 4 lid close → open cycles there are 0 "No handler or method for GPE" lines (PWR-2). The parameter is not needed on this kernel. GPEs on USB-C attach and detach are unverified (DSP-5) |
| 8 | **Shutdown**: Windows shuts down normally. On Linux without linux-surface `0011-surface-shutdown.patch`, EFI ResetSystem(shutdown) returns | `0011-surface-shutdown.patch`, #1058 (open) | A distribution kernel without the patch does not power off | Use the linux-surface kernel. Test whether the I/O overlap of difference 7 is one of the causes (the PM1a control register `0x1804` is in the same range) | Need for the patch confirmed, the link to difference 7 is inferred | Shutdown test on an unpatched kernel with only `pci=hpiosize=0`, confirming power-off by the LED of a USB hub. Unverified (PWR-7). The `quirk_no_shutdown` symbol of `0011-surface-shutdown.patch` is present in the running kernel |
| 9 | **Hibernation**: Windows can hibernate and switches to it when the standby budget is exceeded. According to the wiki Linux needs `HibernateMode=reboot`. With Secure Boot on, kernel lockdown blocks hibernation | 1.4, wiki | No hibernation with Secure Boot enabled; with it disabled the reboot method is needed | Decide the Secure Boot policy, then set `HibernateMode=reboot` and `resume=`. Retest `platform`/`shutdown` once difference 8 is fixed | Lockdown behaviour confirmed (general kernel behaviour, not checked in this source tree); behaviour on the device is inferred | `cat /sys/kernel/security/lockdown`, `cat /sys/power/disk`, a hibernation round trip. Measured: Secure Boot off, lockdown LSM not included, `/sys/power/disk` = `[platform] shutdown reboot suspend test_resume`. The only swap is zram, so hibernation cannot be tested in the measured configuration (PWR-8) |
| 10 | **Lid wake**: `LID0` has no `_PRW`. Windows has no problem. Linux needs `surface_gpe` (linux-surface `0012-surface-gpe.patch`, not upstream), and there are reports of no wake after closing the lid (#1183, #1910) | DSDT `\_SB.LID0`, linux-surface `0012-surface-gpe.patch` | Wake fails after the lid was closed | The linux-surface kernel. On failure, retest together with difference 7 (`hpiosize=0`) | Need for the patch confirmed, cause of the failures inferred | Wake by closing and opening the lid, `/proc/acpi/wakeup`, `cat /sys/firmware/acpi/interrupts/gpe52`. Measured (baseline): `gpe52` 0 counts and enabled, lid `PNP0C0D:00` wakeup enabled. 4 lid close → open cycles (battery, no `pci=hpiosize`): 4/4 s2idle entries and resumes, `gpe52` 9 counts in total. The test while charging is unverified (PWR-2) |
| 11 | **Wi-Fi SAR**: there are no static SAR tables (ACPI or UEFI), and Windows handles it with Surface Radio Monitor. Linux iwlwifi operates without a SAR limit when there are no tables and uses PPAG (UEFI), TAS, SPLC and WRDD | 1.7 | In tablet posture the transmit power may be higher than on Windows (a regulatory difference). The performance difference is small | No kernel option. If needed, something like `iw dev <if> set txpower limit`. Finding the dynamic SAR path requires analysing `SurfaceSarManager.dll` | Absence of the tables confirmed, Windows behaviour inferred | `dmesg | grep -i -E "sar|ppag|tas|wrds"`, `iw dev`, `iw reg get`. Measured: the global regulatory domain is `00` and the phy is self-managed with a country set. There are no SAR/PPAG/TAS lines in the log (`IWLWIFI_DEBUG` is off, so they are not printed). 0 beacon losses over 854 seconds of a 5 GHz 80 MHz connection. After s2idle it reconnects by itself in 3.3 s (2/2). The throughput comparison is unverified (NET-1) |
| 12 | **Thermal and power limits**: Windows has no DTT; Surface SMF regulates CPU power and the fan. Linux only switches the SAM performance profile. There are reports of a fan minimum RPM that persists (#1726) | 1.5, `surface_aggregator_registry.c:373` | Differences in fan noise, surface temperature and sustained performance | Compare RAPL values and temperatures on Linux with Windows under the same load. For the fan minimum RPM, propose to linux-surface that it be reset with SAM FAN command `0x0b` (comment in #1726) | Configuration difference confirmed, effect inferred | `/sys/class/powercap/intel-rapl:0/constraint_*`, `sensors`, `cat /sys/firmware/acpi/platform_profile*`, fan RPM under the same load. Measured (SAM-5, PWR-9): see row G3 in [sam-platform.md](sam-platform.md). Only `low-power` differs in power, and under a 10-minute load the surface sensor rises to 51 °C |
| 13 | **Backlight minimum**: on Windows, 0% of the 0–100 scale still uses the VBT minimum (50/65535). Linux computes the VBT minimum as 0, so level 0 may be PWM 0 | `intel_backlight.c:1221-1229`, 1.3 | The screen may go completely dark at the lowest brightness | No problem if the desktop avoids 0. If needed, a kernel patch for the minimum handling | Calculation confirmed, actual screen behaviour inferred | Write 0 and 1 to `/sys/class/backlight/intel_backlight/{max_brightness,brightness}` and watch. Measured: only `intel_backlight` exists, type raw, max 96000. A write 17686 → 15686 is reflected in `actual_brightness`. The screen at levels 0 and 1 is unverified (DSP-3) |

Items judged to have no difference: the backlight method (DDI PWM at 200 Hz on both, not DPCD AUX), eDP Panel Replay (off on Windows;
on Linux inferred not to meet the conditions on ADL-P eDP), FEC (eDP FEC off on Windows; Linux does not use it either unless the panel
uses DSC), the USB4 connection manager (software CM expected on both), DMA protection (Linux also honours the DMAR opt-in flag) and the
ALS (a kernel path exists; checked in section 4).

Measured on Linux to be the same as on Windows: the EDID (identical per `cmp` to the EDID read on Windows), DPCD `0x000`–`0x00f`, `0x070`,
`0x0b0`, `0x700` and `0x720`–`0x725` (identical to the values read on Windows on AC with `tools/windows/dpcd-read.ps1`; the ones that
differ are `0x170` and `0x2009`), the calibration UEFI variables `Type_1` and `Type_2` (the content without the 4 attribute bytes equals
the Windows dump, and the ICC made with `tools/sdc-blob.py` is byte-identical to the one made on Windows), and the DMC `adlp_dmc.bin`
v2.20, GuC 70.49.4 and HuC loads.

## 4. Checks on Linux and their results

1. **VBT comparison**: compare `head -c 8701 /sys/kernel/debug/dri/*/i915_vbt | sha256sum` with the two values in 1.3 and look at
   `panel_type`, PSR, DRRS, VRR and backlight values with `intel_vbt_decode`. The debugfs file is 8704 B, with three 0 bytes appended, so
   the hash of the whole file matches neither.
   Measured: the runtime VBT is vbt0 (its first 8701 B equal vbt0 from the 24.103.143 capsule and differ from vbt1 in 16 bytes). panel
   type 2, PSR yes, VRR yes and backlight PWM 200 Hz, as in 1.3.
2. **EDID**: whether `edid-decode /sys/class/drm/card*-eDP-1/edid` equals the EDID read on Windows, and whether 120 Hz is in the mode
   list.
   Measured: byte-identical. The mode list has 60 Hz and 120 Hz.
3. **PSR**: with the default, the mode (PSR1/PSR2) in `i915_edp_psr_status` and the sink support. Compare flicker and package C-states
   (Pkg%pc8/pc10 of `turbostat`) with `enable_psr=1`, `=2` and `=0`.
   Measured (`enable_psr=1` only): see difference 1. Source and sink both enter and leave PSR1, but DC6 was not reached once in
   60 seconds, with Pkg%pc8 0.01 and pc10 0.00. That the updating terminal on screen is the reason is inferred; it needs to be measured
   again with a static screen. The debugfs of this kernel has no cumulative PSR entry/exit counters. For the boot without the parameter
   see difference 1 (Pkg%pc8 24–28 over 60 s idle).
4. **Refresh rate**: whether the screen blinks on a 60→120→60 switch, the `vrr_capable` property, and the power at 120 Hz with VRR
   versus fixed 60 Hz (`/sys/class/power_supply/BAT*/power_now`).
   Measured (properties only): see difference 3. The switch and the power comparison are unverified.
5. **Backlight**: the `intel_backlight` maximum, the screen at levels 0/1, flicker while changing brightness.
   Measured: see difference 13.
6. **ALS**: whether an `in_illuminance_*` appears among `iio:device*`, and how the value compares with the Windows reading (47 lux
   indoors) under the same lighting.
   Measured: `iio:device3` (`als`), scale 0.01. Only 2 of 35 sysfs polls return a value (80.9, 82.1 lux); the rest return 0 after
   5–7 seconds. That 0 means no response, not "dark". The buffer path gives 0 samples in 30 seconds of constant lighting. That the
   sensor reports only on a change of more than 4 % is inferred. The covering test is unverified (DSP-4).
7. **s2idle**: with and without `pci=hpiosize=0`, close the lid (AC/battery) and record the GPE counts, `/proc/ioports`,
   `slp_s0_residency_usec` and the battery drain over 1 hour of standby (mWh). The reference is Windows at 52–102 mW and HW DRIPS
   99.8–99.9%.
   Measured: for `/proc/ioports` and GPEs on a boot without the parameter see difference 7, for the lid difference 10, for s2idle
   residency difference 6. The comparison with and without the parameter, the lid on AC and the 1-hour standby power are unverified.
8. **Runtime PC10 with the screen off**: s0ix-selftest-tool `-r off`, and `pmc_core` `ltr_show` to find the blocking IP.
   Baseline only (difference 6). `s0ix-selftest-tool` was not run.
9. **Shutdown and hibernation**: power-off on shutdown with the linux-surface kernel, shutdown of an unpatched kernel with only
   `hpiosize=0`, lockdown state and hibernation.
10. **Wi-Fi**: the result of applying SAR/PPAG/TAS in the iwlwifi log, and whether the country code (`WRDD`) is taken into account.
    Measured: see difference 11.
11. **Thermal**: compare RAPL PL1/PL2, package temperature and fan RPM under the same load with how Windows feels. Whether a fan
    minimum RPM persists right after rebooting from Windows into Linux.
    Measured: see difference 12. The comparison by feel and the persisting fan minimum RPM are unverified.
12. **TBT/USB4**: the kind of connection manager in `dmesg | grep thunderbolt`, and whether GPE errors occur on hotplug.
