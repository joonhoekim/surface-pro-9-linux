# SAM and platform devices: gaps between the Windows baseline and linux-surface

This covers the Surface Aggregator Module (SAM, the SSH protocol over UART), the functions on top of it (battery and AC, temperature sensors, fan, performance mode, tablet mode,
SAM HID), and the ACPI platform devices that depend on SAM (`MSHW0084`, `MSHW0343`, `MSHW0153`, `MSHW0187`/`MSHW0188`, `MSHW0299`,
`MSHW0040`, `MSHW0005`, `ACPI000E`, `PNP0C0B`, the lid GPE). The Windows side is the PnP tree, ACPI tables, driver store and event logs read on this device;
the Linux side is linux-surface `patches/7.3` and its base v7.3.4.

Notation:

- ACPI objects are named by table and path. "Surface SSDT" is the SSDT that defines `\_SB.SSH` and `\_SB.WSID` (OEM table ID `SSDTSURF`).
  The tables are not in this repository. Dump and decompile them on your own device with [`tools/windows/dump-acpi.ps1`](../../../tools/windows/dump-acpi.ps1), or take them from
  <https://github.com/linux-surface/acpidumps> (directory `surface_pro_9_intel`).
- `L/path:N` = file and line in the Linux v7.3.4 source tree. `LS/0007:N` = line in linux-surface `patches/7.3/0007-surface-sam.patch`.
- A Windows driver file named as evidence (for example `SurfaceSmfClient.sys`) is the file in the driver store (`C:\Windows\System32\DriverStore\FileRepository\`) that the fact was taken from.
- `pnp-devices.csv` = the Windows PnP device inventory of this device (`docs/reference/`).
- `EVT` = output of [`tools/windows/sam-events.ps1`](../../../tools/windows/sam-events.ps1) (the SAM event registrations and other entries the SSH driver leaves in the event log).
- `HDR` = the metadata enums of `SurfaceApp\SurfaceCommonHeadersDotNet.dll` in the Surface app (`Microsoft.SurfaceHub` 76.10212.108.0).
  These are Microsoft's names for the SAM target categories and command IDs, taken from the constant table of the .NET metadata.
- SAM device names are written in the Linux form `ssam:dd:cc:tt:ii:ff` (domain:category:target:instance:function). TC is the target category, TID the
  target (1=SAM, 2=KIP), IID the instance, CID the command. The `ssam:` prefix exists only in the modalias and in software node names.
  The sysfs device names (`/sys/bus/surface_aggregator/devices/`) have no prefix, as in `01:02:01:01:00` (measured).
- Results measured on Linux 7.3.4 are in the "On Linux" column of section 3 and in section 4. A value given without a patch range is from the kernel with patches 0001–0017.
- Patch numbers: "patch 00NN" always means this repository's kernel patch (`kernel/patches`, 0001–0023). A linux-surface patch is always named by its file in
  `patches/7.3` (`0007-surface-sam.patch`, `0009-surface-button.patch`, `0011-surface-shutdown.patch`, `0012-surface-gpe.patch`); the two series have overlapping numbers.
- `#N` = issue or pull request N of linux-surface/linux-surface; those of the kernel tree are written `linux-surface/kernel#N`.

## Summary of conclusions

- **The SP9 SAM node set matches Windows.** Every SAM function for which Windows actually enables events (BAT/0, FAN/1, KIP/0, POS/0, HID/1·2·3·5 (KIP),
  HID/6·7 (SAM)) has a corresponding node in the Linux `ssam_node_group_sp9`. No ID error was found.
- **linux-surface has no USB-C UCSI.** The UCSI PPM is exposed only as SAM HID IID 7 (`HID\VID_045E&UP:FF00_U:F900`); there is no ACPI `PNP0CA0`/`USBC000`.
  Linux creates this HID device but has no UCSI transport driver for it, so `/sys/class/typec` appears to stay empty. A driver is posted as linux-surface/kernel#184 (not merged).
  On a kernel that carries it as this repository's patches 0007–0008, `typec` port0 and port1 and the partner appear (measured).
- **The minimum fan RPM persists.** The Windows SMF fan client sets a floor with FAN `SetMinFanRpm` (CID 0x0b) (serial trace in #1726), and Linux does not
  clear it. This is the cause given in #1726 for its symptom (another user's SP9), where the fan sits at ~3800 RPM after rebooting from Windows; there a single `0x0b 0` clears it.
  Patch 0013 sends this request. That the request does not fail is confirmed; the effect of clearing the floor is unverified.
- **Host-side thermal management (SMF) is missing entirely.** Windows receives the 11 SAM temperature sensors (RTS1–8, VTS1–3) as ACPI `MSHW0188` thermal zones, and
  the SMF core keeps changing the CPU power limits (8 channels in the event log, range 6000–130000). Linux only reads temperatures through `surface_temp`, and
  the ACPI thermal zones are not created because they have no `_TMP` (measured: the thermal zones are `x86_pkg_temp` and `iwlwifi_1` only).
  RTS1–8 and VTS1–3 are the names in ACPI `_STR`. The Linux hwmon is named `surface_thermal` (the driver is `surface_temp`), and the labels of its 11 channels are
  `I_RTS1`–`I_RTS8` followed by `I_RTS1`, `I_RTS2`, `I_RTS3` once more (measured). There is no `VTS` name.
- **The SAM RTC and wake alarm are not connected.** For Windows `ACPI000E` (`_GCP`=0), a filter driver implements all TAD methods through SAM.
  On Linux `acpi_tad` binds with 0 capabilities, and the linux-surface `rtc-surface` driver exists but is never loaded because no node group contains `ssam:01:01:01:00:00`.
  On a kernel where patch 0012 adds the node, `rtc1` (surface_rtc) appears and its time agrees with system UTC (measured). There is no alarm.
- Of the SAM events Windows enables, Linux does not enable TMP/0, FAN/1, PMC(=Power)/0, SPT/0 and REG/0. The impact is unverified.

## 1. Windows baseline

### 1.1 Device tree

Read with `Get-PnpDevice` + `DEVPKEY_Device_Parent`/`BiosDeviceName`.

| ACPI path | HID | Windows driver (version) | Children |
|---|---|---|---|
| `\_SB.SSH` | `MSHW0084` | SurfaceSerialHubDriver 9.166.37.0 | None (SSH transport only) |
| `\_SB.WSID` | `MSHW0343` (`_HID` is `ToString(SIDH)`) | SurfaceIntegrationDriver 93.52.12.0 | All SID_DEVICEs below |
| └ `\_SB.WSID.SHPS` (`_ADR 0x9086E8DA`) | `{9086E8DA-…}\Target_SAM&Category_HostRegister` | SurfaceHotPlug 3.206.1.0 ("Surface Hot Plug - SAM") | SAM HID IID 6, 7 |
| └ | `{BFD21D0B-…}\SurfaceBattery` | SurfaceBattery 2.117.139.0 | |
| └ | `{16C280BF-…}\SurfaceSystemManagementFramework` | SMF Core 3.180.139.0 | |
| └ | `{C65C8174-…}\SurfaceSmfThermalClient`, `…SystemClient`, `…FanClient` | SurfaceSmfClient 8.127.139.0 | |
| └ | `{80499AFE-…}\SurfaceSmfAlderlakeClient` | SurfaceIhvCpuSmfClient 1.174.34.0 | |
| └ | `{A1B1D6F0-…}\SurfaceSmfDisplayClient`, `{2683FA3B-…}\SurfaceSmfSensorClient` | 9.32.137.0, 1.9.139.0 | |
| └ | `{4047F442-…}\SurfacePowerStateDriver`, `{D69C83F8-…}\SurfacePowerTrackerCore` | 3.19.139.0, 2.149.19.0 | |
| └ | `{98DE32A9-…}\SurfaceSarManager` ("Surface Radio Monitor"), telemetry, ML service, Intel ICPS | | |
| `\_SB.SHPS` | `MSHW0153` | SurfaceHotPlug ("Surface Hot Plug - KIP") | KIP HID IID 1, 2, 3, 5 |
| `\TPOL` (ThermalZone in the root scope) | `MSHW0187` | SurfaceThermalPolicy 8.85.139.0 | |
| `\_SB.TZ01`–`\_SB.TZ0B` | `MSHW0188` x11 | SurfaceSmfClient ("Thermal Zone Sensor") | |
| `\_SB.SAPD` | `MSHW0299` | SurfaceAcpiPlatformExtensionDriver 3.40.137.0 (PEP) | |
| `\_SB.FAN0` | `PNP0C0B`, `_STR "SAM FAN"` | Inbox ACPI Fan | |
| `\_SB.MSBT` | `MSHW0040` | SurfaceButton 4.64.139.0 (lower filter, VHF) | 2 VHF keyboards |
| `\_SB.SLT` | `MSHW0005`, `_CID PNP0C60` | Inbox msgpiowin32 | |
| `\_SB.SRTC` | `ACPI000E`, `_SUB MSHW0217` | acpitime + SurfaceTimeAlarmAcpiFilter (lower filter) | |
| `\_SB.LID0` | `PNP0C0D` | Inbox ACPI Lid | |

On Windows the bus that enumerates the SAM clients is `MSHW0343` (Surface Integration). Linux uses the same HID as its platform hub (section 2).

### 1.2 SAM event registrations (EVT)

Collected from the "register events succeeded" entries the SSH driver logs at every driver start. All 9 starts left in the log show the same 15 registrations.

| TID | TC | IID | Registration CID | Registration path | User on the Windows side (including inferred) |
|---|---|---|---|---|---|
| 1 | BAT(0x02) | 0 | 0x0b | SAM registry (HDR `Sam/RegisterEvent=0xb`) | SurfaceBattery |
| 1 | TMP(0x03) | 0 | 0x0b | SAM | SMF (trip-point notification, HDR `Thermal/SmfTripPointCrossedNotify=0xb`, inferred) |
| 1 | PMC(0x04, HDR name `Power`) | 0 | 0x0b | SAM | Unverified |
| 1 | FAN(0x05) | 1 | 0x0b | SAM | SMF fan client (HDR `Fan/NotifyRpmTrippointCrossed=0xa`, inferred) |
| 1 | KIP(0x0e) | 0 | 0x0b | SAM | HotPlug (cover attach) |
| 1 | SPT(0x22) | 0 | 0x0b | SAM | Power Tracker (inferred) |
| 1 | POS(0x26) | 0 | 0x0b | SAM | Integration driver, its posture-over-SSH module (`SurfaceIntegrationDriver.sys`) |
| 1 | REG(0x21) | 0 | 0x01 | REG (HDR `HostRegister/CmdRegistration=0x1`) | HotPlug - SAM (inferred) |
| 1 | HID(0x15) | 6, 7 | 0x01 | REG | SurfaceHidMiniDriver |
| 2 | REG(0x21) | 0 | 0x01 | REG | HotPlug - KIP (inferred) |
| 2 | HID(0x15) | 1, 2, 3, 5 | 0x01 | REG | SurfaceHidMiniDriver |

The CommandID in the log is not the event CID but the registration command (SAM 0x0b, REG 0x01). These are the same values as Linux `SSAM_EVENT_REGISTRY_SAM`/`_REG`.

### 1.3 SAM HID instances

| Path | IID | HID VID:PID | Top-level collections → Windows driver |
|---|---|---|---|
| SAM (TID 1) | 6 | 045E:005D | `0020:0001` → "HID Sensor Collection V2" (inbox sensor class). Through WinRT, Accelerometer, Gyrometer, LightSensor and Orientation attach to this device (Compass/Inclinometer are the same fusion device). There is no Magnetometer or Barometer |
| SAM (TID 1) | 7 | 045E:005D | `FF00:F900` → "Surface UCM UCSI HID Client" (`SurfaceUcmUcsiHidClient.sys`, service dependency `UcmUcsiCx`). The INF matches `HID\VID_045E&UP:FF00_U:F900` ("Any UCSI PPM HID") |
| KIP (TID 2) | 1 | 045E:09AE | Keyboard, System Control, 2 Consumer Controls, `0001:0013` |
| KIP | 2 | 045E:09B0 | Keyboard, pen BLE OOB bonding (`FFF4:000A`), pen BLE LC (`FFF4:0001`) |
| KIP | 3 | 045E:09AF | Mouse, touchpad, Digitizer configuration, Navigation (`FF07`), USP (`FF05:0004`) |
| KIP | 5 | 045E:0991 | Surface HID Color (`FF05:0001`), Type Cover v7 firmware update (`FF0B:0101`), USP |

The N in the instance-ID tail `SHP_DEVICE_0000000N_…` is the IID. In ACPI the `_DSM` (5515a847…, fn 6) of the SAM-side HostRegister node returns `0x060C` and
that of the KIP-side `MSHW0153` returns `0x080E`. The meaning of the bits was not determined.

### 1.4 ACPI platform device configuration

| Device | Value | Evidence |
|---|---|---|
| SSH `_CRS` | `UartSerialBusV2` → `\_SB.PC00.UA00`, 8N1, RTS/CTS flow control, buffers 0x40/0x40. The speed is filled in by `_INI` from `PSBR`=0x3D0900 (4 Mbps). `GpioInt(Edge, ActiveHigh, SharedAndWake)` pin 0xAE + a `GpioIo` on the same pin. `_DEP` = UA00. `_S0W` = 3 | Surface SSDT, `\_SB.SSH` (`_CRS`, `_INI`, `_DEP`, `_S0W`) |
| SSH `_DSM` d5e383e1…, rev 0 | Supported bits `E1 0F` (fn 0, 5–11). fn5 power profile=`SSHP` (MNVS), fn6 UART idle with screen on 200 ms, fn7 with screen off 150 ms, fn8 close handle in D3=0, fn9 buffer 0x30, fn0A "SSH driver Interface"=2, fn0B high-resolution timer=1 | Surface SSDT, `\_SB.SSH._DSM` |
| `MSHW0343` `_DSM` a653cdf4… | Supported 0x49 (fn 0, 3, 6). fn3 `OMPR`, fn6 `BCLE` (whether the battery charge limit is in use; determined by `\_SB._OSC` CAP0 bit19 and `SCAP` bit 0x400) | Surface SSDT, `\_SB.WSID._DSM` and the method `OBCL`; DSDT, `\_SB._OSC`, which calls `OBCL` |
| `MSHW0040` | `GpioInt` pins 0x03 (power, SharedAndWake), 0x143, 0x147 (volume, ActiveBoth). `_DSM` 6fd05c69… rev1 fn2 `OMPR`, fn3 `SBFF` | Surface SSDT, `\_SB.MSBT` |
| `MSHW0005` | Only `_HID`, `_CID PNP0C60` and `_STA`. No GPIO resource | Surface SSDT, `\_SB.SLT` |
| `MSHW0187` (TPOL) | `OPTS` = `"\_SB.PAGD"`. No other thermal method | Surface SSDT, `\TPOL` |
| `MSHW0188` TZ01–TZ0C | Created conditionally on the bits of `TZSM`. `_STR` RTS1–RTS8, VTS1–VTS4. `_DSM` e484ea9d… fn1 = sensor number 1–0x0C, fn2 = 1. No `_TMP`, `_PSV` or `_CRT`. Windows has the 11 zones TZ01–TZ0B, so `TZSM`=0x7FF (no VTS4) | Surface SSDT, `\_SB.TZ01`–`\_SB.TZ0C`; PnP `ACPI\MSHW0188\1`–`\B` |
| `MSHW0299` (SAPD) | Only `_HID` | Surface SSDT, `\_SB.SAPD` |
| `PNP0C0B` (FAN0) | Only `_STR "SAM FAN"`, `_DEP` SSH and `_STA`. No `_FIF`/`_FPS`/`_FSL`/`_FST`/`_PR0` | Surface SSDT, `\_SB.FAN0` |
| `ACPI000E` (SRTC) | `_DEP` SSH, `_GCP` = 0. No `_GRT`/`_SRT`/`_PRW` | Surface SSDT, `\_SB.SRTC` |
| `LID0` | `_LID` reads GPIO pad 0x090C0012. No `_PRW`. The lid GPE is `_L52` (both edges detected by inverting the polarity) | DSDT, `\_SB.LID0` and `\_GPE._L52` |
| CMOS RTC | No `PNP0B00` in the DSDT. FADT "CMOS RTC Not Present" = 0, "RTC wake not in fixed reg space" = 1 | FADT boot flags |

### 1.5 Host functions Windows builds on top of SAM

| Function | Description | Evidence |
|---|---|---|
| ACPI fan emulation | The `MSHW0299` driver is a PEP (Platform Extension Plug-in). It injects `_FST` into `\_SB.FAN0` and takes the value from SAM. There is no `_FIF`/`_FPS`/`_FSL`, so Windows does not control the fan through the ACPI path either | `SurfaceAcpiPlatformExtensionDriver.sys` |
| Time and alarm | `SurfaceTimeAlarmAcpiFilter.sys`, a filter below `ACPI000E`, intercepts `_GCP _GRT _SRT _STV _TIV _GWS _CWS _STP _TIP` and talks to SAM over SSH. HDR `Sam/`: `SetUtcTime=0xf`, `GetUtcTime=0x10`, `SetWakeAlarm=0x1a`, `GetWakeAlarmStatus=0x1b`, `ClearWakeAlarmStatus=0x1c`, `GetTimerValues=0x1d`, `GetTimeAndAlarmCapabilities=0x1e`, `GetRealTime=0x1f`, `SetRealTime=0x20` | `SurfaceTimeAlarmAcpiFilter.sys` |
| SMF thermal management | SMF core + thermal, fan and system clients + CPU (Alder Lake), display and sensor clients. The thermal client reads the SAM temperature for each `MSHW0188` zone. It sets the VTS offset in SAM (HDR `Thermal/SetTempSensorOffset=0x24`). On this device every boot logs "VTS offset configuration failed. Instance 9 Error 0xC0000010" (VTS1) | `SurfaceSmfClient.sys`, EVT |
| CPU power limits | The CPU SMF client keeps updating 8 channels. Ranges over the latest 500 entries: ch0 6000–28000, ch1 6000–46000, ch2 101000–130000, ch3 45000–100000, ch4 45000–118000, ch7 65000–130000, ch9 8000–28000, ch17 0–1. The unit appears to be mW and the channels are inferred to be of the PL1/PL2 family. The driver distinguishes three power-limit types: 0 = CAP, 1 = BAP, 2 = BWD. This device has no Intel DTT/DPTF device | `SurfaceIhvCpuSmfClient.sys`, EVT, `pnp-devices.csv` |
| Fan floor | The SMF fan client sends FAN `SetMinFanRpm` (0x0b) (another user's SP9 serial trace, linux-surface#1726). HDR `Fan/`: `GetCurrentSpeed=0x1`, `SetRpmTrippoint=0x8`, `NotifyRpmTrippointCrossed=0xa`, `SetMinFanRpm=0xb`, `SetFanProfile=0xe`, `SetFanMsRequest=0xf`, `GetAllFanAcoustics=0x17` | #1726, HDR |
| Screen-off notification | The SSH driver sends a SAM sleep command when the screen turns off. The HID minidriver and HotPlug also send commands to SAM from their display-state callbacks | `SurfaceSerialHubDriver.sys`, `SurfaceHidMiniDriver.sys` |
| Battery extras | Charge-limit (BCL) state, adapter rating (`Battery/GetPsuAdapterRating=0xf`), user charging mode (`0x9`/`0xa`), discharge-rate EWMA (`0x1a`) | `SurfaceBattery.sys`, HDR |
| Posture | The Integration driver updates `ConvertibleSlateMode` from POS events. `MSHW0005` has no GPIO and is therefore not a direct input source (inferred) | `SurfaceIntegrationDriver.sys` |

For reference, HDR `StandardTypes/TargetCategory` has the same numbers as Linux `SSAM_SSH_TC_*`. Only the names differ (0x04 Power=PMC, 0x21 HostReg=REG, 0x26 Posture=POS).
`Thermal/` is `GetTemperature=0x1`, `GetActiveStatus=0x2`, `SetThermalMode=0x3`, `GetActiveSensors=0x4`, `GetSensorInstanceDescriptor=0xe`, which
agrees with the values used by Linux `surface_temp` (0x01, 0x04, 0x0e) and `surface_platform_profile` (0x02, 0x03).

## 2. State of linux-surface and upstream

### 2.1 SP9 node group

`MSHW0343` → `ssam_node_group_sp9` (L/drivers/platform/surface/surface_aggregator_registry.c:455, group at :373–389).
SP10/SP11/SP12 (Intel) use the same group (LS/0007:244).

| Node (definition line) | Name | Linux driver | Windows counterpart |
|---|---|---|---|
| `ssam_node_root`(:38) | `ssam_platform_hub` | registry | `MSHW0343` |
| `ssam_node_hub_kip`(:43) | `ssam:00:00:01:0e:00` | surface_aggregator_hub (KIP events, TC 0x0e) | KIP/0 events |
| `ssam_node_bat_ac`(:55) | `ssam:01:02:01:01:01` | surface_charger (CID 0x01, 0x0d) | SurfaceBattery (BAT/0) |
| `ssam_node_bat_main`(:61) | `ssam:01:02:01:01:00` | surface_battery (CID 0x01–0x04) | Same as above |
| `ssam_node_tmp_perf_profile_with_fan`(:86) | `ssam:01:03:01:00:01` + `has_fan` | surface_platform_profile (TMP 0x02/0x03, FAN 0x0e; with linux-surface `0007-surface-sam.patch` also FAN 0x0f) | The slider (the calling component on Windows was not identified) |
| `ssam_node_tmp_sensors`(:93) | `ssam:01:03:01:00:02` | hwmon surface_temp (TMP 0x04, 0x01, 0x0e; up to 16 sensors) | `MSHW0188` x11 |
| `ssam_node_fan_speed`(:99) | `ssam:01:05:01:01:01` | hwmon surface_fan (FAN 0x01, read-only) | `FAN0` `_FST` (PEP) |
| `ssam_node_pos_tablet_switch`(:219) | `ssam:01:26:01:00:01` | surface_aggregator_tabletsw (POS event CID 0x03) | POS/0 events |
| `ssam_node_hid_kip_{keyboard,penstash,touchpad,fwupd}`(:195–218) | `ssam:01:15:02:0{1,2,3,5}:00` | surface_hid (REG registry) | KIP HID IID 1, 2, 3, 5 |
| `ssam_node_hid_sam_sensors`(:135) | `ssam:01:15:01:06:00` | surface_hid → hid-sensor-hub | SAM HID IID 6 |
| `ssam_node_hid_sam_ucm_ucsi`(:141) | `ssam:01:15:01:07:00` | surface_hid → hid-generic only | SAM HID IID 7 (UCSI) |

Nodes linux-surface does not have: SAM/SAM (`ssam:01:01:01:00:00`, rtc-surface; this repository's patch 0012 adds it), PMC, SPT, and drivers for ACPI `MSHW0187`/`MSHW0188`/`MSHW0299`.

### 2.2 Other drivers and patches

The patches named in this table are all linux-surface's (`patches/7.3`), not this repository's.

| Item | Linux | Evidence |
|---|---|---|
| SSH | `MSHW0084` serdev. Reads speed and flow control from `_CRS`. Reads only `_DSM` fn5–9 and ignores fn0A/0B. The wake IRQ handler does not implement draining SAM events (the GPIO callback); there is a TODO comment | L/drivers/platform/surface/aggregator/core.c:311–335, controller.c:1013–1160, controller.c:2672–2700 |
| Wake GPIO deferral | Switched to `gpiod_get_optional` and propagates `-EPROBE_DEFER` (fixes SAM not loading, reproduced on SP9) | linux-surface `0007-surface-sam.patch`, commit "propagate -EPROBE_DEFER" |
| Fan during sleep | On suspend entry FAN 0x0f (`SetFanMsRequest`)=2 (standby), on resume 1 | linux-surface `0007-surface-sam.patch`, commit "platform_profile: add fan power state" (LS/0007:271–343) |
| RTC | `rtc-surface` (SAM 0x0f/0x10, read and write only, no alarm). It matches `SSAM_SDEV(SAM, SAM, 0x00, 0x00)`, but the registry has no such node | LS/0007:73–176 |
| Buttons | `soc_button_array` `MSHW0040` (GPIO indices 0, 2, 4), checks the OMPR `_DSM`. linux-surface `0009-surface-button.patch` makes it defer when probed before the GPIO controller | L/drivers/input/misc/soc_button_array.c:553–613, linux-surface `0009-surface-button.patch` |
| Lid GPE | `surface_gpe` gets SP9 (`DMI_PRODUCT_SKU "Surface_Pro_9_2038"`) → GPE 0x52 | linux-surface `0012-surface-gpe.patch` |
| Shutdown | linux-surface `0011-surface-shutdown.patch`: with DMI `Surface Pro 9`, the shutdown callbacks of PCI 461e, 461f, 462f, 466d and 46a8 are skipped. All 5 are present on this device | linux-surface `0011-surface-shutdown.patch`, `pnp-devices.csv` |
| `MSHW0153` | `surface_hotplug` rejects a `MSHW0153` without GPIOs with `-ENODEV` (it is for the SB2/3 dGPU) | L/drivers/platform/surface/surface_hotplug.c:206–212 |
| ACPI thermal zones | `MSHW0188` and `TPOL` are ThermalZone objects, so they are enumerated as LNXTHERM, and `acpi_thermal_add` fails without `_TMP` | L/drivers/acpi/thermal.c:827–830 |
| ACPI fan | `FAN0` has no ACPI4 methods and binds through the ACPI 1.0 path (power state on/off) | L/drivers/acpi/fan_core.c:557–562 |
| `ACPI000E` | `acpi_tad` binds without capabilities: `_GCP`=0, no `_PRW` | L/drivers/acpi/acpi_tad.c:842–853 |

## 3. Gaps

Confidence: **confirmed** = confirmed by Windows measurement or code, **inferred** = there is a basis but it is unverified on Linux.

In the "On Linux" column, "Check" is the command or procedure and "Measured" is what it gave on this device.

| # | Gap | Evidence | Impact | How to close it | Confidence | On Linux |
|---|---|---|---|---|---|---|
| G1 | No driver for UCSI over SAM HID (IID 7) | 1.3, "Surface UCM UCSI HID Client" in `pnp-devices.csv`; no `PNP0CA0`/`USBC000` in the DSDT; no HID transport in upstream `drivers/usb/typec/ucsi/` | Role, PD contract, partner information and DP alt mode state of the 2 USB-C ports do not appear in the typec class. Charging and data themselves are handled by firmware and are likely to work (inferred) | Apply linux-surface/kernel#184 (UCSI-over-HID driver, not merged) and test. The node already exists | Confirmed (absent) | Check: `ls /sys/class/typec`; `FF00:F900` in the report descriptor of `/sys/bus/hid/devices/*045E:005D*`; 2 `typec` ports with #184 applied. Measured (patches 0007–0008): `typec` port0 and port1, `port0-partner` (PD), 2 `ucsi-source-psy`, IID 7 descriptor head `06 00 ff 0a 00 f9`. Each boot logs one line `GET_CABLE_PROPERTY failed (-95)`: SAM answers that it does not support this command, which is expected behaviour (there is no `port0-cable` node). Without patch 0019 the port's own PDO values come out wrong (sink 1 = 3750 mV 120 mA and so on). Raw bytes read through hidraw: SAM puts a length byte in front of the port PDOs and writes at most 3 partner PDOs per command. With patch 0019 (measured): the source of both ports is a single 5 V 3 A, the sink is 5 V 3 A fixed, 5–20 V 60 W battery and 5–20 V 3 A variable, and the charger source is the three 9/12/15 V 3 A. Not fixed: the partner list starts at the second PDO, so it has no 5 V entry, and `voltage_now`=0 |
| G2 | The minimum fan RPM (`SetMinFanRpm` 0x0b) is not cleared | #1726 (SP9 serial trace; cleared with `ctrl.py request 0x05 0x1 0x0b 0x01 0 0x00 0x00`), HDR `Fan/SetMinFanRpm=0xb`, `GetAllFanAcoustics=0x17` (`OverridenMinFanRpm`) | After rebooting from Windows the fan runs at ~3800 RPM even when cool | In the `surface_platform_profile` probe (and resume), if `has_fan`, send `__le16 0` to FAN TID1 IID1 CID 0x0b. Read the remaining floor with 0x17 and log it | Confirmed on another SP9 (#1726); not reproduced on this device | Check: the surface_fan RPM in `sensors` right after a Windows→Linux reboot; read FAN 0x17 through `surface_aggregator_cdev`. Measured (patch 0013): `fan1_input`=0 RPM in all 12 idle samples, no warning from the patch (`failed to clear the minimum fan speed`). No Windows round trip was made, so the effect of clearing the floor is unverified (SAM-4). FAN 0x17 has not been read. Measured through cdev ([power-thermal.md](../control/power-thermal.md) 1.4, the Linux measurements): 0x0b with a `u16` rpm sets the speed of a stopped fan (requests of 1500–7500 give 1588–7407 rpm, 0 gives 0 rpm within a few seconds) and is ignored while SAM is driving the fan itself. 0x0c does nothing, whether given a `float32` or a `u16`. Under load the fan read 4745–5960 rpm |
| G3 | No host thermal management (SMF) | The SMF and CPU power limit rows of 1.5, EVT (hundreds of CPU channel updates) | Linux runs on the SAM firmware's own loop and the BIOS default PLs only. With no throttling based on surface temperature, the surface may get hotter, or conversely there may be coarse throttling that relies on SAM PROCHOT | First: put the `surface_temp` hwmon (RTS/VTS) into a thermald configuration as sensors and `intel_rapl` PL1 as the cooling device. Second: compare the Windows ch0–ch9 values with the RAPL MSRs to find out what the channels mean. Use of TMP `SetSmfTripPoint` (0x09)/notification (0x0b) comes after that | Confirmed (Windows behaviour), impact inferred | Check: under load, compare PkgWatt and PL from `turbostat`, the RTS values from `sensors` and `/sys/class/powercap/intel-rapl:0/constraint_*` with the same load on Windows. Measured (idle): `intel-rapl:0` long_term 35 W, short_term 60 W, equal to the Windows MSR 0x610 values (35/60 W/28 s). There is no `intel-rapl-mmio`. EPP is `performance` on all 12 cores (power-profiles-daemon in the performance state). Under load (`stress-ng --cpu 12`, SAM-5 and PWR-9): only `low-power` is at 6 W; the other three profiles are at 33 W and differ only in the maximum fan speed (4745 / 4918 / 5553 rpm). 10 minutes of `performance` is steady at 29.9 W, with a package maximum of 94 °C and temp9 (VTS1) rising continuously to 51 °C. Windows lowers PL1 to 21–25.5 W at VTS1 47–48 °C, so under the same load the surface is hotter on Linux (measured; how the back feels and a comparison with the same load on Windows are unverified). With the device helper running, the PL1/PL2/EPP of the Windows table are written per profile ([helper.md](../../helper.md)) |
| G4 | The 12 ACPI thermal zones (`TPOL`, TZ01–TZ0B) are empty | No `_TMP` in `\TPOL` or `\_SB.TZ01`–`TZ0B` (Surface SSDT), L/drivers/acpi/thermal.c:827 | The SAM sensors do not appear in `/sys/class/thermal`. The default thermald configuration and desktop tools cannot see the surface temperature. Thermal zone failures may be left in dmesg | If this is a problem, have `surface_temp` register a thermal zone per channel itself, or substitute the thermald configuration of G3. Use the correspondence `_DSM` fn1 sensor number = SAM sensor IID | Inferred | Check: `dmesg \| grep -i thermal`; `ls /sys/class/thermal`; 11 `surface_thermal` channels in `sensors`. Measured: the thermal zones are `x86_pkg_temp` and `iwlwifi_1` only, and `LNXTHERM:00–0a` (TZ01–TZ0B) and `\TPOL` remain without a driver. The hwmon `surface_thermal` has 11 channels, labelled `I_RTS1`–`I_RTS8` and a repeated `I_RTS1`–`I_RTS3` (the values differ: temp1 40.8 °C, temp9 36.9 °C). That channels 9–11 are VTS1–3 is inferred from the `_DSM` fn1 sensor numbers. No thermal error in the log |
| G5 | SAM RTC and wake alarm not connected | The SRTC row of 1.4, the time and alarm row of 1.5; no `rtc-surface` node (2.1); `acpi_tad` with 0 capabilities | The wake-timer path of Windows (the SAM alarm) does not exist on Linux. Whether `rtc_cmos`, which `rtcwake` would use, wakes from s2idle is not known from the Windows side (Windows does not enumerate a CMOS RTC device) | Add the node `ssam:01:01:01:00:00` to `ssam_node_group_sp9` (rtc-surface: time only). For the alarm, implement `set_alarm` in rtc-surface with `SetWakeAlarm` (0x1a)/`Clear…` (0x1c)/`GetTimeAndAlarmCapabilities` (0x1e). The payload format is unverified, so a Windows SSH trace is needed | Confirmed (absent), impact inferred | Check: `ls /sys/class/rtc`; `rtcwake -m freeze -s 60`; with the node added, `rtc1` time = UTC. Measured (patch 0012): `rtc0` = rtc_cmos (has `wakealarm`), `rtc1` = surface_rtc (no `wakealarm`), the rtc1 time is within 1 second of system UTC. `acpi-tad` reports "Missing _PRW". On all four boots checked, the CMOS RTC was read as local time, so the clock started off by the local UTC offset and was then corrected (measured). That the firmware refills the CMOS RTC at boot is inferred. Waking from s2idle works through `rtc0`: all 17 runs of `rtcwake -m freeze` woke after the requested time (PWR-6) |
| G6 | SAM events TMP/0, FAN/1, PMC/0, SPT/0, REG/0 not registered | 1.2; Linux has no notifier for these categories (the TMP in surface_acpi_notify is for `MSHW0091` only) | Unverified. Trip-point, fan RPM trip and power events are not received. SAM does not send events that are not enabled, so this is more likely a missing function than a malfunction | Enable each event with `surface_aggregator_cdev` + `ctrl.py`, record the incoming CIDs and payloads with `trace`, then write drivers only for what is needed | Confirmed (difference), impact inferred | Check: `echo 1 > /sys/kernel/tracing/events/surface_aggregator/enable`, then change lid, power and load. Measured: 27 tracepoints. In 40 seconds idle, 1 event (BAT CID 0x53) and 483 requests, all with status 0, 0 timeouts and resubmits. Events on lid, charger and load changes are unverified (SAM-8) |
| G7 | SAM events are not drained in the wake IRQ | L/…/controller.c:2679–2697 (TODO) | If SAM signals during s2idle, the system either always resumes fully or pending events are left behind. A candidate for the difference in handling fan and battery events during sleep | Implement the procedure in the comment (repeat the callback command → re-arm when it returns 0). The candidate for the command ID is HDR `Sam/GetPendingTransaction=0x17` (inferred) | Confirmed (code), impact inferred | Check: `/sys/power/suspend_stats`, "pm: wake irq triggered" in `dmesg` (dyndbg), attaching and detaching the cover during sleep |
| G8 | No screen-off (not suspended) notification | The screen-off row of 1.5; Linux sends 0x15/0x33 only on the suspend path (L/…/controller.c:1826–1850, the PM callbacks in core.c) | In the idle state with only the screen off, the cover and SAM stay active and may draw more power | Linux has no "display off" kernel event, so sending it by ioctl from user space (logind idle hint) is the realistic way. Low priority | Inferred | Check: compare idle power with the screen off (`upower`) |
| G9 | No battery extras (adapter rating, disabling smart charging, BCL state) | The battery row of 1.5, HDR `Battery/` | When the 80% charge limit is active, Linux shows only "Not charging". No adapter wattage information | Apply linux-surface/kernel#184 (charger Artg, BAT 0x0f) and #167 (smart charging and authentication state, BAT 0x09/0x0a) | Confirmed (absent) | Check: `cat /sys/class/power_supply/*/status` at 80%; `charge_control_end_threshold` with #167 applied. Measured (patch 0010): `ADP1/input_power_limit` = 45 W. At 100 % the `BAT1` status is `Charging`, not `Full`. There is no `charge_control_end_threshold`. The string near 80 % and plugging/unplugging are unverified (SAM-6). #167 is not applied; the device helper reads the smart charging state from user space through the cdev ([helper-reference.md](../../helper-reference.md)) |
| G10 | Lid GPE 0x52 storm and resume problems (#1082) | `\_GPE._L52` + no `_PRW` on `LID0`; linux-surface `0012-surface-gpe.patch` enables 0x52 as a wake GPE. An analysis for the SL5 says that corruption of the PADCFG of an INTC1055 pad produces spurious GPE 0x52 SCIs (linux-surface/kernel#162) | A storm of GPE errors and a slow resume when closing the lid while charging or resuming by the lid | Same GPE and same GPIO controller, so extend the `surface_s2idle_fix` of #162 to the SP9 DMI and test. Wiki workaround `pci=hpiosize=0` | Inferred (applicability to SP9) | Check: close the lid while charging → "No handler or method for GPE" in `dmesg`; the increase of `/sys/firmware/acpi/interrupts/gpe52`. Measured (baseline): `gpe52` 0 counts and enabled, `sci` 0, 0 lines of "No handler or method for GPE". Lid close → open 4 times (on battery, without `pci=hpiosize`, PWR-2): 4/4 s2idle entries and resumes, `gpe52` at 9 in total (about 2 per close/open), 0 GPE error lines. Closing the lid while charging is unverified |
| G11 | Linux sees ACPI objects that "a Windows filter fills in", such as `ACPI000E`, `FAN0`, `MSHW0187`, `MSHW0188` and `MSHW0299`, in their raw form | 1.4, 1.5 | `FAN0` may show up as a cooling device `Fan` that does nothing. The fan RPM comes from surface_fan, so no information is lost | No separate work. To avoid confusion, tidy up together with G4 | Inferred | Check: `Fan` in `ls /sys/class/thermal/cooling_device*/type`. Measured: `cooling_device12` type = `Fan` (cur 1/max 1). `acpi-tad` caps 0x00. `MSHW0153`, `MSHW0005` and `MSHW0299` have no driver |
| G12 | The shutdown problem (#1058) has only a PCI workaround; a cause on the SAM side is unverified | linux-surface `0011-surface-shutdown.patch`, #1058 (open). HDR has `Kip/NotifyPendingShutdown=0xd`, `Kip/ClientCompletedShutdown=0x31` and `Sam/ResetToS0AfterPchShutdown=0x2d`, but whether Windows sends them on this device is not known | On a kernel without linux-surface `0011-surface-shutdown.patch` (upstream alone) the power does not go off | Keep that patch. If the cause needs investigating, find the last command in an SSH trace of a Windows shutdown | Confirmed (workaround exists), cause inferred | Check: the power LED and USB hub power after `poweroff`. Measured: the `quirk_no_shutdown` symbol of linux-surface `0011-surface-shutdown.patch` is in the running kernel. `poweroff` itself is unverified (PWR-7) |

Items that match and need no work:

- The KIP and SAM HID IIDs (1, 2, 3, 5 / 6, 7) and the event registry (REG, per TID) are the same as on Windows (1.2, 1.3, L/drivers/hid/surface-hid/surface_hid.c:212–214).
- Battery and AC are the same down to enabling the BAT/0 event through the SAM registry. Windows has one device and Linux splits it into two; that is all.
- Tablet mode: Windows also uses the POS event (the Integration driver's posture module). The POS switch in the Linux SP9 group is right. `MSHW0005` has no GPIO, so no Linux driver is needed (inferred).
- Buttons: both Windows and Linux read the 3 `MSHW0040` GPIOs as GPIOs (the Windows event log has no SAM event registration for buttons).
- The performance-mode commands (TMP 0x02/0x03, FAN 0x0e) and stopping the fan during sleep (FAN 0x0f) agree with the HDR names.
- The SSH UART configuration (4 Mbps, RTS/CTS, wake GPIO 0xAE) is read by Linux from `_CRS` as it is.
- The SAR manager (Radio Monitor) does not apply to this device, which has no WWAN. Telemetry, Power Tracker and the ML service have no functional impact (inferred).

## 4. Checks on Linux and their results

Each item is the check, followed by its result on Linux 7.3.4 where there is one.

1. SAM nodes: `ls /sys/bus/surface_aggregator/devices/` → `01:02:01:01:00/01`, `01:03:01:00:01/02`, `01:05:01:01:01`,
   `01:26:01:00:01`, `01:15:01:06:00`, `01:15:01:07:00`, `01:15:02:0{1,2,3,5}:00` (with the cover attached) and the driver binding of each.
   No error in `dmesg | grep -i ssam`; whether the power profile and timeout values of `ssam_controller_caps` (dyndbg) equal 1.4.
   Result (measured): the 8 kinds above + RTC (`01:01:01:00:00`) + the 4 KIP devices + the hub = 14 devices, all present and bound to drivers. The only SAM line in the kernel log is the firmware version
   (6.603.139), and there is no error. `ssam_controller_caps` is printed only at probe and is unverified.
2. Temperature: the number of `surface_thermal` channels in `sensors` (11 expected) and their labels. The correspondence with the ACPI names RTS1–8 and VTS1–3 (G4). `ls /sys/class/thermal`, `dmesg | grep -i thermal` (G4).
   Result (measured): see G4.
3. Fan: put load on Windows, reboot into Linux and check the RPM when cool (G2). Read FAN 0x17 through `cdev`, send 0x0b 0 and watch the RPM change.
   Fan stop during suspend (linux-surface `0007-surface-sam.patch`, fan power state).
   Result: the Windows round trip is unverified. For the behaviour of 0x0b see G2.
4. Performance mode: the difference in RPM and PkgWatt under load across the 4 levels of `/sys/firmware/acpi/platform_profile`.
   Result (measured): see G3 (SAM-5).
5. Power limits: PL1/PL2 and PkgWatt with `turbostat`, the `intel-rapl` constraint values. Compare with the Windows CPU SMF channel values (`sam-events.ps1`) (G3).
   Result (measured): see G3. The comparison with the Windows SMF channel values was not made.
6. USB-C: `ls /sys/class/typec` (whether it is empty), a dump of the SAM HID IID 7 descriptor. Port and PD information on a kernel with #184 applied (G1).
   Result (measured): see G1.
7. Battery: `upower -d`, the status shown near 80%, AC plug/unplug events (G9).
   Result (measured, static values only): see G9. `upower` shows the same values (charging, 100 %).
8. RTC and wake: `ls /sys/class/rtc`, `rtcwake -m freeze -s 60` (G5). Reading the time on a kernel with the rtc-surface node added.
   Result (measured): `rtc0` and `rtc1`; for reading the time see G5.
9. Lid and GPE: closing and opening the lid while charging, resuming from s2idle by the lid, the `/sys/firmware/acpi/interrupts/gpe5*` counts (G10).
10. Tablet mode: `SW_TABLET_MODE` in `evtest` when folding back or detaching the cover (the POS switch).
    Result (measured, static values only): with the cover attached, `SW_TABLET_MODE`=0 and sysfs `state`=`laptop`. The transition is unverified (SAM-2).
11. Buttons: key codes and direction of power and volume up/down (GPIO index 2=0x143, 4=0x147), resuming from s2idle with the power button.
    Result (measured, static values only): `gpio-keys.0` = KEY_VOLUMEDOWN and KEY_VOLUMEUP, `gpio-keys.1` = KEY_POWER (wakeup enabled). Pressing them is unverified (SAM-3).
12. Sensors: whether accel_3d, gyro_3d, als and rotation (dev_rotation) appear under `iio:device*` from SAM HID IID 6, and whether the values follow the device's posture
    (`monitor-sensor` if iio-sensor-proxy is present, otherwise iio buffer mode).
    Result (measured): 7 iio devices (gyro_3d, accel_3d, incli_3d, als, dev_rotation, gravity, magn_3d) are under `01:15:01:06:00`. Buffer mode gave 1 acceleration sample in 8 seconds
    (magnitude 9.70 m/s²). A one-shot read of sysfs `_raw` on accel, gravity and als takes more than 5 seconds and then returns 0 (the 0 is not a value but a missing response).
    `dev_rotation` returns a value in 0.2–0.3 seconds. iio-sensor-proxy was not installed on the test system. Orientation changes are unverified (SAM-7).
13. Watching SAM events: enable the `surface_aggregator` tracepoints and look at the events that arrive on lid, power, load and charging changes (G6, G7).
    Result (measured, idle only): see G6.
14. Shutdown: whether the device turns off completely after `poweroff` on a kernel that includes linux-surface `0011-surface-shutdown.patch` (G12).
    Result: unverified (G12).
