# Windows baseline test runner

Collects logs automatically while a person goes through the tests of
[docs/test-plan.md](../../../docs/test-plan.md) on Windows. The result rows have the same format as the Linux
results in [docs/test-results.md](../../../docs/test-results.md), so the two can be compared side by side by
ID. It does not change system settings. The one exception is the speaker endpoint volume during the audio
tests: it is set to a fixed value for the test and restored afterwards.

The runner's prompts, its messages and the result files it writes are in Korean in this version; the words it
prints are given here in English. Results are written locally, into the working copy; no baseline results are
part of this repository.

## Setup

```powershell
powershell -ExecutionPolicy Bypass -File tools\windows\baseline\setup.ps1
```

`baseline.ps1` also calls this script on its first run and whenever `sp9probe.cs` has changed. Running it
again is safe (`-Force` rebuilds everything).

| Dependency | Location | Used for |
|---|---|---|
| `sp9probe.exe` | `out/baseline/` (gitignored). Built from `sp9probe.cs` with the .NET Framework `csc.exe` that ships with Windows (C# 5, WinForms). Nothing to install | Pen and touch canvas (WM_POINTER), Raw Input, power, lid and display notifications, display mode list, CPU load, saving and restoring the speaker volume |
| Python 3.13 venv | `%LOCALAPPDATA%\sp9-baseline\venv`. The base Python is `winget install Python.Python.3.13`. `python -I` ignores the user site-packages, so the packages go into a venv | `hidlog.py`, `audio.py`, `summarize.py` |
| pip: `hidapi`, `numpy`, `sounddevice` | Inside the venv | HID reads, signal analysis, WASAPI playback and recording |
| ffmpeg | `winget install Gyan.FFmpeg` | Camera mode list and capture (DirectShow) |
| `BluetoothStack.wprp` | `%LOCALAPPDATA%\sp9-baseline\`. Downloaded from [microsoft/busiotools](https://github.com/microsoft/busiotools/tree/master/bluetooth/tracing) (MIT) | Bluetooth ETW recording (`wpr`, elevated) |
| [`../run-elevated.ps1`](../run-elevated.ps1) | This repository | Collection that needs elevation (ETW, reading THC Col04 and Col07, sleepstudy, BLE pen capabilities) |

## Running

```powershell
cd <repository>
powershell -ExecutionPolicy Bypass -File tools\windows\baseline\baseline.ps1 -List
powershell -ExecutionPolicy Bypass -File tools\windows\baseline\baseline.ps1 -Test PEN-1,PEN-2
powershell -ExecutionPolicy Bypass -File tools\windows\baseline\baseline.ps1 -Area CAM
```

| Option | Meaning |
|---|---|
| `-List` | ID, name, what is needed (elevation, sound, external equipment), one-line description |
| `-Test A,B` / `-Area PEN` | Interactive run |
| `-DryRun [-StepSeconds 3]` | No person: waits a few seconds at each step and only tries the collection and the summary. Raw output goes to a `_dryrun` folder of the raw output directory; no result is recorded. Loads are shortened to `-StepSeconds` seconds |
| `-Auto` | Measures only the parts that need no person and records without asking (below). Without `-Test`/`-Area` it goes through every ID and skips those that need a person |
| `-AllowSound` | Runs the steps that make sound in dry-run and automatic mode (skipped by default) |
| `-NoElevated` | Skips the collection that needs elevation |

Order of a run:

1. For each test it shows what to do, what is needed and the cautions (sound, elevation), and waits for Enter
   (`s` skips).
2. It starts the collection and waits until the first data arrives.
3. At each step it shows the instruction and waits for Enter. Pen and touch tests open a full-screen canvas
   with the same instruction and a "next (Enter)" button at the top. Automatic steps (load, sound, camera
   capture) go on to the next step when they finish. When a step takes longer than the default of 15 minutes
   (it differs per test) the runner moves on and leaves a mark.
4. It stops the collection, prints the summary, and asks for the result (pass / partial / fail / unverified),
   the basis (measured / observed), the setting, and the value and note. The value and note default to a
   suggestion taken from the summary (Enter = as is, `+text` = append).
5. It writes the result row and the summary file.

### Automatic mode (`-Auto`)

```powershell
powershell -ExecutionPolicy Bypass -File tools\windows\baseline\baseline.ps1 -Auto -AllowSound
```

`$AutoPlan` in `baseline.ps1` defines, per ID, the automatic steps (all of them `S` steps: load, capture,
collection of a fixed length) and the part left to a person. The interactive run is unchanged.

- It asks nothing. The start confirmation, waking the pen and the result questions are all left out.
- When it skips a test it shows the reason as a "skipped: …" line and writes no result row. That is the case
  for tests with no Windows counterpart (`Na`), tests that are observation only (`Manual`), tests that need
  pen or hand input or operating the cover or the buttons, and tests that need equipment that is not connected
  at the moment. The connection state is read once at the start of the run: AC (`BatteryStatus.PowerOnline`),
  number of active displays (`sp9probe display`), Bluetooth A2DP sink (`BTHENUM\{0000110B…}`), Type Cover (KIP
  node).
- It records the result "measured" (when there is a suggested value) or "unverified", the basis "measured
  (automatic)", and as value and note the suggested value followed by "needs a person: …". The raw output
  folder is named `<time>-auto`. Together with `-DryRun` the steps are shortened to `-StepSeconds` seconds and
  nothing is recorded.
- What it never does: standby, lock, shutdown, changing the power mode, the brightness or the refresh rate,
  dropping a connection. Sound is played only at 20 % volume and −24 dBFS (with `-AllowSound`).

| ID | Measured automatically | Left to a person |
|---|---|---|
| CAM-1, 2 | Rear 1920x1080 and 3840x2160, front 1920x1080 and 1920x1440, 3 seconds each; the mode list (IR: list only) | Orientation of the saved frames |
| CAM-3 | Every DirectShow mode of the front and rear cameras, 2 seconds each | Nothing |
| CAM-6 | A frame at the highest resolution (kept in the raw output only) | Comparison with the Linux picture |
| SAM-5 | On AC: 30 seconds idle → 120 seconds load in the current power mode → 30 seconds cool-down | The other two power modes |
| SAM-6 | Battery for 60 seconds (AC, charge, charge and discharge rate) | Plugging and unplugging the charger, the 80 % limit |
| SAM-7 | Acceleration and orientation for 10 seconds in the current posture | The four orientations and lying flat |
| SAM-8 | `sam-events.ps1` before and after; the SMF values over 20 seconds idle → 30 seconds load → 20 seconds cool-down | Detaching the cover and the charger |
| AUD-1 | 1 kHz tone and sweep at 20 % volume (left/right balance, THD) | Listening, the 25–100 % steps |
| AUD-2 | 40/100/3700 Hz tones and a sweep at 20 % volume, with the APO on and bypassed | The difference in sound at high volume |
| AUD-3 | Delay and polarity between the microphones from a sweep; 5 seconds of silence (noise floor) | A speech recording |
| AUD-5, 6, DSP-5 | The endpoint and display lists, only when the device is connected | Checking sound and picture |
| DSP-1 | Package W over 60 seconds of a still screen, brightness, AC | Watching for flicker, the battery state |
| DSP-2 | The current mode and the refresh rates per resolution | Switching 60↔120 Hz |
| DSP-3 | The current brightness and the number of levels | Off at 0, flicker |
| DSP-4 | Lux over 10 seconds in normal light | With the sensor covered |
| PWR-4 | The sleepstudy of the last day (per session: duration, battery mW, SW/HW DRIPS %); on battery only, 5 minutes of idle discharge | 1 hour of standby |
| PWR-9 | On AC: 30 seconds idle → 300 seconds load on all cores → 60 seconds cool-down, per 30-second interval | 10 minutes of load, the temperature of the back |
| NET-1 | Adapter link speed, driver, supported radio standards, signal bars (only what can be read without the location service) | iperf3, reconnection after standby |

## Where the results go

Everything is written locally, under the working copy. Nothing here is part of the published repository.

| Path | Contents |
|---|---|
| `private/windows/baseline/<ID>/<time>/` | All raw output: `markers.csv` (step boundaries), `probe/` (pen.csv, touch.csv, rawinput.csv, events.csv, devices.txt), `hid/` and `hid-elev/` (report binaries), `sampler/`, `audio/` (wav, json), `cam/` (jpg), `bt/bt.etl`, `win/winevents.csv`, `info/`. This directory is gitignored |
| `hw/windows/baseline/<ID>.md` | Setting, result, the steps taken and the collection summary of the last run |
| `hw/windows/baseline/results.md` | `\| ID \| result \| basis \| kernel \| setting \| value and note \|`. The "kernel" column holds the Windows build (`Windows 26200.x`). Running the same ID again adds a row |

Before anything is written to the summary files, Bluetooth and MAC addresses, the tails of device instance
paths, GUIDs, the host name, SSID, BSSID and serial numbers are masked (`Protect-Text` in `common.ps1`). The
bytes of the Col07 reports (the pen's BD address) and of the BLE bonding reports are not copied into the
summary. Keyboard key codes are recorded only when they come from the Slim Pen 2's BLE keyboard (the tail
button), and only in PEN-6. For other keyboards such as the Type Cover only the count is kept.

## Collection back ends

"Confirmed" is what was checked on this device (Windows 26200) without a person. "Path only" is something
whose content needs a person or the pen to be seen.

| Back end | Method | On this device | Limits |
|---|---|---|---|
| Pen and touch pointer | `sp9probe pointer`: on a maximised canvas, `WM_POINTER` + `GetPointerPenInfoHistory` (normalised pressure 0..1024, tilt ±90°, button and eraser flags) + `GetRawPointerDeviceData` (device logical values: native pen 0..4096, HEAT virtual pen 0..65535, tilt −9000..9000 and so on). Touch through `GetPointerTouchInfo` | Confirmed (through log and summary, with injected synthetic pen and touch) | Real pen values need a person to draw |
| Pointer devices | `sp9probe devices`: HID usages and logical and physical ranges per device | Confirmed: the native THC pen (Col09, pressure 0..4096, tilt 0..18000) and touch (Col06) exist together with the HEAT virtual pen (`Microsoft HID RID\000D_0002`, pressure 0..65535, rotation 0D:3F, serial number 0D:5B, 0D:91, hover Z 01:32) and virtual touch (10 contacts) | |
| Raw Input | With `RIDEV_INPUTSINK`: 0D:02 pen, 0D:04 touch, 0D:20 stylus (BLE pen), 0C:01 consumer, 01:80 system, and (PEN-6 only) 01:06 keyboard. Digitizer reports are decoded for buttons, pressure and tilt with the preparsed data | Confirmed (THC Col06 and Col09, the HEAT virtual devices, and the KIP and VHF devices are registered) | |
| Raw HID (hidapi) | `hidlog.py` opens 045E:0C52 (THC), 0C0F (BLE pen) and 09B0 (KIP) per usage page and records timestamps and report bytes. It sends no writes and no feature requests | Confirmed. Unelevated: Col01–03, 05, 08 and KIP FFF4:000A. Elevated: Col04 (heatmap 000D:000F) and Col07 (FFF4:0001) open too | Col06, Col09 and the keyboard are held exclusively by Windows and cannot be opened with hidapi (Raw Input covers them). The BLE pen collections exist only while the pen is awake |
| Bluetooth ETW | `wpr -start BluetoothStack.wprp!BluetoothStack -filemode` (elevated) → `bt.etl` | Confirmed: the trace is recorded (13.6 MB in 5 seconds without BLE traffic, BTHUSB and WPP events), and the HCI packets of a pen session are decoded from it by [`../btetl/btetl.py`](../btetl/btetl.py) ([pen-ble-trace.md](../../../docs/research/gaps/pen-ble-trace.md)) | Windows records ATT write values truncated. Conversion: below |
| Battery | `root\wmi BatteryStatus` (mW, mWh, mV) + `Win32_Battery` % | Confirmed | Sampling stops during standby. The rate is computed from the change in charge |
| CPU, power, temperature | Performance counters: `% Processor Performance` × `Processor Frequency` = effective clock, `Energy Meter(RAPL_Package0_PKG/PP0/PP1)\Power` (mW), `Thermal Zone Information\High Precision Temperature` (0.1 K) | Confirmed | The PL1/PL2 values themselves cannot be read. The channel values SMF writes (event log) are kept instead |
| SMF power limits | `Channel Index/Instance/Value` of `Microsoft-Surface-SurfaceIhvCpuSmfClient/Operational` | Confirmed (for example ch0/51 = 13000, ch1/52 = 35000) | The meaning of the channels is inferred ([sam-events.ps1](../sam-events.ps1)) |
| Event log | Kernel-Power 506/507 (Modern Standby entry and exit), 42/107, 109 and the `Microsoft-Surface-*` logs in the test interval | Confirmed | |
| Sensors | WinRT `Windows.Devices.Sensors`: accelerometer, gyroscope, inclinometer, light, SimpleOrientation | Confirmed (light about 50 lux, acceleration at rest (0.01, −0.81, −0.56) g) | No magnetometer |
| Brightness | `WmiMonitorBrightness` | Confirmed | |
| Display | `EnumDisplaySettingsEx` (current mode, refresh rates per resolution), 250 ms polling and `WM_DISPLAYCHANGE` | Confirmed (2880x1920: 30/48/60/75/100/120 Hz) | PSR and DRRS state are not visible from Windows |
| Power, lid, tablet mode | `RegisterPowerSettingNotification` (lid, AC/DC, display state, power scheme, power saving mode), `SM_CONVERTIBLESLATEMODE`, `WM_DEVICECHANGE` | Confirmed (up to recording the initial values) | The kickstand has no sensor |
| Device nodes | Appearance and disappearance of the pen's BLE and HID nodes, the Type Cover and the KIP nodes through `Win32_PnPEntity` | Confirmed | |
| Power mode | `powercfg /getactivescheme` and `ActiveOverlay{Ac,Dc}PowerScheme` | Confirmed | One mode name (3af9b8d9…) is inferred |
| Audio | `audio.py`: plays log sweeps and tones on the internal speakers (Surface Omnisonic) while recording the internal 2-channel microphone (WASAPI). Sweep inverse filter → impulse response → 1/3-octave response, delay, delay (µs) and polarity between the microphones, left/right balance. Tone → microphone level, SNR, THD. Recording → channel levels, clipping, GCC-PHAT delay, sum − difference | Confirmed (20 % volume, −24 dBFS): shared-mode delay between the microphones L −104 µs / R +125 µs, both microphones the same polarity, SNR 54–59 dB | Recording is shared mode only (capture APO on). PortAudio's WASAPI exclusive capture delivers duplicate buffers on this device and is not used. Exclusive playback (render APO bypassed) works |
| Camera | ffmpeg DirectShow `-list_options`; after a capture, the measured fps from the showinfo timestamps and the last frame as jpg. The mode list including IR through WinRT `MediaFrameSourceGroup` (shared read-only) | Confirmed: front 1920x1080 27 fps, rear 29 fps, rear 3840x2160 29 fps. IR 640x600@60 NV12/P010 (visible only through WinRT), rear VideoRecord 1280x720@120 | IR: mode list only, no frame capture |
| Wi-Fi | Radio type, band, channel, rate and signal from `netsh wlan show interfaces` (without SSID and BSSID). Failing that, the link speed from `Get-NetAdapter`. `netsh wlan show drivers` (supported standards) and the signal bars of the WinRT connection profile work without the location permission (the profile name is not written). Throughput when `iperf3` is present and a server is entered | Only the link speed is confirmed (AX211, receive 585 / transmit 867 Mbit/s) | `netsh wlan` needs the location service to be on (elevation does not help). Turning it on is a settings change, which the tool does not make. iperf3 is installed separately |
| Standby report | `powercfg /sleepstudy` (elevated, 1 day). `summarize.py` turns the XML into a table of per-session duration, change in charge and SW/HW DRIPS | Confirmed | For sessions shorter than 30 minutes the mW value is coarse because of the charge resolution |

## What each test collects, and its Linux counterpart

`-List` shows the same. "Compared value" is the column to read next to the Linux result.

| ID | Collected on Windows | Compared value | Needed |
|---|---|---|---|
| PEN-1 | Pointer (raw pressure), pointer devices | Maximum and range of the raw pressure, minimum and median raw pressure at stroke start ↔ `ABS_PRESSURE` | Pen |
| PEN-2 | Pointer (12 steps: 30/45/60° × 4 directions) | Mean ± deviation of tiltX/Y per step ↔ `ABS_TILT_X/Y` | Pen, protractor |
| PEN-3 | Pointer | Number of side button presses, presses while drawing ↔ `BTN_STYLUS` | Pen |
| PEN-4 | Pointer | Eraser and inverted flags ↔ `BTN_TOOL_RUBBER` | Pen |
| PEN-5 | Bluetooth ETW (elevated), device nodes, pen battery (PnP `{104EA319…} 2`) | Reconnection time (length of step 2), battery % ↔ `upower` | Pen (starting asleep) |
| PEN-6 | Raw Input keyboard (pen BLE only), hidapi (BLE) | VK and scan code per tail button action ↔ `evtest` key codes | Pen (awake) |
| PEN-7 | Pointer + elevated hidapi (Col07) | Number of reports 84/110 ↔ ithc hidraw | Pen |
| PEN-8 | Pointer + Raw Input | Report counts of THC Col09 and Col06 against the HEAT virtual devices ↔ events of the kernel's 0C52 input devices | Pen |
| PEN-9 | Bluetooth ETW (elevated), hidapi (BLE), pen BLE capabilities (elevated hid-caps) | The BLE writes (report 65) during haptics are looked up in the ETW. What is felt | Pen (awake), an application with haptics |
| TOUCH-1 | Pointer | Number of contacts, number of short taps, maximum simultaneous contacts ↔ `libinput debug-events` | |
| TOUCH-2 | Pointer | Number of touches that arrived while the pen was in proximity | Pen |
| TOUCH-3 | Pointer, event log | Number of standby entries and exits, input after resume | Pen |
| TOUCH-4 | Power notifications, event log | Whether it woke | Pen |
| CAM-1, 2 | ffmpeg capture, mode list (including IR) | Resolution and measured fps, orientation ↔ `cam -C30` | |
| CAM-3 | Every DirectShow mode, 2 seconds each | Success and fps per mode. The Windows sensor mode for the front camera is the single 1296x972 mode ([camera.md](../../../docs/research/gaps/camera.md) section 1.5) | |
| CAM-4 | Rear captures near and far | Sharpness (variance of the Laplacian) | A near object |
| CAM-5 | 10 seconds of streaming, front and rear | Which LED lights (observed) | |
| CAM-6 | A frame at the highest resolution (kept in the raw output only) | Picture comparison | The same lighting |
| SAM-1 | Power notifications, device nodes, event log | Changes of the cover nodes, tablet mode | Type Cover |
| SAM-2 | Power notifications (ConvertibleSlateMode) | Tablet mode switch ↔ `SW_TABLET_MODE` | Type Cover |
| SAM-3 | Raw Input consumer and system, event log | Volume and power button reports | |
| SAM-4 | 5 minutes of load, CPU and power | Reboot into Linux right after the load (the fan RPM is read on Linux) | Charger |
| SAM-5 | 3 power modes × 120 seconds of load | Effective clock, package W and temperature per mode ↔ `turbostat` for the 4 `platform_profile` levels | Charger |
| SAM-6 | Battery, power notifications | Number of AC transitions, charge rate | Charger |
| SAM-7 | Sensors (5 postures) | Acceleration vector per posture → mount matrix ↔ `monitor-sensor`, `in_accel_*` | |
| SAM-8 | Event log, `sam-events.ps1` before and after, CPU | Surface log events, SMF channel values ↔ `surface_aggregator` tracepoints | Charger, Type Cover |
| AUD-1 | 1 kHz tone at volume 25/50/75 (100 after confirmation), sweep at 50 % | Left/right balance, THD ↔ `speaker-test` | A quiet room |
| AUD-2 | 40/100/3700 Hz tones and a sweep with the APO on and bypassed | Response and THD with and without processing ↔ filter chain off/on | A quiet room |
| AUD-3 | Sweep (microphone phase), 5 seconds of recorded speech | Delay and polarity between the microphones, sum − difference, clipping ↔ the G5 sox comparison | A quiet room |
| AUD-4 | Nothing (observation) | Whether there is echo | A call partner |
| AUD-5, 6 | Audio endpoint list before and after | Whether there is sound | BT earphones / external monitor |
| DSP-1 | Power notifications, brightness, battery, CPU and power | Flicker (observed), package W | Charger |
| DSP-2 | Display mode polling, mode list | Number of switches, screen going dark (observed) | |
| DSP-3 | Brightness | Lowest level, off at 0 (observed) | |
| DSP-4 | Light sensor | Lux in normal light and covered ↔ `in_illuminance_raw` | |
| DSP-5 | Number of displays, device nodes, event log | Connect and disconnect | USB-C monitor |
| PWR-1, 2, 3 | Event log (506/507), lid notifications | N/M resumes | Charger, cover (PWR-2) |
| PWR-4 | Battery, event log, sleepstudy (elevated) | Standby mW (from the change in charge), HW DRIPS ↔ `energy_now`, `slp_s0_residency_usec` | 1 hour |
| PWR-5, 6, 8 | No Windows counterpart (the reason is in `-List`) | | |
| PWR-7 | Nothing (observation) | Fully off | USB hub |
| PWR-9 | 10 minutes of load, CPU and power, SMF | Clock, W and temperature per 30-second interval ↔ `turbostat` | Charger |
| NET-1 | Radio state, iperf3 (optional) | Band, channel, rate, throughput | Access point, iperf3 server |
| NET-2 | Bluetooth device list, device nodes | Reconnection | Mouse, earphones |

Tests that need the pen awake (PEN-6, PEN-9) check for the BLE HID node before they start and, when it is
missing, wait for you to wake the pen with the tail button. PEN-5 is the opposite: it starts with the pen
asleep.

## Converting the Bluetooth ETW

`bt.etl` is in WPR format. To see the HCI packets use one of the following.

- [`tools/windows/btetl/btetl.py`](../btetl/btetl.py) of this repository: decodes the BTHPORT HCIRAW events into an HCI and ATT
  timeline and a btsnoop file. Its method and limits are in
  [pen-ble-trace.md](../../../docs/research/gaps/pen-ble-trace.md), section 1.
- `btetlparse.exe` from the Windows Driver Kit (`Windows Kits\10\Tools\<arch>\Bluetooth\BTETLParse\`): extracts
  the HCI traffic from the ETL as text and as `.cfa`.
- BTVS (`btvs.exe`) from the Microsoft Bluetooth Test Platform: passes HCI to Wireshark live during a test.
  Repeating the same test with it instead of the ETL lets you save btsnoop or pcapng.

The last two are not installed by the tools of this repository (installing the WDK or the BTP is a person's
decision). Their options are in their own documentation. For pen haptics look at the HID over GATT output
report 65 (ATT Write); for pairing and reconnection look at the LE connection and encryption events.
Addresses stay in the raw output only.

## Files

| File | What it does |
|---|---|
| `baseline.ps1` | Entry point. Test definitions, stepping, starting and stopping the collection, summary, recording the result |
| `setup.ps1` | Prepares the build, the venv, ffmpeg and the wprp |
| `common.ps1` | Paths and shared functions (masking with `Protect-Text` and others) |
| `sp9probe.cs` | Pointer canvas, Raw Input, power and display notifications, device ranges, display modes, load, volume |
| `sampler.ps1` | Periodic collection of battery, CPU, power and temperature, sensors, brightness, device nodes, power mode |
| `elevated.ps1` | The elevated part (starting and stopping the Bluetooth ETW, elevated hidlog, sleepstudy). Called only through `run-elevated.ps1` |
| `hidlog.py` | Reads HID input reports (hidapi) |
| `audio.py` | Speaker playback with microphone recording, and the analysis |
| `camera-sources.ps1` | Mode list of the WinRT frame sources (including IR) |
| `summarize.py` | Raw output folder → Markdown summary and suggested values |
