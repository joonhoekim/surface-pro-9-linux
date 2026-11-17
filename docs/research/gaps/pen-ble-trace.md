# Pen BLE trace: what Windows exchanges with the Slim Pen 2

This is the decoded Bluetooth ETW trace (`bt.etl`, busiotools `BluetoothStack.wprp`) recorded during the Windows baseline tests PEN-9 (haptics) and PEN-5 (sleep and reconnection).
It serves two purposes: the defaults of the Linux haptic tool ([userspace/pen](../../../userspace/pen/README.md)), and a reference for comparison with linux-surface #1656 (on Linux the pen disconnects after about a minute).
Addresses, keys, IRK, LTK and the pen serial are not given. The raw trace (`bt.etl`) and its conversions are not published.

## 1. Method

Most events in the ETL are WPP (argument dumps without a format string) and cannot be read without the TMF. `tracerpt` also emits them only as `Unknown`.
The useful one is a single manifest event. **BTHPORT (`Microsoft-Windows-BTH-BTHPORT`, `8a1f9517-…`) event 402, channel `Microsoft-Windows-BTH-HCI/HCIRAW`** carries HCI packets.

| Field | Size | Meaning |
|---|---|---|
| CommandPacketsAllowed | u8 | |
| AclWriteCredits | u16 | |
| BIP_Type | u8 | 1 = command, 2 = event, 3 = ACL controller→host, 4 = ACL host→controller |
| BIP_DataLen | u32 | number of bytes recorded |
| BIP_Data | | HCI packet (no H4 indicator) |

`BluetoothStack.wprp` enables BTHPORT twice (`BTHPORT`, `BTHPORT.WithStacks`), so the same packet sometimes appears as two lines. The tool counts identical packets within 1 ms as one.

Windows records some packets truncated. This appears to be meant to hide keys and HID data.

| Packet | What is recorded |
|---|---|
| LE Enable Encryption (0x2019) | Only the command header (3 B). Of the 28 B of Rand·EDIV·LTK, 0 B |
| ATT Write Command/Request, Notification | ACL and L2CAP headers + ATT opcode + handle (11 B in total). **The value is missing** |
| ATT Read Response | everything |
| HCI events, other commands | everything (including addresses) |

Besides HCIRAW there is one useful WPP message. `cdd5026b-79c6-3dfe-3848-cc6ee6b30910` id 97 is emitted once per HID report write, in the context of the writing process, and its last 8 B hold the report ID (taken to be hidclass, inferred; the GUID is that of this build).

The tools are in [`tools/windows/btetl/`](../../../tools/windows/btetl/).

- `etldump.cs`: writes every event with `OpenTrace`/`ProcessTrace` as one TSV line of `filetime, provider, id, …, pid, tid, user-data hex`. Built with the .NET Framework `csc` (a 152 MB ETL with 1.66 million events takes a few seconds).
- `btetl.py`: decodes event 402 from the TSV (or, given an .etl, builds `etldump.exe` and dumps first) into HCI, L2CAP and ATT and prints it in time order. With `--btsnoop` it also writes a btsnoop (H4) file (for Wireshark and btmon; it contains the addresses as they are, so keep it private). With `--markers` and `--rawinput` it inserts the test step boundaries and the pen contact spans (the HEAT pen reports of sp9probe) on the same time axis. Only the last octet of addresses is shown. `--data` prints Read Response values (they contain the pen serial, so keep that output private).

```powershell
$r = '<directory of the recorded test run>'
python -I -X utf8 tools\windows\btetl\btetl.py $r\bt\bt.etl --tsv out\bttrace\pen9.tsv `
    --btsnoop out\bttrace\pen9.btsnoop --markers $r\markers.csv --rawinput $r\probe\rawinput.csv > out\bttrace\pen9-hci.txt
```

Times below are seconds, with 0 at the step 1 marker of each test.

## 2. What was recorded

| Test | Length | Events | HCIRAW packets | Notes |
|---|---|---|---|---|
| PEN-9 | 959 s | 1 661 148 | 141 (after deduplication) | OneNote was being installed during step 2 (draw and erase), so the pen lay unused for a long time. That is why one disconnection and one reconnection were captured |
| PEN-5 | 117 s | 240 750 | 2 | Step 1 (let the pen fall asleep) was ended after 57 s, so the pen did not disconnect |

ACL data is recorded also for a connection that was already established before the trace started (the two notifications of PEN-5). Stretches without records are taken to be stretches without BLE traffic.

## 3. Result: haptics (PEN-9)

### 3.1 GATT handles and reports

There was no HOGP service discovery (the device is bonded, so Windows uses its cache). The Report Reference descriptors were therefore not seen, and handles were matched to reports by size, content and writing process.

| Handle | Operation | Value size | Report | Evidence |
|---|---|---|---|---|
| 0x006b | Write Command (no response) | 6 B | **Output 65** (Col05 haptics) | Size 6 = 7 B − report ID. 22 WPP HID writes (id 65, `dwm.exe`) pair one to one with 22 ATT writes at the same times (HID write → ATT write 1–3 ms) |
| 0x0077 | Read | 84 B | **Feature 66** (waveform and duration lists) | Size 85 − 1, the first 32 bits are `0x000D0042` (Auto Trigger Associated Control = Digitizer Tip Switch) |
| 0x007a | Read | 10 B | **Feature 64** (Col05) | 0D:5B serial 32-bit, 0D:91 = `0x045E`, 0D:92 = `0x0C0F`, 0D:94 = 3 |
| 0x006e | Read | 60 B | Col03 Feature 42 (inferred) | The only one whose size, 60, fits. 4 B header + 4 entries of 8 B, the rest 0 |
| 0x0062 → 0x004f | Write Request 16 B → Write Response → Notification 16 B | 16 B | unknown | None of the known HID reports is 16 B. 7 round trips on each reconnection (about 30 ms apart). The values are truncated |
| 0x0047 | Notification | 8 B | Col01 keyboard Input 1 (tail button) | Arrives in PEN-5 at the times the tail button was pressed |

Writes of the sizes that would go to Col02 Output 48, Col03 Output 42·45–47 or Col04 Output 51·52 (1·60·4·101·1 B) do not occur anywhere in the trace. On reconnection `csrss.exe` writes report 1 once, but no ATT write follows (keyboard LEDs; the pen's Col01 has no output report).

### 3.2 Feature 66: the waveform list the pen announces

This is the value Windows read right after reconnection. Ordinal = the Manual/Auto Trigger value.

| Ordinal | Waveform | Name (HUT 0x0E) | Duration ms |
|---|---|---|---|
| 3 | 0x1003 | click | 45 |
| 4 | 0x1006 | press | 60 |
| 5 | 0x1007 | release | 60 |
| 6 | 0x1008 | hover | 35 |
| 7 | 0x1009 | success | 145 |
| 8 | 0x100A | error | 185 |
| 9 | 0x100B | ink-continuous | 1995 |
| 10 | 0x100C | pencil-continuous | 1995 |
| 11 | 0x100D | marker-continuous | 1995 |
| 12 | 0x100E | chisel-marker-continuous | 1995 |
| 13 | 0x100F | brush-continuous | 1980 |
| 14 | 0x1010 | eraser-continuous | 1995 |
| 15 | 0x1011 | sparkle-continuous | 1980 |
| 16–22 | 0x1001 | none | 0 |

There is no buzz-continuous (0x1004) or rumble-continuous (0x1005). Ordinal 4, which #1066 reported as "vibrates", is press, and ordinal 9 of the Windows inking start report `41 00 7f 09 …` in #1066 is ink-continuous. Feature 65 (the Auto Trigger setting) was neither read nor written.

### 3.3 Report 65 writes while inking

The writer is `dwm.exe` (the pid of the WPP HID writes). In the first part of step 2 (pen connected, many strokes at 0–186 s) there is not a single write. No application that produces haptics is taken to have been in use then (OneNote was not installed yet).
The 22 writes after OneNote was installed are all there are. Placed next to the pen contact boundaries (the tip bit of rawinput):

| Pen | Time s | Report 65 writes | Delay |
|---|---|---|---|
| contact | 940.042 | 5 at 940.472–940.531 (minimum spacing 3 ms) | 430 ms (first stroke) |
| lift | 940.594 | 940.628 | 34 ms |
| contact | 940.748 | 940.785 | 37 ms |
| (during the stroke) | | 941.277, 941.400 | |
| lift | 941.757 | 941.771 | 14 ms |
| contact | 942.054 | 942.071 | 17 ms |
| (during a 9.8 s stroke) | | none | |
| lift | 951.870 | 951.880 | 10 ms |
| contact | 951.994 | 952.015 | 21 ms |
| lift | 955.023 | 955.032 | 9 ms |
| (no contact) | | 955.524 | |
| short taps of 0.10 s·0.07 s | 956.080, 958.783 | none | |
| (after the recording ended) | 960.7–963.1 | 7 | no pen data |

- One write at the start and one at the end of each stroke: 10–40 ms after contact, 9–35 ms after lift.
- **There is no periodic retrigger.** No write occurred during the 9.8 s stroke, which is longer than the listed duration of ink-continuous (1995 ms). Whether the vibration continued throughout is not recorded (PEN-9 result "unverified").
- The values are truncated and were not seen. Timing and count do not contradict the #1066 capture (start `41 00 7f 09 00 00 00` = ordinal 9, intensity 127, repeat·cutoff·retrigger 0; stop `41 00 fe 02 00 00 00`).
- The writes are ATT Write Commands. At a connection interval of 15 ms, `Number Of Completed Packets` arrives 10–90 ms later.

## 4. Result: disconnection and reconnection (PEN-9, PEN-5)

### 4.1 PEN-5

The connection held for the 117 s of the trace. There is no connection-related HCI event at all, and the tail button presses (0.7 s, 64.6 s) arrived as keyboard notifications (handle 0x0047) on the existing connection. From 0.7 s to 64.6 s (63.9 s) there was neither BLE traffic nor a digitizer pen report, and the pen still did not disconnect.
So "waking" in this test was not a reconnection. Measuring the reconnection time requires waiting several minutes in step 1.

### 4.2 PEN-9: disconnection

| Time s | What |
|---|---|
| 186.0 | Last digitizer pen report (HEAT, in-range). The last contact is at 176.7 |
| 0–237.8 | No ATT traffic on this connection. No connection parameter update either (L2CAP Conn Param Update Req, LE Connection Update) |
| 237.855 | **Disconnection Complete, reason 0x13 (Remote User Terminated Connection)**: the pen disconnected. The pen had not been used for 51.8 s |
| 237.856–237.863 | Windows: extended scan off → add the pen's identity address (static random) to the Filter Accept List → Intel vendor command 0xFC1E (22 B: 6 B + 16 B, the size of an IRK. The value is not given) → extended scan on |

Windows sends nothing to prevent the disconnection (no keep-alive). Right after the disconnection it moves to an Accept List based background scan. The scan parameters (0x2041) were set before the trace started, so their values were not seen.

### 4.3 PEN-9: reconnection

The pen stayed disconnected for 673 s and advertised when the user picked it up. Relative times, with the reception of the advertisement as 0:

| +ms | What |
|---|---|
| 0 | Intel vendor event, LE Extended Advertising Report: legacy ADV_IND (type 0x0013), address type random (RPA), RSSI −54 dBm, data 0 B |
| +1 | LE Extended Create Connection: filter policy 0 (the RPA of the advertisement given directly), own address public, scan 22.5/11.25 ms, **interval 7.5–15 ms, latency 0, supervision timeout 2000 ms** |
| +34 | LE Enhanced Connection Complete: central, **interval 15 ms, latency 0, timeout 2000 ms** |
| +37 | LE Enable Encryption (stored LTK; the parameters are not recorded). No SMP (CID 6) traffic = re-encryption without re-pairing |
| +39 | The pen requests the MTU (104). Windows: scan off → remove the pen from the Accept List → 0xFC1E (2 B) → scan on again |
| +146 | Encryption Change (on) |
| +146–189 | MTU exchange (host 527, pen 104), LE Data Length Change (108 B) |
| +253–369 | Reads: 0x006e (60 B), 0x0077 (Feature 66), 0x007a (Feature 64) |
| +313–565 | 7 round trips of the 0x0062 write / 0x004f notification, 16 B |
| +1225 | First digitizer pen report (in-range). The pen wakes and advertises when it is picked up, before it reaches the screen |
| +1435 | First contact |

Up to the end of the recording 52 s later there was no connection parameter update. The pen accepts 15 ms / 0 / 2 s as given.

### 4.4 Comparison with the Linux (BlueZ) defaults

The Linux values are from v7.3.4 `net/bluetooth/hci_core.c` (`hci_alloc_dev_priv`), `include/net/bluetooth/hci_core.h`, and BlueZ `src/device.c`, `profiles/input/hog.c` and `src/main.conf`.

| Item | Windows (observed) | Linux kernel and BlueZ default | BlueZ `main.conf` `[LE]` key |
|---|---|---|---|
| Connection interval | 7.5–15 ms (result 15 ms) | 30–50 ms (`le_conn_min/max_interval` `0x18`–`0x28`) | `MinConnectionInterval=6`, `MaxConnectionInterval=12` (units of 1.25 ms) |
| Slave latency | 0 | 0 | `ConnectionLatency=0` |
| Supervision timeout | 2000 ms | 420 ms (`0x2a`) | `ConnectionSupervisionTimeout=200` (units of 10 ms) |
| Scan for a connection attempt | 22.5 / 11.25 ms | 60 / 60 ms | `ScanIntervalConnect=36`, `ScanWindowConnect=18` (units of 0.625 ms) |
| Background scan after a disconnection | Accept List scan (parameters not seen), advertisement → connection 34 ms | auto-connect passive scan 60 / 30 ms | `ScanIntervalAutoConnect`, `ScanWindowAutoConnect` |
| Auto-connect targets | The bonded pen (added to the Accept List as soon as it disconnects) | The BlueZ `input-hog` profile has `auto_connect = true`, so a bonded HoG device is put on the kernel auto-connect list. It is left out, however, when the stored address is an RPA or NRPA (`device_address_is_private`), and an untrusted device is left out after a D-Bus `Disconnect()` until the next `Connect()` (`device_pause_auto_connect`) | `Trusted=true` |
| Re-encryption | Stored LTK, no SMP | The same way (when the bonding information has the LTK and IRK) | |

Reading for #1656:

- On Windows too the pen disconnected by itself (0x13) about 52 s after it was last used. "Disconnects after about a minute" is likely to be the pen's own behaviour. In PEN-5, however, it did not disconnect for 64 s, so it is not a simple fixed timer (motion detection, inferred).
- The difference is in reconnection. When the pen wakes and advertises, Windows connects within 34 ms and has HID ready within 0.6 s. If the symptom on Linux is that the pen "does not come back", then either the pen is not on the BlueZ auto-connect list (`Disconnect()` was called while it was untrusted, or it is stored under an RPA instead of its identity address) or the RPA cannot be resolved for lack of the IRK. The pen advertises with an RPA.
- What to look at first on Linux: the Disconnect Complete reason in `btmon`. 0x13 is the normal behaviour described above. 0x08 (Connection Timeout) makes the 420 ms supervision timeout a candidate cause, so test `ConnectionSupervisionTimeout=200`.
- Measured on Linux since (test PEN-5; [userspace/pen](../../../userspace/pen/README.md), "Seen on the device"): the pen disconnects by itself after about 80 s without use with reason 0x13, as on Windows. After that it does not advertise at all when it is picked up, drawn with or its tail button is pressed (a two-minute unfiltered scan does not contain the empty ADV_IND from an RPA seen on Windows), so neither auto-connect nor `Connect()` has anything to connect to. The auto-connect list and the IRK therefore do not explain the symptom on this device. Whether the pen advertises again only to a host that performed the 16 B exchange of 4.3 is inferred.

## 5. What userspace/pen takes from this

- `haptic.py`: waveforms are selected by name priority. `pulse` = click, `buzz` = ink-continuous when there is no buzz-continuous, `ink` = ink-continuous, `eraser` = eraser-continuous. When Feature 66 cannot be read, the table in 3.2 above (`KNOWN_WAVEFORMS`) is used. `buzz` does not fall to ordinal 4, which on this pen is press (60 ms long).
- `InkFeedback`: like Windows, writes once at the start of contact (ordinal 9, intensity 127) and one stop at lift. It does not write on every pressure value and does not retrigger periodically.
- The intensity of the stop report is 0xFE, as on Windows.
- README: recommended settings for reconnection (`main.conf` `[LE]`) and the order of checks for #1656.

The reconnection logic of `surface-pen-connect` does not depend on this. On Windows the pen advertises 1.2 s before the digitizer sees it, so the design is that kernel auto-connect gets there first and the `Device1.Connect()` of the digitizer trigger is the fallback. On Linux neither has reconnected the pen: after its idle disconnect it was not seen advertising (4.4).

## 6. Unknowns

| Unknown | Why | How to find out |
|---|---|---|
| The report 65 values Windows actually wrote (ordinal, intensity, repeat, retrigger, cutoff); the meaning of the extra writes during a stroke (941.3·941.4 s) and of the 5 consecutive writes on the first stroke | HCIRAW truncates ATT write values | An over-the-air sniffer (nRF Sniffer and the like) needs the LTK because of the encryption. Instead, hook `HidD_SetOutputReport`/`WriteFile` of `dwm.exe` at the API level on Windows, or write the same values on Linux and compare by feel |
| Whether ink-continuous repeats until STOP (or stops after 1995 ms) | All that is known is that there was no write during the 9.8 s stroke | Whether `pen-haptic buzz --ms 5000` vibrates for the whole 5 s on Linux |
| The 16 B round trips on handles 0x0062/0x004f | The values are truncated, no matching report | Whether the same exchange happens on reconnection in Linux `btmon` (it shows decrypted HCI as it is); the UUIDs of the handles in the BlueZ GATT cache (`/var/lib/bluetooth/<adapter>/cache/<pen>`) |
| The pen's disconnection condition | 52 s in PEN-9, held for more than 64 s in PEN-5 | The time until disconnection, with `btmon`, when the pen is only held and moved in the hand and when it lies on the desk |
| The Windows background scan parameters, the meaning of Intel 0xFC1E | Set before the trace / vendor command | Record from right after turning Bluetooth off and on |
| Direct evidence for handle ↔ report ID | There was no service discovery | The Report Reference (0x2908) reads in `btmon` when pairing on Linux |
