# Input device control: what Windows sends and the same control on Linux

This document lists which controls Windows sends, and when, to the touch controller (`045E:0C52` behind THC `8086:51D0`), the Type Cover (KIP) and the power and volume buttons (`MSHW0040`), and where the same control has to be done on Linux.
In short: Windows sends almost nothing while awake; around Modern Standby it rewrites the touch mode (Features 5 and 9) and the touchpad modes, and the cover firmware handles Fn lock and the backlight itself. The one real gap on Linux is that an internal ithc reset does not resend the touch mode (section 4).
For the pen's BLE side the reference is [pen-ble-trace.md](../gaps/pen-ble-trace.md); for the full table of THC collections and the HEAT metadata it is [pen-touch.md](../gaps/pen-touch.md).

Everything was collected read-only: HID capabilities (`tools/windows/hid-caps.ps1`, elevated), current Feature values (`HidD_GetFeature`, no SET), the registry, the decompiled ACPI tables, and short ETW recordings.
Values unique to one device (pen ID, BD address, serial) are not written down.
Source references: `0006-ithc.patch:N` is a line of linux-surface `patches/7.3/0006-ithc.patch` at the commit pinned in [kernel/README.md](../../../kernel/README.md); a bare kernel file name (`hid-input.c:N`) is a file of the Linux v7.3.4 source tree; `src/…` and `etc/…` are paths in the iptsd source. "Patch 0016" and "patch 0017" are this repository's patches in `kernel/patches`.

## 1. Channels

| Channel | Windows component | Linux counterpart | Evidence |
|---|---|---|---|
| HID over THC (SPI) | `IntelTHCBase.sys` 3.0.0.279. Turns HID requests into THC DMA writes. In ETW one GET_FEATURE shows up as a DMA write of type 4 (length 0x78) followed by a DMA read of type 4 | ithc in legacy mode. It uses the same codes: 3 = SET_FEATURE, 4 = GET_FEATURE, 5 = OUTPUT, 7 = reset (`0006-ithc.patch:716`–775, 991–999) | ETW in section 3.2, IntelTHCBase events 101/102 |
| HID over SAM (SSH UART) | `SurfaceHidMiniDriver.sys` (a `mshidkmdf` minidriver). Towards KIP: SHP devices 1 (keyboard), 2 (pen charger), 3 (touchpad), 5 (firmware and colour). Towards SAM: 6 (sensors), 7 (UCSI) | `surface_hid`: category 0x15, output 0x01, GET_FEATURE 0x02, SET_FEATURE 0x03 (`drivers/hid/surface-hid/surface_hid.c:36`–39), SP9 nodes IID 1/2/3/5 (`surface_aggregator_registry.c:195`–213) | PnP parent relations, HIDCLASS rundown |
| Button GPIO → virtual HID | `SurfaceButton.sys` 4.64.139.0 takes the GPIO interrupts and emits them as a VHF HID device (`045E:F002`, Portable Device Control). The inbox `buttonconverter` turns these into key combinations (Ctrl+Alt+Del and so on) | `soc_button_array` → `gpio-keys` | Section 2.4 |
| Cover attach/detach and posture | `SurfaceHotPlug.sys` (KIP connection events, creates and removes the HID PDOs) → laptop/tablet indicator `MSHW0005` (`PNP0C60`, `msgpiowin32`) | `surface_aggregator_hub` (KIP hub), `surface_aggregator_tabletsw` (POS posture) | Section 4 |

## 2. Values read on Windows

### 2.1 Current Feature values of THC Col04 (HEAT)

Same as [pen-touch.md](../gaps/pen-touch.md) section 1.5. 5 = `01` (heatmap mode), 9 = `01`, 17 = per-pen settings (the value is not written down because it contains bytes unique to the pen).
In addition: Col01 Feature 92 = `03 00 00 00 08 00 00 00 08 …` (the rest is 0, with one `08` near the end), Col05 Features 34 and 35 = 0, Col03 Features 41–45 and 72 = 0, and Col02 Feature 90 fails on GET (error 1, not supported).

### 2.2 THC driver settings

| Item | Value | Source |
|---|---|---|
| SPI read and write speed | Min 2, Max 3 (ADL), read IO mode mask 7 (single/dual/quad), write 7, maximum read packet 128 | The device's `Device Parameters` in the registry |
| Power | `EnhancedPowerManagementEnabled = 1`, `SystemInputSuppressionEnabled = 1`, `WakeScreenOnInputSupport = 1` | The device's `Device Parameters` in the registry |
| Service parameters | `MiniMumResetAssertMs = 150`, `LowPowerLTRValue = 1023`, `UseTICReportDescriptorOnly = 1`, `LogPages = 16`, `VerboseOn = 1` | `Services\IntelTHCBase\Parameters` |
| ACPI | THC0 has no HIDSPI, QuickSPI or LTR `_DSM`. All of the values above come from the registry | [pen-touch.md](../gaps/pen-touch.md) section 1.7 |

### 2.3 Type Cover (KIP) collections

The SHP device numbers are the same as the Linux `surface_hid` IIDs. The VID is `045E` throughout.

| SHP/IID | PID | Col | TLC | Reports | Current Feature values |
|---|---|---|---|---|---|
| 1 keyboard | 09AE | 01 | 0001:0006 Keyboard | In 1 (modifiers + 0..0x91), Out 1 (LEDs 1–3: Num/Caps/Scroll), Feature 1: 000C:02C1–02C6 (Extended Keyboard Attributes) | F1 = `01 02 01 02` (the first byte is the report ID. Form factor 2 compact, key type 1, physical layout 2 = 103-key Korean. [typecover.md](typecover.md) section 1) |
| | | 02 | 0001:0080 System Control | In 91: 0001:0097/0098/0099 (System Function Shift = Fn, Fn Lock, Fn Lock indicator) | |
| | | 03 | 000C:0001 Consumer | In 3 (0..0x3FF) | |
| | | 04 | 0001:0013 Call State Management Control | In 94: 0001:00E1 Call Mute Toggle, 1 bit | |
| | | 05 | **000C:0007 keyboard backlight** | In 213: 000C:0515, 1 bit. Out 213: 000C:007B (Keyboard Backlight Set Level) 0..12. Feature 213: 000C:0517, 8 bits × 4. Feature 214: 000C:007B 0..12 | **F213 = `00 03 06 0C` (4 supported levels), F214 = `06` (current level)** |
| 2 pen charger | 09B0 | 01 | Keyboard | In 92: 0x6D–0x6F (F18–F20) | |
| | | 02 | FFF4:000A pen OOB pairing | In 20: BD address 6 B + 0..100 (battery) | |
| | | 03 | FFF4:0001 pen BLE LC | Same structure as THC Col07 (84/85/86/110/111/112/115) | |
| 3 touchpad | 09AF | 01 | 0001:0002 Mouse | In 2 (2 buttons, relative X/Y) | |
| | | 02 | 000D:0005 Touch Pad | In 4: 5 fingers (0D:42 Tip, 0D:47 Confidence, 0D:51 ID 0..5, X 0..1924 (9.62 cm), Y 0..1049 (5.24 cm)), Scan Time, Contact Count, 1 button. Feature 4: 0D:59 Pad Type + 0D:55 Contact Count Max. Feature 6: 0D:60 Latency Mode. Feature 5: FF00:00C6 + FF00:00C7 × 32 | F4 = `05` (maximum 5, clickpad), F5 = `07 FF…FF`, F6 GET fails (121) |
| | | 03 | 000D:000E Device Configuration (`MTConfig`) | Feature 7: 0D:52 Input Mode 0..10. Feature 8: 0D:57 Surface Switch, 0D:58 Button Switch | F7 and F8 GET fail (121, write-only) |
| | | 04 | FF07:0001 "Surface Navigation Accessory" | In 9 and 10 (38 B), Out 9 and 10 (20 B), Feature 9 and 11 (1 B) | GET fails (1784, 121) |
| | | 05 | FF05:0004 USP Encapsulation | In/Out 14 (60 B) | |
| 5 | 0991 | 01 | FF05:0001 "Surface HID Color" | In/Out 13 (2 B + 32 B) | |
| | | 02 | FF0B:0101 CFU | Type Cover firmware update | |
| | | 03 | FF05:0004 USP Encapsulation | In/Out 14 (260 B) | |

The touchpad has no PTP certification blob (FF00:00C5, 256 B). It has FF00:00C6 and 00C7 instead.

### 2.4 Buttons

ACPI `\_SB.MSBT` (`MSHW0040`), defined in one of the SSDTs. The ACPI tables are not in this repository; a dump for this model is at <https://github.com/linux-surface/acpidumps> (directory `surface_pro_9_intel`), or dump them from your own device.

| GpioInt | Pin | Meaning | Attributes |
|---|---|---|---|
| 0 | 0x0003 | Power | Edge, ActiveBoth, **SharedAndWake** |
| 2 | 0x0143 | Volume + | Edge, ActiveBoth |
| 4 | 0x0147 | Volume − | Edge, ActiveBoth |

`_DSM 6fd05c69-…` rev 1: fn0 = `0x0F`, fn1 = 0, fn2 = `OMPR` (OEM platform revision), fn3 = `SBFF`.

The Windows HID path:

| Device | TLC | Content | Current Feature value |
|---|---|---|---|
| VHF `045E:F002` Col01 | 0001:000D Portable Device Control | In 1: Win (07:E3), power (01:81), volume +/− (0C:E9/EA), rotation lock (01:CA). Feature 1: 0001:00CB Control Enable, 5 bits (one per button) | `0E` = power, volume + and volume − enabled; Win and rotation lock disabled (buttons this device does not have) |
| `ButtonConverter` | Keyboard / Consumer / System Control | Ctrl, Alt, Del, Win, F14–F17, volume, system power | (Result of the conversion. Power+Vol− → Ctrl+Alt+Del) |
| VHF `045E:F001` | Keyboard + Consumer | For general key injection | |

Windows button behaviour:

| Behaviour | Detail |
|---|---|
| Combinations | Power + Vol+ = screen capture, Power + Vol− = SAS (Ctrl+Alt+Del) |
| Buttons during standby | On Modern Standby entry and exit `svchost` sets the Control Enable Feature (3.3); the values it writes were not read |
| Disabling the power button | Through the Portable Device Control SET_FEATURE (the Control Enable Feature) |

## 3. What ETW shows

### 3.1 Providers

| Provider | GUID | Use |
|---|---|---|
| HIDCLASS WPP | `47c779cd-4efd-49d7-9b10-9f16e5c25d06` (busiotools `BluetoothStack.wprp`) | A rundown the moment it is enabled (devices with instance paths, collections with usage page and usage). Per request: collection, IOCTL and report ID (event id 21 in this build). There is no payload |
| IntelTHCBase | `937AD94E-CA8D-4B8E-8143-3FCE4ACCB8CB` | Event 100 = trace message, 102 = DMA write (type, length), 101 = DMA read |
| Microsoft-Windows-Input-HIDCLASS | `6465DA78-…` | Device information rundown only |
| Microsoft-Surface-SurfaceHidMiniDriver / SerialHubDriver / HotPlug | `2FEA7205-…` / `8DDB06D0-…` / `69CD60E3-…` | The manifest events are errors and state changes only (HotPlug: `BaseAttached`, `Connected`) |

### 3.2 What was confirmed

- In a 40-second idle recording (nothing touched) these providers are silent apart from the rundown. Windows sends no periodic requests to THC or KIP while idle.
- A GET of Col04 Feature 5 during the same recording leaves `IOCTL 0x000B0192, report 5` in HIDCLASS id 21, type 4 / 0x78 B in THC 102, and type 4 in 101. The decoder finds `IOCTL_HID_SET_FEATURE` (0xB0191), `SET_OUTPUT_REPORT` (0xB0195) and `GET_INPUT_REPORT` (0xB01A2) in the same format.
- So "who (which process) did a SET on which report of which collection, and when" can be obtained from ETW. **The SET payload cannot.** Instead the Features are read again at every stage boundary and the changed values compared (`tools/windows/input-trace.ps1`).

### 3.3 Requests actually sent on display off = Modern Standby entry and exit

Turning the display off with `SC_MONITORPOWER` **entered Modern Standby immediately** (Kernel-Power 506, even while holding `ES_SYSTEM_REQUIRED`). A "display off only" state cannot be produced this way on this device. The device woke about 8 minutes later (507; the wake cause is outside the recording).
Times are seconds with Modern Standby entry (506) as 0.

| Time s | Process | Request |
|---|---|---|
| 0.0 | `ISM` | THC Col04 **SET_FEATURE 9** (THC DMA write, 64 B) |
| 0.0 | `WUDFHost` | SAM sensors (SHP 6, 0020:0001) SET_FEATURE 161, 137, 203 (sensor properties, unrelated to input) |
| 0.3 | `svchost` | Button VHF **SET_FEATURE 1** (Control Enable) ×2 |
| (during standby) | | No THC or KIP requests |
| 498.3 | `svchost` | Button VHF SET_FEATURE 1 ×2 |
| 498.5 | `System` | Touchpad (SHP 3) Col03 **SET_FEATURE 7 (Input Mode) and 8 (Surface/Button Switch)** |
| 498.8 | `System` | THC host-initiated reset of the touch IC (THC trace message), followed by Col04 **SET_FEATURE 9, 5** |
| 498.8 | `System` | Pen charger (SHP 2) Col03 SET_FEATURE 115 |
| 498.8 | `ISM` | Col04 **SET_FEATURE 5, 9, 9** |
| 498.8 | `svchost` | THC Col07 SET_FEATURE 112, 86 (pen pairing) |
| 499.8–500.1 | `WUDFHost` | BLE pen Col03 SET_OUTPUT 45 ×7 (pen reconnection, the same moment as the 16 B round trips in [pen-ble-trace.md](../gaps/pen-ble-trace.md); they are CFU firmware offers, [pen-vendor-exchange.md](../gaps/pen-vendor-exchange.md) section 4) |

What changed in the read snapshots: THC Col01 Feature 92 went from `…08…08…08` to `…09…09…09` (inferred to be a reset or resume count). Col04 Features 5 and 9 are `01` and `01` again after wake. The values during standby could not be read because the process is frozen.

## 4. Control → Windows path → Linux

| Control | Windows path | Value and trigger | Evidence | Same control on Linux | Open |
|---|---|---|---|---|---|
| Touch heatmap mode (IPTS mode) | SET on THC Col04 Feature 5 (FF00:C8, 1 B) → THC DMA type 3. `System` (the driver stack) and `ISM` (the input service) each write it | `01` = multitouch (heatmap). Twice right after the THC host reset on Modern Standby exit. Not written on entry | 3.3, 2.1, [pen-touch.md](../gaps/pen-touch.md) section 1.5 | iptsd writes 1 when it starts (`src/core/linux/runner.hpp:128`) and 0 when it ends (170–175). Resume is handled because ithc creates a new hid device and iptsd starts again (`0006-ithc.patch:1577`–1616, `etc/udev/50-iptsd.rules.in:1`–4). **Gap: an internal ithc reset (RX code 7 → `ithc_reset`, `0006-ithc.patch:792`–794, 1425–1439) keeps the hidraw node, so iptsd does not send the mode again even if the device has fallen back to single touch.** What is needed: ithc replays the last Feature 5 SET after a reset, or iptsd writes the mode again when data stops | Whether the reset actually occurs on Linux (`dmesg`). Whether Windows also writes it again when the device resets itself (RX 7) with the display on |
| Col04 Feature 9 (FF00:C9, 63 B) | `ISM` does the SET. On exit `System` does too | **Once on Modern Standby entry (inferred to be a standby value), 1–3 times right after the reset on exit.** GET while awake = `01` | 3.3 | Not written. Linux suspend holds the touch IC in NRESET, which replaces the entry side. Nothing writes F9 back to `01` after resume. A GET on Linux returns `01`, the same as on Windows ([pen-touch.md](../gaps/pen-touch.md) section 1.5, measured), so the firmware default appears to be `01` | The value written on entry (payload not recorded). Whether writing the same value in the Linux display-off-only state (DPMS) reduces touch power. Feature 9 is also the report through which the Windows touch processor sends its `8e …` messages (display state, per-frame pen feedback); on Linux `surface-pen-feedback` writes the pen feedback ([pen-vendor-exchange.md](../gaps/pen-vendor-exchange.md) section 1) |
| Col04 Feature 17 (FF00:59, 63 B) | Inferred to be HEAT. Per-pen bytes like those in `RecentPens` | After pen pairing | [pen-touch.md](../gaps/pen-touch.md) section 1.5 | Not written (firmware default) | Measured on Linux: 64 B, all 0 ([pen-touch.md](../gaps/pen-touch.md) section 1.5). Windows holds per-pen bytes there. Since: Feature 17 is the firmware's echo of the last display-state message written to Feature 9, not something the host writes ([pen-vendor-exchange.md](../gaps/pen-vendor-exchange.md) section 1) |
| THC transport | `IntelTHCBase.sys`: SET/GET Feature and Output as DMA types 3/4/5 | Type 4 confirmed by ETW | 3.2 | ithc legacy mode uses the same types (`0006-ithc.patch:716`–775) | Seeing types 3 and 5 directly in ETW (trace) |
| SPI clock and IO mode | Registry Min/Max speed 2..3, read IO mask 7, write 7, packet 128 | Driver start | 2.2 | ithc starts with clkdiv 2 ÷8 single and switches to the `spi_cfg` from the device configuration. Writes are always single (`0006-ithc.patch:1015`, 1086–1091) | How many MHz the Windows "speeds 2 and 3" are. Compare with the value ithc picks (`dmesg`) |
| LTR | `LowPowerLTRValue = 1023` (service parameter). No active LTR value is set | Idle entry | 2.2 | ithc defaults: active 50 µs, idle 100 ms. `ithc.activeltr=`, `ithc.idleltr=` (`0006-ithc.patch:1252`–1256, 1405–1415) | The unit of 1023 (if µs, about 1 ms, far shorter than ithc's idle 100 ms). The effect on power and DMA timeouts is to be measured on Linux |
| Reset timing | `MiniMumResetAssertMs = 150` (service parameter). A host-initiated reset on every Modern Standby exit | Start, resume, device-initiated reset | 2.2, 3.3 | ithc has no separate NRESET-low time; after raising it, it waits 1 s for `irq_cause`, and on failure rests 1 s and retries (`0006-ithc.patch:1032`–1049) | If resets fail after resume, add a 150 ms assert |
| Heartbeat | A heartbeat with a response timeout may exist in the THC protocol (inferred) | Period unknown | Inferred | ithc: TODO (`0006-ithc.patch:959`, 964) | Idle ETW showed no THC writes, so it may be off with this firmware (inferred) |
| Touch device power state | Unknown (no TMF) | | | ithc writes ENABLE + POWER_STATE(3) to configuration offset 0x0C (`touch_cfg`), and on suspend leaves only 0x04 and then lowers NRESET (`0006-ithc.patch:1001`–1009, 1441–1470) | Whether Windows changes `touch_cfg` on display off or standby (a THC DMA write of a type outside 3/4/5 would be a candidate) |
| THC idle (selective suspend) | `EnhancedPowerManagementEnabled = 1`, HIDCLASS idle notification → THC D3 | When there is no input | 2.2 | ithc has no runtime PM | Whether the THC device actually enters D3 on Windows (IDLE_NOTIFY in a trace, power measurement) |
| Touch while the display is off | `SystemInputSuppressionEnabled = 1`: the system discards input while the display is off. `WakeScreenOnInputSupport = 1`: input wakes the display. On this device display off is Modern Standby entry, and the only thing sent to the device then is Col04 F9 | Display off = standby entry | The device's `Device Parameters`, 3.3 | Neither the kernel nor iptsd handles display state. The device helper has it as a setting, off by default (`sp9-ctl touch-off on`): while the eDP connector is off it sets `inhibited` on iptsd's virtual touchscreen. The setting toggles; its effect with the display off is unverified ([helper-reference.md](../../helper-reference.md)). It does not reduce power: for that, the same SET once the F9 value is known | The F9 entry value |
| Keyboard backlight | Output 213 `0x7B` Set Level on keyboard Col05 (000C:0007). Levels = Feature 213 `00 03 06 0C`, current = Feature 214. A key press reaches the host as Input 213 `0x515`. This collection has no kernel driver (user mode opens it) | Key press, display off (KIP request CID 0x0A/0x0B on each display state, [typecover.md](typecover.md) section 4) | 2.3, typecover.md section 4 (inferred) | `hid-input` knows only 0x79/0x7A/0x7C (`hid-input.c:1169`–1171). 0x7B/0x515/0x517 are ignored → no LED device from `hid-input`. This repository's patch 0017 adds the LED `surface::kbd_backlight` to `hid-surface` (maximum 3, levels 0/3/6/12 → Output 213, `surface_hid` command 0x01). Writing it works on the device (the level reads back; off at 0 and on at 3, observed); the level update on a key press is unverified ([kernel/PROVENANCE.md](../../../kernel/PROVENANCE.md)). The helper sets the level through this LED and falls back to hidraw without it. On Linux the SAM display-off notification (0x15/0x16) is sent only on suspend prepare and complete (`aggregator/core.c:426`, 446) | Settled by the staged recording ([typecover.md](typecover.md) 6.1): the firmware changes the level by itself on a key press and notifies through Input 213; the host writes nothing; the backlight goes off after about 52 s. Open: whether the level in a desktop follows the key on Linux |
| Fn / Fn Lock | Keyboard firmware. It reports the state in Col02 In 91 (0x97/0x98/0x99). There is no report the host writes | Fn+Esc | 2.3 | The top-row behaviour is the same. `hid-input` does not know 0x97–0x99 and makes them `BTN_0`–`BTN_2` (`hid-input.c:871`–880, 1407–1414). linux-surface's `hid-surface` discards only `BTN_0`. This repository's patch 0016 ignores all three usages; the resulting input device has not been checked on the device ([typecover.md](typecover.md) section 2) | Whether the lock and indicator bits stay 1 |
| Caps Lock LED | Output 1 (LED page 1–3) | Key press | 2.3 | The same through `surface_hid` output command 0x01 (`surface_hid.c:36`) | |
| Cover attach/detach | SAM KIP event → `SurfaceHotPlug` creates and removes the KIP HID PDOs (SHP 1/2/3/5) | On attach and detach | PnP: the KIP HID devices appear and disappear with the cover | `surface_aggregator_hub`: KIP category 0x0E command 0x2C query and event, 250 ms delay on attach (`surface_aggregator_hub.c:289`–328). Measured: the keyboard and touchpad input devices go and come back over 6 cycles (SAM-1) | Whether Windows sends a SET to the keyboard or touchpad right after attach (trace `-Manual`) |
| Laptop/tablet signal | `SurfaceHotPlug` → injects the state into `MSHW0005` (`\_SB.SLT`, `PNP0C60`, no resources) → `ConvertibleSlateMode` | Currently `ConvertibleSlateMode = 1` (laptop) | The `\_SB.SLT` device in the SSDT, `HKLM\…\Control\PriorityControl` | POS posture (category 0x26 commands 0x01–0x03) → `SW_TABLET_MODE`. Detached, folded back, book = 1; closed, laptop = 0 (`surface_aggregator_tabletsw.c:422`–433, 476–597). Measured: 1 detached and folded back, 0 after reattaching and unfolding (SAM-2) | Whether Windows uses the same POS posture |
| Touchpad Input Mode | `System` (the HID stack, `MTConfig` Col03) does a SET on Feature 7 0D:52 | 3 (PTP, per the specification). At start, **on Modern Standby exit** | 2.3, 3.3 | `hid-multitouch` `mt_set_modes` writes 3 on probe, resume and open (`hid-multitouch.c:1819`–1922, 2206) | The value (payload not recorded; 3 per the specification) |
| Touchpad selective reporting | Feature 8 0D:57/0D:58 | **SET only on exit** (inferred to enable). Not sent on entry | 3.3 | On suspend ALL if wakeup is allowed, otherwise NONE; NONE when the input device is closed (`hid-multitouch.c:2214`–2271) | Windows does not turn it off on entry. Whether it is harmless for Linux to send NONE on suspend (touchpad behaviour after resume) |
| Touchpad Latency Mode | Feature 6 0D:60 | No SET on either standby entry or exit | 3.3 | HIGH on suspend and close (`hid-multitouch.c:2214`–2271) | Whether Windows ever writes this report |
| Touchpad vendor collection | Col04 FF07:0001 "Surface Navigation Accessory" | Unknown | 2.3 | None | Which process uses it and when. Possibly sensitivity settings (inferred) |
| Touchpad device classification | No PTP certification blob (FF00:C5) | | 2.3 | FF00:C6 (count 1, size 8) turns on Win8 detection, giving `MT_CLS_WIN_8` (`hid-core.c:857`–859, [typecover.md](typecover.md) section 3) | Check palm and button behaviour on Linux |
| Power button | GPIO 0x03 (SharedAndWake) → `SurfaceButton` debounce → VHF 01:81. Double press and 4-second long press are handled separately (inferred) | Press | 2.4 | `soc_button_array` → `gpio-keys` `KEY_POWER`, debounce 50 ms, wake enabled (`soc_button_array.c:229`–230, 593–597). No handling of double or long press. Measured: wakes the device from s2idle (5/5) | The Windows debounce value (no registry value = driver default) |
| Volume buttons | GPIO 0x143/0x147 → VHF 0C:E9/EA | Press. `svchost` does a SET of the VHF Control Enable (Feature 1) twice on Modern Standby entry and twice on exit; the values were not read | 2.4, 3.3 | `gpio-keys` `KEY_VOLUMEUP/DOWN`, auto-repeat, no wake (pressing them in standby does not wake, so the result is the same) | |
| Button combinations | `SurfaceButton`: power + volume + = screen capture, power + volume − = SAS (Ctrl+Alt+Del) | Pressed together | 2.4 | Not in the kernel. If wanted, map KEY_POWER+KEY_VOLUMEUP to a screenshot in user space (keyd, desktop shortcuts) | |
| Enabling and disabling buttons | VHF Feature 1 (Control Enable) | Currently `0E` | 2.4 | Not applicable | |

## 5. What has not been observed, and how to record it

By default `tools/windows/input-trace.ps1` records idle only. With `-Standby` it sets a wake timer (`SetWaitableTimer` fResume) and runs display off (`SC_MONITORPOWER`, which on this device is Modern Standby right away) → idle → `SetSuspendState` → idle. It turns on "allow wake timers" (`SUB_SLEEP/RTCWAKE`) for the duration and restores it at the end. If the display is still off after the timer wake, press the power button (the script is frozen during standby and may not be able to turn it on by itself).
With `-Manual` a person detaches and attaches the cover, presses the backlight key, and covers the screen with the cover. The Features are read again at every stage boundary. The result is written to `summary.txt` in the run's output directory (requests, THC DMA writes, changed Features, Kernel-Power events).

```powershell
powershell -ExecutionPolicy Bypass -File tools\windows\input-trace.ps1 -Standby -Manual
```

| Question | What to look at in the recording |
|---|---|
| Does 3.3 repeat (F9 on entry; reset + F5 and F9 on exit; touchpad F7 and F8) | The SET timeline of the `displayoff` and `standby` stages |
| Is `SetSuspendState` a different path from display off | Compare the request lists of the two stages, `kernel-power.csv` |
| Who writes what when the backlight key is pressed | SET_OUTPUT of Col05 report 213 and the process name in the `backlight` stage, changes of Feature 214 |
| Are Input Mode and selective reporting written again when the cover is attached | Reports 7 and 8 of 09AF Col03 in `coverattach` |
| SET payloads | Not in ETW. Inferred from changes in the Feature snapshots; if needed, hook the `HidD_SetFeature`/`WriteFile` API in the process concerned (separate work) |
