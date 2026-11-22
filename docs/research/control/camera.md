# Camera control: what Windows does and the same control on Linux

How the three cameras of this device (front OV5693, rear OV13858 + a VCM driven as DW9800K, IR VD55G0) are controlled, as far as
Windows shows it from outside the drivers, and how the same control is done, or has to be done, on Linux (v7.3.4 + linux-surface 7.3 +
`kernel/patches`). In short: power, clock, privacy LED, focus and exposure all have a Linux counterpart; the difference left is the IR
camera, which has no Linux driver.
Topology, SSDB and the INT3472 GPIO table are in [gaps/camera](../gaps/camera.md).

Notation: `L/path:N` = path and line in the Linux v7.3.4 source tree, `LS:N` = line in linux-surface
`patches/7.3/0013-cameras.patch`. Windows facts are from the ACPI tables, the registry and Media Foundation (MF) queries of this device;
how the Windows drivers sequence power, clock and sensor registers is not described here.
The MF control values can be read again with [`camera-controls.ps1`](../../../tools/windows/camera-controls.ps1).
Evidence levels: **confirmed** (a registry read, an MF query, an ACPI table or a measurement on this device) and **inferred** (judged
from names and structure, not observed). The Linux results are in [gaps/camera](../gaps/camera.md) and
[hardware-status.md](../../hardware-status.md).

## Summary

- **ICL2, the IR camera's control logic, has no GPIO of type Strobe/Flash/Torch** (ACPI), so the IR illuminator is not switched through
  INT3472. On Linux it lights from the sensor's own GPIO 1, driven as a strobe (measured, patch 0029); how Windows switches it was
  not traced.
- On the MF side the IR source exposes `IRTORCHMODE` (Off/On/AlternatingFrameIllumination, intensity fixed at 32) and `FACEAUTH_MODE`
  (confirmed).
- **Privacy LEDs**: the front and rear GPIOs drive separate LEDs, each on the side of its camera (observed on Linux, CAM-5); the IR
  camera has no Linux driver, so its LED GPIO has not been exercised.
- **GPIO types of the camera `_DSM` table on this device**: 0x00 reset, 0x08 a second power rail (linux-surface `pwr1`), 0x0B power enable,
  0x0D privacy LED, 0x10 a further supply that upstream int3472 calls `dovdd`.
- **Orientation on Linux**: the linux-surface DMI quirk reports rotation 180 for front and rear. For the front, libcamera turns both
  sensor flips on and the image is upright; ov13858 has no flip controls, libcamera rotates the rear image by 180 and it is upright
  (observed, CAM-1, CAM-2).

## 1. Components

Bound drivers from the PnP tree; the roles are inferred from their names and bindings.

| Component | Role |
|---|---|
| `iacamera64.sys` (service `camera`, `DISPLAY\INT3480`) | AVStream miniport of the IPU (MF controls) |
| `IntelDeviceMFT64.dll`, platform DMFT | User-space MFT |
| `ov5693.sys`, `ov13858.sys`, `vd55g0.sys` | Sensor drivers |
| `iactrllogic64.sys` (`ACPI\INT3472`) | Control logic (GPIOs, PCH clock `_DSM`) |

## 2. By control

| Control | Windows | Evidence | Same control on Linux | Open |
|---|---|---|---|---|
| **Sensor power on** | GPIO on levels come from `_DSM` bit24–31 (all 1 on this device). The order Windows uses was not observed | ACPI | int3472 provides 0x0B/0x10/0x08 (LS) as GPIO regulators (enable time 2 ms, L/include/linux/platform_data/x86/int3472.h:52) and 0x00 as the `reset` GPIO. The order is decided by the sensor driver (section 3) | - |
| **Sensor power off** | Not observed | - | The sensor driver's `power_off` does reset → regulators → clock | - |
| **MCLK** | `_DSM 82c0d13a` `{IMGCLK n, on, freq}` | DSDT; meaning of `freq` from coreboot ADL `camera_clock_ctl.asl` (gaps difference 9) | int3472 `_DSM` clock + **patch 0015** | - |
| **Privacy LED** | Privacy LED (0x0D) GPIO, on value 1 from `_DSM`. Front ICL1 pin 0xA0, rear ICL0 0x49, IR ICL2 0x46 | ACPI | int3472 registers it as the `privacy` LED (`<sensor>::privacy_led`) (L/drivers/platform/x86/intel/int3472/led.c); it is switched on and off with v4l2 subdevice stream start and stop | Whether the Windows trigger is stream start or sensor power. What the IR GPIO (0x46) drives |
| **IR illuminator** | Not switched through an INT3472 GPIO (ICL2 has none of the Strobe/Flash/Torch kind) | ACPI | Patch 0029: sensor GPIO 1 as strobe, on by default on the Surface Pro 9; it stops lighting once the exposure exceeds about 65 % of the frame, so the exposure range is held to 60 % while the strobe is on (measured; gaps difference 8) | How Windows switches it |
| IR illumination intensity | Registry `Services\vd55g0\IRFlashLedIntensity = 100`. MF `IRTORCHMODE` intensity min=max=32 | confirmed: registry, MF query | Not applicable (on/off only) | Whether the intensity reaches the hardware (the fixed intensity of 32 suggests it does not, inferred) |
| **IR torch mode (MF)** | `KSPROPERTY_CAMERACONTROL_EXTENDED_IRTORCHMODE` (id 38): capability 0x7 (Off/AlwaysOn/Alternating), current flags 0x2. WinRT `InfraredTorchControl` has Off/On/AlternatingFrameIllumination, currently On | confirmed: `camera-controls.ps1` | User space (Howdy and the like) switches it on with the V4L2 flash control. Every-other-frame illumination has to be supported by the sensor driver | flags 0x2 and WinRT `CurrentMode=On` disagree |
| Windows Hello face authentication mode | `FACEAUTH_MODE` (id 35); capability 0x3, current flags 0x1 | confirmed: MF query | Not applicable | - |
| **Focus (VCM)** | VCM at I2C 0x0C (SSDB I2C table `0x02000C01`) | ACPI | dw9719 `dw9800k` (bound by patch 0011): 0x02=0x00, 200 µs, 0x02=0x00, 0x02=0x02, 0x06=SAC<<6 (=0x40), 0x07=0x10 (L/drivers/media/i2c/dw9719.c:71–73,190–195); `V4L2_CID_FOCUS_ABSOLUTE` 0..1023 → 0x03 (dw9719.c:22,246); on suspend down to 0 in steps of 16 at 1 ms intervals, then 0x02=0x01 (dw9719.c:262–277). Measured: a written value reads back from 0x03/0x04 and the image changes over DAC 400–800 (gaps difference 4) | - |
| MF focus range | `VideoDeviceController.Focus`/`FocusControl`: rear min 450, max 700, default 450. Modes Auto/Single/Manual/Continuous. None on front and IR | confirmed: `camera-controls.ps1` | A libcamera AF algorithm (the patched libcamera of `userspace/libcamera`) or manual `focus_absolute` | Whether 450..700 are DAC codes or normalised values |
| **Exposure and gain** | Not observed at register level (MF range in the next row) | - | ov5693: exposure 0x3500 <<4, VTS margin 8, no group hold (L/drivers/media/i2c/ov5693.c:43–66,407); analogue gain upstream `<<4` 1..127, linux-surface 1..2047 without shift (LS:1079–1108), patch 0025 limits it to 16–992. ov13858: exposure 0x3500, VTS 0x380E, analogue 0x3508 0..0x1FFF, digital 0x5100/02/04 0..16384, no group hold (L/drivers/media/i2c/ov13858.c:48–77,1174–1248), patch 0026 limits analogue to 0x80–0x7c0. VD55G0: no driver | Without group hold, exposure and VTS can land in different frames (needs observation) |
| MF exposure range | `ExposureControl`: front and rear 0.5 ms..500 ms (5000..5 000 000 ×100 ns), IR 0.5..33 ms. EV compensation front and rear −2..+2 (1/6), IR −3..+3 (1/4) | confirmed: MF query | The libcamera AGC converts to v4l2 `exposure` (lines) and `analogue_gain` | - |
| **Flip (front)** | Not observed at register level | - | ov5693 default: 0x3820=0x00, 0x3821=0x1E; the `HFLIP`/`VFLIP` controls change bit2·1 / bit6·1 (ov5693.c:88–96,317–318,356–384). The linux-surface DMI quirk reports rotation 180 (LS:1040–1052); libcamera turns both flips on and the image is upright (observed, CAM-2) | Whether the image is mirrored (CAM-2) |
| Flip (rear) | Not observed at register level | - | ov13858 has no flip control. libcamera applies the 180 rotation and the image is upright (observed, CAM-1) | Where Windows handles the rear 180 |
| MIPI clock lane (OV5693) | - | - | linux-surface#2171 writes 0x4800=0x2D; upstream `0e12587945f6` sets bit 5 on non-continuous clock links. Patches 0001–0006 (gaps difference 1) | - |
| Tuning data | The camera driver packages carry AIQB/CPF tuning files and graph settings (driver files in the PnP query) | PnP | AIQB/CPF are only for Intel's proprietary `libia_*`. libcamera uses its own YAML tuning | Which files are read (can be checked with ProcMon) |
| Extended controls 40 and 30 | Not offered by the front, rear or IR source (id 40 EYEGAZECORRECTION, 30 VIDEOHDR) | confirmed: MF query (the id names rely on memory of the ksmedia enumeration) | Not applicable | - |
| Platform DMFT | Class key `FD_disabled=1` | confirmed: registry | Not applicable (Windows Studio Effects) | - |

## 3. Power sequence on Linux

| Camera | Linux |
|---|---|
| Front OV5693 (ICL1) | `reset`=1, `powerdown`=1 → **xvclk on** → avdd·dovdd·dvdd (int3472 provides only avdd, 2 ms) → reset=0 → 5–7.5 ms (ov5693.c:667–689) |
| Rear OV13858 (ICL0) | (LS) reset=1 → **xvclk on** → avdd·pwr1 together → 5–10 ms → reset=0 → 20 ms (LS:757–784) |
| IR VD55G0 (ICL2) | The vd55g0 driver of patch 0028 (ST's, from linux-surface/kernel#164) with `avdd`, `dovdd` and the reset GPIO; the sensor answers at probe and streams (measured) |

The two Linux drivers enable the clock before the rails, and `regulator_bulk_enable` does not guarantee an order between the rear
rails. The rear camera delivers frames on Linux (measured, CAM-1), so no problem from this order has been seen.

For the IR camera, upstream int3472 calls 0x0B and 0x10 `avdd` and `dovdd`, and patch 0028 requests them under those names.

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

## 5. Tuning data

The camera driver packages carry AIQB/CPF tuning files and graph settings for the three sensors.
On Linux there is no open implementation that can use AIQB/CPF. Intel `ipu6-camera-hal` + `ipu6-camera-bins` (proprietary `libia_*`,
out-of-tree PSYS) read the same format but have no configuration for this device. libcamera (software ISP or the IPU6 pipeline) has to
use its own YAML tuning.
The Linux IPU firmware is `intel/ipu/ipu6ep_fw.bin`; whether it is the same as the IPU firmware Windows loads was not compared.

## 6. Work on the Linux side

1. **IR illumination**: done with patches 0027–0029 (sensor GPIO 1 as strobe, `V4L2_CID_FLASH_LED_MODE` as in vd55g1). Every-other-frame
   illumination, which MF offers (`IRTORCHMODE`), is not implemented; what Howdy-like user space needs is not decided.
2. **Privacy LED**: done for front and rear (CAM-5): the LED GPIO of the streaming camera goes high and low afterwards
   (measured), and the LED on that camera's side lights (observed). For the IR camera it is unverified.
3. **Power sequence**: the order of the rear rails and of clock and rails is changed only when a problem is observed.

## 7. Open

- What triggers the privacy LED on Windows (stream start or sensor power). Which physical LED lights when
  each camera is opened on Windows needs to be observed by hand (a single group can be opened for this, as in
  `camera-controls.ps1 -Group 'IR'`).
- How the IR illuminator is switched, and where every-other-frame illumination is implemented.
- Whether `IRFlashLedIntensity` reaches the hardware. The MF intensity is fixed at 32.
- What the MF focus range 450–700 corresponds to on the VCM. Possibly calibration values from the EEPROM (0x50).
- Where Windows handles the rear 180 degrees.
- The mismatch between IR `IRTORCHMODE` flags 0x2 and WinRT `CurrentMode=On`.
- The extended control id names were attached from memory of the ksmedia.h enumeration (not checked against the SDK header). The id
  numbers themselves are query results.
