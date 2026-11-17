# Slim Pen 2: continuous haptics, cover-slot coupling, reconnection, and the 16 B exchange at connection

Windows exchanges seven 16 B writes and notifications with the pen every time it connects ([pen-ble-trace.md](pen-ble-trace.md) 4.3), and Linux does not.
That exchange was taken as a candidate common cause of the three things that did not work on Linux (continuous haptics, reconnection after the pen's idle disconnect, automatic bonding), and its values were captured on Windows.
It turned out to be unrelated, and each of the three has its own cause. This note records all four results.

Evidence levels are as in the other notes: **measured** (logs, captures, values read back), **observed** (felt or seen by a person), **inferred**, and **static analysis** (read from Windows binaries on a partition mounted read-only; nothing was run).
Facts from binaries are given without positions. Bluetooth addresses, keys, the pen ID and the host ID value are specific to one unit and are not given. The captures contain keys and addresses and are kept outside the repository.

| Problem | State on Linux | Cause | Section |
|---|---|---|---|
| Continuous haptics | **Played on Linux** (observed) | The digitizer, not Bluetooth. The Windows touch processor feeds pen state back to the digitizer on every frame; the pen plays the continuous waveforms only once the digitizer has a valid pen speed from the host. Linux sent nothing | 1 |
| Automatic bonding | **Works on Linux** with the pen in the cover's pen slot (measured): the cover procedure + LE legacy OOB pairing (kernel patch 0023) bonds within 1 s and BlueZ stores the LTK and IRK. Done by hand with tools, not by a daemon | The keys go through the Type Cover, and the pen uses a second address for it | 2 |
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

Feature 9, 64 B with the report ID; offsets count from the report ID.

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

### 1.6 Not separated yet

- Whether the FastHostId (`8e a1`) is needed at all.
- Whether the speed value changes the strength of the vibration (speed 0 against 63). The daemon's default assumes it does: its full scale of 290 mm/s was chosen by feel as "about as strong as on Windows", and there is no controlled comparison behind it.

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

Done with the pen lying in the cover, on the booted 7.3.4 kernel with a `bluetooth.ko` that carries [patch 0023](../../../kernel/PROVENANCE.md) loaded as a module.

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
the responder path and the path combined with Secure Connections OOB data are compile-tested only, and no kernel built with the patch had been booted at the time of writing.

Left to do: put this procedure into `surface-pen-connect` so that laying the pen in the cover bonds it without a person, and the HoG attachment right after bonding.

A different route to a working bond, without the kernel patch: [`tools/linux/pen-bond-from-windows.py`](../../../tools/linux/pen-bond-from-windows.py) copies the bond Windows made through the cover into BlueZ (both systems use the same adapter address). No result obtained with a copied bond is recorded in this note.

### 2.6 Holding the bonding collections open blocks the coupling on Windows

While the cover's Col02 and Col03 and the THC's Col07 were held open and read with hidapi, coupling failed twice out of two attempts (measured). The cover sent reports 110 and 20 and Windows did issue Create Connection,
but it was cancelled after 7.7 s (status 0x02). The address Windows connected to was then the pairing address, not the coupling address (measured: capture). Whether the pen was advertising is unknown. As soon as the reading stopped, coupling worked.
Which of the three collections blocks it was not separated. Tracing this on Windows therefore has to use ETW only, without opening the collections.

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
| Linux, cover coupling with patch 0023 offering the identity key | **Reconnects.** The pen disconnected by itself after 50 s and was connected again 3.5 s after it was picked up and drawn with |

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
| Whether the FastHostId is needed for the continuous waveforms; whether the speed value sets the strength | 1.6 |
| Continuous waveforms with a pen bonded through the cover on Linux | Not recorded: the haptic tests of section 1 used a pen bonded in pairing mode |
| A kernel built with patch 0023 has not been booted; its responder path and its path with Secure Connections OOB data are compile-tested only | The coupling and the reconnection were measured with the patched `bluetooth.ko` loaded into the booted 7.3.4 |
| Coupling without a person: the procedure of 2.5 is not in `surface-pen-connect`, and BlueZ does not attach HoG right after the bonding | 2.5 |
| Reconnection beyond the one recorded measurement (50 s idle, 3.5 s) | No repeat count, and nothing across a suspend or a reboot, is recorded |
| Reconnection of a pairing-mode bond when the host offers its identity key; a pairing-mode bond on Windows | No data (section 3) |
| The bytes Windows writes to Features 86, 112 and 115 of the cover | HIDCLASS ETW has no contents. That Feature 86 is the host address rests on static analysis and on what the cover does on Linux (2.3). Reading them with GET_FEATURE would need the collection open, which blocks the coupling (2.6), so do it with the pen out of the cover |
| The `NOT H` form of the coupling address | Static analysis only; never seen over the air |
| How the AES-CMAC key for button events follows from the CouplingKey; the Output 85 path | Not seen; the path has never run on this unit (2.2, 2.4) |
| Which of the three open collections blocks coupling on Windows | 2.6 |
| Whether the CFU writes depend on the pen's answers | 4.2 |
