# Cameras: Windows reference values and the differences in linux-surface

The subjects are the IPU6 (`8086:465d`), the front OV5693 (`OVTI5693`, `MSHW0340`), the rear OV13858 (`OVTID858`, `MSHW0341`),
the IR VD55G0 (`SMO55F0`, `MSHW0342`) and three `INT3472` control-logic devices. The Windows side is the ACPI tables, driver packages and
registry read on this device; the Linux side is linux-surface `patches/7.3/0013-cameras.patch` and its base, v7.3.4.

Notation: `LS:N` is a line number in linux-surface `patches/7.3/0013-cameras.patch`, and `L/path:N` is a file and line number in the
Linux v7.3.4 source tree (a bare file name such as `ipu-bridge.c:N` is a file in the same tree). ACPI objects are named by device path,
method and package index; unless a table is named they are in the DSDT. The ACPI tables are not in this repository: dump and decompile
them on your own device with [`tools/windows/dump-acpi.ps1`](../../../tools/windows/dump-acpi.ps1), or see
<https://github.com/linux-surface/acpidumps> (directory `surface_pro_9_intel`). Values attributed to a Windows driver (`ov5693.sys` and
so on) are from the driver packages listed in 1.4. `#N` is an issue or pull request of linux-surface/linux-surface, and `kernel#N` one of
linux-surface/kernel. A patch number such as "patch 0015" is one of this repository's patches in `kernel/patches`; linux-surface's
patches are named by file. Test IDs (`CAM-n`) refer to [test-plan.md](../../test-plan.md) and [test-results.md](../../test-results.md).

## Summary

- **The direct reason the front OV5693 does not work with linux-surface 7.3 alone is `MIPI_CTRL00` (0x4800).** The Windows `ov5693.sys` writes `0x4800=0x2d` in every
  mode table for IPU6; the Linux `ov5693.c` never writes this register. At the default value (0x00) clock-lane gating is off and the IPU6
  CSI-2 receiver receives no packets. A user reported 2592x1944 at 28.6 fps on an SP9 with only bit 5 set (linux-surface/kernel#180,
  upstream media `next` 0e12587945f6), and **the linux-surface 7.3 patches, the base kernel here, do not contain this fix** (this repository's patches 0001–0006 add it).
- Windows runs the SP9 front camera in **a single mode, 1296x972 binned**. That mode differs from Linux in the MIPI PLL multiplier (0x30b3)
  and in analog registers. The Linux binned mode does not change the PLL and produces no frames on IPU6 (#2234).
- The VCM of the rear OV13858 is SSDB `vcmtype=0x11`, and in the Windows driver's table 0x11 is **DW9800**. The ipu-bridge VCM table ends
  at 9, so it drops the VCM with "Unknown VCM type 17". There is no autofocus.
- INT3472 GPIO type 0x08 (rear control logic) is a type upstream does not know, and linux-surface registers it on every device under the
  non-standard name `pwr1`.
- The IR VD55G0 has no Linux driver. It shares the boot and streaming registers with `vd55g1`, but the status register address, the
  firmware patch and the exposure/frame register block differ, so adding a model ID does not make it supported (4.1). Patch 0015 sets the
  24 MHz MCLK this sensor asks for.

Measurements on Linux 7.3.4 (libcamera 0.7.2 SoftISP) are in the "Verification on Linux" column of section 3 and in section 5. Values
without a stated patch range are from a kernel with patches 0001–0017. In short:

- The front camera delivers frames in the 2592x1944 mode (measured: 90/90 frames at 28.65 fps, the image is upright and in focus,
  `0x4800`=0x20 while streaming).
- The front 1296x972 binned mode fails (measured). The symptom is not `Transfer FIFO overflow` but `Frame sync error` on every frame, and
  only when at least one flip is enabled. Patch 0020 stops offering this mode (it does not fix it). libcamera requests of 1280x720 and
  below therefore go to the 2592x1944 mode too and deliver frames (measured: 1280x720 120/120).
- The rear camera delivers frames (measured: 1920x1080 30/30), the VCM binds as `dw9800k` and `focus_absolute` appears. Whether the image
  orientation is right and whether it focuses is unverified. When the camera is not in use the rear sensor and VCM are powered off
  (patch 0021, measured). Patch 0022 addresses boots in which the lens device did not appear (with that kernel it appeared in 1/1 boots).
- Three defects that showed in applications (purple lines, exposure that moved oddly, no rear focus) were not in the kernel but in
  libcamera's software ISP and in the gain ranges the sensor drivers advertise (section 6). With libcamera 0.8.0 and the patches of
  `userspace/libcamera` the lines are gone and the exposure settles (measured with `cam`, then observed in a camera application). The
  contrast autofocus is implemented and sweeps the lens (measured). Whether the rear camera focuses is **unverified**: an opaque protective
  film covers the rear camera of the test unit (observed), and the rear image stays diffuse at every lens position (measured; 6.3).

## 1. Windows reference values

### 1.1 Topology

| Item | Front | Rear | IR |
|---|---|---|---|
| ACPI path | `\_SB.PC00.I2C3.CAMF` | `\_SB.PC00.I2C2.CAMR` | `\_SB.PC00.I2C2.CAM3` |
| HID / `_SUB` (returned by the method `GCSM`) | `OVTI5693` / `MSHW0340` | `OVTID858` / `MSHW0341` | `SMO55F0` / `MSHW0342` |
| `_DEP` control logic | `ICL1` (I2C3, `_UID 1`) | `ICL0` (I2C2, `_UID 0`) | `ICL2` (I2C2, `_UID 2`) |
| I2C controller (PnP parent) | `8086:51eb` (I2C3) | `8086:51ea` (I2C2) | `8086:51ea` (I2C2) |
| `_CRS` I2C (all 400 kHz, 0x61A80) | 0x36 sensor, 0x50 EEPROM | 0x10 sensor, 0x0C VCM, 0x50 EEPROM | 0x60 sensor |
| `_DSM 26257549…` I2C device table (bus<<24 \| addr<<8 \| kind) | `0x03003600`, `0x03005002` | `0x02001000`, `0x02000C01`, `0x02005002` | `0x02006000` |
| `_PLD` | FRONT, Rotation 0 | BACK, Rotation 0 | FRONT, Rotation 0 |
| Windows driver | `ov5693.sys` | `ov13858.sys` | `vd55g0.sys` |

The low byte of the I2C device table reads as 0 = sensor, 1 = VCM, 2 = EEPROM (consistent across the three sensors). The front camera has
no VCM (fixed focus). The IPU6 is `PCI(0500)` `\_SB.PC00.IPU0` with service `iaisp64`, and the AVStream camera `INT3480` enumerates as
`IPUA` under GFX0 (defined in the SSDT with OEM table ID `IPUSSDT`; PnP query).

### 1.2 SSDB (sensor `SSDB`, 108 bytes) decoded

Decoded exactly as `struct ipu_sensor_ssdb` (ipu-bridge). The buffers are the `SSDB` objects of `CAMF` (front), `CAMR` (rear) and `CAM3` (IR).

| Field (offset in the buffer) | Front OV5693 | Rear OV13858 | IR VD55G0 |
|---|---|---|---|
| sku (0x01) | 0x20 | 0x20 | 0x20 |
| link (0x1C) = CSI-2 port | **2** | **1** | **3** |
| lanes (0x1D) | **2** | **4** | **1** |
| maxlanespeed (0x46) | 0 | 0 | 0 |
| romtype (0x4E) | 0x09 | 0x09 | 0x00 |
| vcmtype (0x4F) | 0 | **0x11** | 0 |
| platforminfo/subinfo (0x50/0x51) | 0x0F/0x01 | 0x0F/0x01 | 0x0F/0x01 |
| flash (0x52) | 2 | 2 | 2 |
| privacyled (0x53) | 1 | 1 | 1 |
| degree (0x54) | **0** | **0** | 0 |
| mipilinkdefined (0x55) | 1 | 1 | 1 |
| mclkspeed (0x56) | **19 200 000** | **19 200 000** | **24 000 000** |
| controllogicid (0x5A) | 1 | 0 | 2 |
| mclkport (0x5E) | 0 | 0 | 0 |

### 1.3 INT3472 control logic (CLDB + GPIO `_DSM 79234640…`)

All three have CLDB `control_logic_type=1` (discrete, not a PMIC) and `sensor_card_sku=0x20`. There is no TPS68470.
There is no clock GPIO (type 0x0c); the sensor MCLK is switched on through `\_SB.ICLK.CLKC/CLKF` (PCH IMGCLKOUT),
called by `_DSM 82c0d13a…`. A GPIO `_DSM` value is `bit24-31 = sensor-on value, bit8-15 = pin, bit0-7 = type`. Every pin matches the
`_CRS` pin number and every on value is 1.

| Control logic | CLDB (id, clock_source) | `_CRS` pin (GPI0) → `_DSM` value | Type as read by upstream v7.3.4 |
|---|---|---|---|
| ICL1 → front | id 1, IMGCLK **3** | 0xB0 → `0x0100B00B`, 0x4C → `0x01004C00`, 0xA0 → `0x0100A00D` | 0x0B power enable (`avdd`), 0x00 reset, 0x0D privacy LED |
| ICL0 → rear | id 0, IMGCLK **2** | 0x4A → `0x01004A0B`, 0x43 → `0x01004300`, 0x49 → `0x0100490D`, 0x44 → `0x01004408` | avdd, reset, privacy LED, **0x08 = not defined upstream** |
| ICL2 → IR | id 2, IMGCLK **1** | 0xAF → `0x0100AF0B`, 0x10 → `0x01001010`, 0x47 → `0x01004700`, 0x46 → `0x0100460D` | avdd, **0x10 = DOVDD** (upstream's reading), reset, privacy LED |

The Windows control-logic driver `iactrllogic64.sys` has a discrete control path with PCH clock control, indicator on/off and flash on/off.
Windows too handles these three devices through the discrete path.

### 1.4 Driver packages

All driver versions are 63.22000.10.17589. The INFs contain almost no sensor configuration AddReg (only
`WdfDirectedPowerTransitionEnable`, security settings, OV13858 `IsSupportCameraMask=0` and VD55G0 `IRFlashLedIntensity=100`). Lane count,
port and MCLK come from ACPI. The values `MipiLanes/MipiPort/MipiDataFormat/MipiMBps` that the driver reads from its `Services\camera` key
do not exist on this device (registry query).

| Package | Files (size, first 16 characters of SHA-256) |
|---|---|
| `ov5693.inf_amd64_e536eece2320831e` | `ov5693.sys` 211552 c63640289c538188, `OV5693_13P2BA540_ADL.aiqb` 474124 107f55f89cd2f7a1, `.cpf` 120 0eede1700ba00b30, `graph_settings_ov5693_13P2BA540_ADL.xml` 68739 be2f4aa5e6611334 |
| `ov13858.inf_amd64_4382786dbe6d4ffa` | `ov13858.sys` 223328 9b5837e07302e7ad, `OV13858_YHCE_ADL.aiqb` 416756 7e990860e1c8ddb1 (= `YHCEW`), `OV13858_YHCEU_ADL.aiqb` 422096 e181a2ec98878352, `YHCEU_M`/`YHCEU_S` 420440 91f0c699cc7518ee, graph_settings `YHCE` 976815 03e3d1691854a25e, `YHCEU`/`YHCEW` 493912 fa89b68d334322f7 |
| `vd55g0.inf_amd64_2b810928b82e0fa5` | `vd55g0.sys` 253024 bed09c0a383f2ee3, `VD55G0_ST_ADL.aiqb` 82748 3e97ea1445cc6601, `.cpf` 176 77a4973a62c32719, graph_settings 16317 6880d44dccef2da2 |
| `iactrllogic64.inf_amd64_db150e2b4aabfa94` | `iactrllogic64.sys` 177784 a11bc6bb3e4620c7 (`ACPI\INT3472`, `ACPI\INT346F`) |

### 1.5 Sensor modes according to graph_settings

| Sensor | Modes (width x height @ fps, 10 bit) | Source |
|---|---|---|
| OV5693 `OV5693IPU6` | **1296x972@60 only**. ISYS crop 1280x736, outputs 640x480 and 1280x720 | graph_settings_ov5693_13P2BA540_ADL.xml |
| OV13858 YHCE | 4096x3072@30, 2048x1152@60, 1364x768@120 | graph_settings_OV13858_YHCE_ADL.xml |
| OV13858 YHCEU/YHCEW | 2048x1536@30 | graph_settings_OV13858_YHCEU_ADL.xml |
| VD55G0 | 644x604@60 | graph_settings_vd55g0_ST_ADL.xml |

bayer_order is GRBG in all four files (including the monochrome VD55G0), so it is of little use as actual phase information.
Which of YHCE/YHCEU/YHCEW the OV13858 module is, the driver decides by reading the EEPROM (0x50).
The value for this device was not determined on Windows.

### 1.6 `ov5693.sys` register tables

The driver has two 1296x972 tables and four full-resolution tables, plus one 1296x976 table.

| Output | 0x30b3 (PLL multiplier) | 0x4800 | VTS (0x380e/f) |
|---|---|---|---|
| 1296x972 | **0x70** | **0x2d** | 0x07c0 |
| 1296x972 | **0x70** | **0x2d** | 0x03e0 (appears to be the 60 fps variant) |
| 1296x976 | 0x7d | none | 0x07c0 (inferred to be for pre-IPU6 devices) |
| 2592x1944 (two tables) | 0x83 | 0x2d | 0x07c0 / 0x0f78 |
| 2592x1556 (two tables) | 0x83 | 0x2d | 0x07c0 / 0x0f78 |

Each table starts with `0x0103=0x01` (soft reset); the tables that write `0x4800=0x2d` do so as the second entry. Each has `0x0100=0x00`
near the end. The 1296x972 table with VTS 0x03e0 compared with the Linux `ov5693_global_regs` (L/drivers/media/i2c/ov5693.c:175–):

| Register | Windows 1296x972 | Linux | Note |
|---|---|---|---|
| 0x4800 MIPI_CTRL00 | 0x2d | not written | The upstream fix sets only bit 5 (0x20) |
| 0x30b3 PLL multiplier | 0x70 | 0x83 | 0x83 is 419.2 MHz, so 358.4 MHz if linear (inferred) |
| 0x3620/0x3621/0x3622 | 0x44/0xb5/0x0c | 0x54/0xc7/0x0f | Analog values specific to binning. The full-resolution tables match Linux |
| 0x371f | 0x1f | 0x0c | As above |
| 0x3820/0x3821 | 0x01/0x1f | 0x00/0x1e (changes per mode) | bit0 = V/H binning |
| 0x4307 | 0x30 | 0x31 | 0x30 in every Windows table |
| 0x3600, 0x3708/0x3709 | 0xbc, 0xe6/0xc7 | absent | Full resolution has 0x3708/9=e2/c3 |
| 0x4806, 0x4816, 0x4831, 0x4d00/01 | 0x00, 0x52, 0x64, 0x04/0x71 | absent | MIPI and temperature sensor related |
| 0x3800–0x3815 | 0,0,0,0,0a3f,07a3, output 0510x03cc, HTS 0a80, offsets 0008/0003, 0x3814/15=0x31 | computed by the mode code | Bins the whole array and crops with the ISP window offset |

The full-resolution 2592x1944 table differs from Linux only in `0x4307` (0x30 versus 0x31) and in whether 0x4800 is written.

### 1.7 Driver constants for the VCM and the IR sensor

- In the VCM table of `ov13858.sys`, **id 0x11 = "DW9800"**. Entries 1–9 of the same table are AD5823, DW9714, AD5816, DW9719, DW9718,
  DW9806B, WV517S, LC898122XA and LC898212AXB, in the same order as the ipu-bridge table; 0x0a–0x0e are NoneVCM, 0x0f is AK7371, 0x10 is
  BU64297GWZ, 0x12 is DW9808 and 0x13 is LC898217.
- `vd55g0.sys` compares the model ID it reads against 0x53354730 ("S5G0"). The VD55G0 model ID is 0x53354730.
- The full-mode tables of `ov13858.sys` write `0x4800=0x60`. PLL1 (0x0300–0x0302 = 07/01/c2) matches the Linux 1080 Mbps table
  (L/drivers/media/i2c/ov13858.c:123–).

## 2. State of linux-surface and upstream

| Subject | State | Source |
|---|---|---|
| IPU6 `8086:465d` | Upstream `intel-ipu6`, ADL-P path with `intel/ipu/ipu6ep_fw.bin` | L/drivers/media/pci/intel/ipu6/ipu6.c:537–540 |
| ipu-bridge sensor table | v7.3.4 has none of `OVTI5693`, `OVTID858`, `SMO55F0`. linux-surface adds `OVTI5693` (declared with 1 lane, 419.2 MHz) and `OVTID858` (540 MHz) | L/drivers/media/pci/intel/ipu-bridge.c:51–105, LS:965–967 |
| ipu-bridge rotation | A DMI quirk sets 180 degrees for both `OVTI5693` and `OVTID858` on the SP9. The loop checks every entry for the same model, so both apply | LS:1040–1052, LS:1134–1146, ipu-bridge.c:385–388 |
| ipu-bridge VCM table | 9 types (`ad5823`…`lc898212axb`); `vcmtype > 9` warns and is treated as 0 | ipu-bridge.c:208–218, 446–449 |
| ov5693 | The only ACPI ID is `INT33BE`; linux-surface adds `OVTI5693`. Fixed at 2 lanes, 419.2 MHz, 19.2 MHz; `0x4800` not written; the binned mode uses the same PLL. The 7.3 patch changes only the gain range | ov5693.c:121–122,1239–1258,1397–1400; LS:952, LS:1079– |
| ov13858 | Upstream has no regulator, reset or clock control. linux-surface adds `avdd`+`pwr1` regulators, `reset`, `xvclk` and runtime PM (with many debugging `dev_info` calls). No H/V flip controls | LS:697–943 |
| int3472 discrete | Handles types 0x00, 0x01, 0x02, 0x0b, 0x0c, 0x0d, 0x10, 0x12, 0x13; the rest get an "unknown" warning and are ignored. Without a GPIO clock it registers the `_DSM 82c0d13a` clock, with the arguments fixed at `{clock_source, enable, 1}` and the frequency taken from the sensor SSDB mclk | discrete.c:235–276,364–433,458–459; clk_and_regulator.c:22–46,82–101 |
| int3472 0x08 | linux-surface registers 0x08 as the regulator `pwr1` on every device | LS:979–1000 |
| dw9719 | Supports the `dw9800k` model (fixed configuration, no ID register) | L/drivers/media/i2c/dw9719.c:71–73,136–147,446 |
| vd55g1 | Models 0x53354731/0x53354733 only, no ACPI matching, supplies `vcore/vddio/vana`, xclk 6–27 MHz | L/drivers/media/i2c/vd55g1.c:131–165,488–492 |
| Wiki | SP9 front not working, rear working, IR not working | linux-surface wiki, `Camera-Support.md` |
| Front camera fix | linux-surface/kernel#180 (v7.2, bit 5 only, open), linux-surface#2171 (0x2d, closed), upstream media `next` 0e12587945f6. Nothing in the 7.3 patch directory writes 0x4800 | `gh pr view`; `grep 4800 patches/7.3/*` gives 0 results |
| Binned mode | linux-surface#2252 (SP7+, port of the Windows table, open), #2234 | `gh pr view 2252` |

## 3. Differences

In the last column, the text before "Measured" is the check and the text after it is the result on this device.

| # | What is wrong or missing | Evidence | Effect | How to close it | Confidence | Verification on Linux |
|---|---|---|---|---|---|---|
| 1 | **ov5693 does not write `MIPI_CTRL00` (0x4800)** | Every Windows IPU6 table has `0x4800=0x2d` (1.6). ov5693.c has no 0x4800. An SP9 user confirmed streaming with bit 5 (kernel#180) | 0 frames from the front camera, `stream stop time out`. The reason the wiki lists the front camera as not working | `cci_update_bits(0x4800, BIT(5), BIT(5))` before stream-on. Port kernel#180 or upstream 0e12587945f6 to the 7.3 patches | Confirmed (code and Windows tables); operation on the SP9 is an external report | `cam -c '\_SB_.PC00.I2C3.CAMF' -C10` with and without the patch; while streaming, `i2ctransfer -y <bus> w2@0x36 0x48 0x00 r1`. Measured (kernel with patches 0001–0006): 2592x1944 mode, 90/90 frames, 0 missing sequence numbers, `0x4800`=0x20 while streaming. The failure on a kernel without the patches was not tested (CAM-2). With patch 0020, 1280x720 and 640x480 requests also go to this mode and deliver 120/120 (28.6 fps) and 30/30 frames |
| 2 | **The binned 1296x972 mode differs from the Windows values** | Windows: 0x30b3=0x70, 0x3620–22=44/b5/0c, 0x371f=1f, 0x3600=bc, 0x3708/9=e6/c7, 0x4307=30, 0x4816=52, 0x4831=64, 0x4d00/01=04/71 (the `ov5693.sys` 1296x972 table). Linux keeps the full-resolution values such as 0x83 | No frames in the binned mode (#2234 reports empty buffers with FIFO overflow; this device shows `Frame sync error`). Using full resolution only gives high noise and the AGC goes to maximum (#2252) | Per-mode PLL and analog tables. Add 358 400 000 to `link_freq_menu_items` and change the ipu-bridge `OVTI5693` entry to `IPU_SENSOR_CONFIG("OVTI5693", 2, 419200000, 358400000)`. Port the ov5693 commit of #2252. The patch is on hold (4.2) | Value differences confirmed; 358.4 MHz is derived with the same formula that gives 419.2 MHz for 0x83 (4.2) | Capture with `cam` in the binned mode (section 5 item 6) and look for `csi2-* error` lines (`Frame sync error`, `Transfer FIFO overflow`) in the kernel log. Measured: in the sensor's 1296x972 mode, 120/120 libcamera buffers are errors (bytesused 0) and 20/20 buffers of a direct `v4l2-ctl` capture carry the error flag. The log shows `csi2-2 error: Frame sync error`. The buffer sequence number advances by 2 and the frame interval is 34.88 ms (the sensor reports an interval of 1/60); `0x30b3` stays at the full-resolution value 0x83. Whether the Windows value 0x70 is the cause was not tested (CAM-3). In the same mode, with both flips off 20/20 frames arrive at 57 fps, and with either one on every frame is a `Frame sync error` (measured). Because of the 180 rotation libcamera enables both flips. The cause is unverified, so patch 0020 does not fix this mode; it stops offering it on `clock-noncontinuous` links |
| 3 | The HID `OVTI5693` is not in upstream ov5693 or ipu-bridge | ov5693.c:1397–1400, ipu-bridge.c:51–105 | With an upstream-only kernel the front camera does not bind at all. Solved in the linux-surface kernel | Submit the two lines upstream (a plan is mentioned in the text of #2171) | Confirmed | `ls /sys/bus/i2c/devices/ \| grep OVTI5693`, `dmesg \| grep ov5693`. Measured: `Found supported sensor OVTI5693:00`, `ov5693 3-0036` linked to `Intel IPU6 CSI2 2` |
| 4 | **Rear VCM type 0x11 (DW9800) is unknown** | SSDB vcmtype 0x11 (1.2). Windows table id 0x11 = DW9800 (1.7). The ipu-bridge table has 9 types | `Unknown VCM type 17`, no `lens-focus`, no autofocus | Extend `ipu_vcm_types` to match the Windows enum (empty entries for 0x0a–0x0e, 0x11=`"dw9800k"`) and treat an empty name as no VCM. The dw9719 driver binds by the `dw9800k` i2c id | Type mapping confirmed; DW9800 = DW9800K compatibility is inferred | `dmesg \| grep -i vcm`; with the sensor powered, `i2ctransfer -y <bus> w1@0x0c 0x00 r1`; `v4l2-ctl -d /dev/v4l-subdevN -L` (focus_absolute). Measured (patch 0011): no "Unknown VCM type" line, `Instantiated dw9800k VCM`, `focus_absolute` 0–1023. The written value reads back from VCM registers 0x03/0x04. The image changes only in the DAC range 400–800 (low-frequency RMS difference 8.1 % between 0 and 1023, 0.43 % when repeating the same value). Whether near and far focus is reached is unverified (CAM-4; the scene of the measurement with patch 0021 had a mean brightness of 7 %, too dark to judge sharpness). libcamera has no focus control, so there is no autofocus. With patch 0021 the VCM powers up on the first `focus_absolute` write: writing 900 gives register contents `03 84`. Without patch 0022, in one of two boots the lens device did not appear, with `Error -13 runtime-resuming sensor, cannot instantiate VCM` (measured). With patch 0022 it appeared in the one boot observed (1/1). Whether the race is gone needs more boots |
| 5 | **Handling of INT3472 type 0x08** | ICL0 0x44 → `0x01004408` (1.3). Upstream ignores it; linux-surface maps it globally to `pwr1` | Upstream: the rear sensor's second rail is not switched on, giving -121 (inferred). linux-surface: the name is non-standard, so it is used only by the OV13858 patch, and it conflicts with the SP7+ OV8865, which uses the same type (wiki) | In `int3472_gpio_map`, restrict to `hids={"OVTID858"}`, `type_from=0x08`, `type_to` of the `POWER_ENABLE` kind, `con_id="dvdd"`. ov13858 requests the standard names `avdd/dovdd/dvdd` | Existence of 0x08 confirmed; the kind of physical rail is inferred | Consumers of `INT3472:00-*` in `/sys/kernel/debug/regulator/regulator_summary`; whether reading the OV13858 ID fails with the 0x08 rail off. Measured: `GPIO type 0x08 … con_id=pwr1`; on `INT3472:00`, avdd, pwr1 and the 19.2 MHz clk have the rear sensor as consumer. The test with the 0x08 rail off was not done |
| 6 | ov13858 power control exists only as a linux-surface-specific patch | LS:697–943 (non-standard `pwr1`, many diagnostic `dev_info` calls, unconditional power-on at probe) | Not upstreamable as is. Log noise | Clean up to standard names (`avdd`, `dovdd`, `dvdd`), a `reset` GPIO and `devm_v4l2_sensor_clk_get`, and submit upstream | Confirmed | Amount of `dmesg \| grep ov13858` output. Measured: 7 lines of `ctrl(id:…) is not handled` each time the camera is opened and 4 `dev_info` lines per power transition; several hundred lines per boot |
| 7 | The rotation does not come from firmware | SSDB degree 0, `_PLD` Rotation 0 (all three sensors). linux-surface sets 180 for front and rear by DMI | The value is right according to user confirmation (LS:1119–1125). But ov13858 has no H/V flip controls, so 180 is passed on only as a property and the actual rotation is left to user space. Where Windows gets the rotation from was not found | Adding `V4L2_CID_HFLIP/VFLIP` (0x3820/0x3821) to ov13858 lets libcamera flip in the sensor (#177 is closed) | Rotation value confirmed externally; absence of flip confirmed | `v4l2-ctl -d /dev/v4l-subdevN -C camera_sensor_rotation` and the actual image orientation. Measured: `camera_sensor_rotation`=180 on both sensors. The front image is upright (hflip=vflip=1). For the rear, libcamera offers only rot180 (a `rot0` request is adjusted) and the image orientation is unverified (CAM-1). Both cameras' PipeWire nodes carry `api.libcamera.rotation: 180` |
| 8 | **No VD55G0 driver** | The `vd55g0.sys` model ID is 0x53354730; vd55g1 has only 0x53354731/33. No ipu-bridge or ACPI matching | No IR camera, nothing of the Windows Hello kind | Extending vd55g1 does not work (4.1). If VD55G0 support from ST or a separate driver appears, attach the ACPI ID `SMO55F0`, the supply mapping (`avdd`→`vana`, `dovdd`→`vddio`, no `vcore`), ipu-bridge `IPU_SENSOR_CONFIG("SMO55F0", 1, 420000000)` (Windows MIPI 840 Mbps, 1 lane, link 3) and the 24 MHz MCLK (patch 0015) | Model ID and register map differences confirmed | Read the ID after power-on with `i2ctransfer -y <bus> w2@0x60 0x00 0x00 r4`. Measured: 0x60 does not respond (`Remote I/O error`). That is expected, since no driver powers the sensor. The model ID is unverified |
| 9 | The frequency argument of the int3472 DSM clock is fixed at 1 | clk_and_regulator.c:37–39. `\_SB.ICLK.CLKF` changes only IMGCLKOUT bit 0. The IR SSDB mclk is 24 MHz | The IR sensor is given 19.2 MHz while the clk rate is reported as 24 MHz. Front and rear (19.2 MHz) are unaffected | Argument 0 when `mclkspeed == 24000000`, otherwise 1. **Patch 0015** | Meaning of the bit confirmed: coreboot ADL `camera_clock_ctl.asl` (0 = 24 MHz, 1 = 19.2 MHz), and `iactrllogic64.sys` maps 19.2 MHz→1 and 24 MHz→0 | Once an IR driver exists, compare `clk_summary` with the behaviour of the sensor PLL. Measured: `INT3472:01-clk` shows a rate of 24000000. It has no consumer, so the `_DSM` argument and the actual output frequency are unverified |
| 10 | The EEPROM (0x50, romtype 9) is not used | 0x50 is in `_CRS` and the I2C device table; Windows identifies the module and takes LSC/AF calibration from the NVM (per `ov13858.sys` and `ov5693.sys`) | Operation without per-module calibration. libcamera has no IPU6 tuning for OV13858/OV5693 and uses `uncalibrated.yaml` | Expose it read-only through at24 and decode the calibration, or write libcamera tuning yaml files. The `.aiqb` files are for the Intel HAL only | Confirmed (existence); contents unverified | Read 0x50 with `i2ctransfer` (never write). Measured: the rear 0x50 responds while the sensor is powered (only 1 byte was read). libcamera uses `uncalibrated.yaml`. The comparison with Windows photos is unverified (CAM-6) |
| 11 | Three privacy LEDs | ICL0 0x49, ICL1 0xA0, ICL2 0x46 (1.3) | Linux attaches each to its sensor as a `privacy` LED. The IR sensor has no driver, so its LED is not used. Which physical LED each one is, is unverified | Verify, then add a quirk if needed | Unverified | Watch the LED while each camera streams; `/sys/class/leds/`. Measured (GPIO only): `OVTI5693_00::privacy_led`=1 and gpio-160 high while the front camera streams, `OVTID858_00::privacy_led`=1 and gpio-73 high while the rear camera streams, 0 after stopping. The IR LED (gpio-70) does not change. Which LED actually lights is unverified (CAM-5) |
| 12 | IPU0 `_DSM 9a9e6ab4…`/`7192a970…` (CSLU/CSSP) are not used | The IPU SSDT (OEM table ID `IPUSSDT`), device `IPU0`; 0 grep hits in Linux | Effect unknown (the values are NVS variables and cannot be read from a table dump) | If needed, just record the `acpi_evaluate_dsm` results on Linux | Unverified | Read the return values with `acpiexec` or a debugging module |

To restate why the front camera does not work: power, clock, reset, I2C address, lane count, MCLK and link frequency agree between Windows
and Linux (ICL1 provides avdd, reset and privacy; the DSM clock gives 19.2 MHz on IMGCLK 3; 2 lanes at 419.2 MHz equals the Windows
full-resolution PLL value 0x83). What differs is the configuration of the sensor's MIPI block (0x4800), and on top of that the PLL and
analog differences of the binned mode.

## 4. Patches

The patches are in [`kernel/patches`](../../../kernel/patches).

| Difference | Result |
|---|---|
| 1 (0x4800) | `kernel/patches/0001`–`0006` (backport from upstream v7.4) |
| 4 (VCM) | Patch 0011 |
| 9 (DSM clock frequency) | Patch 0015: the third `_DSM` argument is 0 when the SSDB mclk is 24 MHz. For 19.2 MHz sensors (front and rear) it stays 1 |
| 8 (VD55G0) | No patch (4.1) |
| 2 (binned mode) | No patch fixes it (4.2). Patch 0020: the mode is not offered on `clock-noncontinuous` links |
| Sensor gain ranges (6.2) | 0025 (ov5693: 16–992), 0026 (ov13858: 0x80–0x7c0) |

### 4.1 VD55G0 is not supported by vd55g1

The VD55G0 column is from the register tables of the Windows driver and from the order in which it reads, writes and polls registers.

| Item | VD55G0 (`vd55g0.sys`) | vd55g1 (L/drivers/media/i2c/vd55g1.c) |
|---|---|---|
| Model ID | 0x0000, 32 bits, 0x53354730 | 0x0000, 0x53354731/0x53354733 |
| Status (FSM) | Polls **0x002c** up to 1, 2 and 3 | **0x001c**, same values (READY_TO_BOOT 1, SW_STBY 2, STREAMING 3) |
| Firmware patch | Depending on the revision in 0x0004 (16 bits): for 0x1010 it writes 3979 bytes, for 0x1111 551 bytes, to 0x2000, then 0x0200=2 | Only model 2 writes its own patch to 0x2000 and checks for 2.9 in 0x0012; model 3 uses 0x0200=1 |
| Colour version | Not read | 0x0670 |
| Boot and stream | 0x0200, 0x0201=1, 0x0202=1 | Same |
| Clock and MIPI | 0x0220=24 000 000, 0x0224=840 000 000, 0x0300 (line length)=1201 | Same registers |
| Exposure, frame, window | Context blocks from 0x044c at a spacing of 0x30: 0x044c mode, 0x044d analog gain, 0x044e exposure, 0x0450 digital gain, 0x0458 frame length (1942), 0x045c–0x0465 = 603/0/643/0/603 | From 0x0500 at a spacing of 0x50; the window is start + size (0x0510–0x0517). AE target 0x0486 |
| Pixel array | 644x604@60 (driver mode descriptor, graph_settings) | 804x704 |

What they share is the system registers and the clock rules. Applying the vd55g1 divider rules (mipi_div 1, pixel_div 6) to 840 Mbps gives
140 MHz, and 140 MHz / (1201 × 1942) = 60.03 fps, which matches the Windows mode.
Because of what differs, a vd55g1 with only the model ID added fails probe with a timeout polling 0x001c right after power-on. Even with
the status address corrected, the exposure and window settings are written in the 0x0500 range and the write to 0x0486 lands inside the
second context block of the VD55G0 (0x047c–0x04ab). The meaning of the fields in a context block can only partly be inferred, and the
firmware patches for the two revisions exist only in the Windows binary and cannot be carried into a GPL driver. Without VD55G0 support
from ST or a datasheet, no driver is written here.
The values to attach once a driver exists are in section 3, difference 8. The ipu-bridge entry goes in together with the driver.

### 4.2 Link frequency of the OV5693 binned mode, and why the patch is on hold

Of the PLL registers, the Windows 1296x972 tables and the full-resolution tables differ only in 0x30b3
(0x3098–0x309c = 02/19/02/01/00, 0x30b2=00, 0x30b4=03, 0x30b5=04 and 0x30b6=01 are the same).
The Linux value 419.2 MHz is exactly 19.2 MHz × 0x83 (131) / 6. Putting 0x70 (112) into the same formula gives 19.2 × 112 / 6 =
**358.4 MHz** (716.8 Mbps per lane), and the pixel rate for 2 lanes and 10 bits is 358.4 × 2 × 2 / 10 = 143.36 MHz (167.68 MHz at full
resolution). The premise that 0x30b3 is a linear multiplier is supported by the match for the value 0x83.

There is still no patch, for these reasons.

- PLL and analog values alone do not bring the mode to life. #2252, verified on a real SP7+, additionally needed whole-array binning with
  an ISP window offset (0x3800–0x3815), a Bayer phase chosen by the parity of the offset (Windows offset 3 = GRBG, the driver advertises
  BGGR), the vertical flip bit when binning, and a 30 fps limit. The last three come from hardware observation, not from the Windows table.
- ov5693 computes VTS and fps from `OV5693_PIXEL_RATE` (a value derived from the link frequency). The system PLL (0x3098–0x309c) is the
  same in both modes, so the tables do not settle whether `PIXEL_RATE` has to become 143.36 MHz in the binned mode.
- #2252 streamed on the SP7+ while reporting a link frequency of 419.2 MHz. Whether the link frequency value decides success or failure
  has not been confirmed.

What remains to be tried: port the ov5693 commit of #2252 on top of patch 0006 (leaving out the 0x4800 hunk, which overlaps with it) and,
together with it, change `LINK_FREQ` to 358.4 MHz in the binned mode (section 5 item 6).

## 5. Checks on Linux and their results

1. `dmesg | grep -iE 'int3472|ipu-bridge|ipu6|ov5693|ov13858|vcm'`: the three INT3472 probes, a "GPIO type 0x08 unknown" or `pwr1` line,
   "Unknown VCM type 17", IPU6 firmware authentication (`CSE authenticate_run done`; the kernel does not print the firmware file name
   when loading succeeds), binding of `OVTI5693` and `OVTID858`.
   Measured: three INT3472 probes, `GPIO type 0x08 … con_id=pwr1`, no "Unknown VCM type", `Instantiated dw9800k VCM`,
   `Connected 2 cameras`, the firmware authentication line is present.
2. `/sys/kernel/debug/gpio`, `/sys/kernel/debug/regulator/regulator_summary`, `/sys/kernel/debug/clk/clk_summary`:
   which sensor `INT3472:0x-avdd` and `-clk` are attached to, and the clk rate 19.2 MHz/24 MHz.
   Measured: `INT3472:00` = rear (avdd, pwr1, clk 19.2 MHz), `INT3472:02` = front (avdd, clk 19.2 MHz), `INT3472:01` = IR (avdd, dovdd,
   clk 24 MHz, no consumer).
3. `media-ctl -p`: the links OV5693→CSI2 port 2 (2 lanes) and OV13858→port 1 (4 lanes).
   Measured: `ov13858 2-0010` → `Intel IPU6 CSI2 1`, `ov5693 3-0036` → `Intel IPU6 CSI2 2`, both ENABLED and IMMUTABLE. This output does
   not show the lane count.
4. `cam -l`, `cam -c '\_SB_.PC00.I2C2.CAMR' -C30` (rear baseline), `cam -c '\_SB_.PC00.I2C3.CAMF' -C30` (front; expected to fail on
   unmodified 7.3).
   Measured: 2 cameras. Rear 1920x1080 30/30, 1280x720 120/120, 3840x2160 10/10, 0 buffer errors. For the front see difference 1.
5. If the front camera fails: read 0x4800 with `i2ctransfer` while streaming and check whether frames arrive after setting bit 5; then
   retest with a module carrying the 0x4800 patch.
   Measured: 0x20 (bit 5) while streaming. The kernel is patched, so the front camera did not fail.
6. Binned mode: capture with `cam -c … -s width=1280,height=720` and confirm the sensor mode from the `Input 1296x972` line in the log
   (for a `width=1296,height=972` request libcamera 0.7.2 SoftISP picks the sensor's 2592x1944 mode). Presence of `csi2-* error` lines;
   retest after porting #2252.
   Measured: fail (difference 2). The retest after porting #2252 was not done. On a kernel with patch 0020 the driver does not offer this
   mode, so this request goes to the 2592x1944 mode. Testing the binned mode again needs an `ov5693` module without patch 0020.
7. Rotation: the `camera_sensor_rotation` value and the actual image orientation (front and rear).
   See difference 7.
8. VCM: presence of the `focus_absolute` control with and without the ipu-bridge patch, and the change in focus.
   See difference 4. The state without the patch was not tested.
9. Privacy LED: which LED lights while each camera streams.
   See difference 11.
10. IR: the model ID at 0x60 with `i2ctransfer` (0x53354730 expected). It can be read only while the `INT3472:01` regulators are on.
    Measured: no response (difference 8).
11. The results are reflected as "works/fails" in this document and in the hardware status table
    ([hardware-status.md](../../hardware-status.md)).

### 5.1 Linux-side observations not covered by section 3

| Observation | Evidence | Open |
|---|---|---|
| Without patch 0021 the rear sensor and VCM stay powered when the camera is not in use | Measured: `i2c-OVTID858:00` was runtime active for 1006 of 1025 seconds of uptime, with avdd, pwr1 and the 19.2 MHz clock of `INT3472:00` on. The libcamera monitor of wireplumber keeps the lens subdevice open, `dw9719_open` takes a runtime PM reference, and ipu-bridge sets a PM link from the VCM to the sensor (code). The front camera has no VCM and is suspended when not in use. With patch 0021 (the VCM powers up on the first focus write, not on open): in the idle state with wireplumber holding the lens node open, both sensor and VCM are `suspended` (measured) | After one focus write both stay on until wireplumber closes the node. Image quality when focus is never written is unverified |
| CSI-2 errors are logged when a stream stops | Measured: for the front camera `csi2-2 error: Transfer FIFO overflow` and `Inter-frame long packet discarded` on every stream; for the rear, in 7 of 29 raw captures. The delivered frames have no errors and none are missing | Cause unverified |
| After power-up the VCM position and the control value differ | Measured (once): the VCM register read 512 with the control value at 0. The driver apparently does not write the current value after power-up (inferred) | Confirm that it reproduces |
| Auto exposure converges slowly (libcamera 0.7.2) | Measured: front mean 0.02 → 0.39 over 90 frames, rear 0.07 → 0.42 over 120 frames | Fixed with the patched libcamera (6.2) |

## 6. Defects in libcamera's software ISP, and the patches

A camera application (GNOME Snapshot, for one) gets its frames through PipeWire, WirePlumber's libcamera monitor and libcamera's simple
pipeline with the software ISP (SoftISP) and its GPU debayer. The measurements below were taken on that path. Raw frames were captured
straight from the IPU6 ISYS node with [`tools/linux/cam-raw.sh`](../../../tools/linux/cam-raw.sh) (libcamera is not involved); processed
frames with `cam`. The fixes are ten patches on libcamera 0.8.0 ([`userspace/libcamera/`](../../../userspace/libcamera/README.md)) and
the kernel patches 0025 and 0026. On NixOS the option `hardware.surface-pro-9.camera.libcamera` (on by default) rebuilds PipeWire and
WirePlumber against this libcamera ([`nix/libcamera.nix`](../../../nix/libcamera.nix), [nixos.md](../../nixos.md)).

Where the results stand: the patched libcamera was measured with `cam` and through PipeWire with a GStreamer pipeline, and after it
was installed through the NixOS option a person looked at both cameras in Snapshot: no purple lines, and the brightness regulates
normally (observed; [test-results.md](../../test-results.md) CAM-2, CAM-7). The response to a change of lighting was not measured.

### 6.1 Purple lines: the GPU debayer

| Measurement | Value |
|---|---|
| Magenta columns in a processed 1920x1080 frame (libcamera 0.7.2, GPU) | Rear 791 and 1271 (rows 311 and 791), front 731 and 1211 (row 731). Spacing 480. In those columns green is lower by 11–46/255 and red and blue are higher |
| The same frame through the CPU debayer (`LIBCAMERA_SOFTISP_MODE=cpu`) | No lines |
| Column and row means of the raw frames (front and rear, per Bayer position) | No column or row differs from its neighbours |
| libcamera 0.8.0 unpatched | Lines in the same columns |
| With patch 0002 (front and rear; 640x480, 1280x720, 1920x1080, 2560x1440) | No lines; no colour error in the two outermost rows either |

The cause is neither the sensor nor the IPU6 but the way the GPU debayer scales. It draws the whole input scaled to the output size and
reads the nearest texel; where the input position of an output pixel falls on a texel boundary, the centre pixel, its neighbours (their
distance is computed from `stride − 1` and `height − 1`, slightly too large) and the Bayer phase (`floor()`) refer to different pixels.
1920/2108 = 480/527, so such a column and row occur every 480 pixels: the period matches the measured positions. Only the vertical
lines stood out, presumably because there is a single horizontal one near the top (inferred). The same code had three more defects
(from the code, and measured): the scaled image is anchored in one corner, so an output of another aspect ratio is cropped on one side
only; it scales by skipping pixels, which gives staircases; and the statistics window is not the area the output shows but **the corner
of the input, of the size of the output** (6.2).

Patch 0002 debayers the centred area of the input that has the aspect ratio of the output pixel for pixel (texels are read at their
centres) and, when the output is smaller, scales in a second pass (bilinear down to 70 %, mipmaps below). The statistics come from the
same area. Debayer time at a 1280x720 output: 7.3 ms a frame for the front camera, 5.5 ms for the rear (measured; 0.7.2: 7.1–7.7 ms).

### 6.2 Exposure

| Measurement | Value |
|---|---|
| Raw mean against the front `analogue_gain` (in sixteenths) | Proportional from 16 (1x) to 992 (62x). From 993 it drops to the level of 512 (32x) and stays there up to 1023; 1024–2047 repeat lower gains; below 16 the sensor attenuates. The driver's range was 1–2047 |
| Rear `analogue_gain` (in 1/128) | Proportional from 0x80 (1x) to 0x7c0 (15.5x). 0x800 and 0x1000 equal 0x400, 0x1fff equals 0x7ff; 0x40 is brighter than 0x80. The driver's range was 0–0x1fff |
| Black level (10 bit) | Front 16, rear 64 |
| Control delay (written after the frame start; first frame that shows it; [`cam-ctrl-delay`](../../../tools/linux/cam-ctrl-delay.c)) | Exposure 2 and gain 2 for both sensors. libcamera's default assumption is exposure 2, gain 1 |
| First frame after start | The front camera delivers a black one (plain black level) about one time in three |
| Lighting | At 5 ms exposure the frame mean swings between 157 and 193 (flicker of 60 Hz mains) |
| libcamera 0.7.2, front, 640x480 | Settles at 33 ms × 10.4x with a mean output green of 220/255 (blown out). Same scene with the patches: 33 ms × 2.4x, mean 110–116 |
| libcamera 0.7.2, rear, dim scene | The gain climbs to 8191 and the mean swings between 86 and 116 |
| libcamera 0.8.0 unpatched, front, at start | The gain flips 64 → 1 → 32 → 128 → 16 → 64 → 8 → … for 40 frames |
| With the patches, front | Starting from the previous session's values: settled from the first result (frame 5), standard deviation 0.03–1.7 afterwards. Starting dark (exposure 300 lines, 1x): settled at frame 7. Starting bright (62x): settled at frames 19–23 |
| With the patches, rear | Settled at frame 7, standard deviation 0.1–0.6 afterwards |

Several causes overlapped:

- **The metering area was wrong** (GPU path of 0.7.2 and 0.8.0). The statistics window is the top-left corner of the input, of the size
  of the output: with a 640x480 output the exposure is set from the top-left 6 % of 2592x1944. When that corner is dark the image is
  blown out. Patch 0002.
- **Gain ranges the sensors do not follow.** Auto exposure believes the gain reaches 127.9x (front) and 64x (rear) and raises it there,
  where the image gets darker again or does not change. Kernel patches 0025 and 0026 make the ranges the real ones; the tuning files
  carry `minAnalogueGain` and `maxAnalogueGain` as well (patches 0004 and 0010), which takes effect on a kernel without them too.
- **The assumed delay.** With a gain delay of 1 the statistics are attributed to values one frame ahead. Patch 0003 (2 and 2).
- **Loop delay.** The result for statistics frame N went out one frame late, missed frame N+4 and was evaluated only at N+8, so the
  regulation fell apart into two loops taking turns. Patch 0005.
- **The brightness estimate of dark frames.** The gain was applied to the lower edge of each histogram bin, so the first bin always
  stayed 0: a frame that needed 17x got 1434x (measured). Patch 0005.
- **The black first frame** sent the exposure to its maximum. Patch 0005 ignores the statistics of the first frame and leaves the
  sensor values alone until the first result.
- **Flicker.** The exposure time had no relation to the mains period, so the brightness swung and bands moved through the image.
  Patch 0004: once the exposure reaches the flicker period (8.33 ms at 60 Hz) it stays at a multiple of it and the gain makes up the
  rest. The NixOS option `camera.mainsFrequency` selects 50 or 60. In scenes bright enough to need less than one period the flicker
  remains.
- libcamera 0.7.2 also had a smallest gain step of 1/100 of the range (1.28x for the front camera; from the code). 0.8.0 does not.

libcamera 0.8.0 prefers the CSI-2 packed formats on IPU6. On this device every packed buffer arrives with the error flag and no frame
comes out (measured: also when captured directly with `v4l2-ctl`, front and rear, with CSI-2 receiver errors in the kernel log).
Patch 0001 reverts that preference. Why packed capture fails was not investigated.

### 6.3 Rear focus

| Measurement | Value |
|---|---|
| Simple pipeline of libcamera 0.7.2 and 0.8.0 | Finds the lens (`focusLens`) and does not use it. There is no focus control (from the code) |
| `cam` with the patches | `Lens range 400 (far) to 800 (near, 10 dioptres)`; at start it steps from 400 to 800 in steps of 50 (9 positions, 4 frames each), and the metadata carry `AfState`, `LensPosition` and `FocusFoM`. Afterwards the VCM registers hold the last position written (440 = `01 b8`) |
| Device standing on a desk (the rear camera faces a nearby surface) | The contrast varies by less than a factor of 1.2 over the range, so the scan ends as "nothing to focus on" and the lens rests at the default position (1 dioptre = 440). A raw sweep (350–850, steps of 25, up and down) has no maximum either |
| Device held by a person towards a distant scene (a row of ceiling lights) | The contrast of the raw sweep is the same 0.00117 from 350 to 550 and falls above that, to 0.00048 at 850. Even in the sharpest frame a light is a diffuse blob about a tenth of the image width across, with a halo (Tenengrad 0.0037; the front camera looking at the same room gives 0.029–0.070) |
| An object held about 30 cm in front of the camera | The contrast is flat over the whole range, 0.00054–0.00059. The object is a blob without contours |
| Autofocus on those two scenes (three runs each) | "Nothing to focus on" in 4 of 6 runs; in the 2 runs in which the scene moved it chose an end position |
| Reference frame taken on Windows (unattended) | Diffuse in the same way. What the camera was facing is not known |

Implementation (patches 0007–0009): the SoftISP statistics gain a focus measure, the green contrast of the centre half of the image
(squared differences of neighbouring green pairs, divided by the squared brightness). The `Af` algorithm of the IPA scans from the far
end in coarse steps, stops once the contrast has fallen below 60 % of its maximum for two steps, scans the neighbourhood of the maximum
in fine steps and rests at the position a parabola through the samples gives. Continuous mode (the default) scans once at start and
again when the contrast differs from its reference by more than 35 % for four statistics frames in a row. Applications see `AfMode`
(manual, auto, continuous), `AfTrigger` and `LensPosition` (dioptres). PipeWire's libcamera plugin selects continuous mode when
`AfMode` exists (from the code).

**The rear image does not get sharp at any lens position (measured).** With the distant scene the contrast is highest at the low values
(350–550) and falls as the value rises; facing a nearby surface it rises slightly towards 800. That agrees with the direction of
`lensRange: [400, 800]`, low values being far. But at the best position a distant light is still a large blob. A blur of that size
would take a lens almost 1 mm away from its infinity position (estimated with a focal length of 3.5 mm at F2), more than the VCM
travels, and the blob has a diffuse rim instead of the edge of a defocus disc. So the blur is not a matter of the focus position.
**An opaque protective film covers the rear camera of the test unit (observed).** Until it is removed and the measurement is repeated
with [`tools/linux/cam-af-check.sh`](../../../tools/linux/cam-af-check.sh), focus cannot be judged, and the dioptre scale of the lens
range is not settled either. The focus range Windows Media Foundation reports, 450–700, lies inside the range used here.

Lens power: libcamera opens the lens node when it enumerates the cameras, and after one focus write the VCM and the rear sensor stay
powered until the node is closed (the consequence of patch 0021; measured: with the WirePlumber of 0.7.2 holding the node,
`i2c-OVTID858:00` stays `active` after a focus write). Autofocus writes the focus in every stream, so the sensor would stay on for the
whole session. Patch 0008 closes the lens node while the camera is not streaming. With a WirePlumber built against this libcamera no
process holds the lens node when idle, and the rear sensor is `suspended` again within 3 s of the end of a stream (measured).

### 6.4 Other measurements

| Observation | Evidence | Open |
|---|---|---|
| The CSI-2 packed output of the IPU6 ISYS does not work on this device | Measured: `pBAA` and `pgAA`, 20/20 buffers with the error flag, `csi2-N error` in the kernel log | Cause on the kernel side unverified. libcamera avoids it with patch 0001 |
| Frame rate | Measured: front 28.64 fps (the sensor's default vertical blanking), rear 29.95 fps. Debayer 5.5–7.3 ms a frame | — |
| Through PipeWire | Measured with a WirePlumber built against the patched libcamera, started in place of the session's: the nodes `Built-in Front Camera` and `Built-in Back Camera` appear, and GStreamer `pipewiresrc` received 75/75 frames at 1280x720 and at 1920x1080 from each; no lines, no change of brightness after the first result | — |
| Colour | `Awb` is grey-world and there is no colour correction matrix (the tuning files have none). Saturation is low | A CCM and lens shading correction need shots of a colour chart |
| Noise at high gain | SoftISP has no noise reduction. In dim light the gain goes up to 62x (front) and 15.5x (rear) | — |
| Rear image orientation | Unverified (CAM-1) | A person |
| Privacy LED | Unverified (CAM-5) | A person |
