# Test plan

These are the tests that decide the rows of the documents in [research/gaps/](research/gaps/README.md) on an
installed system: for each, the procedure and the expected result. Results are recorded in
[test-results.md](test-results.md) ([How to record](#how-to-record)). A test with no result, or with the result
unverified, has no record yet; it does not mean that the part fails on this device.

The Kind column says how a test is run:

- **scriptable**: runs unattended from a script: reads of sysfs, debugfs and logs, counters, loops with timers,
  suspend with an RTC wake, sound played through the speakers and recorded through the microphones.
- **manual**: needs physical interaction or perception: pressing buttons, plugging cables, moving the cover,
  touching the screen or drawing with the pen, looking at the screen or LEDs, listening, feeling heat, a second
  device, or a comparison with Windows.
- **mixed**: part of the procedure runs from a script and part is manual; the column names the manual part.

Of the 50 tests, 6 are scriptable, 27 manual and 17 mixed. The read-only state the tests start from is collected
by [`tools/linux/post-install-check.sh`](../tools/linux/post-install-check.sh)
([table at the bottom](#post-install-check-unattended)).
IDs are fixed. When an item is dropped its number is not reused, and a new item takes the number after the last
one of its area.

## How to record

- Results accumulate in [test-results.md](test-results.md), one row each. When an ID is tested again a row is
  added (the earlier rows stay).

  ```text
  | ID | Result | Evidence | Kernel | Config | Values, notes |
  | PEN-1 | pass | measured | 7.3.4 | default | max 4096, first stroke starts at 112 |
  ```

- **Result**: pass (same as the expected result) / partial / fail / unverified. **Evidence**: measured (logs,
  numbers, counters) / observed (seen, heard or felt on the device). "The driver bound" and "the device node exists"
  are not evidence.
- Repeated tests (suspend, mode switches) are written N/M (for example: resumed 9 times out of 10).
- **Kernel** is `uname -r`; **Config** is whatever affects the comparison: kernel parameters, iptsd settings,
  filter chain present or not.
- Keep raw logs (`evtest`, `libinput debug-events`, recordings) outside the repository. Bluetooth addresses and
  serials get into them easily. Copy the values to be committed by hand.
- When a result is in, update the "confirmed on Linux" cell of the matching row in the research document and the
  hardware status table ([hardware-status.md](hardware-status.md)).

## Tools

`hardware.surface-pro-9.diagnostics.enable` of the NixOS module (on by default) puts the tools called by the tests
below and by the collector on the system PATH: `cam`, `v4l2-ctl` and `media-ctl`, `edid-decode`, `amixer`,
`arecord` and `speaker-test`, `hda-verb`, `pactl`, `libinput`, `hid-decode`, `evtest`, `drm_info`, `i2ctransfer`,
`intel_vbt_decode`, `acpidump` and `iasl`, `sensors`, the iptsd tools. Tools that are not part of the option
(`sox`, `iw`, `fwupdmgr`, `s0ix-selftest-tool`) are not installed; run them with
`nix shell nixpkgs#<package> -c <command>`.
The pen tools (`pen-haptic`, `surface-pen-connect`, `surface-pen-feedback`) are on PATH only when `hardware.surface-pro-9.pen.enable` is
set (off by default). Otherwise use the ones in
`$(nix build .#surface-pen-tools --no-link --print-out-paths)/bin/`, run from a checkout of this repository.

- Before using the iptsd tools (`iptsd-dump`, `iptsd-calibrate`, `iptsd-show`) run `sudo systemctl stop 'iptsd@*'`,
  and start it again afterwards (post-install test section of [pen-touch.md](research/gaps/pen-touch.md)).
- The PkgTmp and CoreTmp columns of `turbostat` appear only when the `msr` module is loaded (`sudo modprobe msr`).
- A single-shot read of an iio sensor's sysfs `_raw` file can take more than 5 seconds and then return 0. That 0
  is not a value but a missing response: the sensor hub reports only on change
  ([sam-platform.md](research/gaps/sam-platform.md) test 12). Read in buffer mode; for the accelerometer the
  option `sensors.enable` (or the udev rule of `userspace/platform`) sets the change sensitivity to 0, and then
  single reads return values as well.
- The recorded results were taken under a Wayland compositor that is not GNOME. The early rows were taken
  without iio-sensor-proxy; `sensors.enable` installs it (`monitor-sensor`).
- With `touch.daemon = "digitizerd"` there is no iptsd on the system: the touch and pen rows then test
  `sp9-digitizerd` ([digitizerd.md](digitizerd.md)), and the configuration column of a result says which daemon ran.
- The camera rows are run with the libcamera the session uses: with `camera.libcamera` (the default) that is
  libcamera 0.8.0 with the patches of `userspace/libcamera`, and `cam` on PATH is the same build.

## Reading the tables

The "Research row" column names the row of a research document that the test decides. "difference N" (or an
identifier such as G2) is a row of that document's table of differences; "test N" is an item of its post-install
test list.

## Pen (PEN)

| ID | Test | Kind | Research row | Method | Expected result |
|---|---|---|---|---|---|
| PEN-1 | Pressure range and stroke-start pressure | manual | [pen-touch](research/gaps/pen-touch.md) difference 2 | `sudo evtest` on the IPTS pen device. Touch lightly → press hard, 10 times | ABS_PRESSURE reaches the maximum 4096. The value at which the lightest stroke starts (record it) |
| PEN-2 | Tilt | manual | [pen-touch](research/gaps/pen-touch.md) difference 3 | Hold the pen against a protractor at 30/45/60° from the screen, in four directions | ABS_TILT_X/Y change in the same direction and magnitude as the angle and jitter while held is small. Record mean and deviation for each angle |
| PEN-3 | Side button and false detection | manual | [pen-touch](research/gaps/pen-touch.md) difference 4 | Press the button 10 times, then draw continuously for 30 seconds without the button | 10/10 presses recognised, 0 BTN_STYLUS while drawing |
| PEN-4 | Eraser | manual | [pen-touch](research/gaps/pen-touch.md) test 5 | Touch with the eraser end | `BTN_TOOL_RUBBER` 1, acts as an eraser in a drawing application |
| PEN-5 | BLE pairing and battery | manual | [pen-touch](research/gaps/pen-touch.md) difference 7 | Hold the tail button to enter pairing mode → `bluetoothctl` (agent `NoInputNoOutput`, `scan on`, `pair`, `trust`, `connect`) → `upower -d`. Then leave the pen unused until it disconnects by itself (about 80 s), pick it up, draw, and press the tail button. Separately, with kernel patch 0023 and the device helper: put the pen into the cover's pen slot with `sp9-ctl pen auto-couple on`, or run `sp9-ctl pen couple` ([helper.md](helper.md), the section on the pen), and repeat the idle disconnect | It connects and the pen battery % is shown. Record whether it reconnects after the pen has disconnected, and the time taken, and how the pen was bonded. A pen bonded in pairing mode does not advertise after it has disconnected (measured), so no reconnection is the expected outcome for it; a pen bonded through the cover is expected to reconnect within seconds of being picked up |
| PEN-6 | Tail button | manual | [pen-touch](research/gaps/pen-touch.md) difference 9 | `evtest` on the BLE keyboard device; single, double, long press | Each action arrives as a different key combination (record the key codes) |
| PEN-7 | Pen reports through the digitizer | manual | [pen-touch](research/gaps/pen-touch.md) difference 7 | Only read the ithc hidraw (`sudo cat /dev/hidrawN | xxd`) while touching the screen with the pen | Record whether report IDs 0x54 (84) and 0x6E (110) are received. Do not record the address bytes |
| PEN-8 | Duplicate input on the kernel side | manual | [pen-touch](research/gaps/pen-touch.md) difference 12 | If `libinput list-devices` shows a 045E:0C52 kernel device, run `evtest` on it and touch with pen and finger | No events arrive on the kernel device |
| PEN-9 | Haptics | manual | [pen-touch](research/gaps/pen-touch.md) difference 6 | While the pen is connected (right after pairing, before the pen disconnects for idling), use `pen-haptic` to write the click and the continuous waveform of report 65 Manual Trigger and the Feature 65 Auto Trigger, then draw with the pen tip on the screen. Do it without and with the pen speed going to the digitizer, which the continuous waveforms need: `sp9-ctl pen ink on` of the helper, or `surface-pen-feedback` on a system without the helper (not both) | The click and the continuous vibration during a stroke are each felt. Record what was felt per waveform and intensity |

## Touch (TOUCH)

| ID | Test | Kind | Research row | Method | Expected result |
|---|---|---|---|---|---|
| TOUCH-1 | Missed light taps and swipes | manual | [pen-touch](research/gaps/pen-touch.md) difference 1 | Run `libinput debug-events`, tap lightly with a fingertip 20 times, three-finger swipe 10 times. Once with the iptsd defaults and once with the 24/20/5 settings | Fewer misses with 24/20/5 than with the defaults. Record N/20 and N/10 for each setting |
| TOUCH-2 | Palm rejection | manual | [pen-touch](research/gaps/pen-touch.md) difference 5 | Lay the palm on the screen while the pen hovers, `DisableOnStylus` off/on | No touch events when on |
| TOUCH-3 | Recovery after suspend | mixed: touch and pen after each resume | [pen-touch](research/gaps/pen-touch.md) difference 13 | Enter and leave s2idle 10 times, touch and pen each time | 10/10 recovered, no ithc "DMA RX timeout" or "failed to reset" in `dmesg` |
| TOUCH-4 | Wake by touch | manual | [pen-touch](research/gaps/pen-touch.md) difference 14 | Tap the screen and touch it with the pen while the screen is off and in s2idle | It is currently expected not to wake. Only record the result |

## Camera (CAM)

Capture with `cam` (libcamera-tools). Use the names from the output of `cam -l`.

| ID | Test | Kind | Research row | Method | Expected result |
|---|---|---|---|---|---|
| CAM-1 | Rear capture and orientation | mixed: placing the object, judging orientation | [camera](research/gaps/camera.md) difference 7 | `cam -c '\_SB_.PC00.I2C2.CAMR' -C30 -F`, with an object whose top and bottom are recognisable at least 30 cm in front | 30 frames, the image is upright (record whether it is flipped) |
| CAM-2 | Front capture | mixed: judging orientation | [camera](research/gaps/camera.md) difference 1 | `cam -c '\_SB_.PC00.I2C3.CAMF' -C30`. Once on the unmodified linux-surface kernel and once on the kernel with the 0x4800 patch | Unmodified: 0 frames expected (`stream stop time out`); patched: 30 frames and correct orientation |
| CAM-3 | Binned mode | scriptable | [camera](research/gaps/camera.md) difference 2 | `cam -c … -s width=1280,height=720 -C30`, and check that the log shows `Input 1296x972`, i.e. that the sensor is in the binned mode (the SoftISP of libcamera 0.7.2 picks the sensor's 2592x1944 mode for a `width=1296,height=972` request). A kernel with patch 0020 does not offer this mode, so no request reaches the binned mode: this test can only be run with an `ov5693` module built without 0020 | Frames arrive without `Frame sync error` or `Transfer FIFO overflow` (failure expected before a fix) |
| CAM-4 | Autofocus | mixed: holding the camera at each scene, watching the preview | [camera](research/gaps/camera.md) difference 4 and section 6.3 | On the kernel with the VCM patch, change `v4l2-ctl -d /dev/v4l-subdevN -c focus_absolute=…` while watching a preview. With the patched libcamera: hold the rear camera at a distant scene with detail and then at print 30 cm away, each with [`tools/linux/cam-af-check.sh`](../tools/linux/cam-af-check.sh) | Focus moves nearer and farther. The position autofocus chooses is close to the sharpest position of the sweep and differs between the two scenes |
| CAM-5 | Privacy LED | manual | [camera](research/gaps/camera.md) difference 11 | Watch the device while streaming from the front and the rear camera in turn | Record which LED lights (the white LED next to the front camera is expected) |
| CAM-6 | Image quality | manual | [camera](research/gaps/camera.md) difference 10 | Compare with the Windows Camera app under the same indoor lighting | Record the differences in exposure, colour and noise with two photos (kept outside the repository) |
| CAM-7 | Lines and exposure in the processed image | mixed: looking at the preview | [camera](research/gaps/camera.md) section 6 | For the front and the rear camera, `cam -c N -s width=1920,height=1080 -C60 -F` and a preview in a camera application (PipeWire path). Look for coloured columns or rows at a fixed spacing; note exposure time, gain and mean brightness per frame from the metadata. Compare a raw frame from [`tools/linux/cam-raw.sh`](../tools/linux/cam-raw.sh) with [`cam-raw-stats.py --columns`](../tools/linux/cam-raw-stats.py) | No column or row differs from its neighbours; the brightness settles within about a second of the start and does not swing afterwards; the image is neither blown out nor dark |

## SAM and platform (SAM)

| ID | Test | Kind | Research row | Method | Expected result |
|---|---|---|---|---|---|
| SAM-1 | Type Cover detach/attach | manual | [sam-platform](research/gaps/sam-platform.md) test 1 | Detach and attach the cover 5 times; keyboard, touchpad and backlight key each time | 5/5 working again, the nodes `01:15:02:0{1,2,3,5}:00` in `/sys/bus/surface_aggregator/devices/` appear and disappear |
| SAM-2 | Tablet mode | manual | [sam-platform](research/gaps/sam-platform.md) test 10 | `evtest` on the tablet mode switch; fold the cover back, detach it | `SW_TABLET_MODE` 1/0. If the desktop reacts to this switch, record that behaviour too |
| SAM-3 | Buttons | manual | [sam-platform](research/gaps/sam-platform.md) test 11 | Power, volume up/down, after each of 5 reboots | 5/5 working, volume direction correct, the power button resumes from s2idle |
| SAM-4 | Leftover minimum fan RPM | manual | [sam-platform](research/gaps/sam-platform.md) G2 | 5 minutes of load under Windows → reboot into Linux → `sensors` after 5 minutes idle | Before the patch: about 3800 RPM expected even at low temperature. After the patch: 0 or low RPM |
| SAM-5 | Performance modes | mixed: noise | [sam-platform](research/gaps/sam-platform.md) test 4 | For each of the 4 `platform_profile` levels the same load (`stress-ng --cpu 12 -t 120`): `turbostat` PkgWatt, fan RPM, noise | PkgWatt and RPM differ by level. Record as a table |
| SAM-6 | Battery and charging | mixed: plugging AC, the desktop indicator | [sam-platform](research/gaps/sam-platform.md) G9 | Plug and unplug AC 5 times; state near 80% | `ADP1/online`, `BAT1/status` and the desktop indicator update on every plug event. Record the status string when limited to 80% |
| SAM-7 | Rotation and posture sensors | manual | [sam-platform](research/gaps/sam-platform.md) test 12 | Turn the device to four orientations while reading the acceleration and orientation values. `monitor-sensor` if iio-sensor-proxy is present, otherwise `accel_3d` in iio buffer mode | Orientation changes come out correctly. Whether the screen follows is recorded only on a desktop that handles rotation |
| SAM-8 | Observing SAM events | mixed: lid and charger changes | [sam-platform](research/gaps/sam-platform.md) G6, G7 | Enable the `surface_aggregator` tracepoints and change lid, charger and load | Record TC and CID of the incoming events (for development) |

## Audio (AUD)

| ID | Test | Kind | Research row | Method | Expected result |
|---|---|---|---|---|---|
| AUD-1 | Speakers: left/right, distortion | mixed: listening for distortion and placement | [audio](research/gaps/audio.md) G2, G3 | `speaker-test -c2 -t wav`; the same music at volume 25/50/75/100% | Left and right are correct and there is no distortion up to 75%. Record the state at 100% |
| AUD-2 | Speaker filter chain | mixed: listening | [audio](research/gaps/audio.md) G4 | The same source with the filter chain off/on, `speaker-test -t sine -f 40/100/3700` | When on, low-frequency distortion at maximum volume and harshness at 3–4 kHz are reduced |
| AUD-3 | Built-in microphones | mixed: speaking from the front | [audio](research/gaps/audio.md) G5 | `arecord -D hw:0,0 -c2 -r48000 -f S16_LE -d 5 m.wav`, speaking from the front. The sox sum/difference comparison of G5 | Signal on both channels, record whether the phase is inverted, no clipping at any Mic Boost step |
| AUD-4 | Calls | mixed: the call | [audio](research/gaps/audio.md) G5 | Test a video call application with the echo-cancel source (while the speakers play) | The other party hears no echo |
| AUD-5 | Bluetooth audio | manual | [audio](research/gaps/audio.md) test 9 | A2DP playback, HFP call microphone | Both produce sound |
| AUD-6 | External monitor audio | manual | [audio](research/gaps/audio.md) test 9 | Play through the speakers of a USB-C DP/HDMI monitor | Sound is produced |

## Display (DSP)

| ID | Test | Kind | Research row | Method | Expected result |
|---|---|---|---|---|---|
| DSP-1 | Screen flicker and PSR | mixed: watching for flicker | [display-power](research/gaps/display-power.md) differences 1, 2 | Boot with `i915.enable_psr` default/1/0 in turn; static screen, typing and scrolling for 10 minutes; on AC and on battery | No flicker with `=1`. Record for each setting whether there is flicker, the mode in `i915_edp_psr_status`, and `turbostat` Pkg%pc8/pc10 |
| DSP-2 | 60↔120 Hz | mixed: watching the screen while switching | [display-power](research/gaps/display-power.md) difference 3 | Switch 5 times with the compositor's output mode setting; VRR on/off if the compositor supports it | Record whether the screen goes off and on when switching. No display faults while staying at 120 Hz |
| DSP-3 | Backlight minimum | manual | [display-power](research/gaps/display-power.md) difference 13 | Write 0 and 1 to `brightness`; go all the way down and up with the brightness keys | Record whether the screen is completely off at 0; no flicker while adjusting |
| DSP-4 | Ambient light sensor | manual | [display-power](research/gaps/display-power.md) test 6 | Under the same indoor lighting that read 47 lux on Windows, read `als` in iio buffer mode while covering the sensor (a single-shot read of `in_illuminance_raw` returns 0 on a timeout) | Similar to the Windows value and drops when covered |
| DSP-5 | External monitor and dock | manual | [display-power](research/gaps/display-power.md) difference 7, [sam-platform](research/gaps/sam-platform.md) G1 | Connect and disconnect a monitor and a charger on each of the two USB-C ports | A picture appears, and GPE errors do not pile up in `dmesg` on hotplug |

## Power (PWR)

| ID | Test | Kind | Research row | Method | Expected result |
|---|---|---|---|---|---|
| PWR-1 | s2idle round trip | mixed: the power-button resume (an RTC wake covers the rest) | [display-power](research/gaps/display-power.md) difference 6 | Enter and resume with the power button 10 times | 10/10 resumed, `suspend_stats` fail 0 |
| PWR-2 | Lid (cover) and GPE | manual | [sam-platform](research/gaps/sam-platform.md) G10, [display-power](research/gaps/display-power.md) differences 7, 10 | Close → open the cover 5 times while charging, without/with `pci=hpiosize=0` | With: 5/5 resumed, `gpe52` rises by no more than a few counts per cycle, no "No handler or method for GPE" |
| PWR-3 | Audio path and consecutive suspends | scriptable | [audio](research/gaps/audio.md) G1 | After playing sound, `rtcwake -m freeze -s 20` 3 times in a row; `00:1f.3` runtime_status before each | 3/3 entered and resumed. If it blocks with SOF, run the same test with `dsp_driver=1` |
| PWR-4 | Standby power | scriptable | [display-power](research/gaps/display-power.md) difference 6 | Battery, 1 hour of s2idle with the screen off. `energy_now` and `slp_s0_residency_usec` before and after | Reference: Windows 52–102 mW, HW DRIPS 99.8–99.9%. Record mW and residency on Linux |
| PWR-5 | Runtime screen-off PC10 | scriptable | [display-power](research/gaps/display-power.md) difference 6 | s0ix-selftest-tool `-r off`, `pmc_core/ltr_show` | Record the residency and the blocking IPs (wiki: 0.01%) |
| PWR-6 | RTC wake | scriptable | [sam-platform](research/gaps/sam-platform.md) G5 | `sudo rtcwake -m freeze -s 60` | Record whether it wakes by itself after 60 seconds |
| PWR-7 | Shutdown | manual | [sam-platform](research/gaps/sam-platform.md) G12, [display-power](research/gaps/display-power.md) difference 8 | `poweroff` 5 times; LED of a connected USB hub and the power LED | 5/5 completely off |
| PWR-8 | Hibernation | scriptable | [display-power](research/gaps/display-power.md) difference 9 | Only with Secure Boot off. `HibernateMode=reboot`, `systemctl hibernate` 3 times | 3/3 restored. With Secure Boot on, being refused by lockdown is the normal outcome |
| PWR-9 | Thermals and sustained performance | mixed: the back's temperature by hand, the Windows comparison | [sam-platform](research/gaps/sam-platform.md) G3, [display-power](research/gaps/display-power.md) difference 12 | During `stress-ng --cpu 12 -t 600`: `turbostat` PkgWatt and PkgTmp, `sensors` RTS, fan, temperature of the back as felt by hand. Compare with the same load on Windows | No throttle oscillation over 10 minutes. Record the numbers as a table |

## Network (NET)

| ID | Test | Kind | Research row | Method | Expected result |
|---|---|---|---|---|---|
| NET-1 | Wi-Fi | mixed: the Windows comparison | [display-power](research/gaps/display-power.md) difference 11 | Connect to a 5 GHz AP, compare throughput with Windows at the same place (`iperf3`), reconnection after resume | Connection holds, reconnects automatically after resume |
| NET-2 | Bluetooth | manual | none (no research document has a Bluetooth row; status in [hardware-status.md](hardware-status.md)) | Connect a mouse and earphones, reconnection after resume | Connection holds, reconnects after resume |

## Post-install check (unattended)

`sudo tools/linux/post-install-check.sh` creates per-area files and `summary.txt` in `~/sp9-check-<host>/`. It does
not use the network and only reads (no i2c, no `hda-verb`, no camera streaming, no sysfs writes). Without root it
skips debugfs, the ACPI tables, libinput, turbostat and `iptsd-check-device`.
DIFF and MISSING refer to the state of the device only. For an item whose tool is not installed it reads sysfs and
procfs instead (`/proc/asound/pcm`, the SecureBoot EFI variable, the SAM and media nodes) or records SKIP with the
tool's name. On NixOS it appends `/run/current-system/sw/bin` and the user profile to PATH and looks for the iptsd
tools next to the running daemon. For the kernel log it uses the longer of the journal and dmesg (when the ring
buffer overflows, the early boot lines are missing from dmesg).

| File | Research test items covered |
|---|---|
| `00-system.txt`, `kernel-log.txt` | Kernel, cmdline, Secure Boot, lockdown, firmware load failures, DMI (without serials) |
| `10-pen-touch.txt`, `hid/` | pen-touch tests 1, 2, 6, 7 (devices, descriptors), 9 (fwupd), iptsd state and configuration |
| `20-camera.txt` | camera tests 1, 2, 3, 4 (up to `cam -l`; without it, only sub-device registration), 7 and 8 (presence of the `camera_sensor_rotation` and `focus_absolute` controls) |
| `30-sam-platform.txt` | sam-platform tests 1, 2, 6, 8 (list), 9 (GPE counts), 10 and 11 (input devices exist), 12 (iio list) |
| `40-audio.txt`, `codec-*.txt` | audio tests 1, 2, 7, 10 |
| `50-display.txt`, `debugfs/` | display-power tests 1 (the runtime VBT, cut to its own length field, by sha256 against the `vbt/` rows of [`docs/reference/firmware-hashes.tsv`](reference/firmware-hashes.tsv): MATCH names the row, DIFF means no row has that hash), 2 (the EDID by sha256 against the `edid.bin` row: MATCH or DIFF; the EDID has a serial field that can differ per unit, so DIFF alone does not mean another panel), 3 (current PSR state), 4 (`vrr_range`, DRRS state, refresh rate per mode), 5 (backlight device and maximum), 6 (channel present, read time). When the hash table is not next to the script (the copy the helper installs), VBT and EDID are reported as INFO with the hash |
| `60-power.txt` | Current values of the counters of display-power tests 7 and 8, RAPL, NVMe APST, PCI runtime PM |
| `70-net.txt` | display-power tests 10, 12 |
| `acpi/` | The ACPI tables as Linux sees them (without MSDM), and a comparison of the DSDT with the `acpi/dsdt.dat` row of the hash table (hash and size of a dump made under Windows): MATCH when the hash is equal, DIFF when the length differs, INFO when the length is equal and the hash is not. The firmware rewrites the checksum and the NVS addresses in the DSDT at every boot, so INFO is the normal result on the reference firmware version; a hash cannot tell those bytes from another change |

MATCH in `summary.txt` means "equal to the value the documents expect", not a confirmation that something works.
That is decided by the tests in the tables above.
