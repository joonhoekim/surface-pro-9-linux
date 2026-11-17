# Type Cover: Fn lock, edge scrolling, backlight and the other reports

For each behaviour of the Type Cover (KIP hub `MSHW0153`; SHP device 1 keyboard `045E:09AE`, 2 pen charger `09B0`, 3 touchpad `09AF`, 5 firmware and colour `0991`) this document records where the state lives and who changes it, and how to get the same behaviour on Linux.
The channels and the requests sent around standby are in [input.md](input.md). This document only looks deeper at the cover.

Everything was collected read-only on the cover (GET_FEATURE, GET_INPUT_REPORT, ReadFile, Raw Input, and static analysis of the drivers). No SET was sent.

| Material | Content |
|---|---|
| [`docs/reference/typecover/layout.txt`](../../reference/typecover/layout.txt) | The caps of all 16 collections and the bit position of every field (found by writing into an empty report with HidP_Set*). Use it as is when building hidraw reports on Linux |
| [`docs/reference/typecover/features.txt`](../../reference/typecover/features.txt) | Every Feature that can be read, with its interpretation, and the GET_INPUT results for Fn and backlight |
| [`docs/reference/typecover/devices.txt`](../../reference/typecover/devices.txt) | The driver, filters and INF attached to each collection |
| [`docs/reference/typecover/ptp-registry.txt`](../../reference/typecover/ptp-registry.txt) | Precision Touchpad settings (HKLM defaults, HKCU user values) |
| Staged recording | A recording in stages during which a person presses keys (section 6). The verdicts are in 6.1 |

The drivers and programs examined are `SurfaceHidMiniDriver.sys` 3.98.5.0, `SurfaceService.exe` 6.215.59.0, `SurfaceIntegrationDriver.sys`, `SurfaceHotPlug.sys`, `kbdhid.sys`, `MTConfig.sys` and `SurfaceBroker.exe`.
The tools are [`tools/windows/typecover-probe.ps1`](../../../tools/windows/typecover-probe.ps1) (an automatic part + `-Interactive`) and [`typecover-summary.py`](../../../tools/windows/typecover-summary.py).

## 1. Composition on the cover side

| Fact | Evidence |
|---|---|
| There is no Surface filter driver above the keyboard and touchpad collections. Keyboard Col01 = `kbdhid`, touchpad Col02 = `input.inf` (Windows PTP), Col03 = `MTConfig`, the rest = no driver (user mode opens them). Below them there is only `mshidkmdf` + the lower filter `SurfaceHidMini` | `devices.txt` |
| The KIP path of `SurfaceHidMiniDriver` is its generic HID module (it passes received reports through unchanged). Its keyboard module (legacy keyboard) is for the SAM TC 0x08 keyboard (laptop-type devices) | The driver's strings |
| `SurfaceHidMiniDriver` answers every GET_INPUT_REPORT to the cover with error 121 (after about 8 s). The state cannot be read with GET; it is known only when an input report arrives | `GetInput 91` and `GetInput 213` in `features.txt` |
| `SurfaceService` is not involved in key input. What it has for the cover is writing the PTP registry defaults and a backlight mode command sent through SAM (one for the keyboard and one for the legacy keyboard) | The program's strings |

Keyboard Feature 1 (Extended Keyboard Attributes) = `01 | 02 01 02 00 00 00`: Form Factor 2 (compact), Key Type 1 (full travel), Physical Layout 2 (103-key, Korean), the rest 0 (per the HUT 1.4 values). The first byte is the report ID, as in [input.md](input.md) section 2.3.

## 2. Fn lock

**Mechanism: cover firmware (static evidence here; the staged recording is in 6.1).** The Fn and Fn lock state is in the cover firmware. The host only receives notifications.

| Evidence | Content |
|---|---|
| Report | Keyboard Col02 System Control (0001:0080) In 91, one byte: bit 0 = 0001:0097 System Function Shift (Fn pressed), bit 1 = 0098 System Function Shift Lock, bit 2 = 0099 System Function Shift Lock Indicator (`layout.txt`). The cover has no Output or Feature with these usages. The host has no way to write Fn lock or to read it with GET |
| No conversion on the host side | The keyboard collection has only `kbdhid` (no filter) and the KIP path of `SurfaceHidMiniDriver` passes reports through. No host component could turn Fn+key into another usage |
| Top row as a result | Depending on the lock state the firmware chooses usages of keyboard Col01 (F1–F12, 0x3A–0x45) or of consumer Col03 (the 0x000–0x3FF array) and sends those (inferred from the static evidence; which usage arrives from which collection is recorded in the stages `frow-1`, `frow-2` and `frow-held`, see 6.1) |

**State on Linux.** `surface_hid` passes the same reports up unchanged, so the top-row behaviour (usages per lock state) is the same. The problem is In 91 itself.

- `hid-input` does not know 0x97–0x99. The 0x9x branch of System Control does `goto unknown` (`hid-input.c:871`–880) → being 1-bit fields they become `BTN_0`, `BTN_1`, `BTN_2` in order starting from `BTN_MISC` (`hid-input.c:1407`–1414).
- linux-surface's `hid-surface` (linux-surface `0015-hid-surface.patch`, `CONFIG_HID_SURFACE=m`) binds to `045E:09AE`/`09AF` (BUS_HOST, generic group) and discards only `BTN_0` events. Fn pressed (0x97) is blocked, but 0x98 and 0x99 remain as `BTN_1` and `BTN_2`. If the lock indicator bit stays 1 for as long as the lock is on, it becomes a "button held down" (how the bits behave is to be checked in the staged recording).
- `hid-surface` does not set `HID_QUIRK_INPUT_PER_APP` (`hid-generic.c:64` does). All 09AE collections therefore end up in one input device, and Call Mute (0001:00E1) below also gets a number such as `BTN_3`. The device split and the numbers are to be checked on Linux with `evtest`.

**Linux implementation.**
1. Kernel (recommended): change the `input_mapping` of `hid-surface` to decide by usage instead of by event code. 0x10097 → `KEY_FN` or ignore, 0x10098 and 0x10099 → ignore (the state is discarded, without a sysfs attribute or `LED_MISC`), 0x100E1 → `KEY_MICMUTE`. This keeps the intent of the current `BTN_0` filter (Fn must not leak as a button) and makes Call Mute usable. It is small enough to send to linux-surface.
2. User-space alternative: in hwdb, under `evdev:input:b0019v045Ep09AE*`, `KEYBOARD_KEY_10098=reserved`, `KEYBOARD_KEY_10099=reserved`, `KEYBOARD_KEY_100e1=micmute`. But the key remapping of `60-evdev` applies only to devices udev classifies as keyboards, so it depends on the second item above (whether everything ends up in one device). That is not checked on the device, so 1 was chosen.
3. A function for the host to change Fn lock is not built (there is no report to write).

Item 1 is this repository's patch 0016 (6.1). It has not been checked on the device: the resulting input device, the `BTN_*` numbers and the Call Mute key are unverified ([kernel/PROVENANCE.md](../../../kernel/PROVENANCE.md)).

## 3. Edge continuation of two-finger scrolling

**Mechanism: the Windows PTP host produces it (verdict in 6.1).** This is the behaviour where scrolling continues although the fingers have stopped at the edge of the pad. There are two candidates.

| Candidate | What the recording would then show |
|---|---|
| The touchpad firmware keeps moving the coordinates it sends | In the touchpad Raw Input (Col02 In 4) the contact X/Y keep changing at the edge too |
| The Windows PTP host produces it | The contact coordinates in Raw Input stay within 0.3 mm (reports keep arriving) while `WH_MOUSE_LL` wheel events and scrolling of the list continue |

There is no place for a Surface component to intervene. Touchpad Col02 has no filter (`devices.txt`), and `SurfaceService` only writes PTP registry defaults. The HKLM values are palm rejection zones: `SuperCurtainLeft/Right = 700`, `SuperCurtainTop = 200`, `AAPNonCurtainTop = 1000`, `AAPNonCurtainBaseWidth = 2571`, `SpaceBarOffset = 1000` (`ptp-registry.txt`). HKCU has no value related to edge continuation either (`PanEnabled = 1`, `ScrollDirection = 0`; `EnableEdgy = 1` is the screen-edge swipe). So it is one of the two. For the stages `scroll-edge` and `scroll-mid`, `typecover-summary.py` reports for every "interval in which the contacts are stationary (0.8 s or longer, 2 or more contacts)" the report rate, the distance to the edge (mm), and the number of wheel events and list movements in that interval. If, compared with the control stopped in the middle (`scroll-mid`), the wheel continues only at the edge, it is edge continuation.

**Touchpad facts.** Feature 4 = `05`: Contact Count Max 5, Pad Type 0 (a clickpad that is pressed). Feature 5 = FF00:00C6 `07` + FF00:00C7 32 bytes of `FF` (there is no 256-byte certification blob 0xC5). Feature 6 Latency Mode, 7 Input Mode and 8 Surface/Button Switch cannot be read with GET (121, write-only). Coordinates 0..1924 × 0..1049 (9.62 × 5.24 cm, one unit 0.05 mm), Scan Time 16 bits, 1 button (`layout.txt`). Touchpad input can be received through Raw Input (registering 000D:0005) (confirmed in the automatic test; ReadFile fails with sharing violation 32). The report rate is measured in the staged recording. A touchpad click that arrived during the automatic test appeared in Raw Input as a synthesized mouse button with `hDevice = 0`, and in `WH_MOUSE_LL` it has no injected flag. That is, pointer events produced by the PTP host appear as input without a device.

**State on Linux.** The device classification is the Win8 path (as in [input.md](input.md) section 4). `hid-core` sees 0xFF0000C6 (count 1, size 8) in the Feature and sets `HID_SCAN_FLAG_MT_WIN_8` (`hid-core.c:857`–859, 974–976) → `hid-multitouch` `MT_CLS_WIN_8` (`hid-multitouch.c:2747`–2749). Scrolling is produced by libinput. libinput's two-finger scrolling emits events only while the fingers move and emits nothing once they stop. Kinetic scrolling after the fingers lift is done by the toolkit (GTK, Qt). There is no edge continuation.

**Linux implementation.**
The recording decided for the second case (6.1).
- Had the firmware moved the coordinates it sends, the same would happen on Linux and there would be nothing to do.
- As it is synthesized by the host, on Linux the compositor would have to produce it. Only the compositor knows the scroll direction setting (natural scrolling). If a daemon outside the compositor emits the wheel through a uinput virtual device, that device is classified as a mouse and does not get the touchpad settings, so the continued scrolling can go the opposite way from the finger scrolling. The speed and the edge width are taken from the Windows recording (wheel event interval and delta). A libinput patch is not chosen because of the burden of distributing and updating it. A compositor can implement it, with the measurements in 6.1 as defaults. It is not part of this repository's helper.

## 4. Keyboard backlight

**Reports (confirmed).** Keyboard Col05 `000C:0007`:

| Report | Field | Value |
|---|---|---|
| Output 213 | 000C:007B Keyboard Backlight Set Level, 8 bits 0..12, byte 1 | Writes the level |
| Feature 213 | 000C:0517, 8 bits × 4 (constant fields) | `00 03 06 0C` = supported levels 0, 3, 6, 12 |
| Feature 214 | 000C:007B, 8 bits (constant field) | Current level (`06`) |
| Input 213 | 000C:0515, 1 bit, relative | Inferred to be the backlight key notification (a usage without a name in the hid-debug table) |

This collection has no usage for an off timeout or an automatic (ambient light) setting. The HUT usages 0x7C (OOC), 0x7D/0x7E (minimum, maximum) and 0x7F (Keyboard Backlight Auto) are absent as well. The only means of control left on the cover is writing the level. If there is an off timeout, it is inside the firmware or a SAM command.

**Display state → cover.** `SurfaceHidMiniDriver.sys` registers a callback for `GUID_CONSOLE_DISPLAY_STATE` (6fe69556-…). When the display state is 0 (off) the callback builds the 7-byte request `0E 02 00 00 0A 00 00`, when it is 1 (on) `… 0B …`, and sends it through the driver's SAM send routine (timeout 0x1F40 = 8000 ms). For 2 (dimmed) it only writes a log entry. Compared with another request of the same driver (`08 02 00 01 …` = TC 0x08 keyboard, TID 2, response expected, CID 0), the layout reads as TC, TID, IID, response flag, CID. From this it is inferred that **KIP (TC 0x0E) TID 2 CID 0x0A = display off, 0x0B = display on, no payload**. The cover appears to turn the backlight off on this (this is the path referred to in [input.md](input.md) section 4).

**Not settled by static analysis.** Whether, on a backlight key press, the firmware changes the level by itself and only notifies through In 213, or the host (which process) writes Output 213. The stage `backlight-cycle` reads Feature 214 every 250 ms and captures the process of SET_OUTPUT 213 with ETW. The idle off timeout (`backlight-timeout`, 90 s of observation + the person's answer) and the reaction to ambient light (`als`) are taken from the same recording. The results are in 6.1. Still open: when the keyboard backlight mode command of `SurfaceService` (SAM, 32-bit payload) is sent, and what the registry value `EnableGranularKeyboardBrightness` of `SurfaceIntegrationDriver` turns on.

**State on Linux.** `hid-input` knows only consumer 0x79/0x7A/0x7C as keys (`hid-input.c:1169`–1171). It creates no LED or brightness device for 0x7B and 0x517, and 0x515, being an unknown consumer usage, becomes `KEY_UNKNOWN` (`hid-input.c:1321`). The kernel has no code that tells KIP the display state either. The aggregator core sends only SAM display off and on (TC 0x01 CID 0x15/0x16), on suspend prepare and resume ([input.md](input.md) section 4). That is the state without this repository's patch 0017, which adds the LED (6.1). The helper writes the level through that LED and, without it, through hidraw Output 213 (`helper/crates/sp9-helper-core/src/input.rs`, the keyboard backlight entry in [helper-reference.md](../../helper-reference.md)).

**Linux implementation.**
1. Level: a `::kbd_backlight` LED on `hid-surface` (maximum 3, mapped to 0/3/6/12, Output 213), which the desktop's keyboard brightness keys and `UPower` use as is. This is patch 0017 (6.1); writing the LED works on the device. The helper uses the LED if it exists and hidraw if not.
2. Display-off notification (not implemented): the helper could send KIP TC 0x0E TID 2 IID 0 CID 0x0A/0x0B (no payload, no response) through `/dev/surface/aggregator` in step with DPMS (DRM connector off). Whether the backlight stays on with only the display off has not been looked at on Linux; if it does, check that this request turns it off.
3. Backlight key: the staged recording shows that the firmware changes the level by itself (6.1), so the only thing to do on Linux is not to emit 0x515 as `KEY_UNKNOWN` and to report the new level. Patch 0017 does both; the level update on a key press is unverified on the device.

## 5. The other collections and special keys

| Collection | Content | Linux |
|---|---|---|
| 09AE Col01 keyboard | In 1: 8 modifiers + array 0x00–0x91 (including Korean 0x90/0x91). Out 1: LEDs Num/Caps/Scroll | `hid-input` keys; the Caps Lock LED is `surface_hid` output 0x01 |
| 09AE Col03 consumer | In 3: one array 0x000–0x3FF | Known usages become keys (volume, brightness, media), unknown ones `KEY_UNKNOWN` |
| 09AE Col04 0001:0013 **Call State Management Control** | In 94: 0001:00E1 **Call Mute Toggle**, 1 bit | `goto unknown` (`hid-input.c:978`) → `BTN_x`. `hid-surface` discards `BTN_0` on this device, so depending on the number it can disappear. Becomes `KEY_MICMUTE` with implementation 1 of section 2 |
| 09B0 Col01 pen charger keyboard | In 92: keyboard 0x6D–0x6F (F18–F20). Notification that the pen is inserted or removed | `KEY_F18`–`F20`. Check that they do not trigger desktop shortcuts. If needed, a different key code through hwdb |
| 09B0 Col02 FFF4:000A pen OOB pairing | In 20: BD address 6 B + 3 status bits + battery 0..100 | No driver. Not read for this document (address). [pen-ble-trace.md](../gaps/pen-ble-trace.md) |
| 09AF Col04 FF07:0001 Surface Navigation Accessory | In 9 and 10 38 B, Out 9 and 10 20 B, Feature 9 (error 1784) and 11 (121) | Unknown. Whether input arrives is to be seen from ReadFile in the staged recording |
| 09AF Col05, 0991 Col03 FF05:0004 USP Encapsulation | In/Out 14 (60 B, 260 B). 4 32-bit headers + body | Unknown (appears to be a tunnel) |
| 0991 Col01 FF05:0001 Surface HID Color | In/Out 13: 8 bits ×2 + int8 ×32 | Unknown |
| 0991 Col02 FF0B:0101 CFU (Type Cover v7 firmware update, WUDF) | Feature 32 can be read: `03 00 00 04 22 00 0A 00 21 00 2F 00 …` (appears to be a CFU firmware version response and reads as component 3, protocol 4; the interpretation is not settled). 33 gives 1784, 34–41 give 121 | A target for the CFU plugin of `fwupd`. No update is performed |

The actual usages of the special keys (mute, volume, display brightness, Print Screen, Copilot, Home/End and so on) are filled in from the `special` and `frow-*` stages of the recording. The Copilot key normally arrives as keyboard page LWin+LShift+F23 (0xE3+0xE1+0x72) and fits in the array range of this keyboard (0x00–0x91).

## 6. Staged recording (interactive)

```
powershell -ExecutionPolicy Bypass -File tools\windows\typecover-probe.ps1 -Interactive
```

An administrator console and a test window open. Follow the instructions in the window and press Enter at each stage (5–10 minutes). Do not press the play/pause key. The display stays on to the end and the device does not enter standby.

| Stage | What is done | Question answered |
|---|---|---|
| `fn-toggle` | Fn alone, 4 times | Behaviour of each bit of In 91 (pressed, lock, indicator) |
| `frow-1`, `fn-switch`, `frow-2`, `fn-switch-back`, `frow-held` | The top row in both lock states and with Fn held | Which usage arrives from which collection in each state; is there a host SET |
| `backlight-cycle`, `backlight-timeout`, `als` | One round of the backlight key, 90 s untouched, covering the light sensor | Changes of Feature 214 and In 213, the process that sent SET_OUTPUT 213, the off timeout, the reaction to ambient light |
| `scroll-edge`, `scroll-mid`, `scroll-side` | Two-finger scroll, then 3 s stationary at the edge and in the middle | The verdict of section 3 |
| `swipe3`, `swipe4`, `tap-click` | Three- and four-finger swipes, tap, click | PTP reports and the events produced by the host |
| `capslock`, `special` | Caps Lock, the remaining special keys | LED output, usages of the special keys |

The result is written to the run's output directory as `summary.md` (per stage: keys, usages, stationary intervals of the touchpad, wheel, polling, Feature changes, ETW requests) and `summary.txt` (an ETW summary in the format of input-trace.py). Keyboard Raw Input keeps only key names; letter, digit and punctuation keys are masked as `char`, and letter usages of the keyboard page as `07:**`. To summarize again only: `-Summarize <run folder>`.

### 6.1 Verdicts

These were determined from one staged recording.

| Behaviour | Verdict | Evidence |
|---|---|---|
| Edge scroll continuation | **Produced by the host (Windows PTP)** | In every interval (1.0–5.2 s) in which the two contacts were stationary within 1–2 mm of the edge, the touchpad kept reporting 90–160 times per second with the contact coordinates fixed, and wheel events continued during that time (768 in one interval). All wheel events have `hDevice = 0` (synthesized, without a device). In the control stopped in the middle (`scroll-mid`) no "stationary interval" occurred and there was no continuation (user observation: 0). The horizontal edge (`scroll-side`) is the same (horizontal wheel `0x800`) |
| Fn lock | Cover firmware | Pressing Fn brings 0x97 (pressed) and 0x98, 0x99 in Col02 In 91, and when the lock changes the F row changes in the firmware: lock off = consumer keys (Mute 0xE2, Vol− 0xEA, Vol+ 0xE9, Play/Pause 0xCD, brightness 0x70/0x6F), backlight (0x0515), PrtSc, Home, End, PgUp, PgDn; lock on = F1–F12. There is no conversion on the host side |
| Backlight key | Cover firmware | The key arrives as consumer usage 0x0515 in Col05 In 213. On every press the level in Feature 214 changes 0→3→6→12→0, and the host sent no SET to the cover in between (ETW) |
| Backlight auto-off | Present, about 52 s (observed by a person) | No SET went to the cover in that interval, so it is attributed to the firmware (inferred) |
| Reaction to ambient light | None | The backlight stayed the same when the light sensor was covered |
| 3- and 4-finger swipes, tap | Host | The cover sends only contacts (up to 4 simultaneous contacts confirmed). Button and pointer events are all synthesized |

Conclusions for Linux:
- **Edge continuation** does not exist on Linux (libinput emits no scrolling once the fingers stop). A compositor can implement it (section 3). Windows values: edge zone within about 2 mm, about 150 wheel events per second. It stops immediately when the fingers lift.
  The speed is determined by the distance d from the nearer contact to the edge (wheel units per second, 120 = one notch): d ≤ 1.0 mm about 13000, 1.5 mm about 6500, 2.0 mm about 3600, 3.0 mm 0. The influence of the preceding scroll speed could not be separated in the measurement.
- **Fn lock, the backlight key and auto-off** are firmware, so they are expected to behave the same on Linux. The desktop, however, does not know the backlight level. 0x0515 has no mapping in Linux `hid-input`, so the key event is lost. When the helper sees Col05 In 213 it reads Feature 214 again and announces the level (for the settings application and the UPower keyboard backlight indication).
- The consumer keys of the F row (volume, brightness, playback) come out through the standard Linux mapping. The handling of `BTN_1`/`BTN_2` (0x98, 0x99) is settled in `hid-surface` as described in section 2.

**Linux implementation.** Two kernel patches (their verification status is in [kernel/PROVENANCE.md](../../../kernel/PROVENANCE.md)):
- [`0016`](../../../kernel/patches/0016-HID-surface-Ignore-Fn-usages-and-map-Call-Mute-Toggl.patch): `hid-surface` does not map 0001:0097–0099 and emits 0001:00E1 as `KEY_MICMUTE` (implementation 1 of section 2).
- [`0017`](../../../kernel/patches/0017-HID-surface-Add-keyboard-backlight-LED-for-the-Type-.patch): a `surface::kbd_backlight` LED (brightness 0–3 = levels 0/3/6/12, read from 0x517 of Feature 213). Writing is Output 213, reading is Feature 214. When 0x515 arrives in In 213 it reads Feature 214 again and announces it through `brightness_hw_changed`; 0x515 itself is not emitted as a key (emitting it as `KEY_KBDILLUMTOGGLE` would make the desktop change the level once more). The helper's hidraw path is to be removed once this patch is confirmed on the device.
- Checked on the device: writing `/sys/class/leds/surface::kbd_backlight/brightness` (0/1/2/3 read back; off at 0 and on at 3, observed). Still to check: whether `brightness_hw_changed` holds the new level on a key press (if the firmware changes the level later than it notifies, the previous level is read), and whether `BTN_*` have disappeared in `evtest`.

### 6.2 Pen storage flap (folding the cover)

The posture in which the pen storage part of the cover is attached to the bottom of the device and hides the pen was changed several times, and the state before and after and the ETW were compared with [`posture-trace.ps1`](../../../tools/windows/posture-trace.ps1).
There is no difference in any readable state: all cover Features, the KIP device list, the PnP devices and the posture node of the Surface power tracker (`I_POSTURE` stays 1) were the same.
During the trace the SAM serial port (`\_SB.PC00.UA00`) woke 8 times at irregular intervals, but without a control recording it cannot be said whether the posture changes caused this (no inference made).
There is nothing to do on Linux. To check anyway, enable logging of unknown events in `surface_aggregator` and see whether an event arrives on folding and unfolding.

## 7. Open questions

- Do the Fn lock indicator (0x99) and lock (0x98) bits stay 1 while the lock is on, or are they 1 only when it changes (whether `BTN_1`/`BTN_2` remain pressed)? Does the lock state survive detaching and attaching the cover, or a reboot?
- The exact speed curve of edge scroll continuation (the relation between wheel delta and the last scroll speed).
- Whether the cover firmware determines the backlight auto-off time and whether it can be changed (if on Linux it turns off after the same time without host involvement, it is the firmware).
- Is the inferred layout of KIP CID 0x0A/0x0B right (send it through the Linux cdev and see whether the backlight reacts)?
- Which key is Call Mute Toggle (0001:00E1)? What does Windows do with this key?
- The purpose of the Navigation Accessory (FF07), USP and Color collections.
- Into how many input devices 09AE is split on Linux and how the `BTN_*` numbers are assigned (`evtest`), and how libinput classifies this device.
