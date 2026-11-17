# Display and panel control: what Windows does and the same control on Linux

This document lists how Windows controls the built-in panel (LGD `0x0719`, 2880×1920, eDP) by component, channel and value, and adds
how to produce the same control on Linux. The observed values and the list of differences are in
[`docs/research/gaps/display-power.md`](../gaps/display-power.md); this document covers "who sends what through which channel".
Confidence is given as **confirmed** (confirmed from data or the binaries) or **inferred** (circumstantial evidence).

All collection was read-only. No setting was changed. All values were collected on **AC** power.

## 1. Components and channels

| Component | Location | What it does | Channel |
|---|---|---|---|
| Intel KMD `igdkmdn64.sys` 32.0.101.7085 | `iigd_dch.inf` + the Surface extension `iigd_ext_zc.inf` | Modes and link, PSR, VRR (Adaptive Sync), DPST, PWM backlight, pipe colour processing | Registers directly, DPCD (AUX), VBT |
| `SurfaceOemPanel.dll` 6.137.139.0 (internal name `SurfaceBrightnessFilterDriver`) | UMDF lower filter of the monitor device (`LowerFilters=WUDFRd`), `surfaceoempanel.inf` | Computes and applies colour (degamma, CSC, regamma) and brightness (nits↔PWM, LED current) from the per-unit factory calibration values | Reads UEFI variables, Intel IGCL `ctlPixelTransformationSetConfig`, reads the active ICC profile (`mscms`) |
| `ColorCalibrationComputation.dll` | Same package | Matrix and gamma computation library (newmat) | DLL calls |
| `surfaceoempanelcustomization.inf` 5.19.137.0 | Extension INF (`LGD0719`, `LGD06B2`) | Registers the ICC profiles `SurfacesRGB.icm` and `SurfaceVivid.icm`, `MicrosoftApprovedAcmSupport=1`, Dolby Vision extension component | Registry |
| `SurfaceSmfDisplayClient.sys` 9.32.137.0 | SMF class | Reports the current brightness and monitor arrival to SMF/SAM (inferred) | Power setting callbacks, SAM SSH |
| Surface TCON firmware 161.33.3.35 | UEFI capsule `surfacetcon.inf` | TCON firmware update (includes the EDID strings) | UEFI capsule (ESRT `{4216782b-…}`) |
| OS (dxgkrnl, DWM, power manager) | — | Refresh rate selection and DRR, brightness level, ALS automatic brightness, ACM | CCD database, power settings, brightness interface |

## 2. Control table

| Control | Windows path | Value and trigger | Evidence | Same control on Linux | Open |
|---|---|---|---|---|---|
| Refresh rate 60↔120 Hz | OS CCD configuration → Intel KMD modeset. The two modes have different timings (60: htotal 3000/vtotal 1975, 120: 2980/2000) | Currently 60 Hz (`VSyncFreq` 35550000/592500), `VirtualRefreshRate` 60, `BoostRefreshRateMultiplier` 1 = **DRR off** | `GraphicsDrivers\Configuration\LGD0719…\00\00`, EDID | DRM mode selection (the compositor). Switching 60↔120 is a full modeset (DRRS is not possible, display-power.md difference 3) | The values that change when "Dynamic (DRR)" is chosen in Settings (`VirtualRefreshRate`/`Boost`) are unverified, because no setting was toggled |
| Adaptive sync (VRR, the basis of DRR) | The Intel KMD **enables MSA ignore** on eDP | DPCD `0x007`=`0xc0` (bit6 `MSA_TIMING_PAR_IGNORED`=1), **`0x107`=`0x80` (bit7 `MSA_TIMING_PAR_IGNORE_EN` set, even in the 60 Hz state)**. `AdaptiveVsyncEnable=1`, `AdaptiveVsyncEnableUserSetting=7`, `MonitorFlickerProfile_…_60` = 30000/60000/16666/16666. No Adaptive-Sync SDP (`0x2214`=0), no Adaptive Sync block in DisplayID | DPCD read on AC (`tools/windows/dpcd-read.ps1`), class key, the KMD setting name `AlwaysInVrrMode` | i915 `intel_vrr_is_capable()`: VBT `vrr`=1 + DPCD `0x007` bit6 + EDID range 30–120 → all three are met; connector `vrr_capable`=1 (measured, display-power.md difference 3). The 120 Hz mode + compositor VRR (KWin "Adaptive sync: Always", mutter `variable-refresh-rate`) has the same effect as Windows DRR (vblank stretched when idle) | Why Windows enables MSA ignore even at 60 Hz (whether it is always-on VRR timing = `AlwaysInVrrMode`). The unit of the flicker profile |
| Media refresh rate matching | Intel KMD | `MediaRefreshRateSupport=0x803F` (24/25/30/48/50/60 Hz + fractional) | INF comment | VRR + display sync in the player (mpv and others) | — |
| PSR1/PSR2 | Intel KMD, Surface extension INF | `PSR2Disable=1`, `PsrDisableInAc=1`, `Psr2DrrsEnable=1`. Sink: `0x070`=`0x03` (PSR2 + Y coordinate), `0x071`=`0x1b` (no retraining needed on exit, setup time 55 µs, Y coordinate required), ALPM capable (`0x02e`=`0x03`). **On AC `0x170`=0, `0x2008`=0 → PSR off confirmed** | `[PwrCons_ADLP_AddSwSettings]` in `iigd_ext_zc.inf`, DPCD | PSR1 only: this repository's patch 0014 (PSR2 quirk), or `i915.enable_psr=1` on a kernel without it. For the AC/DC split, write AC `0x01` (off) and battery `0x03` (force PSR1) to debugfs `i915_edp_psr_debug` (`intel_display_types.h:1762-1764`, Linux v7.3.4); the device helper does this as its PSR policy (`sp9-ctl psr off psr1`; the AC side works, the battery side is unverified) | The `0x170` value on battery (whether PSR1 actually turns on): run `dpcd-read.ps1` again on battery |
| Panel Replay | Intel KMD | `DisplayPcFeatureControl=0x11C0000` (DP Panel Replay off). Sink `0x0b0`=0 (**not supported**) | INF, DPCD | Not applicable (sink does not support it) | — |
| Backlight path | Intel KMD → **PWM pin** (VBT block 43 DDI PWM, 200 Hz) | DPCD `0x721`=`0x00` (PWM pin mode), `0x720`=`0x04` (FRC on, TCON DBC off). The TCON also supports AUX brightness (`0x702`=`0xff`, `0x724` PWM bits 10); there is no Intel HDR AUX interface (`0x340` all 0) | DPCD, VBT | `intel_backlight` (as the VBT says). `i915.enable_dpcd_backlight=2` can force the VESA AUX path, but that differs from Windows | — |
| Brightness level → luminance | OS brightness (0–100, WMI 101 steps, ACPI `_BCL`→ASLE) → `SurfaceOemPanel` converts nits↔PWM (it translates a luminance in millinits into a brightness value in millipercent) | Calibration table of 20 points (PWM 0.2%→relative luminance 0.0021 … 100%→1.0, nearly linear), minimum PWM 0.2% (`BacklightPwmDutyCycleMinimumInMilliPercent=200`), minimum 1.8 nits, curve gamma 1.8. Measured on the unit examined (one unit's example): PWM 0.2% = 1.1 nits, 100% = 553.6 nits | UEFI `Type_2` table 2, `Type_1` table 4, the DLL's strings | sysfs `intel_backlight` is linear in duty cycle. To match Windows, a 0.2% floor + a gamma curve in user space (a small daemon or the desktop's brightness curve) | The OS slider % → nits curve (whether the nits-based brightness interface is used, KMD `DisableNitsBrightness`) |
| Maximum luminance and LED current | `SurfaceOemPanel` adjusts the LED current within 12–20 mA to meet the target of 450 nits (`IsTargetLuminanceForBacklightCurrentAdjustmentEnabled=1`) | At calibration 553.6 nits at 19.6 mA (one unit's example) → about 16 mA for 450 nits (calculated, inferred). ICC `MHC2` maximum 450 nits | `Type_2` table 1; the driver has an event for failing to read the backlight current | The path is not known. Linux keeps the default current of the TCON/LED driver, so the maximum brightness may be higher than on Windows and may cost more power | **The channel through which the current is written** (a DPCD vendor register? SAM? The `0x350` area changes on every read and appears to be a status register). A comparison measurement of the maximum luminance with the same picture |
| DPST (content-based backlight) | Intel KMD | `PowerDpstAggressivenessLevel=6`, `DpstEpsmWeight=100`, `FeatureTestControl=0x9200` (Surface adds `0x8000` = LACE off). TCON DBC is off (`0x721` bit5=0) | Class key, INF | Not in i915. No substitute (display-power.md difference 4) | Where the Windows setting "change brightness based on content" is stored and how it is tied to the DPST level |
| ALS automatic brightness | OS power setting `ADAPTBRIGHT` (**on** for both AC and DC); the sensor is the SAM HID ALS | The curve is the OS default (no ACPI `_ALR`). The VBT ALS table (0/10/80/300/1000 lux → 70/73/85/100/150%) belongs to the Intel side | `powercfg /qh SUB_VIDEO`, ACPI search | iio-sensor-proxy + GNOME automatic brightness (or a daemon such as wluma) | The lux → brightness curve the OS uses |
| Colour calibration (factory, per unit) | `SurfaceOemPanel` reads the UEFI variables `Type_1` (measurement, 410 B), `Type_2` (configuration, 4865 B) and `MsPlatformDisplayInfoVariable` (15 B, includes the TCON version), computes degamma (4096), a 3×3 CSC and regamma (4096) for the target colour space of the active ICC profile (the `mscs` tag), and **writes them to the pipe with IGCL `ctlPixelTransformationSetConfig`**. It applies them again on display on (`GUID_CONSOLE_DISPLAY_STATE`) and on a TCON recovery event | Device name "Surface Calibrated Panel" (calibration values present). Variable attributes `0x7` (NV+BS+RT) → visible in Linux efivarfs. The measured values are in section 4 | UEFI variable listing (`tools/windows/uefi-vars.ps1`), the DLL's strings and imports (`GetFirmwareEnvironmentVariableW`) | efivarfs `Type_1-0c3b397f-4162-4ed2-9aaa-5baf400e3aaf` → with `tools/sdc-blob.py` either (a) build an ICC profile of the native gamut and assign it in colord/KWin, or (b) for the sRGB mode write directly to DRM `DEGAMMA_LUT` (ADL-P 129 points, same for all channels) → `CTM` → `GAMMA_LUT` (1024) (when the compositor does not manage them) | The LUT values Windows actually writes (readable with an IGCL Get; not attempted) |
| Colour mode sRGB/Vivid | "Colour profile" in Settings = an ICC association. Class key `ICMProfile`={`SurfacesRGB.icm`, `SurfaceVivid.icm`} (INF comment: the last entry is the default). No per-user association key | sRGB: target primaries sRGB, D65. Vivid: primaries 0.650/0.318, 0.297/0.655, 0.155/0.061 (nearly the same as the measured native gamut of the panel) + D65. Both profiles have TRC = sRGB, `MHC2` identity matrix, 0.5–450 nits | ICC interpretation (section 3) | Use the same ICC targets, but because the panel is in its native gamut, "Vivid" ≈ the native gamut with only the white point moved to D65, "sRGB" = the CTM above | Which profile is currently active (unverified: the WCS query failed because `EnumDisplayDevices` returned nothing in the session used for collection) |
| Automatic colour management (ACM) and HDR | OS (Advanced Color) | `MicrosoftApprovedAcmSupport=1`, `AutoColorManagementSupported=1`, power setting "Advanced Color quality bias" AC 1/DC 0. Dolby Vision extension (`PQConfig.dv` = the same file as `SurfaceVivid.icm`) | Registry, hashes | Compositor colour management (KWin 6, mutter). No Dolby Vision counterpart | Whether ACM is on |
| TCON features | Fixed behaviour of the TCON firmware + KMD DPCD | Sink OUI `00-22-B9` (INF comment: Analogix), device ID `Spruce`, firmware rev 1.3, eDP 1.4b (`0x700`=4), FRC on, no overdrive control (`0x703`=0), no self-calibration | DPCD, `surfacetcon.inf` | Same TCON, so no separate control is needed. Firmware update is an ESRT capsule (fwupd) | The signal that the TCON-ready wait thread of `SurfaceOemPanel` waits for |
| SMF brightness reporting | `SurfaceSmfDisplayClient.sys` subscribes to the power setting `GUID_VIDEO_CURRENT_MONITOR_BRIGHTNESS` and to monitor interface arrival | The strings are WPP, so the command is unverified | The two GUIDs are present in the driver | No such driver. If it serves the thermal model, the effect is small (inferred) | The command sent to SAM |
| Link | Intel KMD | HBR2 ×4 (`0x100`=`0x14`, `0x101`=`0x84`), downspread off, `eDPFecDisable=1`, `NoFastLinkTrainingForeDP=1` | DPCD, INF | Same as the i915 default | — |

## 3. ICC profiles

The profile files are not in this repository. They belong to the Windows driver packages: two are installed as colour profiles and
the others stay in the driver store. (Where Windows keeps installed colour profiles, normally
`%SystemRoot%\System32\spool\drivers\color`, is general Windows knowledge and was not recorded for this device.) The two
installed files carry no unit-specific tag (no serial or the like): `dmnd`=`MSFT`, `dmdd`=`Surface`, and `DVB1` (Dolby Vision
metadata) holds only the model name `Surface_Pro_9`.

| File | sha256 | desc | Primaries xy (R, G, B) | White | TRC | Other |
|---|---|---|---|---|---|---|
| `SurfacesRGB.icm` (installed) | `d9219fbb…9af5` | sRGB | 0.640/0.330, 0.300/0.600, 0.150/0.060 | D65 (`chad` inverted) | 1024 points, differs from the sRGB EOTF by 2e-5 | `lumi` 450 nits, `MHC2` identity matrix, LUT of 2 points, 0.5–450 nits, `ACFI`=0, `mscs`=sRGB→XYZ matrix |
| `SurfaceVivid.icm` (installed, = `PQConfig.dv`) | `7e985dba…ee8c` | Vivid | 0.650/0.318, 0.297/0.655, 0.155/0.061 | D65 | Same | Same, `mscs`=matrix of the Vivid primaries |
| `SurfacesRGB-AcOff.icm` (driver store only) | `af7f5035…cb58` | sRGB | Same | | | `ACFI`=2 |
| `SurfaceVivid-AcOff.icm` (driver store only) | `aa2625ee…c2a8` | Vivid | Same | | | `ACFI`=2 |
| Old `SurfacesRGB.icm`/`SurfaceEnhanced.icm` (`surfaceoempanel` package, not used for `LGD0719`) | `f0353e59…7861` / `73afcc49…ffdf` | | | | | 400 nits |

An ICC profile describes only the **target colour space**. Bringing the panel to that target is done by `SurfaceOemPanel` with the
pipe LUTs and CSC. Assigning these ICC profiles as they are on Linux therefore assumes, without the calibration, that "the panel
already is sRGB/Vivid", which is wrong. Linux has to use the native-gamut profile of section 4.

## 4. Per-unit calibration data (SDC)

The variables themselves are not in this repository; read them from your own device (efivarfs on Linux, section 5). Their structure
is described in the header of `tools/sdc-blob.py`. The values below are the interpretation for one unit (rounded) and are given as
one unit's example. They are measurements, not identifiers, but they differ from unit to unit.

- Measurement (100% PWM, 19.6 mA): white 553.6 nits, xy 0.3003/0.3057 (about 7600 K, bluish), black 0.369 nits (contrast about 1500:1).
  Primaries (black subtracted) R 0.6534/0.3135, G 0.2981/0.6544, B 0.1519/0.0585. Additivity 0.997.
- PWM 0.2%: 1.11 nits. Grey levels 30/180/230 are measured as well.
- Configuration: target luminance 450 nits (luminance calibration), 300 nits (colour calibration), allowed luminance reduction at most 18%,
  CSC coefficient range −0.1–1.0 (diagonal ≥0.6), LED current 12–20 mA, gamma1 = sRGB piecewise (0.04045, 12.92, 0.055, 2.4),
  gamma2 = 2.2, LUT sizes degamma/regamma 4096, CSC 3×3.
  `TargetPrimaries` = 0.664/0.326, 0.292/0.640, 0.151/0.0585, white D65 (purpose unverified; inferred to be the reference primaries of the process).
- TRC model: effective gamma at the 50% point 2.41 at high brightness and 2.08 at low brightness (gamma drops at low luminance → there is a per-brightness TRC correction).
- Linear sRGB (D65) → panel RGB matrix (normalized to a white maximum of 1.0, about 540 nits at 100% PWM):
  `[[0.9268, 0.0619, -0.0091], [0.0261, 0.9688, 0.0051], [-0.0021, 0.0542, 0.7647]]`.
  To put it into DRM `CTM` (S31.32 sign-magnitude), `DEGAMMA_LUT`=sRGB EOTF and `GAMMA_LUT`=the inverse of the panel TRC are needed with it.

## 5. What to do on the Linux side

1. Read the DPCD over the same ranges and compare with the Windows values: `/dev/drm_dp_aux*` (kernel `CONFIG_DRM_DISPLAY_DP_AUX_CHARDEV`) or
   `/sys/kernel/debug/dri/*/eDP-1/i915_dpcd`. In particular `0x107` bit7 (with VRR enabled), `0x170` (PSR), `0x721`. Measured: the ranges
   compared are equal except `0x170` and `0x2009` (display-power.md, the paragraph after the differences table).
2. `vrr_capable` is 1 (measured). Compare the power of 120 Hz + VRR always on with fixed 60 Hz (display-power.md section 4, item 4): not done.
3. PSR: with this repository's kernel PSR1 is selected without a parameter (patch 0014); the AC/DC split is the helper's PSR policy
   (`i915_edp_psr_debug` 0x01/0x03; works on AC, battery unverified).
4. Colour: `python3 tools/sdc-blob.py /sys/firmware/efi/efivars/Type_1-0c3b397f-… /sys/firmware/efi/efivars/Type_2-0c3b397f-… --icc sp9.icc`
   → assign it in colord (`colormgr device-add-profile`) or in the KWin display settings. The device helper runs the same script
   (`sp9-ctl icc`); the file is produced, and the colour with it assigned is unverified.
5. Brightness: a mapping that uses the 0.2% floor and the gamma 1.8 curve (a small daemon if the desktop does not support it). Compare the maximum luminance with Windows using a luminance meter.
6. Automatic brightness: check that iio-sensor-proxy sees the SAM ALS, then use the desktop's automatic brightness.

## 6. Open items

- The channel that changes the LED current and whether it is actually applied (the key to the difference in maximum luminance and power on Linux).
- The CCD values with DRR on, and the condition under which Intel enables MSA ignore even at 60 Hz.
- DPCD on battery (whether PSR1 is actually active, ALPM).
- The active colour profile (sRGB/Vivid), the ACM state, where the "change brightness based on content" setting is stored, the OS ALS curve.
- The command `SurfaceSmfDisplayClient` sends to SAM.
- The meaning of the `FeatureTestControl` bits other than `0x8000` (not in the INF comments).

## 7. Collection methods

| Target | Method |
|---|---|
| Driver INFs and binaries | `C:\Windows\System32\DriverStore\FileRepository\{surfaceoempanel,surfaceoempanelcustomization,surfacesmfdisplayclient,iigd_ext_zc,iigd_dch,surfacetcon}.inf_amd64_*` (INF settings and comments; imports and strings of the binaries) |
| Registry, UEFI variables, WMI, power, ETW | `tools/windows/collect-display.ps1` (administrator). The output holds instance paths and per-unit calibration data and is not in this repository |
| DPCD | `tools/windows/dpcd-read.ps1` (administrator, Intel IGCL `ctlAUXAccess`, read-only). DPCD can be read on Windows too |
| SDC calibration | `tools/sdc-blob.py` |
| ETW providers | `Microsoft-Surface-SurfaceOemPanel` `{3D53356D-424B-496A-A094-2154D8D2B304}` (Operational channel, no events = no colour pipeline errors), `Intel-Gfx-Display-External` `{31AB337F-…}`, `Microsoft-Windows-DisplayColorCalibration`, `Microsoft-Windows-DxgKrnl` |

The VBT and the EDID are not in this repository either. On Linux the VBT is `/sys/kernel/debug/dri/0/i915_vbt` (decode with
`intel_vbt_decode`), or extract it from a firmware image with `tools/vbt-extract.py`; the EDID is `/sys/class/drm/card*-eDP-1/edid`.
