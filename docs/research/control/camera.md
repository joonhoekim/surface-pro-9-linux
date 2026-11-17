# Camera control: what Windows does and the same control on Linux

This document describes which Windows component controls the three cameras of this device (front OV5693, rear OV13858 + VCM DW9800,
IR VD55G0), through which path and with which values, and how the same control is done, or has to be done, on Linux
(v7.3.4 + linux-surface 7.3 + `kernel/patches`).
Topology, SSDB, the INT3472 GPIO table and the mode table differences are in [gaps/camera](../gaps/camera.md) and are not repeated here.

Notation: `L/path:N` = path and line in the Linux v7.3.4 source tree, `LS:N` = line in linux-surface
`patches/7.3/0013-cameras.patch`. Facts about Windows drivers name the driver they come from, without positions inside the file.
The MF control values can be read again with [`camera-controls.ps1`](../../../tools/windows/camera-controls.ps1).
Evidence levels: **confirmed** (confirmed by driver analysis, the registry or an MF query) and **inferred** (judged from names and
structure, behaviour not observed).
Nothing here was measured as "works". Everything below is what Windows *does*, not a statement that it *works* on Linux; the Linux
results are in [gaps/camera](../gaps/camera.md) and [hardware-status.md](../../hardware-status.md).

## Summary

- **The IR illuminator (Windows Hello) is switched by a GPIO output (strobe) of the VD55G0 sensor, not by an INT3472 GPIO.** ICL2 has no
  GPIO of type Strobe/Flash/Torch. `StartIRFlash` in `iacamera64.sys` passes a pattern value to sensor driver control `0x27`, and
  `vd55g0.sys` writes it to **register 0x0467 (context 0 GPIO0 control)**: pattern 0 → 0x05, 0xAA → 0x02, 0xFF → 0x12. The default
  pattern is the default of the registry setting `GSStrobePattern`, **0xAA** (every other frame). The mode table sets GPIO0–3
  (0x0467–0x046A) all to 2. The structure is the same as `GPIO_MODE_STROBE` (=2, ctx offset 0x1d) of the vd55g1 driver; on the VD55G0
  the ctx offset is 0x1b (confirmed: values and path; inferred: meaning of the bits).
- On the MF side the IR source exposes `IRTORCHMODE` (Off/On/AlternatingFrameIllumination, intensity fixed at 32) and `FACEAUTH_MODE`
  (confirmed).
- **The privacy LED is a software control separate from sensor power.** `SensorOn` in `iactrllogic64.sys` does not touch PrivateLED
  (type 0x0D); `IndicatorOn/Off` switch it separately. When switching off, **if the LED has been on for less than 500 ms, the switch-off
  is deferred by a timer for the remaining time** (confirmed). Whether it is tied to sensor power in hardware, and whether the three LED
  GPIOs are physically the same LED, is unverified.
- **The Windows GPIO type names are in the binary**: 0x00 Reset, 0x01 Enable, 0x02 Strobe, 0x03 Torch, 0x04 Flash, 0x05 LedRear,
  0x06 LedFront, 0x07 Power0, **0x08 Power1**, 0x09 Standby, 0x0A WriteProtect, **0x0B PowerEn**, 0x0C Mclk, **0x0D PrivateLED**,
  0x0E AF, 0x0F IO, **0x10 Avdd**, 0x11 Core, 0x12 Handshake, 0x13 HDMI_INT_IO, 0x14 HDMIDetect (name table in `iactrllogic64.sys`).
  linux-surface's `pwr1` (0x08) matches the Windows name `Power1`. Upstream calling 0x10 `dovdd` differs from the Windows name (`Avdd`).
- **The Windows power sequence is rails → reset assert → clock → reset release** (2–5 ms per step). Linux ov5693/ov13858 (LS) enable
  the clock before the rails.
- **Among the DW9800 initialisation values, `VCM_FREQ` (0x07) differs**: Windows 0x60, Linux dw9719 `dw9800k` default 0x10. Ringing
  (0x02=0x02) and SAC (0x06=0x40) are the same. Windows settles the lens by moving it slowly, in steps of 32 at 10 ms intervals.
- **Windows hard-codes the front vertical flip by the `_SUB` string.** If `_SUB` is `MSHW0340` (SP9 front) or `MSHW0260`, `ov5693.sys`
  overwrites 0x3820=0x42, 0x3821=0x18 after the mode table (confirmed). If the SSDB `degree` is 180 (or 1) it additionally toggles the
  H/V bits; on the SP9 it is 0, so this does not apply. The rear camera does not use sensor flip.
- **The tuning files Windows actually uses are the `*_MSHW034x_ADL.*` files of the Surface extension packages** (inferred, based on
  `ExtensionPath`). They differ from the files of the Intel base packages in the gaps document (section 5).

## 1. Components and channels

| Component | Role | Channel downwards |
|---|---|---|
| `iacamera64.sys` (service `camera`, `DISPLAY\INT3480`) | AVStream miniport. Handles MF extended controls, calls 3A, decides the IR illumination pattern, loads the IPU firmware | Sensor driver query interface (function pointers), IPU MMIO |
| `IntelDeviceMFT64.dll`, platform DMFT | User-space MFT. 3A (`libia_*`) and AIQB tuning | `iacamera64` IOCTL |
| `ov5693.sys`, `ov13858.sys`, `vd55g0.sys` | Sensor and VCM I2C register writes, mode, exposure, gain, flip, IR strobe | SPB (I2C), control logic query interface |
| `iactrllogic64.sys` (`ACPI\INT3472`) | GPIO power sequence, PCH clock `_DSM`, privacy LED, Flash/Torch | GPIO, `_DSM 82c0d13a` (clock), `_DSM 79234640` (GPIO table) |

## 2. By control

| Control | Windows path | Values / trigger | Evidence | Same control on Linux | Open |
|---|---|---|---|---|---|
| **Sensor power on** | `iactrllogic64` `DiscreteControl::SensorOn`. Entered through function 1 of the control logic query interface. The caller is taken to be the sensor driver (inferred: the sensor driver registers for arrival notification of the `ControlLogic` interface, per a string in `vd55g0.sys`) | Order and delay (ms) by type: Enable (0x01) inactive 2 → Power0 (0x07) 5 → Power1 (0x08) 5 → PowerEn (0x0B) 2 → Avdd (0x10) 2 → Handshake → Reset (0x00) assert 2 → Mclk GPIO 2 or `ControlPCHClock` (_DSM) → [100 if Handshake] → Reset release 2 → Enable active 2 → [300 if Handshake]. Levels come from `_DSM` bit24–31 (all 1 on this device) | confirmed. Delays are in ms; a delay of 5 ms or less is done as repeated 50 µs stalls. There is one GPIO slot per type | int3472 provides 0x0B/0x10/0x08 (LS) as GPIO regulators (enable time 2 ms, L/include/linux/platform_data/x86/int3472.h:52) and 0x00 as the `reset` GPIO. The order is decided by the sensor driver (section 3) | Handshake, Enable and Power0 do not exist on this device |
| **Sensor power off** | `DiscreteControl::SensorOff` | Reset assert → Avdd off → Power1 off 5 → Power0 off 5 → Enable inactive → Mclk/PCH clock off → PowerEn off → Handshake off | confirmed | The sensor driver's `power_off` does reset → regulators → clock | The delay after Avdd off is not determined (it is passed in a register) |
| **MCLK** | `ControlPCHClock` → `_DSM 82c0d13a` `{IMGCLK n, on, freq}` | 19.2 MHz → 1, 24 MHz → 0 (gaps difference 9) | confirmed (gaps difference 9) | int3472 `_DSM` clock + **patch 0015** | - |
| **Privacy LED on** | `DiscreteControl::IndicatorOn`, called through function 3 of the control logic query interface | Sets the PrivateLED (0x0D) GPIO to the `_DSM` on value (1, active-high). Front ICL1 pin 0xA0, rear ICL0 0x49, IR ICL2 0x46. 2 reference slots, depending on a kind argument of 4/5 | confirmed | int3472 registers it as the `privacy` LED (`<sensor>::privacy_led`) (L/drivers/platform/x86/intel/int3472/led.c); it is switched on and off with v4l2 subdevice stream start and stop | Who calls interface function 3 (the sensor driver or `iacamera64`) was not found. Whether the trigger is stream start or sensor power is also unverified |
| **Privacy LED off** | `IndicatorOff`, interface function 4 | **If less than 500 ms (0x1F4) have passed since it was switched on, it is switched off by a timer after the remaining time**; otherwise immediately, to `1 - on value` | confirmed | Linux switches it off immediately. If a minimum on time is needed, a quirk that adds a delay to the int3472 LED | Whether the three GPIOs are the same physical LED (the SP9 may have no LED on the rear) |
| Privacy LED default state | `iacamera64` setting `DefaultPrivacyIndicatorStatus` (id 0x57, 0..1, default 0) | No registry value (default used) | confirmed: settings table of `iacamera64.sys` | Not applicable | Meaning of 0/1 |
| **IR illuminator on/off and pattern** | `iacamera64` `CBayerSensorCameraControl::StartIRFlash` → sensor driver interface `SetControl(0x27, pattern)` → `vd55g0.sys` → I2C 0x60 register **0x0467** | Pattern selection: 0xAA/0xFF if `TriggerRollingShutterIRByTorch` (id 0x48) is on; otherwise, on the global shutter path, `GSStrobePattern` (id 0x4F, default 0xAA). Values written: 0x00 → 0x05, 0xAA → 0x02, 0xFF → 0x12 (1 byte). The mode table has 0x0467–0x046A = 2 | confirmed | A VD55G0 driver is needed (gaps difference 8). Following the `st,leds` + `V4L2_CID_FLASH_LED_MODE` structure of vd55g1, writing 2 to the ctx0 GPIO0 control (0x0467) does it. As this is an ACPI device, `st,leds = <0>` has to come from an ipu-bridge swnode or a driver default | Meaning of the bits of the values 0x05 and 0x12 (no datasheet). Whether the sensor alternates by itself for 0xAA (every other frame), and whether context 1 (block 0x047C, GPIO 0x0497) is used. Whether the illuminator is physically on GPIO0 |
| IR illumination intensity | Registry `Services\vd55g0\IRFlashLedIntensity = 100` (INF AddReg), `iacamera64` setting `IRFlashLedIntensity` (id 0x49, range 30..100, default 30). MF `IRTORCHMODE` intensity min=max=32 | The actual register path was not found | confirmed: registry, MF query | Not applicable (on/off only) | Whether the intensity reaches the hardware (the fixed intensity of 32 suggests it does not, inferred) |
| **IR torch mode (MF)** | `KSPROPERTY_CAMERACONTROL_EXTENDED_IRTORCHMODE` (id 38). `CRawSnrCaptureFilter::ExtendedControl_SetIRTorchMode` | capability 0x7 (Off/AlwaysOn/Alternating), current flags 0x2. WinRT `InfraredTorchControl` has Off/On/AlternatingFrameIllumination, currently On | confirmed: `camera-controls.ps1` | User space (Howdy and the like) switches it on with the V4L2 flash control. Every-other-frame illumination has to be supported by the sensor driver | flags 0x2 and WinRT `CurrentMode=On` disagree |
| Windows Hello face authentication mode | `FACEAUTH_MODE` (id 35), `CRawIrSnrCaptureFilter::ExtendedControl_SetFaceAuthMode`, profile `KSCAMERAPROFILE_FaceAuth_Mode` 640x480@60 NV12 (INF) | capability 0x3, current flags 0x1 | confirmed | Not applicable | - |
| **AF VCM initialisation** | `ov13858.sys` VCM table id 0x11 function 1 → I2C (SSDB I2C table `0x02000C01`, address 0x0C) | Write order (`{reg, val}`): 0x02=0x01, 0x02=0x00, **1 ms**, 0x02=0x02, 0x06=0x40, **0x07=0x60**. Then a read of 0x00; the start position goes from reference−50 up to reference+50 in steps of +32 at 10 ms intervals | confirmed. In the write table a reg of 0xFF means a delay | dw9719 `dw9800k` (bound by patch 0011): 0x02=0x00, 200 µs, 0x02=0x00, 0x02=0x02, 0x06=SAC<<6 (=0x40), **0x07=0x10** (L/drivers/media/i2c/dw9719.c:71–73,190–195). Giving the ipu-bridge VCM swnode `dongwoon,vcm-prescale = 0x60` makes it the same as Windows | Origin of the reference values (a reference position and an upper limit; EEPROM or AIQB) |
| AF position write | VCM function 3 | Register 0x03, 16 bit, clamped to 1..1023. The travel and the time are recorded, and function 6 judges the lens settled once travel×10 ms (at most 40 ms) has elapsed | confirmed | `V4L2_CID_FOCUS_ABSOLUTE` 0..1023 → 0x03 (dw9719.c:22,246) | - |
| AF status | Function 4 reads 0x03, function 5 reads 0x05 bit0 (busy) | - | confirmed | dw9719 does not read the status | - |
| AF off (lens landing) | Function 2 | Lowers the position from the current one to the reference value in steps of 32. Above the upper limit without delay, above reference+90 at 1 ms, below that at 10 ms intervals. The reference value is written last | confirmed | dw9719 suspend: down to 0 in steps of 16 at 1 ms intervals, then 0x02=0x01 (dw9719.c:262–277) | - |
| MF focus range | `VideoDeviceController.Focus`/`FocusControl` | Rear min 450, max 700, default 450. Modes Auto/Single/Manual/Continuous. None on front and IR | confirmed: `camera-controls.ps1` | A libcamera AF algorithm (the IPU6 software ISP has none) or manual `focus_absolute` | Whether 450..700 are DAC codes or normalised values |
| **Exposure (OV5693)** | `ov5693.sys`. Group hold 0x3208=0x00 → VTS 0x380E (16 bit) → **0x3500 (24 bit) = lines<<4** → 0x350A (16 bit) analogue gain → 0x3400/02/04 digital gain (&0xFFF) → 0x3208=0x10 → 0x3208=0xA0 | VTS = max(requested, exposure+6) | confirmed | ov5693: 0x3500 <<4, VTS margin 8, no group hold (L/drivers/media/i2c/ov5693.c:43–66,407) | Without group hold, exposure and VTS can land in different frames (needs observation) |
| Analogue gain (OV5693) | The same function; the **value as is** to 0x350A (no shift) | - | confirmed | Upstream uses `<<4`, 1..127. linux-surface changed it to 1..2047 without shift (LS:1079–1108) — the same as Windows | - |
| **Exposure (OV13858)** | `ov13858.sys`. Group hold → VTS 0x380E (at most 0x7FFF, at least exposure+8) → 0x3500 = lines<<4 → 0x3508 analogue (16 bit as is) → 0x5100/02/04 digital (the same value) → 0x3208=0x10, 0xA0 | - | confirmed | ov13858: the same registers, analogue 0..0x1FFF, digital 0..16384, no group hold (L/drivers/media/i2c/ov13858.c:48–77,1174–1248) | - |
| **Exposure and gain (VD55G0)** | `vd55g0.sys`. 0x0448=1 (hold) → frame length 0x0458/0x0488, exposure 0x044E/0x047E, analogue 0x044D/0x047D, digital 0x0450/0x0480 → 0x0448=0 | The same values to contexts 0 and 1 | confirmed | No driver. vd55g1 uses blocks at 0x0500+0x50×ctx | Whether 0x0448 is a group hold (name inferred) |
| MF exposure range | `ExposureControl` | Front and rear 0.5 ms..500 ms (5000..5 000 000 ×100 ns), IR 0.5..33 ms. EV compensation front and rear −2..+2 (1/6), IR −3..+3 (1/4) | confirmed | The libcamera AGC converts to v4l2 `exposure` (lines) and `analogue_gain` | - |
| **Flip (front)** | `ov5693.sys` (stream start). After the mode table, if `_SUB` is `MSHW0260`/`MSHW0340`: **0x3820=0x42, 0x3821=0x18**. Then, if the SSDB degree is 180 or 1: 0x3820 ^= 0x42, 0x3821 ^= 0x06 | SP9 degree 0 → the second step is not done | confirmed | ov5693 default: 0x3820=0x00, 0x3821=0x1E; the `HFLIP`/`VFLIP` controls change bit2·1 / bit6·1 (ov5693.c:88–96,317–318,356–384). The linux-surface DMI quirk reports rotation 180 (LS:1040–1052) | The Windows value 0x3821=0x18 also clears the H binning bit0 of the mode table. The left-right direction of the Windows front image is mixed with mirroring in the MFT and the application and needs checking by a person (CAM-2) |
| Flip (rear) | `ov13858.sys`. If the filter GUID is **not** `{213A57B6-…-CD4C54DAE676}` (INF `FilterSurfaceRearOV13858`), 0x3820 bit4 is cleared and bit3 set. If degree is 180/1 or the module is `CJALC11-1(R)`, 0x3820 ^= 0x18 | On the SP9 rear the GUID matches and degree is 0, so there is **no sensor flip**. 0x3820 has the mode table value (0xA8/0xAB), the same as the Linux table | confirmed | ov13858 has no flip control. 180 is a user-space rotation | Where Windows handles the rear 180 (ISP/MFT, inferred) |
| **Stream start and stop** | OV: 0x0100 at the end of the mode table. VD55G0: 0x0201=1 start, 0x0202=1 stop, status 0x002C polled until 1·2·3 | - | confirmed (gaps 4.1) | `s_stream` of each driver | - |
| MIPI clock lane (OV5693) | Second entry of the mode table, 0x4800=0x2D | - | confirmed (gaps difference 1) | Patches 0001–0006 | - |
| Tuning data | `iacamera64` looks for it by the format `graph_settings_%S_%S_%S.xml` (sensor, module, platform), and the sensor driver reads `ExtensionPath` | Section 5 | confirmed: the name format and `ExtensionPath` are in `iacamera64.sys` and `ov5693.sys`; inferred: the files actually loaded | AIQB/CPF are only for Intel's proprietary `libia_*`. libcamera uses its own YAML tuning | Which files were read (can be checked with ProcMon) |
| Extended controls disabled | INF `ExtPropControls\40`, `\30` `Disable=1` (front, rear OV13858, IR) | id 40 (EYEGAZECORRECTION), 30 (VIDEOHDR) — they do not appear in the MF query | confirmed (the id names rely on memory of the ksmedia enumeration) | Not applicable | - |
| Platform DMFT | This build (22431+) uses the `iacamera_SV2` section: front and rear `EnablePlatformDmft=1`, `FD_disabled=1` | Class key `FD_disabled=1` | confirmed: INF, registry | Not applicable (Windows Studio Effects) | - |

## 3. Power sequence comparison

Applying the SensorOn order above to the `_DSM` table (gaps 1.3) gives the following per camera (ms).

| Camera | Windows | Linux |
|---|---|---|
| Front OV5693 (ICL1) | PowerEn 0xB0 on, 2 → Reset 0x4C assert, PCH IMGCLK3 on (19.2 MHz), 2 → Reset release, 2 | `reset`=1, `powerdown`=1 → **xvclk on** → avdd·dovdd·dvdd (int3472 provides only avdd, 2 ms) → reset=0 → 5–7.5 ms (ov5693.c:667–689) |
| Rear OV13858 (ICL0) | **Power1 0x44 on, 5** → PowerEn 0x4A on, 2 → Reset 0x43 assert, IMGCLK2 on, 2 → release, 2 | (LS) reset=1 → **xvclk on** → avdd·pwr1 together → 5–10 ms → reset=0 → 20 ms (LS:757–784) |
| IR VD55G0 (ICL2) | PowerEn 0xAF on, 2 → Avdd (0x10) 0x10 on, 2 → Reset 0x47 assert, IMGCLK1 on (24 MHz), 2 → release, 2. Then the sensor driver polls for 0x002C=1 (waiting for boot), patches the firmware, 0x0200 | No driver. vd55g1 does regulators → clock → reset release → 5–10 ms (L/drivers/media/i2c/vd55g1.c:1733–1747) |

There are two differences. Windows **enables the rails first and enables the clock while holding reset**. The two Linux drivers enable
the clock before the rails (this may differ from the datasheet order; the effect is unverified). The rear Power1 (0x08) is enabled
**5 ms before** PowerEn on Windows. `regulator_bulk_enable` does not guarantee an order. The rear camera delivers frames
on Linux (measured, CAM-1), so no problem from this order has been seen.

Renaming the rails by the Windows names, 0x0B of the IR camera is `PowerEn` and 0x10 is `Avdd`. When mapping them to the vd55g1 supply
names (`vcore/vddio/vana`), these names should be consulted, not the upstream int3472 names (`avdd`=0x0B, `dovdd`=0x10) (which rail is
which pin is inferred without a schematic).

## 4. Controls exposed by MF

Results of `tools/windows/camera-controls.ps1` (read only). None of the three cameras supports Flash or Torch, so the conclusion is that
**there is no rear flash LED**.

| Item | Front | Rear | IR |
|---|---|---|---|
| Exposure (`ExposureControl`) | 0.5–500 ms, auto | 0.5–500 ms, auto | 0.5–33 ms, auto |
| EV compensation | −2..+2, 1/6 | −2..+2, 1/6 | −3..+3, 1/4 |
| Focus | none | 450–700, Auto/Single/Manual/Continuous | none |
| White balance | 2500–12500 K | 2500–12500 K | (shown only, 2500–7500) |
| Extended (id) | 5,6,7,11,12,15,16,17,18,21,24,29,34,42,43 (+27,28 on the record pin) | front + 19,20 (FOCUSPRIORITY/STATE) | 5,6,7,11,12,15,16,17,18,21,24,34,**35,38**,42,43 |
| MAXVIDFPS_PHOTORES | 2560x1440@30 | 3968x2232@30 | 640x600@60 |
| FIELDOFVIEW payload | 27, 24 | 29, 26 | 36, 20 |

## 5. Tuning data handoff (files and hashes only)

The sensor driver reads `_SUB` (MSHWxxxx) and `ExtensionPath` (which the extension INF sets to `%13%` = the extension package folder),
and `iacamera64` looks for the graph settings by the format `graph_settings_<sensor>_<module>_<platform>.xml`. The extension packages
provide `<sensor>_<MSHW>_ADL.*`, so **the SP9 is taken to use the extension files below** (inferred; which files are actually opened is
to be checked with ProcMon). First 16 characters of the SHA-256.

| Package | File | Size | SHA-256 |
|---|---|---|---|
| `ov5693_extension.inf_amd64_0e77ea9981f64929` | `OV5693_MSHW0340_ADL.aiqb` | 534412 | 2cf85617b6a22875 |
| | `OV5693_MSHW0340_ADL.cpf` | 304 | 1ef43362ce06f453 |
| | `graph_settings_OV5693_MSHW0340_ADL.xml` | 399056 | c2e6d72773696aaa |
| `ov13858_extension.inf_amd64_fc7eeb2e30bbde91` | `OV13858_MSHW0341_ADL.aiqb` | 806276 | 583dc6f6187f413f |
| | `OV13858_MSHW0341_ADL.cpf` | 304 | c351f119cfcdecdd |
| | `graph_settings_OV13858_MSHW0341_ADL.xml` | 446997 | c4c25fe18cdfb171 |
| `vd55g0_extension.inf_amd64_fe656f7e1fff988c` | `VD55G0_MSHW0342_ADL.aiqb` | 66692 | 4c674c29dcd48611 |
| | `VD55G0_MSHW0342_ADL.cpf` | 148 | 05ecf7a7ae65c9c2 |
| | `graph_settings_VD55G0_MSHW0342_ADL.xml` | 16011 | 7c18764ded9f8caa |
| `iacamera64.inf_amd64_f5bd627220875420` | `cpd_component_signed.bin` / `_adln.bin` / `_rpl.bin` (IPU firmware; the driver refers to the `_adln` name) | 462848 each | 836f1cde0a21dd9f / d1ce77f40ced8f32 / 69ade6450d62c49e |
| | `TPG1_INTEL.aiqb` = `TPG2_INTEL.aiqb` (for the test pattern) | 319972 | c44f6e1d2c7b857f |

The hashes of `*_ADL.aiqb`, `.cpf` and graph_settings of the Intel base packages (`ov5693.inf`, `ov13858.inf`, `vd55g0.inf`) are in
[gaps/camera](../gaps/camera.md) 1.4.
On Linux there is no open implementation that can use AIQB/CPF. Intel `ipu6-camera-hal` + `ipu6-camera-bins` (proprietary `libia_*`,
out-of-tree PSYS) read the same format but have no configuration for this device. libcamera (software ISP or the IPU6 pipeline) has to
use its own YAML tuning.
The Linux IPU firmware is `intel/ipu/ipu6ep_fw.bin`; whether it is the same as the Windows `cpd_component_signed_adln.bin` was not compared.

## 6. Work on the Linux side

1. **DW9800 `VCM_FREQ`**: candidate patch that puts `dongwoon,vcm-prescale = 0x60` into the VCM swnode created by ipu-bridge (or a
   default in dw9719 limited to the ACPI SP9). To be decided by comparing the shake (ringing) after a focus move at 0x10 and at 0x60.
2. **Once a VD55G0 driver exists**, IR illumination: expose ctx0 GPIO0 (0x0467)=2 (strobe), off 0x05, always on 0x12 through
   `V4L2_CID_FLASH_LED_MODE`. Every-other-frame illumination is the Windows default (0xAA), so a way to match Howdy-like user space has
   to be decided together with it.
3. **Privacy LED**: stream the three cameras one at a time and see which LED lights (`/sys/class/leds/*privacy*`). Measured
   (CAM-5): the LED GPIO of the front or rear camera goes high while that camera streams and low afterwards; which physical LED
   lights is unverified.
   If a minimum on time of 500 ms as on Windows is judged necessary, an int3472 LED quirk.
4. **Power sequence**: whether the rear Power1 has to be enabled before PowerEn, and whether the clock has to move behind the rails, is
   patched only when a problem is observed.
5. **Front flip**: compare the actual register values of Windows (0x3820=0x42/0x3821=0x18) and Linux (rotation 180 → libcamera enables
   HFLIP·VFLIP) with i2ctransfer, and match the image orientation against the Windows frame of CAM-2.

## 7. Open

- Who switches the privacy LED on (the caller of control logic interface function 3) and the trigger. Which physical LED lights when
  each camera is opened on Windows needs observation by a person (a single group can be opened for this, as in
  `camera-controls.ps1 -Group 'IR'`).
- The meaning of the bits of the VD55G0 GPIO control values 0x02/0x05/0x12, where every-other-frame is implemented (sensor firmware or
  context switching), and whether the illuminator is on GPIO0.
- Which register `IRFlashLedIntensity` reaches (not found). The MF intensity is fixed at 32.
- The relation between the DW9800 reference position and upper limit and the MF focus range 450–700. Possibly calibration values from
  the EEPROM (0x50).
- Where Windows handles the rear 180 degrees (it does not use sensor flip).
- The mismatch between IR `IRTORCHMODE` flags 0x2 and WinRT `CurrentMode=On`.
- The extended control id names were attached from memory of the ksmedia.h enumeration (not checked against the SDK header). The id
  numbers themselves are query results.
