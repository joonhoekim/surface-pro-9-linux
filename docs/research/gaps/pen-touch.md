# Pen and touch: differences between linux-surface and the Windows baseline

Subject: the touch controller behind THC `PCI\VEN_8086&DEV_51D0` (ADL-P THC0, `00:10.6`) (HID `045E:0C52`, named "Touch G6" by its firmware) and the Surface Slim Pen 2 (BLE `045E:0C0F`).
The Windows values were read on this device. The HID capabilities are the output of [`tools/windows/hid-caps.ps1`](../../../tools/windows/hid-caps.ps1) (Col04 and Col07 need elevation: run it through `run-elevated.ps1`).
Bluetooth addresses, pen identifiers and host ID values are specific to one unit and are not given.

## 1. Windows baseline

### 1.1 Processing structure

| Layer | Windows component | Evidence |
|---|---|---|
| PCI THC | `IntelTHCBase.sys` 3.0.0.279 (`intelthcbase.inf`). Passes the controller's HID descriptor up unchanged (`UseTICReportDescriptorOnly = 1`) | `docs/reference/driver-packages.txt`, registry `HKLM\SYSTEM\CurrentControlSet\Services\IntelTHCBase\Parameters` |
| Heat map → touch and pen | Inbox HEAT framework (`heat.inf` binds to `HID_DEVICE_UP:000D_U:000F`, `HeatCore.dll`) + the Surface-specific software processor `TouchPenProcessor0C52.dll` 18.124.139.0 (22 MB). The counterpart of iptsd on Linux | `C:\Windows\INF\heat.inf`; `HKR,Heat,SoftwareProcessor,...\TouchPenProcessor0C52.dll` in `surfacetouchpenprocessorupdate.inf` (matches Col04 only) |
| Injection of the processed result | Not a HID collection: HEAT injects directly into the input stack. Among all HID devices in the system the only Digitizer TLCs are Col06 (0x04) and Col09 (0x02) of the THC and the Type Cover touchpad | search for `TLC UP 000D` in the full `hid-caps.ps1` dump |
| Pen BLE bonding | `SurfacePenBleLcAddrAdaptationDriver.sys` 3.55.3.0 (THC Col07, keyboard pen charger KIP `09B0` Col03). Inbox "Bluetooth Loosely-Coupled Pen" (`bthlcpen.inf`, `BthLC\Pen` VHF) | `pnp-devices.csv`, DriverStore `bthlcpen.inf_amd64_*\bthlcpen.inf` |
| Pen haptics | The inbox input stack (`ISM.dll`) contains a pen haptic device that drives the HID Simple Haptic Controller; `Windows.UI.Input.Inking.dll` has a "pen haptics enabled" setting | strings in `ISM.dll` and `Windows.UI.Input.Inking.dll` |
| Firmware update | Touch: `SurfaceCFUOverHid` (Col08). Pen: `SurfacePenCfuOverBleLc.dll` (Col0A node; the transport is BLE GATT) | `driver-packages.txt` |

### 1.2 THC HID collections (`045E:0C52`, ver 0001, product "precise touch")

| Col | TLC (page:usage) | Windows name | Reports (ID: content) |
|---|---|---|---|
| 01 | FFFF:0001 | Surface Touch Utility | Feature 92: 21 vendor values (32-bit, 16-bit, array of 16-bit × 20) |
| 02 | FFFF:0001 | Surface Touch Utility | Input 91: 32-bit × 128, Feature 90: 32-bit × 128 |
| 03 | FF0B:000B | Surface Touch Communications | Input 46–55 (15–7487 B), Feature 41–45 (15–510 B), Feature 72 (2 B) |
| 04 | **000D:000F** (Capacitive Heat Map Digitizer) | Surface Touch Pen Processor | Input 7,8,10,11,12,13,18,26,28: `0D:56` Scan Time 16-bit + `0D:61` data 61/253/997/1537/1817/1997/3497/4337/7485 B. Feature 5: `FF00:C8` 1 B (mode). Feature 6: `0D:63` 119 B (metadata). Feature 9: `FF00:C9` 63 B. Feature 17: `FF00:59` 63 B |
| 05 | FF0F:0050 | Surface Digitizer Utility | In/Out 31 (60 B), In 33·34, Feature 34·35 (32-bit values) |
| 06 | 000D:0004 Touch Screen | Surface Touch Screen Device | Input 64: Tip Switch, X 0..32767 (physical 0..27.31 cm), Y 0..32767 (0..18.08 cm). No Contact ID or Contact Count = single touch |
| 07 | FFF4:0001 | Surface Pen BLE LC Adaptation Driver | Input 84·110: `FF0F:0050` 4 B + `FFF4:0002` 6 B (the length of a BD address). Output 85·111, Feature 86·112·115. Sub-collections FFF4:0005/6/7/13/15/16 |
| 08 | FF0B:0101 | Surface Touch G6 Firmware Update (CFU) | In/Out/Feature 95·96·98·101 |
| 09 | 000D:0002 Pen, sub-collection 000D:0020 Stylus | Surface Touch Pen Device | Table 1.3 |
| 0A | FFA1:0060 | Surface Pen Cfu Over Ble LC Connection | no preparsed data (held by a UMDF driver) |
| 0B | FF0F:0051 | Surface HID Pen ID | Feature 102: 8-bit × 2 + 32-bit × 14 |

Evidence: `HID\VID_045E&PID_0C52&ColNN&UP:..._U:...` in `docs/reference/pnp-devices.csv`, `hid-caps.ps1 -Filter 'dev_51d0'`.

The descriptor Linux (ithc) receives from the controller (1288 B) has 9 application collections (measured). Col01 and Col02 (FFFF:0001, reports 90·91·92) are absent,
and the 9 that correspond to Col03–Col0B appear in the same order with the same report IDs and sizes. The descriptor begins, after its global items, with the FF0B:000B collection.
The first two collections are taken to be added by the Windows THC driver (inferred; not checked on the Windows side). So "passes up unchanged" in 1.1 and the Col numbers
in this table do not carry over to Linux as they are. Col0A, which could not be read on Windows, is Input 88 with a single 16-bit value (FFA1:0003) (measured).

### 1.3 Pen collection Col09, report ID 1 (16 B)

| usage | Meaning | Logical range | Physical range and unit |
|---|---|---|---|
| 0D:42 | Tip Switch | 1 bit | |
| 0D:44 | Barrel Switch (side button) | 1 bit | |
| 0D:45 | Eraser | 1 bit | |
| 0D:3C | Invert | 1 bit | |
| 0D:32 | In Range | 1 bit | |
| 01:30 | X | 0..9600 | 0..2739 × 10⁻² cm (27.39 cm) |
| 01:31 | Y | 0..7200 | 0..1826 × 10⁻² cm (18.26 cm) |
| 0D:30 | Tip Pressure | 0..4096 | no unit |
| 0D:3D / 0D:3E | X Tilt / Y Tilt | 0..18000 | −9000..9000 × 10⁻² degrees |
| 0D:56 | Scan Time | 0..65535 | 10⁻⁴ s |
| FF00:01 | vendor | 8 bit | |

There is no Transducer Serial Number (0D:5B). Pen identification is done by Col0B Feature 102 and by Col07.

### 1.4 HEAT metadata (Feature 6, 119 B)

The same bytes are stored in `HKLM\SYSTEM\CurrentControlSet\Enum\HID\VEN_8086&DEV_51D0&...&Col04\...\Device Parameters\Heat\CurrentParams` and match the value read with `HidD_GetFeature(6)`. They decode as they are with iptsd's `protocol::hid::Frame` + `protocol::metadata::Frame` (105 B) structures.

| Field | Value |
|---|---|
| HID frame | size 0x77, type 0 (nested) → size 0x70, type 2 (Metadata) |
| rows × columns | 46 × 68 |
| width × height | 27389 × 18259 (mm×100) = 27.389 × 18.259 cm |
| unknown_byte | 1 |
| transform (xx, yx, tx, xy, yy, ty) | 408.79, 0, 0, 0, 405.76, 0 (= 27389/67, 18259/45). No negative value → InvertX/InvertY = false |
| 16 unknown floats | 178, 182, 180, 1, 178, 182, 180, 1, 90, 171, 100, 20, 172, 177, 175, 2 |

### 1.5 Other Col04 feature values (Windows running, elevated `HidD_GetFeature`)

| ID | Value | Interpretation |
|---|---|---|
| 5 | `01` | Multitouch (heat map) mode. The same report as set-mode in iptsd and ithc |
| 9 | `01 00 …` | unknown |
| 17 | `01 00 90 01 …`, then from offset 0x22 the same per-pen bytes as `Heat\VendorSpecific\RecentPens` | 0x90 is the same value as iptsd's `report::Type::Button`. Appears to be per-pen configuration the host writes into the digitizer (inferred) |

Values read on Linux (measured, `HIDIOCGFEATURE`): Feature 5 = `01` and Feature 9 = `01 00 …`, the same as Windows; Feature 17 is 64 B, all 0.
Corrected since ([pen-vendor-exchange.md](pen-vendor-exchange.md) section 1): Feature 9 is the report through which the Windows touch processor sends its `8e …` messages to the digitizer (display state, and pen feedback on every frame), and Feature 17 is not written by the host: the firmware echoes the last display-state message in it (static analysis; the echo was measured on Linux). All 0 on Linux means that no such message had been sent.

### 1.6 TouchPenProcessor0C52.dll: configuration keys and functions

The processor's configuration names (registry value names, from its strings) cover: pen pressure curve parameters, the list of recent pens, a pixel shift in micrometres for the left and for the right panel, a ZFI enable switch, a fast host ID, and a touch enable switch.
Its HEAT path keys cover: pressure curve, recent pens, handedness mode, screen rotation, fast host ID, touch enable.
The values present in the registry are only `CurrentParams`, `SoftwareProcessor`, `VendorSpecific\FastHostId` and `VendorSpecific\RecentPens` (the pressure curve is at its default).

What the processor's strings say about its algorithms:

| Function | What is there |
|---|---|
| Palm decision | An LLR classifier with an original and a final palm value, a switch that activates palm heuristics, a "must be palm" flag, a handedness decision |
| Pressure | Tilt-dependent pressure with a current tilt pressure factor, a moving-average pressure, pressure before and after ZFI, a pressure high/low logic switch |
| Tilt | Median and IIR filters on the tilt output, a tilt noise state |
| Hover and prediction | Hover height, a predictor with a fine position delta, forward prediction in border post-processing |
| Buttons | Barrel switch, eraser, eraser detection |
| Pen identification | The raw pen ID from the digital pen, the last detected pen ID, a "full pen ID differs" condition |
| Calibration | A check of the calibration map version supplied by the firmware against the versions the processor supports |

### 1.7 THC device parameters and ACPI

| Item | Value | Evidence |
|---|---|---|
| THC driver parameters | `MiniMumResetAssertMs = 150`, `LowPowerLTRValue = 1023`, `UseTICReportDescriptorOnly = 1` | `Services\IntelTHCBase\Parameters` |
| Device parameters | SPI read and write speed 2..3, IO mode mask 7 (single/dual/quad), `SpiReadMaxPacketSizeSupported = 128`, `MSISupported = 1`, `WakeScreenOnInputSupport = 1`, `SystemInputSuppressionEnabled = 1` | `Enum\PCI\VEN_8086&DEV_51D0...\Device Parameters` |
| ACPI THC0 | `_ADR 0x00100006`. The only `_DSM` is the common `PCIC/PCID` one — no HIDSPI (`6e2ac436…`), QuickSPI (`300d35b7…`) or LTR (`84005682…`) DSM. `_CRS` GpioInt Edge ActiveHigh pin 0x131. Power resource `PRTP`: `_ON` sets GPIO 0x090C0009 to 1, `_OFF` does nothing. `_S0W 3` | DSDT, device `THC0` |
| ACPI THC1 | `00:10.7`, has the HIDSPI/QuickSPI/LTR DSMs (when `TMD1 == 1`). This device has no 51D1 device | DSDT, device `THC1` |
| Touch firmware | CFU component 11, current = offered version (`FirmwareUpdateStatus = 5`) | `Col08\Device Parameters\SurfaceCFUOverHid` |

The DSDT is not in this repository. Dump and decompile it from your own device with [`tools/windows/dump-acpi.ps1`](../../../tools/windows/dump-acpi.ps1), or see <https://github.com/linux-surface/acpidumps> (directory `surface_pro_9_intel`).

### 1.8 Slim Pen 2 (BLE, `045E:0C0F` rev 0596)

With Windows running, Feature 86 = 0, 112 = `03`, 115 = `43 01`. On Linux 112 = `02`, 115 = `fe ff`, and 86 returns only the one report ID byte (measured). An earlier reading of this collection (output reports 85 and 111 signed with AES-CMAC, the 16 B field of Output 111 being the signature) was wrong. Established since ([pen-vendor-exchange.md](pen-vendor-exchange.md) section 2): the 16 B of Output 111 are the pairing key the host makes up, sent as they are; AES-CMAC authenticates the pen's button events; bit 0010 of Feature 112 is written by the Windows Bluetooth service, and the bonding driver adds it to what it passes up when the device reports bit 0022 (hence `03` on Windows and `02` on Linux). The channel that bonds the pen is the Type Cover's copy of this collection: writing the host address to its Feature 86 makes the cover announce the pen in its slot.

GATT services (PnP nodes): `1800`, `1801`, `180A` Device Information, `180F` Battery, `1812` HID, vendor `e7661000-3f85-40f5-8745-284e9095a1e2`, `e7661100-…`, `e7661200-…`, `e7661201-…`, `7a441000-7fc2-45bd-83e6-d6d78c5669d1`.

| Vendor UUID | Use | Evidence |
|---|---|---|
| service `e7661100-…`, characteristics `e7662100`–`e7662104` | Pen firmware CFU | `SurfacePenCfuOverBleLc.dll` |
| `e7661201-…`, `e7662301-…`, `e7660010-…` | Pen telemetry | `SurfaceBtleLcPenTelemetry.dll` |
| `e7661000-…`, `7a441000-…` | Unknown. Not present in any of the Surface binaries examined | byte pattern search (CFU, telemetry, SurfaceService, bonding driver, ISM) |

HID over GATT collections (they exist only while the pen is connected; their capabilities were not read on Windows because the pen was asleep):

| Col | TLC | Windows name |
|---|---|---|
| 01 | 0001:0006 Keyboard | Surface HID Keyboard (tail button) |
| 02 | FF0B:0100 | Surface Pen Generic Device |
| 03 | FF0B:0101 | SurfacePen0C0F Firmware Update |
| 04 | FF0B:0053 | HID-compliant vendor-defined device |
| 05 | 000D:0020 Stylus | HID-compliant device (the haptic collection is taken to be here, inferred) |

Pen–host auto-bonding, per the trace messages of `SurfacePenBleLcAddrAdaptationDriver.sys`: the driver converts a pen ID to a MAC address; it handles a host Bluetooth address usage and a host auto-bonding capability feature report; on the pen side it distinguishes an auto-bonding capability flag (Slim Pen 2) and a known-host flag; and it runs on Laptop Studio, Pro 9 or newer, starting on the pen charger first and then on the digitizer.
That is: the digitizer reports the pen ID it reads from the inking signal (Col07 Input 84/110), the driver turns it into the pen's BD address, and it writes the host BD address and the auto-bonding capability into the digitizer as Features. The same driver also binds to "Surface HID Pen Bluetooth OOB Coupling" (FFF4:000A) of the keyboard pen charger (KIP `09B0`).
The address the driver derives is not the address of pairing mode but a second one, per host; the derivation and the coupling sequence that works on Linux are in [pen-vendor-exchange.md](pen-vendor-exchange.md) section 2.

## 2. linux-surface today

| Component | What it does | Where |
|---|---|---|
| ithc | Binds to `51d0`. THC0 has no HIDSPI DSM, so `use_quickspi = false` → legacy (IPTS) mode. Reads the HID descriptor from the device and registers a `hid_device` (BUS_PCI, 045E:0C52). Supports Feature get/set and Output set | linux-surface `patches/7.3/0006-ithc.patch:1217`–1235 (ID table), 1391, 1754–1761 (`acpi_check_dsm`), 716–776 (`raw_request`), 2731 (`0x51d0`) |
| ithc LTR | With no ACPI LTR DSM the defaults apply: active 50 µs, idle 100 ms. Reset: release NRESET, wait for `irq_cause`; on failure sleep 1 s and retry (up to 6 times) | `0006-ithc.patch:1404`–1415, 1032–1049 |
| Upstream intel-thc-hid | Does not know `51d0` (QuickSPI/QuickI2C cover newer platforms only) | kernel `drivers/hid/intel-thc-hid/` |
| iptsd device detection | `0D:56`+`0D:61` Input = touch data, `FF00:C8` 1 B Feature = mode, `0D:63` Feature = metadata. Matches Col04 exactly | iptsd `src/ipts/protocol/descriptor.hpp:21`–61 |
| iptsd metadata | Uses only rows/cols/width/height/invert. Discards the 16 unknown floats | `src/ipts/parser.hpp:182`–197, `src/core/linux/config-loader.hpp:37`–41 |
| iptsd presets | No preset for the SP9 (`0x0C52`). Relies on the metadata | `etc/presets/` (no match for `0C52`) |
| iptsd touch defaults | `ActivationThreshold 40`, `DeactivationThreshold 36`, `OrientationThresholdMax 15`. The comments in `etc/iptsd.conf`, however, say 24/20/5 | `src/core/generic/config.hpp:41`–48, `etc/iptsd.conf:87`, 96, 132 |
| iptsd palm | Only validates contact size and aspect ratio. `DisableOnPalm` and `DisableOnStylus` default to false | `config.hpp:29`–30, `src/apps/daemon/touch.hpp:246`–254, `daemon.hpp:64` |
| iptsd pen (DFT) | Position = interpolation of the 0x06 window, tilt = position of the second transmitter and `dft_tilt_distance 0.6` cm, pressure = frequency interpolation of 0x0B (no curve), button/eraser = phase of the 0x09 window, MPP2 button = 0x0A window, MPP2 contact = 0x07 window | `src/core/generic/dft.hpp:108`–309, `config.hpp:61`–69 |
| iptsd pen uinput | X 0..9600, Y 0..7200, pressure 0..4096, tilt ±9000, `BTN_STYLUS`, `BTN_TOOL_RUBBER`. Resolution is computed from the metadata width/height | `src/apps/daemon/stylus.hpp:26`–72 |
| Pen haptics | The kernel's `hid-haptic` is for hid-multitouch touchpads only (`HID_HAPTIC` "Haptic touchpad support"). It does not bind to a BLE pen. iptsd has no haptic code either (`use_haptic=disabled` at `src/meson.build:189` is only an SDL option for the debug tools) | kernel `drivers/hid/Kconfig:95`–101, `drivers/hid/hid-multitouch.c:608` |
| Pen BLE | Generic HID through BlueZ HOGP. Manual pairing (long press on the tail button). No auto-bonding or OOB bonding | linux-surface #1656, #1066 |
| Wiki, SP9 | Recommends `[Contacts] ActivationThreshold=24, DeactivationThreshold=20, OrientationThresholdMax=5` when touch misbehaves | linux-surface wiki `Surface-Pro-9.md:27`–40 |

## 3. Differences

| # | Wrong or missing | Evidence | Effect | How to fill it | Confidence | Checked on Linux |
|---|---|---|---|---|---|---|
| 1 | There is no iptsd preset for the SP9, and the touch thresholds in the code defaults (40/36/15) differ from the values in the documentation and wiki (24/20/5) | iptsd touch defaults in section 2, wiki | Light touches and swipes are missed (iptsd #171) | `etc/presets/surface-pro-9.conf`: `[Device] Vendor=0x045E Product=0x0C52`, `[Contacts] ActivationThreshold=24 DeactivationThreshold=20 OrientationThresholdMax=5`. Width/Height are not set: the metadata is enough | confirmed (the values differ); the best values are inferred | Compare the rate of missed light taps with `iptsd-calibrate` and `libinput debug-events` → measured: the preset is loaded as `/etc/iptsd.d/surface-pro-9.conf`, and at 24/20/5 there were 0 ghost touches in 150 seconds without contact. The comparison of missed taps and swipes is unverified (TOUCH-1) |
| 2 | No pen pressure curve, tilt-compensated pressure or ZFI. iptsd's pressure is the linear value of the frequency interpolation | 1.6 (pressure), `dft.hpp:233`–248 | Pressure feel and the start of the first stroke differ from Windows | Add a `[Stylus] PressureCurve` kind of setting to iptsd (gamma/spline post-processing), or correct it with a libinput tablet pressure curve (`libinput` pressure range quirk, the GNOME/KDE pen pressure curve) | confirmed (the function is absent); the numbers are unknown | With `evtest`: the minimum ABS_PRESSURE at which a stroke starts, the distribution of values at the same force → measured: the ABS_PRESSURE range of the iptsd virtual stylus is 0..4096 (declared). Reaching 4096 and the stroke start pressure are unverified (PEN-1) |
| 3 | No tilt filter; `TiltDistance 0.6` cm is not a value confirmed for the SP9 and Slim Pen 2 | 1.6 (tilt filters), `config.hpp:69` | Jitter in the tilt values, angle error | Check whether the unknown floats of the metadata (178/182/180 …) relate to tilt or the transmitters, then set `[DFT] TiltDistance` in the preset; add a median filter to iptsd | inferred | Fix the pen at 30/45/60° with a protractor and record ABS_TILT_X/Y → measured: ABS_TILT_X/Y range −9000..9000, resolution 5730 (declared). The comparison against angles is unverified (PEN-2) |
| 4 | Default for false side button detection | iptsd #102, #99 (per user, anywhere from 2000 to 20000), `config.hpp:64`·67 | Spurious right clicks while drawing | Settle a `[DFT] ButtonMinMag` value in the SP9 preset (needs measurement). Before that, find out whether the Slim Pen 2 uses the MPP2 path (`Mpp2ButtonMinMag`) | confirmed (the issues); the value is inferred | Collect the magnitude of the 0x09 and 0x0A windows with `iptsd-dump`, button pressed and released → measured: only that `BTN_STYLUS` is declared. The count of false detections is unverified (PEN-3) |
| 5 | Palm decision is the size and aspect ratio rule only. Windows uses an LLR classifier + handedness + screen rotation as inputs | 1.6 (palm decision; handedness mode and screen rotation keys) | Palm touches are taken as input while using the pen | First step: `[Touchscreen] DisableOnStylus = true` in the preset (blocks touch while the pen is in proximity). Longer term: add to iptsd a block of the hand-side region relative to the pen position (handedness) | confirmed (the functions differ); the effect is inferred | With the pen hovering and the palm on the screen, whether `libinput debug-events` shows touch events → measured: the drop-in overrides it with `DisableOnStylus = true` (load order from the journal, the override from reading the code). The effect is unverified (TOUCH-2) |
| 6 | Pen haptics not supported | 1.1 (pen haptics in the Windows input stack), #1066 (manual vibration achieved with report 65), the kernel's `HID_HAPTIC` is for touchpads only. Windows ETW analysis: [pen-ble-trace.md](pen-ble-trace.md) | No vibration | Short term: a user-space tool that writes report 65 to the BLE pen's hidraw (start/stop while inking, intensity). Longer term: widen `hid-haptic` to the Simple Haptic Controller of the Stylus TLC, or expose an FF device. Getting the pen's descriptor comes first. Prototype: [userspace/pen](../../../userspace/pen/README.md) | confirmed (absent); the protocol details are inferred | Decode `/sys/class/hidraw/*/device/report_descriptor` with `hid-decode`, check the usages of page 0x0E, then write report 65 → observed (writing directly to the pen's hidraw, PEN-9): the click (ordinal 3) of report 65 Manual Trigger is felt. ink-continuous (ordinal 9) was not felt, neither with the same bytes as the Windows capture nor at intensity 255. Writing 9 to the Auto Trigger of Feature 65 gives a weak, intermittent vibration during a stroke: unlike the continuous vibration on Windows. No difference was felt between waveforms 9–13 or between intensities 100/255. **Resolved since** (observed): the cause was the digitizer, not report 65. The continuous waveforms play once the host feeds the pen's speed back to the digitizer through Feature 9 (difference 11), which the device helper or `surface-pen-feedback` does; the values Windows writes while inking were captured and are the ones tried here ([pen-vendor-exchange.md](pen-vendor-exchange.md) sections 1, 5) |
| 7 | Pen auto-bonding through the digitizer (Col07) and OOB bonding through the keyboard charger are not implemented | 1.8 (bonding driver), Col07 capabilities. Windows ETW analysis: [pen-ble-trace.md](pen-ble-trace.md) | Pairing is manual, reconnection after the pen sleeps is slow (a clue for "is Windows different" in #1656: Windows gets the pen's address through the digitizer channel, outside BLE) | A user-space daemon: read Col07 Input 84/110 (pen ID + 6 B address) from the ithc hidraw and connect and pair that address through BlueZ D-Bus. Do not write the host-address Feature before its format is settled. Prototype: [userspace/pen](../../../userspace/pen/README.md) | confirmed (Windows behaviour); the meaning of the reports is inferred | On Linux, check (read only) whether reports 84/110 arrive on the ithc hidraw when the pen touches → measured: the ithc descriptor has the FFF4:0001 collection and Input 84·110 (12 B each). Whether they arrive is unverified (PEN-7). Pairing by hand works: put the pen into pairing mode with a long press on the tail button and bond with a `NoInputNoOutput` agent (2/2; without an agent, `AuthenticationFailed`); the battery shows 96 % (PEN-5). On the digitizer, a SET of Features 86·112 reads back unchanged and a stroke brings no Input 84·110 (the format was a guess, made without knowing the bytes Windows writes). A pen bonded in pairing mode disconnects by itself after about 80 s (0x13) and does not advertise afterwards when picked up, when drawing or when the tail button is pressed. **Corrected since** ([pen-vendor-exchange.md](pen-vendor-exchange.md) sections 2–4, measured): bonding without pairing mode works through the Type Cover's pen slot (the cover's copy of this collection + LE legacy OOB pairing, kernel patch 0023; the device helper runs it when the pen is put into the slot), and a pen bonded that way, to which the host gave its identity key, reconnected 3.5 s after being picked up following a 50 s idle disconnect. The exchange Windows does on reconnection is a firmware update check, not the condition for advertising |
| 8 | Nothing links pen identification (Col0B Feature 102, RecentPens) to the BLE pen | 1.2, 1.5 | No per-pen settings, no way to address haptics to a pen | Pen ID ↔ BLE device mapping in the same daemon as #7 | inferred | Whether the Feature 102 GET value changes when a different pen is brought to the screen → measured: Feature 102 is 59 B with 14 non-zero bytes (the values are specific to the unit and not given). The test with a different pen is unverified |
| 9 | The tail button arrives only through the BLE keyboard collection. On Windows the Surface app maps single/double/long press to actions | 1.8 Col01 Keyboard | On Linux only key combinations arrive | Check the incoming key combinations with `evtest`, then use desktop shortcuts or remap with hwdb `KEYBOARD_KEY_…` | confirmed (the path); the key codes are inferred | `evtest`, tail button once, twice, long → measured (PEN-6): as `Surface Slim Pen 2 Keyboard`, once Meta+F20, twice Meta+F19, long Meta+F18 |
| 10 | The 16 unknown floats of the metadata are discarded | 1.4, `parser.hpp:182` | Meaning unknown. Possibly tilt or frequency parameters | Add debug output to iptsd to record the values, compare with other Surface devices | inferred | Whether the `iptsd-dump` metadata output shows the same values → measured: the 16 unknown floats read through `hidraw` Feature 6 equal the Windows values. `iptsd-dump` was not run |
| 11 | iptsd does not handle Col04 Features 9·17 | 1.5 | Feature 9 carries the host's pen feedback (among it the pen's speed); without it the Slim Pen 2 does not play its continuous haptic waveforms. Feature 17 is the firmware's echo, not a per-pen configuration as first inferred ([pen-vendor-exchange.md](pen-vendor-exchange.md) section 1) | The device helper ([helper.md](../../helper.md)) or, without it, `surface-pen-feedback` ([userspace/pen](../../../userspace/pen/README.md)) writes the pen speed to Feature 9. iptsd itself sends nothing | inferred | `hidraw` GET_FEATURE 9·17 → measured: Feature 9 equals Windows and 17 is all 0 (1.5). A GET of Feature 17 makes ithc deliver one more response of 1 B holding only the ID, which leaves `hid_input_report failed with -22` in dmesg, and the Feature read right after it can receive that 1 B (reproduced 3 times out of 3). With patch 0018: reading Features 17·112 back to back, every response belongs to the requested report and there are 0 lines of `hid_input_report failed` (measured). Why the firmware sends a second response is unverified |
| 12 | The firmware's own Col06 (single touch) and Col09 (native pen) can also be picked up as kernel hid-multitouch input devices | 1.2, hid-core group selection | If these reports arrive in multitouch mode, input is duplicated with the iptsd uinput devices | If they do: hide those collections in ithc, or hwdb/udev `LIBINPUT_IGNORE_DEVICE=1` | inferred | Whether `libinput list-devices` has an "IPTS"/"045E:0C52" kernel device and whether `evtest` shows events on it → measured: the kernel-side devices do exist (`Intel Touch Host Controller Touchscreen`·`… Stylus`, hid-generic). libinput takes them as touch and tablet devices and the compositor opens them. Their evdev state on a running system showed no trace of events (X, Y and pressure all 0). Whether events arrive during contact is unverified (PEN-8). The tilt range of the kernel-side stylus is 0..18000, so its origin differs from the iptsd device (−9000..9000) |
| 13 | LTR and reset timing differ: Windows `LowPowerLTRValue 1023`, `MiniMumResetAssertMs 150` / ithc idle 100 ms, no explicit reset assertion time | 1.7, section 2 | Possible DMA RX timeout, reset failure after resume | If problems appear, adjust with the module parameters `ithc.idleltr=`, `ithc.activeltr=`. Add a 150 ms reset assertion as a patch when a failure reproduces | inferred | "DMA RX timeout", "failed to reset" in `dmesg | grep ithc` → measured (TOUCH-3): both lines 0 after 17 s2idle cycles. Whether touch and pen work after resume is unverified |
| 14 | Wake-screen-on-input (`WakeScreenOnInputSupport`) and GpioInt 0x131 are not used | 1.7 | No wake by touch while the screen is off | GPIO wake support in ithc if needed | inferred | Whether pen or touch wakes the device from s2idle → measured: `power/wakeup` of `0000:00:10.6` = disabled. ithc has no wake setup code, so it is expected not to wake (inferred, TOUCH-4) |
| 15 | No path for touch firmware and pen firmware updates (CFU over HID, CFU over BLE GATT) | 1.1, 1.8 | Firmware cannot be updated from Linux (it has to be done on Windows) | Check whether fwupd has a Surface CFU plugin. If not, update on Windows | confirmed (the path); fwupd support is unverified | `fwupdmgr get-devices` → measured: fwupd is not installed on the test system. The `cfu` plugin quirks of nixpkgs fwupd 2.1.6 have no `0C52` or `0C0F` entry, so the devices are not expected to be listed even when it is installed (inferred, not run) |

## 4. Test items after installing Linux

1. `dmesg | grep ithc`: whether it binds in legacy mode; `device_id = $TIC`, product `0x0c52` and the error flags in the config line.
   → measured: `no ACPI config, using legacy mode`. The config line has `device_id` 0x43495424 (`$TIC`), product 0x0c52, `error` 0x00000000.
2. Whether the descriptor from `cat /sys/bus/hid/devices/*0C52*/report_descriptor | hid-decode` equals Col03–Col0B of table 1.2 (9 collections, report IDs).
   → measured: TLC, report IDs and sizes of the 9 collections equal the table. Col01 and Col02 are absent (1.2).
3. iptsd metadata: rows 46, columns 68, width 27.389, height 18.259, no invert.
   → measured: rows 46, columns 68, width 27389, height 18259, transform 408.79/405.76 (no inversion) of Feature 6 (120 B) equal 1.4.
4. Touch: compare missed light taps and three-finger swipes between the defaults and 24/20/5.
   → unverified (TOUCH-1).
5. Pen: with `evtest`, pressure reaching 0..4096, tilt ± angle, false side button detection (count of BTN_STYLUS glitches in 30 seconds of continuous drawing), `BTN_TOOL_RUBBER` on the eraser end.
   → unverified (PEN-1–4). Only the ranges and key declarations were measured. A stroke drawn with the pen was seen (observed; the pressure range was not measured).
6. Whether `libinput list-devices` shows kernel-side 0C52 input devices, and whether events are duplicated.
   → measured: they exist (difference 12). Whether events are duplicated is unverified (PEN-8).
7. BLE: after pairing, get the pen descriptor with `hid-decode` (the 0x0E haptic usages under Col05 Stylus), vibration with report 65, the tail button key combinations, battery (`upower -d`).
   → differences 6, 7, 9. Pairing, battery and the tail button work. A pen bonded in pairing mode does not reconnect; one bonded through the cover's pen slot does. The click works, and the continuous waveforms work with the digitizer feedback ([pen-vendor-exchange.md](pen-vendor-exchange.md)).
8. Whether reports 84/110 arrive on the ithc hidraw when the pen touches (read only).
   → unverified (PEN-7).
9. Touch and pen recovery after entering and leaving s2idle; touch and pen in `fwupdmgr get-devices`.
   → for the kernel log after s2idle see difference 13. `fwupdmgr` is not on the test system (difference 15).

The iptsd tools (`iptsd-dump`, `iptsd-calibrate`, `iptsd-show`) set Feature 5 back to single-touch mode when they exit, and the daemon does not write the mode again (from reading the code,
not run). Running them while the daemon is running can turn touch off, so do `sudo systemctl stop 'iptsd@*'` first. For whether these tools, `libinput` and
`hid-decode` are on the PATH, see the tools section of [test-plan.md](../../test-plan.md).
