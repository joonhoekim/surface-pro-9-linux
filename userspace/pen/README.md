# userspace/pen — Slim Pen 2 connection helper, digitizer feedback and haptic tool (Linux, prototype)

This is a prototype. What was seen on the device is under "Seen on the device" below; no other behaviour has been checked on Linux. The basis is [`docs/research/gaps/pen-touch.md`](../../docs/research/gaps/pen-touch.md) (1.2, 1.8 and differences #6 and #7 of section 3), the analysis of a Windows Bluetooth ETW trace in [`docs/research/gaps/pen-ble-trace.md`](../../docs/research/gaps/pen-ble-trace.md), the later results in [`docs/research/gaps/pen-vendor-exchange.md`](../../docs/research/gaps/pen-vendor-exchange.md) (continuous haptics, cover-slot coupling, reconnection), and linux-surface #1066 (haptics) and #1656 (disconnection).
What is not confirmed is marked **inferred**, together with a way to check it.

| File | What it does |
|---|---|
| `pen-haptic` | Writes the Simple Haptic Controller output report (65) to the hidraw of the pen's BLE HID. Run without a command it only reads (`discover`) |
| `surface-pen-connect` | Keeps the pen Trusted and reconnects it through BlueZ when the digitizer sees the pen (iptsd stylus evdev or the THC FFF4:0001 reports) |
| `surface-pen-feedback` | Feeds the pen's speed back to the digitizer (THC Feature 9), which the pen needs for its continuous haptic waveforms. With `--ink-haptics` it also starts the inking waveform on contact and stops it on release |
| `surface_pen/hiddesc.py` | Report descriptor parser and report pack/unpack. All report layouts are computed from the device's descriptor |
| `surface_pen/haptic.py` | Haptic library (`PenHaptics.trigger/pulse/buzz/stop`, inking feedback `InkFeedback`, `pressure_to_intensity`) |
| `surface_pen/col07.py` | Decoding of the reports of the THC FFF4:0001 collection (Col07 on Windows), construction of the host-address Feature, and `coupled_address`: the address the pen uses toward the host it is coupled with through the cover |
| `surface_pen/feedback.py` | The digitizer messages `8e a1` (display state, FastHostId) and `8e a5` (pen feedback V06), the speed estimate, and the `surface-pen-feedback` daemon |
| `surface_pen/bluez.py` | BlueZ D-Bus (`org.bluez.Device1`) |
| `systemd/surface-pen-connect.service`, `systemd/surface-pen-feedback.service` | System units |
| `tests/test_reports.py` | Unit tests of report pack/parse that run without hardware |

## Seen on the device

Kernel 7.3.4, BlueZ 5.87, pen firmware as shipped. The measurements and what Windows does are in [pen-vendor-exchange](../../docs/research/gaps/pen-vendor-exchange.md).

- **Manual pairing works.** Put the pen into pairing mode with a long press on the tail button and `Pair()` with a `NoInputNoOutput` agent. Without an agent: `AuthenticationFailed` (`No agent available for request type 2`). This is a Just Works pairing.
- **Continuous haptics play when the digitizer is told the pen's speed** (observed). The pen plays the continuous waveforms only while the digitizer has a valid pen speed from the host; `surface-pen-feedback` sends it (Feature 9 of the digitizer). After that, ink-continuous at intensity 127 on report 65 vibrates almost as it feels on Windows. Without the feedback only the click is felt: continuous waveforms are not felt through Manual Trigger and come weak and intermittent through the Auto Trigger of Feature 65 (`41 00 <intensity> <ordinal> 00 00`; after a test, set Auto Trigger back to 1, none). Making the bytes and the ATT write type of Output 65 equal to Windows changed nothing.
- **Bonding through the cover's pen slot works, by hand** (measured, with kernel patch 0023 loaded as a module). With the pen lying in the Type Cover's pen slot: write Features 112 and 86 (host address) to the cover, take the pen ID from Input 110, compute the pen's coupling address (`col07.coupled_address`), give the same 16 random bytes to the kernel as the Temporary Key and to the cover in Output 111, connect to the coupling address and pair ([`tools/linux/le-legacy-oob.py`](../../tools/linux/le-legacy-oob.py)). The pen is bonded within 1 s and BlueZ stores the LTK and IRK. BlueZ attaches the pen's HID only after bluetoothd is restarted. None of this is in `surface-pen-connect` yet.
- **Reconnection depends on the bond.** A pen bonded in pairing mode disconnects by itself after about 80 s (0x13) and does not advertise afterwards when it is picked up, when a stroke is drawn or when the tail button is pressed; the `Device1.Connect()` of `run` ends in a timeout. A pen bonded through the cover with the host's identity key (patch 0023 offers it) disconnected by itself after 50 s and was connected again 3.5 s after it was picked up and drawn with (measured). Without the host's identity key the cover bond does not reconnect either.
- **Writing the digitizer's bonding Features did nothing.** After a SET of Features 86·112 on the THC with `--write-host-features --ab-usage 0x10`, the values read back unchanged (86 is 0, 112 is `02`), a stroke brings no input report 84·110 and the pen does not appear on BLE. The collection that announces the pen is the cover's copy, not the digitizer's.
- The 16 B exchange Windows performs on every connection is a firmware update check (CFU offers). It is not imitated.
- Tail button: once Meta+F20, twice Meta+F19, long Meta+F18.

What was still to be captured on Windows, and the answers:

| What | Answer |
|---|---|
| The values of the 7 writes (handle 0x0062) and notifications (0x004f) of 16 B on reconnection | Output 45 and Input 45: CFU firmware offers, which the pen rejects as not newer. Unrelated to advertising ([pen-vendor-exchange](../../docs/research/gaps/pen-vendor-exchange.md) section 4) |
| The bytes of Feature 86 (host address) and 112 that the Windows service SETs | Not captured (the trace has request types only). From static analysis and from what the cover does on Linux: Feature 112 bit 0010 = 1, Feature 86 = the host address, LSB first, bit 0009 = 0; written to the cover ([pen-vendor-exchange](../../docs/research/gaps/pen-vendor-exchange.md) sections 2.2, 2.3) |
| The value of report 65 while inking, and whether Windows writes Feature 65 | Start `00 7f NN 00 00 00` once per stroke, stop `00 fe 02 00 00 00` once; NN follows the application's tool. No Feature 65 write was seen. The continuous vibration was weak on Linux for another reason: the digitizer feedback ([pen-vendor-exchange](../../docs/research/gaps/pen-vendor-exchange.md) sections 1, 5) |

## Dependencies and installation

- Python 3.10 or later. `pen-haptic`, `surface-pen-feedback` and the hidraw part of `surface-pen-connect probe` use only the standard library.
- The BlueZ part of `surface-pen-connect` needs **dbus-next**: Debian/Ubuntu `python3-dbus-next`, Fedora `python3-dbus-next`, Arch `python-dbus-next`.
- `/dev/hidraw*` and `/dev/input/event*` are root-only, so run as root (system unit).

```sh
sudo cp -r userspace/pen /opt/surface-pen
sudo cp /opt/surface-pen/systemd/surface-pen-connect.service /opt/surface-pen/systemd/surface-pen-feedback.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now surface-pen-connect surface-pen-feedback
```

On NixOS the options are `hardware.surface-pro-9.pen.enable`, `pen.connectService` and `pen.feedbackService` ([docs/nixos.md](../../docs/nixos.md)).

Tests: `python3 -I userspace/pen/tests/test_reports.py` (also runs on Windows).

## pen-haptic

```sh
sudo ./pen-haptic                     # discover: node, report 65 layout, read Feature 65·66 (waveform list)
sudo ./pen-haptic pulse --intensity 200 --ms 30
sudo ./pen-haptic buzz --intensity 128 --ms 500   # continuous waveform → stop report after ms
sudo ./pen-haptic stop
./pen-haptic buzz --dry-run           # print the report bytes without writing
```

Use as a library. Inking feedback like that of Windows writes only at contact boundaries (once at the start of the stroke with ordinal 9 ink-continuous and intensity 127, one stop at lift, no periodic retrigger):

```python
from surface_pen.haptic import PenHaptics, InkFeedback
ink = InkFeedback(PenHaptics())              # kind="eraser" gives eraser-continuous
ink.update(contact)                          # on every stylus event; writes only on a change
```

To vary the intensity with pressure, pick the value with `pressure_to_intensity(pressure)` and call `h.trigger(h.ordinal("ink"), i)`. Windows does not do this (it does not write on every change of value).

### Report 65 (pen descriptor, the 443 B posted in #1066)

A Simple Haptic Controller (0E:01) logical collection inside the Stylus (0D:20) application collection. Output report 65 is 7 B:

| Byte | usage | Logical range | Physical range | Meaning (HUT 0x0E) |
|---|---|---|---|---|
| 0 | | | | Report ID 65 |
| 1 | 0E:24 Repeat Count | 1..255 | | Repeat count. The Windows capture sends 0 |
| 2 | 0E:23 Intensity | 1..255 | | Intensity |
| 3 | 0E:21 Manual Trigger | 1..22 | | Waveform ordinal. 1 = NONE, 2 = STOP, 3..22 are the Feature 66 waveform list |
| 4 | 0E:28 Waveform Cutoff Time | 0..254 | 0..2540, s×10⁻³ | Units of 10 ms |
| 5–6 | 0E:25 Retrigger Period | 0..2000 | 0..2000, s×10⁻³ | ms, little endian |

Feature 65 (6 B): Repeat, Intensity, Auto Trigger (1..22), Retrigger Period. Feature 66 (85 B): Auto Trigger Associated Control (32-bit, fixed at the value `0x000D0042` = Digitizer Tip Switch), Duration of ordinals 3..22 (16-bit × 20), Waveform (16-bit × 20, 4099..12287 = 0x1003..0x2FFF).
The unit tests reproduce the three captures from #1066 (`41 00 7f 09 00 00 00` start, `41 00 fe 02 00 00 00` stop, `41 01 ff 04` vibration check) with this layout.

The Feature 66 Windows read on reconnection (ordinal → waveform, duration in ms): 3 click 45, 4 press 60, 5 release 60, 6 hover 35, 7 success 145, 8 error 185, 9 ink 1995, 10 pencil 1995, 11 marker 1995, 12 chisel-marker 1995, 13 brush 1980, 14 eraser 1995, 15 sparkle 1980, 16–22 none.
There is no buzz-continuous. When Feature 66 cannot be read this table (`KNOWN_WAVEFORMS`) is used, and the selection is `pulse` = click (3), `buzz`·`ink` = ink-continuous (9), `eraser` = eraser-continuous (14).

### Inferences and how to check them

| Inference | Basis | Check |
|---|---|---|
| With Cutoff 0 a continuous waveform goes on until STOP (also beyond the listed 1995 ms) | Windows did not write report 65 again during a 9.8 s stroke (pen-ble-trace 3.3). On Windows the vibration lasted through a 9.1 s stroke with only a start and a stop written (observed, pen-vendor-exchange section 5) | Whether `buzz --ms 5000` vibrates for the whole 5 s on Linux, with the feedback daemon running |
| The firmware may ignore Repeat, Cutoff and Retrigger | #1066: "only intensity and waveform have an effect" | Change only these values with the same ordinal and compare by feel |
| On Windows the pen adjusts the vibration intensity during inking by itself, without BLE (only start and stop go over BLE) | Analysis of the #1066 capture. ETW: `dwm.exe` writes once 10–40 ms after contact and once 9–35 ms after lift (the values are truncated in the record) | The mechanism is known since: the host computes the pen's speed and gives it to the digitizer, which passes it to the pen (pen-vendor-exchange section 1). Whether the speed value changes the strength was not compared under control (speed 0 against 63) |
| A wrong ordinal can make the pen hang (re-pairing needed) | #1066 | Use only ordinals that are in the `discover` list |

## surface-pen-feedback

```sh
sudo ./surface-pen-feedback                  # pen speed to the digitizer
sudo ./surface-pen-feedback --ink-haptics    # also start the inking waveform on contact and stop it on release
```

The Slim Pen 2 plays its continuous waveforms only while the digitizer has a valid pen speed from the host. Windows' touch processor sends it in a per-frame message through Feature 9 of the digitizer; iptsd sends nothing ([pen-vendor-exchange](../../docs/research/gaps/pen-vendor-exchange.md) section 1). This daemon fills that gap from outside iptsd:

- It finds the digitizer hidraw node (`045E:0C52`, the one with Feature 9) and writes one `8e a1` message (display state and the FastHostId) at start and again when the pen comes into range after 30 s or more.
- It reads the iptsd stylus evdev, estimates the speed the way Windows does (pairs of positions 14–32 ms apart, smoothed over the last three estimates), scales it to the 6-bit field and writes an `8e a5` (V06) message when the value or the tip state changes, at most every 7 ms, and once a second while the pen stays in range. A pen standing still gets speed 0. The digitizer keeps the last value it received.
- Only the fields a host without the pen's digital data can fill are set: sequence number, FastHostId, tip bit, ZFI, speed. LocalId, the Ack fields and the protocol type are left at "none".
- The FastHostId is a 10-bit value made up once and kept in `/var/lib/surface-pen/fast-host-id` (`--state`, `--fast-host-id`). Whether the pen needs it for the waveforms was not separated.
- `--full-scale` is the pen speed in mm/s that maps to the largest value (default 290, chosen by feel as about as strong as on Windows); `--speed-gain` multiplies the speed before scaling.
- `--ink-haptics` writes report 65 to the pen like a Windows application: ink-continuous at contact (eraser-continuous with the eraser end), stop at release, intensity `--ink-intensity` (default 127). The unit `systemd/surface-pen-feedback.service` runs it with this option. Without the option another program has to start the waveform (`pen-haptic buzz`, the helper's pen page).
- The pen has to be connected over Bluetooth for the waveform itself; the speed goes through the digitizer and does not need Bluetooth.

Observed on the device: with the speed fed back, the continuous waveform is felt during strokes, almost as on Windows. The tests behind this used a pen bonded in pairing mode and are described in [pen-vendor-exchange](../../docs/research/gaps/pen-vendor-exchange.md) section 1; the unit and the NixOS option `pen.feedbackService` have no recorded test of their own.

## surface-pen-connect

```sh
sudo ./surface-pen-connect probe        # Col07 hidraw layout, read Feature 86/112/115, stylus evdev, BlueZ pen state
sudo ./surface-pen-connect monitor      # read only: print pen proximity evdev events and reports 84/110
sudo ./surface-pen-connect run          # keep Trusted + Device1.Connect() when the pen is seen
```

- Triggers: `--trigger evdev,col07` (both by default). evdev is a press of `BTN_TOOL_PEN`/`BTN_TOOL_RUBBER`/`BTN_TOUCH` on the `IPTSD Virtual Stylus 045E:0C52` that iptsd creates. col07 is input report 84/110 on the THC hidraw.
- When there is no stylus evdev it looks again every 5 seconds, and it looks again when the node it was watching disappears. iptsd is an `iptsd@<hidraw>.service` started by udev, so it cannot be waited for with unit ordering (`After=`), and the event number changes when it restarts.
- Connection attempts are made only at intervals of `--cooldown` (5 s by default). Nothing is done when the pen is already connected.
- An unpaired pen is left alone by default (`--pair` for `Device1.Pair()`).
- Only the last octet of addresses is shown (`--show-addr` for the whole address).

### Reconnection: BlueZ settings and the order of checks for #1656

On Windows too the pen ends the connection by itself about 52 s after it was last used (reason 0x13). Windows sends no keep-alive; when the pen disconnects, it puts the pen on the Accept List and scans, connects as soon as an advertisement arrives, and re-encrypts with the stored LTK.
On Linux the same job is done by BlueZ auto-connect (bonded HoG devices). This tool keeps the pen Trusted so that auto-connect is not switched off after a D-Bus `Disconnect()`.
Whether the pen advertises at all after its idle disconnect depends on the bond: it does for a bond made through the cover's pen slot in which the host gave its identity key, and it does not for the Just Works bond of pairing mode ([pen-vendor-exchange](../../docs/research/gaps/pen-vendor-exchange.md) section 3). The steps below are for the first case.

1. Look at the Disconnect Complete reason of the disconnection with `btmon`. 0x13 is the pen's normal behaviour, and the problem is reconnection. 0x08 points to the supervision timeout.
2. If the advertisement (ADV_IND, RPA) is visible when the pen is picked up but no connection follows, check in `bluetoothctl info` for Paired, Trusted and Bonded and that the address is the identity address (not an RPA).
3. To use the same connection parameters as Windows, put the following into `/etc/bluetooth/main.conf` (the kernel defaults are 30–50 ms and a supervision timeout of 420 ms):

```ini
[LE]
MinConnectionInterval=6
MaxConnectionInterval=12
ConnectionLatency=0
ConnectionSupervisionTimeout=200
ScanIntervalConnect=36
ScanWindowConnect=18
```
- **Writing to the digitizer is off by default.** Only with `--write-host-features` does the tool GET → modify → SET → GET Feature 86 (host BD address) and, when `--ab-usage 0x10` or the like is given, Feature 112.

### THC FFF4:0001 collection (Col07 on Windows, elevated `hid-caps.ps1`)

ithc registers one `hid_device` per device, so on Linux this is the same hidraw node that iptsd uses. It is found in `report_descriptor` as the FFF4:0001 application collection.
The report sizes and usages below are values confirmed from the Windows capabilities; the order of fields inside a report cannot be known from the Windows capabilities, so the code computes the offsets from the actual descriptor.

| Report | Sub-collection | Fields |
|---|---|---|
| Input 84 (12 B) | FFF4:0007 | FF0F:0050 8-bit × 4, FFF4:0002 8-bit × 6, button FFF4:0008 |
| Input 110 (12 B) | FFF4:0013 | FF0F:0050 8-bit × 4, FFF4:0002 8-bit × 6, buttons FFF4:0008, 0011, 0012 |
| Output 85 | FFF4:0005 | FF0F:0050 8-bit × 4, FFF4:0002 8-bit × 6, FFF4:0004 8-bit × 8, button FFF4:0008 |
| Output 111 | FFF4:0015 | FF0F:0050 8-bit × 4, FFF4:0002 8-bit × 6, FFF4:0014 8-bit × 16, button FFF4:0008 |
| Feature 86 | FFF4:0006 | FFF4:0003 8-bit × 6, button FFF4:0009 |
| Feature 112 | FFF4:0016 | buttons FFF4:0010, FFF4:0022 |
| Feature 115 | (TLC) | FFF4:0021 8-bit × 2 |

Elevated `HidD_GetFeature` values with Windows running (the pen asleep): Feature 86 = all 0, Feature 112 = `03`, Feature 115 = `43 01`.

The flow that the trace messages of the bonding driver (`SurfacePenBleLcAddrAdaptationDriver.sys` 3.55.3.0) describe:

- While the user inks on the screen the digitizer sends input reports up, and the driver looks at two flags of the pen: its auto-bonding capability (a Slim Pen 2) and whether it already knows a host (a Slim Pen 2 that was auto-bonded before).
- The conversion from pen ID to pen BD address involves a hash (a BCrypt hash object is used together with a GUID and two host ID values).
- On the HID SET_FEATURE path there are a host Bluetooth address usage and a host auto-bonding capability feature report, including a host hardware auto-bonding capability usage: the user-space PenService SETs the host address and the auto-bonding capability on the digitizer ("PenService enables auto-bonding").
- On the write path the pen ID is translated, and there are a MAC signing and a matching signature validation with AES-CMAC and a security key. This was first read as "output reports 85 and 111 are signed, and the 16 B field of Output 111 is the tag". That was wrong: the 16 B of Output 111 are the pairing key (the SMP Temporary Key) the host makes up, written unprocessed, and the driver does not touch them; the AES-CMAC path belongs to the 8 B key of Output 85 and has never run on this unit ([pen-vendor-exchange](../../docs/research/gaps/pen-vendor-exchange.md) sections 2.2, 2.4).
- The Surface OOB hardware auto-bonding capability usage is set by the driver when it passes a GET response up. So one of the bits of the `03` in Feature 112 may be a value the driver adds. Confirmed since by static analysis: when the device reports bit 0022 the driver also sets bit 0010 on the way up, so the `02` read on Linux is `03` for the Windows service.

The Type Cover (KIP `09B0`) has the same collection, and that copy is the one that announces a pen lying in the pen slot. `col07.coupled_address(pen_id, host_addr)` computes the address the pen then uses toward this host: the low 46 bits of a SHA-1 over a constant of the Windows bonding driver and the reversed pen ID, as a static random address, XORed with the host address. The address field of Input 110 that Linux reads is the pen's pairing-mode address; on Windows the driver rewrites it to the coupling address ([pen-vendor-exchange](../../docs/research/gaps/pen-vendor-exchange.md) section 2.4).

### Inferences and how to check them

Checked since, with the results under "Seen on the device": the ithc descriptor does have the FFF4:0001 collection with input reports 84 and 110 (measured); a SET of Features 86 and 112 on the digitizer changed nothing; a pen bonded in pairing mode was not seen advertising after its idle disconnect, so `Device1.Connect()` times out. The rows about the pen's address, Features 86 and 112 and the output reports are answered for the cover's copy of the collection in [pen-vendor-exchange](../../docs/research/gaps/pen-vendor-exchange.md) section 2; on the digitizer's copy, nothing arrived on Windows when the pen was put into the cover, and whether reports 84 and 110 arrive there when the pen touches the screen is still open.

| Inference | Basis | Check |
|---|---|---|
| The Linux ithc descriptor has the FFF4:0001 collection as it is | Windows `UseTICReportDescriptorOnly = 1` (the descriptor comes from the controller) | `surface-pen-connect probe`, `hid-decode /sys/class/hidraw/hidrawN/device/report_descriptor` |
| Reports 84/110 arrive in legacy (IPTS) multitouch mode too | Windows receives them from the same THC | Run `monitor` and draw on the screen with the pen |
| 84 = the Slim Pen 1 (MPP 2.5) path, 110 = the Slim Pen 2 (MPP 2.6) path. In 110, FFF4:0011 = the pen's auto-bonding capability, FFF4:0012 = the pen knows a host | The sub-collection layout; the driver's trace messages tie these two flags to a Slim Pen 2 and to a previously auto-bonded Slim Pen 2 | The report number and flags in the `monitor` output; the change of 0012 when the pen is bonded alternately to Windows and to Linux |
| The 6 B of FFF4:0002 are the pen's BD address, LSB first as in HCI | The usage is 6 B; the driver flips or reverses the MAC | `run --verbose` prints "matches BlueZ pen (LSB-first/MSB-first)". If it does not match, the pen ID → address conversion (hash) is needed, so the col07 trigger is used only as a "pen was seen" signal. Since, on the cover's copy: the field is the pen's pairing-mode address, and the address of a cover bond is the hashed one (`coupled_address`) |
| The 4 B of FF0F:0050 are the pen ID the digitizer reads over MPP | The touch pen processor handles a raw pen ID from the digital pen; same usage page as Col0B Feature 102 | Whether the value changes between two pens |
| Feature 86 = the host Bluetooth address (FFF4:0003), Feature 112 = the host auto-bonding capability report (FFF4:0010 = host hardware auto-bonding capability, FFF4:0022 = Surface OOB hardware auto-bonding capability) | The size of 6 B, the order of the driver's messages. On Windows 86 reads as 0 (the digitizer does not return the value on GET, or PenService had not written it yet) | The 86/112 values of `probe` right after a Linux boot. The GET values and any change in the pen's behaviour after `--write-host-features --ab-usage 0x10` |
| Writing the host address may still not make the pen come to Linux by itself. Windows appears to send the pen an instruction to connect with Output 85/111, and this tool does not write output reports | #1656 (no traffic on the BLE side wakes a sleeping pen) | Answered for the cover: Output 111 carries the 16 B pairing key, unsigned, and after it the pen accepts a connection on its coupling address ([pen-vendor-exchange](../../docs/research/gaps/pen-vendor-exchange.md) section 2.3). This tool still writes no output report |
| When the pen touches the screen it starts advertising and `Device1.Connect()` succeeds | Windows ETW: the pen sends ADV_IND with an RPA when it is picked up (1.2 s before the digitizer sees it), and Windows connects within 34 ms through its Accept List scan. #1656 reports that only the tail button wakes it | `btmon` + `run --verbose`: whether an advertisement is visible when the pen is picked up, whether kernel auto-connect gets there first |
| The stylus evdev name is `IPTSD Virtual Stylus …` | iptsd `src/apps/daemon/uinput-device.hpp:163`, `stylus.hpp:45` | `cat /sys/class/input/event*/device/name` |

### Pen BLE HID capabilities (Windows)

Not collected: the pen's HID collections exist only while it is connected, and the pen was asleep (BLE node `connected=False`). Right after pressing the tail button they can be read with the following (the script masks Bluetooth addresses):

```
powershell -ExecutionPolicy Bypass -File tools\windows\hid-caps.ps1 -Filter 0c0f
```
