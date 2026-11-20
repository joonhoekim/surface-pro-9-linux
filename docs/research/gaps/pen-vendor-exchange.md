# Slim Pen 2: continuous haptics, cover-slot coupling, reconnection, and the 16 B exchange at connection

Windows exchanges seven 16 B writes and notifications with the pen every time it connects ([pen-ble-trace.md](pen-ble-trace.md) 4.3), and Linux does not.
That exchange was taken as a candidate common cause of the three things that did not work on Linux (continuous haptics, reconnection after the pen's idle disconnect, automatic bonding), and its values were captured on Windows.
It turned out to be unrelated, and each of the three has its own cause. This note records all four results.

Evidence levels are as in the other notes: **measured** (logs, captures, values read back), **observed** (felt or seen by a person), **inferred**, and **static analysis** (read from Windows binaries on a partition mounted read-only; nothing was run).
Facts from binaries are given without positions. Bluetooth addresses, keys, the pen ID and the host ID value are specific to one unit and are not given. The captures contain keys and addresses and are kept outside the repository.

| Problem | State on Linux | Cause | Section |
|---|---|---|---|
| Continuous haptics | **Played on Linux** (observed) | The digitizer, not Bluetooth. The Windows touch processor feeds pen state back to the digitizer on every frame; the pen plays the continuous waveforms only once the digitizer has a valid pen speed from the host. Linux sent nothing | 1 |
| Automatic bonding | **Works on Linux** with the pen in the cover's pen slot (measured): the cover procedure + LE legacy OOB pairing (kernel patch 0023) bonds within 1 s and BlueZ stores the LTK and IRK. The device helper does it when the pen is put into the slot | The keys go through the Type Cover, and the pen uses a second address for it | 2 |
| Reconnection after the idle disconnect | **Works on Linux** for a bond in which the host gave its identity key (measured: the pen disconnected by itself after 50 s, and reconnected 3.5 s after it was picked up and drawn with) | A pen that did not receive the host's identity does not advertise for reconnection | 3 |
| The 16 B exchange | Not needed | It is a firmware update check (CFU offers). It does not switch on haptics or connection keeping | 4 |

There is no evidence that the three problems had one cause; they did not.

## 1. Continuous haptics: the host feeds pen speed back to the digitizer

The continuous waveforms (ink, pencil, …; ordinals 9–15 of [pen-ble-trace.md](pen-ble-trace.md) 3.2) are a texture that follows the speed of the stroke. The host computes that speed,
hands it back to the digitizer, and the digitizer passes it up to the pen. It has nothing to do with how the pen was bonded: the pen used in the tests below was bonded in pairing mode.

### 1.1 What Windows sends (static analysis)

The Windows touch processor (`TouchPenProcessor0C52.dll`; iptsd takes its place on Linux) sends messages through Feature 9 of the digitizer's heat-map collection (usage FF00:C9, 63 B; [pen-touch.md](pen-touch.md) 1.2).
All of them start with `8e <type>`. iptsd sends none of them.

| Message | Content | When |
|---|---|---|
| `8e a1` DisplayStates | Display state, hinge angle, the pen's average voltage and distance, pen ID, **FastHostId** | At start, when the display state changes, when a new pen ID is seen |
| `8e a5` pen feedback V06 (or `8e a2`, V03) | Sequence number, pen ID, pen energy, position, LocalId, tip bit, FastHostId, ZFI, **speed** (6 bits), Ack | On every frame while a pen is present. The roughly 130 SET_FEATURE 9 requests a second in the Windows input trace are taken to be this (inferred: the trace has no contents) |

- FastHostId is a 10-bit value the host makes up at random once and keeps in the registry (`…Col04\…\Device Parameters\Heat\VendorSpecific\FastHostId`).
- Feature 17 is not something the host writes: the firmware echoes the last `8e a1` in it.
- Feature 115 of the FFF4 collection is the FastHostId the firmware has received (`fe ff` means none yet). The bonding driver carries that value over to the cover.
- The speed Windows sends is `round(min(v, 30) × 63 / 30)`, where v is the distance between two positions 14–32 ms apart divided by the time in ms, smoothed over three samples.

### 1.2 On Linux (measured and observed)

- Writing one `8e a1`, built from the Feature 17 value read on Windows, to Feature 9: Feature 17 echoes the same value and Feature 115 changes from `fe ff` to the FastHostId that was written (measured). The state is gone after a reboot.
- In that state, with a pen bonded in pairing mode, the continuous waveform (ink, intensity 127) was triggered. No vibration, neither during a stroke on the screen nor in the air (observed). FastHostId alone is not enough.
- **Once the digitizer is told the pen's speed, the continuous waveforms play.** After `8e a5` messages with a valid speed, the continuous waveform (ink, intensity 127) vibrates almost as it feels on Windows (observed).
- This firmware asks for V06: byte 7 of the payload of the DFT metadata (report 0x5f) is 1 for every data type (measured).

Which field does it: for each row, the setting was sent in five frames only, then nothing more, and the continuous waveform was triggered while the pen kept drawing (observed).

| Setting sent | Vibration |
|---|---|
| Everything off (tip 0, ZFI 0, speed `ff`) | none |
| Tip bit only | none |
| ZFI only | none |
| Speed 40 only | yes |
| Tip, ZFI, speed 40 | yes |
| Everything off again | very weak |

- **The speed field switches it on.** The tip bit and ZFI are not needed.
- **The digitizer keeps the last value it received.** With nothing sent after the five frames, the pen vibrated for the whole 3.5 s; in a test that alternated no feedback and feedback, all five spans vibrated. The message does not have to go out on every frame: sending it when the value changes is enough.
- With only the FastHostId set and no feedback ever sent, there was no vibration. Whether speed alone, without a FastHostId, is enough was not separated.

### 1.3 The V06 `8e a5` message

Feature 9, 64 B with the report ID; offsets count from the report ID. This is the message as it was first sent from Linux, built from static analysis, and the one the tests of 1.2 used. The frames Windows really sends were read later and differ in several fields (1.8); the helper now sends those.

| Offset | Field | Value sent |
|---|---|---|
| 1–2 | `8e a5` | |
| 3 | Sequence number | +1 on every message |
| 4 | Section bitmap | `02` (pen section) |
| 9–12 | Pen ID | 0, or the 4 B the cover announced |
| 37 | LocalId | `ff` (none) |
| 38 | Attribute bitmap | bit 0 = tip |
| 40–41 | FastHostId | the host's value, little endian |
| 43 | ZFI | 1 |
| 45 | Speed | 0–63 (`ff` = unknown) |
| 47–48 | Valid bitmap | `0x0652` (`0x06d3` with a pen ID) |

Everything else is 0. LocalId, AckStatus, DigitizerAck and the protocol type are used by the identification procedure between pen and digitizer and are not made up.

These are the values those tests sent. What the helper sends now is the last column of the table in 1.8 (attributes `4c`/`4e`, protocol type 1, process type 2 and 4, valid bitmap `0x0352`/`0x0752`).

### 1.4 What was ruled out before this

| Tried on Linux | Result |
|---|---|
| The bytes of the Windows tool and of the Windows capture on Output 65 (`41 01 ff 09 00 00 00` every 1.4 s, repeat 0, `41 00 7f 09 00 00 00`), and the Auto Trigger of Feature 65 | Without the feedback: no continuous vibration (measured writes, observed). Clicks (`41 01 ff 03 00 00 00`, three of them on the same connection) are felt, so the link works and only the continuous waveforms were missing |
| The write type. Windows writes Output 65 as ATT Write Command (0x52, no response), BlueZ 5.87 as Write Request (0x12) (measured on both sides; the characteristic, properties 0x0e, allows both, and `forward_report` in BlueZ `profiles/input/hog-lib.c` picks Write first). BlueZ was changed to prefer Write Without Response | Every Output 65 write went out as Write Command (measured with `btmon`), three clicks were felt, and the continuous waveforms (ink at 127 and 255, pencil, brush) still did not vibrate, in the air or during a stroke (observed). **The write type is not the cause.** The BlueZ change is not in this repository |
| Connection interval and supervision timeout | The same on both sides: 15 ms and 1 s (on Linux after the pen asked for the change) |
| Replaying the 16 B exchange (section 4) | The pen answers as on Windows; no continuous vibration afterwards (observed) |
| FastHostId alone (`8e a1`) | No vibration (1.2) |

The same bytes are reported to give a continuous vibration on a Surface Pro 9 under Windows "without drawing" (README and source of the MIT-licensed [windows-slim-pen-2-tool](https://github.com/stephenlwaldron/windows-slim-pen-2-tool); not checked here, and its description does not say whether the pen was away from the screen).
That tool writes only Output 65 and does no initialisation, which already pointed at the Windows pen stack rather than the application.

### 1.5 In this repository

[`userspace/pen/surface_pen/feedback.py`](../../../userspace/pen/surface_pen/feedback.py) builds the two messages, and `surface-pen-feedback` is a daemon around it ([userspace/pen](../../../userspace/pen/README.md)):
it computes the speed from the positions of the iptsd stylus device and sends `8e a5` when the value changes. The continuous waveform itself is still started and stopped over Bluetooth with Output 65 (section 5).
The device helper `sp9-helperd` does the same with `sp9-ctl pen ink on` ([helper.md](../../helper.md), the section on the pen), and with the helper alone the continuous waveform played during strokes (observed). Run only one of the two: both write Feature 9.
Since then the helper sends the frames of 1.8 (contact in attribute bit 1, a speed only for tip contact, full scale 300 mm/s), takes its strength setting as the intensity byte of Output 65, and writes one click for the eraser end. The strength and the eraser click were confirmed on the device (observed: 30 % is not felt, 200 % is clearly stronger than 100 %; one click at eraser touch-down). The frames of 1.8 and the scale of 300 mm/s have not been felt on the device yet; `surface-pen-feedback` still sends the message of 1.3.

### 1.6 Not separated yet

- Whether the FastHostId (`8e a1`) is needed at all.
- How much the speed value changes the strength of the vibration. Faster strokes feel stronger, and between full scales of 580 and 145 mm/s (the helper's former strength 50 % and 200 %) a difference seems to be there but is small (a person's impressions). The former default full scale of 290 mm/s was chosen by feel as "about as strong as on Windows"; measured on Windows the largest value is reached at 300 mm/s, and the strength setting there is the intensity byte, not the speed scale (1.8).

### 1.7 The eraser end

With the eraser end there is no vibration on Linux (observed by a person). The input is there: the iptsd stylus device reports `BTN_TOOL_RUBBER`, `BTN_TOUCH` and positions, and the helper, at the time of these tests, wrote the start of the eraser waveform (ordinal 14) to the pen (measured).

Tried while the eraser end was rubbed on the screen, none of them vibrated:

| Written to the pen (Output 65) | Digitizer feedback (`8e a5`) |
|---|---|
| start eraser (14) | as the helper sends it (tip bit, speed) |
| start ink (9), then eraser (14), as Windows Journal does | the same |
| start ink (9) | the same |
| start eraser (14) | speed 40, attributes 0, ZFI 0 |
| start eraser (14) | speed 40, attribute bit 1, ZFI 0 |
| start eraser (14) | speed 40, attribute bits 0 and 1, ZFI 1 |
| start eraser (14) | speed 40, attribute bit 1, ZFI 1 |

The ink waveform, which plays with the tip, does not play with the eraser end either, so it is not a matter of which waveform is chosen. Attribute bit 1 was tried as a possible "eraser end" marker; it turned out to be the contact bit (1.8).

**On Windows there is no vibration while erasing with the eraser end either (observed by a person, OneNote).** Linux behaves like Windows here, so there is nothing to fix.

A vibration with the eraser end is still wanted on Linux (it tells the user that the haptics are active). Imitating Windows cannot give it, so it has to be made separately.

Tried while rubbing with the eraser end, with frames shaped like those of Windows (1.8: attributes `4e`, protocol type 1, process type alternating 2 and 4, about 130 a second) (observed by a person):

| Digitizer feedback | Written to the pen | Vibration |
|---|---|---|
| Exactly what Windows sends for the eraser (ZFI 3, speed `ff`, valid `0x0352`) | start eraser (14) | none |
| A valid speed of 40 for the contact (ZFI 1, valid `0x0752`) | start eraser (14) | none |
| The same | start ink (9) | none |
| Speed 63 | start eraser (14), intensity 254 | none |
| None | a click (ordinal 3) every 0.3 s | yes |

While the eraser end is in contact the pen plays no continuous waveform, whatever the digitizer is told. A click does play. The helper writes one click at eraser touch-down.

### 1.8 The `8e a5` frames Windows really sends

The Feature 9 buffer the touch processor writes to the digitizer was read on Windows while strokes were drawn (measured: two runs, 179 s with 8130 frames and 143 s with 8515 frames; 1 frame missing by the sequence numbers).
While the pen is in range it is written 134 times a second. Each frame was matched with the pen's Raw Input at the same time (tip, side button, eraser, pressure, tilt, position) and with an HCI capture (Output 65) to tell the states apart.

| Offset | Field | Hover (pen identified) | Tip contact | Eraser contact | What the Linux helper sends |
|---|---|---|---|---|---|
| 4 | Section bitmap | `02` | `03` | `02` | `02` |
| 5, 7–8 | unknown | `01`, `90 01` | same | same | `01`, `90 01` |
| 9–12 | Pen ID | 4 B (the same as in the cover's report 110) | same | 0 | 0 |
| 13–14 | A 16-bit value that fell slowly from 5761 to 5677 in 3 minutes (taken to be the pen energy) | | | | 0 |
| 37 | LocalId | `02` | `02` | `ff` | `ff` |
| 38 | Attribute bitmap | `4c` | `4e` | `4e` | `4c`, `4e` in contact |
| 42 | Protocol type | `01` | `01` | `01` | `01` |
| 43 | ZFI | 3 (sometimes 2) | 1 (sometimes 2); 0 at the edge of the screen | 3 | 3 for hover and eraser, 1 for tip contact |
| 44 | Process type | 2 and 4 alternating per frame | same | same | 2 and 4 alternating (parity of the sequence number) |
| 45 | Speed | `ff` | 0–63 | `ff` | 0–63 for tip contact only, `ff` otherwise |
| 47–48 | Valid bitmap | `0x03db` | `0x07db` | `0x0352` | `0x0352`, `0x0752` with a speed |

- **Contact is attribute bit 1** (`4c` → `4e`). Bits 2, 3 and 6 are always set and bit 0 never was. The message of 1.3 put the tip into bit 0 (because the speed alone switches the continuous waveforms on, this did no harm). The helper now puts it into bit 1, as Windows does.
- `0x0400` of the valid bitmap is the "speed valid" bit (set only in contact). The Windows values also have `0x0008` and `0x0100`, which the message of 1.3 did not set.
- **While rubbing with the eraser end Windows sends the speed as `ff` (unknown).** The contact bit is set, but ZFI is 3 as in hover and there is no pen ID or LocalId. Journal meanwhile writes a start for ink (9) and for eraser (14) to the pen, and there is no vibration (observed by a person). This agrees with the speed switching the continuous waveforms on: Windows gives no speed for the eraser end.
- Speed: 0–6 for slow strokes, 0–63 for fast strokes (median 35–46), 0 when pressed and held still. The correlation with the pen speed computed from the Raw Input positions is 0.994 and one step is 4.74 mm/s. 63 is reached at 300 mm/s: the formula from static analysis, `min(v, 30) × 63 / 30` with v in 0.01 mm/ms, is exactly 300 mm/s.
  The helper's speed estimator run over the Windows Raw Input gives the Windows values at a full scale of 300 (ratio 1.000; 0.855 at 353). The earlier figures of 5.6 mm/s per step and about 350 mm/s for 63 came from a wrong fit. Below about 15 mm/s the value is 0 (a dead zone).
- For about 1 s after the pen has left, hover frames without a pen ID (LocalId `ff`, valid `0x0352`) continue, then they stop.
- **The side button does not appear in the feedback.** 1112 frames drawn with the side button pressed have the same values as plain strokes.
- Also during tip contact there are frames without pen ID and LocalId (section `02`, valid `0x0752`, a valid speed): about half of the contact frames, independent of the side button and of the application. When the pen counts as identified was not separated. The pen vibrated in both cases.
- **Pressure and tilt change no field** (pressure 95–62607, tilt 3–50°). Only ZFI follows the position: frames drawn within 5 mm of the screen edge have ZFI 0 (77 % of the 630 contact frames with ZFI 0 are at the edge).
- **After the display is switched off and on, `8e a1` is sent again**: once right after it comes on, and once more 3.8 s later with a raised sequence number. Its content was the same before and after (it holds the pen ID and the FastHostId). `8e a5` frames follow.
- No `8e a2` was seen in this buffer. With a debugger attached, an `8e a2` (section `04`) went out about once per five `8e a5` (it uses another buffer, or stays for less than the polling interval).

The tactile signal strength setting (Settings → Pen and Windows Ink):

| Setting | Intensity byte of the Output 65 start written to the pen | Digitizer feedback |
|---|---|---|
| Lowest | 2 | the same (speed 0–63) |
| Default | 127 | the same |
| Highest | 254 | the same |
| Off | no start is written (only the stop) | the same |

The strength on Windows is the one intensity byte of Output 65. It does not rescale the speed value.

How it was read: the buffer was read, read-only and from outside, from the memory of the process that hosts the touch processor (`ISM.exe`, with `TouchPenProcessor0C52.dll` 18.124.139.0), with a polling script that is not in this repository. No debugger is involved and input is not affected.
The input service does not write Feature 9 through `HidD_SetFeature` or `NtDeviceIoControlFile` but through `NtRIMDeviceIoControl` of `win32u` (device, an IOCTL, buffer, length 186). A debugger breakpoint there shows the same bytes, but it slows the touch processor to 25 frames a second, and detaching the debugger during a stroke kills the input service (0x80000004; Windows restarts it).

## 2. Cover-slot coupling

On Windows, after the pen is removed from the paired devices, laying it in the pen slot of the Type Cover is enough to bond it again (observed, three times). The tail button is not held and the pen does not enter pairing mode.
The address and the pairing key travel outside Bluetooth, through the cover: it is an out-of-band (OOB) pairing.

### 2.1 The cover's pen collections

Two collections of the Type Cover (KIP, PID 09B0) on vendor page FFF4 announce the pen (measured on Windows, read with hidapi).

| Collection | Report | Content | When |
|---|---|---|---|
| Col03 FFF4:0001 (pen bonding; `SurfacePenBleLcAddrAdaptationDriver` binds to it) | Input 110, 11 B | 4 B pen ID (differs per pen, not given) + 6 B pen Bluetooth address + 1 B flags (`07`) | Once when the pen is put in |
| Col02 FFF4:000A ("Surface HID Pen Bluetooth OOB Coupling") | Input 20, 8 B | 6 B pen Bluetooth address + 1 B flags + 1 B battery % (0–100) | Flags `07` when put in; on removal `03`, then `01` after 0.2 s |

Report 110 arrives 0.35 s before report 20. The bonding collection on the touch controller's side (THC Col07, the same descriptor) received nothing at that moment.

The reports of the cover's FFF4:0001 collection (descriptor from the Linux hidraw node; the same collection of the digitizer is identical, [userspace/pen](../../../userspace/pen/README.md)):

| Report | Fields |
|---|---|
| Input 84 | pen ID 4 B, pen address 6 B, bit 0008 |
| Input 110 | pen ID 4 B, pen address 6 B, bits 0008, 0011, 0012 |
| Output 85 | pen ID 4 B, pen address 6 B, bit 0008, usage 0004 8 B |
| Output 111 | pen ID 4 B, pen address 6 B, bit 0008, usage 0014 16 B |
| Feature 86 | usage 0003 6 B, bit 0009 |
| Feature 112 | bits 0010, 0022 |
| Feature 115 | usage 0021 2 B |

Requests to Col03 during a coupling on Windows (measured: HIDCLASS ETW, which gives the request type and the report ID but not the contents):

| +s | Request | Report (size in the descriptor) | By |
|---|---|---|---|
| 0 | SET_FEATURE | 115 (2 B) | System (a kernel driver) |
| +0.004 | GET_FEATURE | 112 (two flag bits) | svchost |
| +1.03 | SET_FEATURE | 112 | svchost |
| +1.03 | SET_FEATURE | 86 (6 B, the size of an address) | svchost |
| +2.01 | GET_FEATURE | 115 | System |

Output reports 85 and 111 do not appear in this trace. That does not mean they were not written: the service writes Output 111 to the device handle rather than through the set-output-report request (2.2).

### 2.2 What Windows does (static analysis)

- The coupling is done by the loosely-coupled-devices component of `Microsoft.Bluetooth.Service.dll` (its OOB coupling channel, temporary key and coupling key parts). It generates a TemporaryKey of 16 B and a CouplingKey of 8 B with `BCryptGenRandom`.
- The SMP Temporary Key (TK) is not derived from anything the cover reports (measured: about 2,700 combinations of arrangement, reversal and MD5/SHA1/SHA256 of the 4 B pen ID, the pen address and the host address were tried against the Pairing Confirm and the STK of the capture, and none matches. The same computation does match for the Just Works pairing on Linux, where TK is 0).
- `SurfacePenBleLcAddrAdaptationDriver.sys` has nothing to do with the TK. It makes the pen's coupling address out of the pen ID (2.4).

The order of what the service does:

1. When it opens the channel it reads Feature 112, writes bit 0010, and writes the 6 B host address (usage 0003) to Feature 86. Bit 0009 is set only when there is a record that the pen is in the slot.
2. It reads the pen ID, the pen address and bits 0008, 0011 and 0012 from Input 84 or 110.
3. For a pen it has not seen before it generates the 16 B TemporaryKey and writes it to Output 111: the pen address (usage 0002) as received, bit 0008 set if it was set in the input, and the key in usage 0014 unprocessed. This is a plain write to the device handle, not a set-output-report request, which is why HIDCLASS ETW did not record it.
4. It puts the same 16 B into a 46 B OOB data block (pen address + tag `0x1011` + key) and pairs from that block (`DafCreateAssociationContextFromOobBlob`). No scan: it connects straight to the address from the report. The SMP TK is these 16 B.
5. For a pen it already knows it does not send the key again. There is a separate path that sends the 8 B CouplingKey, kept in the registry (`…\Bluetooth\LooselyCoupledDevices`, value `CouplingKey`), with Output 85.
6. AES-CMAC is used to authenticate pen button events (single, double, press and hold): the CMAC of 4 B pen ID ‖ 4 B counter is compared with the tag in the report, and the counter guards against replay. How the 16 B CMAC key follows from the CouplingKey was not seen.

So Output 111 does not carry a signature: its 16 B field is the pairing key itself.

On the Bluetooth side (measured, capture):

- Windows puts the pen's address on the Filter Accept List before any scan result and issues LE Extended Create Connection. The connection is up within 0.01–0.2 s.
- SMP is LE legacy pairing with the OOB data flag set in both request and response (IO capability NoInputNoOutput, MITM requested). Windows offers its identity key, the pen accepts it and so receives the host's IRK and public address.
- After bonding, the service discovery and the CCCD writes (0x000d, 0x001c and the six Input reports) are followed by the exchange of section 4, 5.8 s after the connection. No vendor value is exchanged only at bonding. In the cover the pen disconnects by itself 11 s after bonding.

### 2.3 What the cover does on Linux (measured)

The cover is the hidraw node `Microsoft Surface 045E:09B0`.

- When nothing is written, putting the pen in and taking it out brings no Input 110 or 20 (only keyboard report 92 arrives).
- **Writing Feature 86 makes the cover announce the pen that is in the slot at once**, and again on every write. Writing only Feature 112 brings nothing.
- Bit 0010 of Feature 112 selects the report: 1 gives Input 110 (and Input 20), 0 gives Input 84. Reading 112 returns `02` whatever was written.
- The flags of Input 110 were `07` (as on Windows) with bit 0009 of Feature 86 at 0, and `05` with it at 1. After Output 111 has been written once they are `05` also with the bit at 0.
- The flags of Input 20 become `07` on each announcement and fall back to `03` after 1 s. On Windows they stay `07` until the pen is taken out.
- **Writing Output 111 right after Input 110 (pen ID and address as received, bit 0008 as in the input, any 16 B) and then issuing LE Extended Create Connection to the pen's coupling address connects.** The order is Feature 112 = 1, Feature 86 = host address (LSB first, bit 0009 = 0), Input 110, Output 111, connect.
- A pen in pairing mode (tail button held) does not set the OOB flag in its Pairing Response, also after a key has been written through the cover. It bonds with Just Works.
- Feature 115 reads `ff ff`, idle and right after Input 110.

### 2.4 The pen has two addresses

For cover coupling the pen uses a static random address different from the one of pairing mode (measured: the connection and bonding address in the Windows capture, the name of the bond's key in the Windows registry, and the connection made from Linux).

| | Pairing mode (tail button held) | Cover coupling |
|---|---|---|
| The pen's address | The address carried in the cover's Input 110 as Linux hidraw sees it | A different address. `SurfacePenBleLcAddrAdaptationDriver` makes it and hands it to the service |
| Advertising | Undirected ADV_IND | ADV_DIRECT_IND aimed at the host. It does not show up in scan results |
| Pairing | Just Works | LE legacy OOB |
| Address at reconnection after bonding | (see section 3) | A new address each time (RPA). Windows keeps the coupling address on the Filter Accept List and resolves with the IRK |

The tests in which the pen was seen "not advertising" on Linux looked at the pairing-mode address only. On the coupling address the pen accepts a connection after Output 111.

The coupling address follows from the pen ID and the host address (static analysis, verified with this unit's values):

    H = the low 46 bits of the first 8 B, read little endian, of SHA1(a 16 B constant of the driver ‖ the 4 B pen ID reversed), with the two top bits set to 1
    coupling address = H XOR host address   (bit 0012 of Input 110 set, and Feature 86 written)
    coupling address = NOT H                (otherwise: Input 84, or Feature 86 never seen. Not seen over the air)

This is `coupled_address` in [`userspace/pen/surface_pen/col07.py`](../../../userspace/pen/surface_pen/col07.py). Because the host address goes in, the address differs per host.

What the Windows filter driver (`SurfacePenBleLcAddrAdaptationDriver`) does (static analysis):

- On the way up it rewrites the address field of Input 84 and 110 to the coupling address. Pen ID and flags are untouched. That is why the address read from Input 110 with hidapi on Windows is the coupling address, while Linux hidraw sees the pairing address from before the rewrite.
- On the way down it leaves the address field of Output 85 and 111 alone, so on Windows the address the cover receives is the coupling address. It only fills in the pen ID it remembers for that address. On Linux the pen accepted the connection with the pairing address in that field as well.
- It does not change Feature 86; it only notes the host address. When Feature 112 is read and the device reports bit 0022, it also sets bit 0010 on the way up (the value that reads `02` on Linux is `03` for the service).
- Feature 115 carries a 16-bit value between the two devices that have this collection (cover and digitizer). It has nothing to do with addresses.
- The 8 B of Output 85 (the CouplingKey) are stored in the registry as a security key and used to verify, with AES-CMAC, an 8 B MAC sent by another component. This unit's registry has no such value: the path has never run here.

### 2.5 Coupling on Linux (measured)

First done by hand with the pen lying in the cover, on the booted 7.3.4 kernel with a `bluetooth.ko` that carries [patch 0023](../../../kernel/PROVENANCE.md) loaded as a module.

1. Make BlueZ pairable (bondable on the management interface). When it is off the pairing request goes out as "No bonding" and no keys arrive.
2. Write Feature 112 = 1 and Feature 86 = host address (LSB first, bit 0009 = 0) to the cover. The cover announces the pen with Input 110 (flags `07`).
3. Compute the coupling address from the pen ID and the host address (2.4).
4. Pick 16 random bytes as the TK, hand them to the kernel ([`tools/linux/le-legacy-oob.py`](../../../tools/linux/le-legacy-oob.py) `add-tk`), and write the same 16 B in the same order to Output 111.
5. Issue LE Extended Create Connection to the coupling address (the pen's directed advertising lasts only about 1 s). The connection is up within 0.1 s.
6. Pair Device (`le-legacy-oob.py pair`). The request has the OOB flag, Bonding + MITM (0x05) and the keys EncKey, IdKey, Sign. The pen's response has the OOB flag, Bonding (0x01), host key IdKey, pen keys EncKey and IdKey. The pen gives its LTK, IRK and address; the host gives its IRK (0, because privacy is off) and its public address.
7. Remove the TK.

- The pen is bonded within 1 s and BlueZ stores the LTK and the IRK.
- The byte order of the TK is the order written to the cover (the Pairing Confirm passed).
- Right after the bonding BlueZ does not attach the pen's HID (HoG): the connection was made outside BlueZ and service discovery finished before the bonding. After a restart of bluetoothd the stored bond gets HoG attached and auto-connect armed. From then on the pen advertises with an RPA and BlueZ resolves it with the IRK and connects.

Kernel patch 0023 adds what the stock kernel lacks: LE legacy pairing with a TK supplied out of band (through the extended form of Add Remote OOB Data), with the host's identity key offered in that pairing (section 3). The initiator path is what ran here;
the responder path and the path combined with Secure Connections OOB data are compile-tested only.

This procedure now lives in the device helper `sp9-helperd` ([helper-reference.md](../../helper-reference.md#cover-coupling)): with `sp9-ctl pen auto-couple on` a pen without a bond is coupled when it is put into the cover, and `sp9-ctl pen couple` does it once. Measured with the helper in this order: connection 0.4 s, pairing 0.5 s, Paired, Bonded and Trusted in BlueZ, and the pen's HID attached without restarting bluetoothd. The helper has since changed the order (the key is handed over first, and address resolution is off while it connects: 2.8 and 2.9). A pen that already has a bond gives `already-coupled`. A `pen couple` issued right after a coupling fails because the pen does not answer the connection, and leaves the pen without a bond until it is run again a few seconds later.
Pair Device has to wait for the security channel of the new connection: issued right after the connection comes up, the kernel returns success at once without pairing, so the helper takes the pen's New Long Term Key event as the proof and repeats the step when it does not come.
A kernel built with 0023 has been booted since, and coupling (through the helper) and reconnection work on it as well (measured).

A different route to a working bond, without the kernel patch: [`tools/linux/pen-bond-from-windows.py`](../../../tools/linux/pen-bond-from-windows.py) copies the bond Windows made through the cover into BlueZ (both systems use the same adapter address). No result obtained with a copied bond is recorded in this note.

### 2.6 Holding the bonding collections open blocks the coupling on Windows

While the cover's Col02 and Col03 and the THC's Col07 were held open and read with hidapi, coupling failed twice out of two attempts (measured). The cover sent reports 110 and 20 and Windows did issue Create Connection,
but it was cancelled after 7.7 s (status 0x02). The address Windows connected to was then the pairing address, not the coupling address (measured: capture). Whether the pen was advertising is unknown. As soon as the reading stopped, coupling worked.
Which of the three collections blocks it was not separated. Tracing this on Windows therefore has to use ETW only, without opening the collections.

### 2.7 A bonded pen lying in the cover (measured on Linux)

- A pen that has just been bonded stays connected while it lies in the cover (for more than 100 s).
- After a resume from suspend the pen in the cover is disconnected and does not reconnect by itself (60 s). The bond is intact. "Bonded, in the cover, not connected" is therefore a normal state.
- In that state the pen does not accept a connection without a key: Feature 86 → Input 110 (flags `07`) → Create Connection to the coupling address alone does not connect within 4 s.
- **With a key it does**: adding Output 111 (any 16 B, not given to the kernel) to the same sequence connects, and 0.1 s later the host starts encryption with the stored LTK (bluetoothd and the kernel do this by themselves) and succeeds. BlueZ shows Connected and the pen's HID attaches. The pen is back on its old bond, without a new pairing.
- After the host drops the connection (`bluetoothctl disconnect`, `RemoveDevice` of BlueZ) the pen does not advertise for more than 30 s, also when it is moved. After the pen has disconnected by itself (idle) it is back 3.5 s after being picked up.
- A bonded pen must therefore not be judged stale because it is not connected, have its bond removed and be coupled again. A version of the helper that did so removed a good bond, the pen did not answer, and it was left without a bond (measured). Whether Windows has replaced the bond is told by whether encryption is refused in the "key, then connect" sequence above (a refusal has not been seen yet). The helper is built on this: [helper-reference.md](../../helper-reference.md#cover-coupling).

### 2.8 The key reaches the pen late

A reading of the traces below: **a key written with Output 111 reaches the pen only 0.4–1 s later, and the pen accepts a connection before that.** A pairing started too early meets a pen without a key (no OOB flag in its response) or a pen that holds an earlier key (Confirm value failed). The Confirm value failures turned out to have another cause (2.9), and whether the hand-over order matters is not confirmed.

Measured:

- After a reboot, `sp9-ctl pen couple --force` ended four times in a row in `Pair Device: authentication failed`. The traces of those attempts: Pairing Request (OOB flag, 0x05) → Pairing Response (OOB flag) → the host's Confirm, the pen's Confirm, the host's Random → Pairing Failed from the pen, reason Confirm value failed.
  Checked offline, the host's Confirm value was not computed with a zero key: the kernel used the TK it was given.
- A coupling by hand (the procedure of 2.5) with **the same 16 B that had been written before** then worked at once (LTK + IRK, Paired and Bonded).
- In another trace: Output 111 written, connection complete 30 ms later, Pair Device 300 ms after that. The pen's Pairing Response had **no OOB flag** (it had no key yet). The kernel fell back to another method and raised a User Confirmation Request, which was rejected: Pairing Failed (Passkey entry failed).
  **On the same link**, in the pairing bluetoothd started by itself 0.85 s later, the pen's response had the OOB flag. A failed pairing does not take the link down.
- Right after that the procedure by hand (a new random key; adding the TK, connecting and pairing run as separate processes, so it is slow) worked at the first attempt.

Measured later, on a kernel whose patch 0023 removes the TK once a pairing has used it:

- Key written 55 ms before the connection, Pair Device 1 s after it: Request and Response both had the OOB flag, and the pen answered Pairing Failed, Confirm value failed. The second SMP exchange on the same link (bluetoothd's pairing) had no OOB flag in the Request (the kernel had already removed the TK): Passkey entry failed, and the link went down.
  Two more attempts 3.5 s apart, each asking the cover again, writing the same key and connecting: the pen answered without the OOB flag at +1 s and at +2 s.
- The procedure by hand right afterwards: the first attempt (a new random key) was refused (no OOB flag); a second attempt 25 s later with **yet another** new key worked. So it is not repeating the same key that makes it work, and a key written just before the connection cannot be relied on to arrive in time.
- **Handing the key over first and pairing afterwards worked (3 times out of 3).** Each time on a pen whose bond had just been removed (50 s after the host dropped the link): Features 112 and 86 → Input 110 → TK to the kernel → Output 111 (the key) → **2.5 s without connecting** → Feature 86 again → Input 110 → Output 111 (the same key) → LE Extended Create Connection → Pair Device 0.3–0.5 s after the connection → New Long Term Key at the first attempt.

Interpretation (inferred): the pen takes a key over the cover's link only while it is not connected, and that hand-over takes longer than the interval between the write and the connection. With a key handed over in advance the pen is ready at the second write and the connection.

What the helper does with this:

- A coupling starts by handing the key over (Output 111 without a connection, 2.5 s), then asks the cover again, writes the same key again and connects.
- Pair Device is sent about 0.4 s after the connection. With 1 s it overlaps the pairing bluetoothd starts by itself, and on a kernel where the TK is used once that pairing consumes the key.
- Repeating a few seconds after a refusal did not help. The next round comes more than 20 s later.
- A key written without a pairing stays in the pen (it is not harmless).
- Whether the cover hands the key over late or the pen applies it late, and how much margin the 2.5 s have, was not separated.

### 2.9 The cause of Confirm value failed: a link on which the controller resolved the pen's private address

The Confirm value failures of 2.8 were not a matter of the key. **Every link on which the pairing failed was one where the controller had resolved the pen's RPA and reported the identity address, and on such a link the kernel computes the Confirm value wrongly** (measured).

- In two failed traces of the helper, the LE Enhanced Connection Complete has the peer address type Resolved Random (0x03) and a filled-in peer resolvable private address, all five times. In 22 successful traces (2 by hand, 20 with the helper) the type is Random (0x01) and the RPA field is 0 every time.
- Computing c1 offline from the values of a failed trace, the host's Confirm matches only with the TK (as given to the kernel), responder address = **the identity address** and responder address type = **3**. In the successful traces it matches with the identity address (= the static coupling address) and type 1.
  The pen computes with the address it used over the air (the RPA) and type 1, so the two values cannot agree. The kernel's `le_conn_update_addr()` puts the identity address and the type (0x03) of the event into `resp_addr` and `resp_addr_type` as they are and does not use `peer_rpa`. A bonded device is not paired again on such a link, so this does not show in normal use.
- How such a link comes about: the pen is still on the controller's resolving list (address resolution on), and the pen still trusts its old bond and advertises for reconnection from an RPA. The Create Connection issued to the coupling address then matches that RPA advertising and not the directed advertising of the cover coupling.
  Reproduced (2 times out of 2): drop the link of a bonded pen (the kernel puts the pen on the list and scans), and remove the bond a few seconds later. The next coupling comes up as a Resolved Random link and ends in Confirm value failed. Removing a bond does not make the kernel touch the controller's list (measured: in the 8 s after `bluetoothctl remove` the only HCI command is Disconnect).
  Removing the bond while the pen is connected does not lead there (while the pen keeps its connection in the cover the kernel does not scan and the pen is not on the list): in that state the pen coupled at the first attempt 14 times out of 14.
- **The fix in the helper**: right before the Create Connection of a coupling it sends LE Set Address Resolution Enable = 0 on the raw HCI socket, and sets it back to 1 when the connection attempt is over. With resolution off only the directed advertising from the static coupling address matches. In the reproduced state the pen coupled at the first attempt on a Random link 6 times out of 6 (0 out of 2 before the fix).
- The proper fix on the kernel side is to set `resp_addr` (as central) to `peer_rpa` with type random on a connection whose address the controller resolved. There is no patch for that yet.

What this result puts in question:

- The interpretation of 2.8, that the key has to be handed over first, remains unconfirmed. Success and failure in those measurements may have been decided by the kind of link and not by the order of the hand-over (every failed trace was a Resolved link). An order without the early hand-over was not measured again on a Random link.
- The observation that the pen answered without the OOB flag (a pen without a key yet) stands apart from this cause. Those traces were Resolved links as well.
- After a pairing in which a wrong key was given to the kernel on purpose (Confirm value failed in both rounds), a coupling with the same key 35 s later worked at the first attempt: a failed pairing leaves nothing behind in the pen (measured).

## 3. Reconnection depends on the host's identity key

- The pen disconnects by itself when unused (Disconnection Complete, reason 0x13): after 69 s lying on a desk, and 11 s after bonding in the cover's slot. Windows sends nothing in that time.
- When it moves, the pen wakes and advertises (measurement lined up with a person's observation). Picking it up is enough; the tail button does not have to be pressed. The tail button notification (handle 0x0047, `08 00 03 00 00 00 00 00`, then all zeros 1 ms later) arrives on a connection that already exists.
- Right after a disconnection Windows puts the pen's address on the Filter Accept List and switches scanning back on. The scan parameters are set when the adapter is powered on: passive, filter policy 1, interval 367.5 ms, window 37.5 ms.
  The Windows capture has no standard Resolving List commands: every time Windows adds to the Filter Accept List it issues an Intel vendor command (0xFC1E) and resolves the pen's RPA itself. Linux does the same job with the standard commands (the pen on the Resolving List, address resolution in the controller on).
- At reconnection the pen sends ADV_IND (connectable, scannable, undirected, 0 B of data) from an RPA.

**Whether the pen advertises for reconnection depends on the bond the pen holds, not on the host OS or the state of the digitizer** (measured):

| Bond | After the idle disconnect |
|---|---|
| Linux, pairing mode, Just Works (the host does not offer its identity key: the kernel offers it only with `HCI_PRIVACY`) | No reconnection advertising. In one test the pen disconnected after 53 s and nothing was received in 100 s of picking it up, drawing and pressing the tail button. In an unfiltered active scan (86 s, 1,144 advertisements from 14 addresses) there was neither the pairing address nor the coupling address, no RPA that resolves with any IRK the pen had given, and no ADV_IND with 0 B of data (checked again offline). A directed advertisement aimed at another address would not be visible through HCI |
| Windows, through the cover | The pen advertises while Linux is listening: in the first 8 s of one Linux capture there are 160 ADV_IND with 0 B of data from two RPAs, and both resolve with the IRK the pen gave in its last Windows bonding |
| Linux, cover coupling, host identity key not offered (the kernel default with privacy off) | The pen takes no host key at all and did not advertise for reconnection: 20 s of unfiltered scanning after the disconnect contain no address that resolves with the pen's IRK |
| Linux, cover coupling with patch 0023 offering the identity key | **Reconnects.** The pen disconnected by itself after 50 s and was connected again 3.5 s after it was picked up and drawn with. Checked again on a booted kernel built with 0023, with the bond made by the helper |

There is no capture of a pen bonded in pairing mode on Windows: all 15 connections of the Windows capture are the four cover couplings and their reconnections.
So what a pairing-mode bond does when the host does offer its identity key was not tested.

To repeat the unfiltered scan: bluetoothd comes back through D-Bus activation, so block it with a runtime drop-in that replaces `ExecStart` of `bluetooth.service`. The kernel starts a filtered scan for bonded devices by itself even without bluetoothd,
so switch scanning off (`hcitool cmd 0x08 0x0042`) and address resolution off (`0x08 0x002d`) before writing scan parameters; otherwise the controller answers Command Disallowed. Look for the pen by resolving RPAs with its IRK, not by address.

## 4. The 16 B exchange is a CFU firmware offer

**The exchange is a firmware update check: the offer phase of Component Firmware Update (Microsoft's HID firmware update protocol). It is not an initialisation that switches on haptics or connection keeping, and Linux has no reason to imitate it.**

### 4.1 The handles

| Fact | Evidence |
|---|---|
| Both handles are Report characteristics of the HID service (0x1812): 0x0062 is written (properties 0x0e), 0x004f notifies (0x1a) | The attribute table in the BlueZ GATT cache (`/var/lib/bluetooth/<adapter>/cache/<pen>`) |
| **0x0062 = Output 45, 0x004f = Input 45**: the exchange is a write and a notification of the same report ID | Measured. Report Reference descriptors (0x2908) captured with `btmon` at bonding. Input: 0x0047 = 1, 0x004b = 44, 0x004f = 45, 0x0053 = 51, 0x0057 = 52, 0x005b = 50. Output: 0x005f = 42, 0x0062 = 45, 0x0065 = 51, 0x0068 = 52, 0x006b = 65. Feature: 0x006e = 42, 0x0071 = 47, 0x0074 = 65, 0x0077 = 66, 0x007a = 64 |
| Report 45 is 32 bits × 4 (16 B) on vendor page FF0B, the same shape as Input and as Output | The pen's descriptor (Linux hidraw) |
| At bonding Linux (BlueZ) enables the notifications of all six Input reports (CCCD) and writes nothing else. In 70 s connected the pen sent no input report (tail button not pressed) | Measured (`btmon`, reading hidraw). The pen does not speak first; Input 45 is taken to come only in answer to Output 45 (inferred) |
| The GATT writes are otherwise the same on both systems: eight CCCDs (0x000d, 0x001c and the six Input reports) | Measured |
| The earlier Windows trace of [pen-ble-trace.md](pen-ble-trace.md) does not contain these values | Measured (string search): BTHPORT HCIRAW truncates ATT values |

### 4.2 The values

What Windows writes on every connection (handle 0x0062, Output 45, ATT Write Request) and what the pen answers (handle 0x004f, Input 45, notification).
Measured: five reconnections (three after switching the radio off and on, one on taking the pen out of the cover, one woken 258 s after a disconnect that followed 69 s of rest) and one bonding; all 14 values are the same in all six.
Bytes are in the order sent over the air.

| # | Write (Output 45) | Notification (Input 45) | Write → notification |
|---|---|---|---|
| 1 | `00 00 ff a0 00 00 00 00 00 00 00 00 00 00 00 00` | `00 00 00 a0 00 00 00 00 00 00 00 00 01 00 00 00` | 44 ms |
| 2 | `01 00 ff a0 00 00 00 00 00 00 00 00 00 00 00 00` | `00 00 00 a0 00 00 00 00 00 00 00 00 01 00 00 00` | 29 ms |
| 3 | `00 00 11 a0 8b 96 00 05 01 00 00 00 14 05 43 00` | `00 00 00 a0 00 00 00 00 00 00 00 00 02 00 00 00` | 28 ms |
| 4 | `00 00 11 a0 8b 96 00 05 01 00 00 00 04 05 43 00` | `00 00 00 a0 00 00 00 00 04 00 00 00 02 00 00 00` | 27 ms |
| 5 | `00 00 13 a0 8b 75 00 05 01 00 00 00 04 05 43 00` | `00 00 00 a0 00 00 00 00 00 00 00 00 02 00 00 00` | 28 ms |
| 6 | `00 00 10 a0 8b 8e 00 05 01 00 00 00 04 03 43 00` | `00 00 00 a0 00 00 00 00 00 00 00 00 02 00 00 00` | 28 ms |
| 7 | `02 00 ff a0 00 00 00 00 00 00 00 00 00 00 00 00` | `00 00 00 a0 00 00 00 00 00 00 00 00 01 00 00 00` | 28 ms |

- The order is write → Write Response → notification → next write. The next write goes out 1–6 ms after the notification (about 30 ms between the first three writes, where reads are interleaved). The seven take 250–290 ms. The intervals in the table are those of one reconnection.
- Whether the next write depends on the notification is unknown: the notifications were the same all six times.

### 4.3 What it is

- Writes 3–6 equal the four `*.offer.bin` files (16 B each) of the pen firmware driver package (`surfacepen0c0ffirmwareupdate.inf`) except for the fourth byte: `00` in the file, `a0` over the air (a token the driver inserts) (measured: files compared with the capture).

| Write | Offer file | Component | Version | The pen's answer |
|---|---|---|---|---|
| 3 | `BLE-HI` | 0x11 | 5.0.150.139 | reject (2), reason 0 |
| 4 | `BLE-LO` | 0x11 | 5.0.150.139 | reject (2), reason 4 |
| 5 | `HAPTIC` | 0x13 | 5.0.117.139 | reject (2), reason 0 |
| 6 | `MCU-B0` | 0x10 | 5.0.142.139 | reject (2), reason 0 |

- Writes 1, 2 and 7 are information packets for component 0xFF. Their first byte 0, 1, 2 is, in the CFU specification, start of the entire transaction, start of the offer list and end of the offer list, and the pen answers accept (1).
- The last word of the notification is the status, the third word the reject reason. In the CFU specification reason 0 is old (not newer) firmware and 4 is a bank mismatch. The conversation says that the pen's firmware is already at that version or later and there is nothing to update.
- It is done by `SurfaceCFUOverHid.dll` (UMDF, `surfacecfuoverhid.inf` 5.65.139.0), which binds to Col03 of the pen's BLE HID (`HID\VID_045E&UP:FF0B_U:0101`, device name "SurfacePen0C0F Firmware Update"). That is why it happens on every connection.
- The Linux project [slimpend](https://github.com/zeroxoneafour/slimpend) has four 16 B "initialisation" values (`initialize_device` in `src/hid/pen.rs`; disabled, unexplained, and sent to report 42 by mistake). They are the first four writes of this exchange (measured) and have nothing to do with haptics.

### 4.4 When it happens

Reconnection (bonded pen, reception of the advertisement as 0):

| +ms | What |
|---|---|
| 0 | Advertisement received, LE Extended Create Connection |
| +12–27 | Connection complete, LE Enable Encryption (stored LTK, no SMP) |
| +140 | Encryption Change |
| +190–350 | MTU exchange, reads of 0x006e (Feature 42), 0x0077 (Feature 66), 0x007a (Feature 64) |
| +280–530 | The seven round trips (interleaved with the reads: the first write after the read of 0x0077, the second after the read of 0x007a) |

At bonding the same seven round trips come 5.8 s after the connection (2.2).

### 4.5 On Linux

Writing the same seven values on Linux makes the pen answer with the same seven values as in the Windows capture (measured). Afterwards there was still no continuous waveform (observed), and the pen did not reconnect within 10 s of switching the radio off and on.
The exchange is not imitated: these are firmware offers, and a wrong value amounts to proposing an update to the pen (the same kind of risk as the hung pen described in [userspace/pen](../../../userspace/pen/README.md)).

## 5. Output 65 while inking on Windows

From the same capture: what Windows wrote to handle 0x006b (Output 65) while strokes were drawn in OneNote and Journal (ATT Write Command; the 6 B after the report ID). All 28 writes have one of two shapes.

| Value | Meaning | When |
|---|---|---|
| `00 7f NN 00 00 00` | Start: intensity 127, waveform ordinal NN, the rest 0 | Once at the start of a stroke |
| `00 fe 02 00 00 00` | Stop: intensity 254, ordinal 2 | Once at the end of a stroke |

- The ordinal follows the application's tool. OneNote used only 13 (brush); Journal used 9 (ink), 10 (pencil), 12 (chisel-marker) and 14 (eraser).
- The Journal eraser writes a start for 9 and one for 14 in succession, 2–9 ms apart, and two stops at the end.
- In this capture nothing is written during a stroke: strokes of 3.3 s, 3.6 s and 9.1 s have only the start and the stop (the listed duration of the waveforms is 2.0 s). Exception: an earlier Journal trace, kept outside the repository, has spans of up to 0.5 s with about 140 writes a second.
- The vibration lasts for the whole stroke (observed, the 9.1 s stroke). The pen repeats the waveform until it receives the stop.
- These are the bytes the Linux unit tests use (`41 00 7f 09 00 00 00`). The reason continuous waveforms were weak or absent on Linux is neither this value nor the 16 B exchange; it is section 1.

## 6. How the values were captured

For untruncated values the full packet logging of BTHPORT has to be on. Three values under `BthPort\Parameters` are written (`MaxEtwBytes` = 0x400, `EtwDropLargeEvents` = 0, `EtwLogSensitiveData` = 1)
and IOCTL 0x411008 (input 0x1000) is sent to the radio so that it reads them again. This is what the "Full Packet Logging" button of the Bluetooth Virtual Sniffer (`btvs.exe`) does (from a disassembly of btvs.exe).
Writing the values and toggling the adapter does not switch it on (measured: the values stayed truncated). The script that was used to switch it on and off is not in this repository.

```powershell
winget install Microsoft.BTP WiresharkFoundation.Wireshark
# switch full packet logging on (elevated): the three registry values and the IOCTL above
Start-Process C:\BTP\v1.14.0\x86\btvs.exe -ArgumentList '-Mode','Wireshark','-Remote','on' -Verb RunAs
& 'C:\Program Files\Wireshark\tshark.exe' -i TCP@127.0.0.1:24352 -w <capture directory>\btvs.pcapng
# when done: delete the three values, send the IOCTL, and reboot
```

- btvs accepts the tshark connection once. To attach tshark again, start btvs again.
- To provoke a reconnection, switch the Bluetooth radio off and on (Settings, or `Windows.Devices.Radios.Radio.SetStateAsync`). An awake pen is back within seconds.
- A capture taken while this is on contains the LTK, the IRK and the addresses. Keep it out of any repository.
- Switching it off takes effect only after a reboot. After deleting the values and sending the IOCTL, after writing `EtwLogSensitiveData` = 0 and sending the IOCTL, and after switching the radio off and on, the values of a reconnection were still recorded in full (measured). Delete the values, then reboot.

## 7. Still unverified

| Open | Notes |
|---|---|
| Whether the FastHostId is needed for the continuous waveforms; how much the speed value sets the strength | 1.6 |
| Continuous waveforms with a pen bonded through the cover on Linux | Not recorded: the haptic tests of section 1 used a pen bonded in pairing mode |
| The responder path of patch 0023 and its path with Secure Connections OOB data | Compile-tested only. The initiator path was measured, first as a module and then on a booted kernel built with the patch |
| In the helper: the frames of 1.8, the scale of 300 mm/s, the dead zone and the choice of inking waveform, by feel; the stale-bond path after Windows couples the pen again (the probe's `stale-bond`, replacing the bond) and `pen couple --force`; `unsupported-kernel` on a kernel without 0023; speed feedback after an iptsd restart; `pen test` | [helper-reference.md](../../helper-reference.md), the list of what is still to verify. Measured or observed since with the helper: the probe of a bonded pen (by request and automatic) ends in `connected` with the bond kept; `8e a1` is restored within 3 s of a resume; the strength and the eraser click (observed) |
| Whether the key has to be handed to the pen before the connection, on a link that is not resolved; how much margin the 2.5 s have; a kernel fix for the Confirm value on a resolved link | 2.8, 2.9 |
| When the pen counts as identified in the Windows frames (pen ID and LocalId present or not); the fields at offsets 5 and 7–8; where the sequence number of `8e a1` is | 1.8 |
| Reconnection: how often and how fast | Recorded: one measurement with a time (50 s idle, 3.5 s) and a re-check on the booted kernel with 0023, without a time. A pen in the cover does not reconnect by itself after a suspend (2.7); the test record has one reconnection after a suspend ([test-results.md](../../test-results.md) PEN-5) |
| Reconnection of a pairing-mode bond when the host offers its identity key; a pairing-mode bond on Windows | No data (section 3) |
| The bytes Windows writes to Features 86, 112 and 115 of the cover | HIDCLASS ETW has no contents. That Feature 86 is the host address rests on static analysis and on what the cover does on Linux (2.3). Reading them with GET_FEATURE would need the collection open, which blocks the coupling (2.6), so do it with the pen out of the cover |
| The `NOT H` form of the coupling address | Static analysis only; never seen over the air |
| How the AES-CMAC key for button events follows from the CouplingKey; the Output 85 path | Not seen; the path has never run on this unit (2.2, 2.4) |
| Which of the three open collections blocks coupling on Windows | 2.6 |
| Whether the CFU writes depend on the pen's answers | 4.2 |
