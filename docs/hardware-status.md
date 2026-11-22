# Hardware status

What each part does on Linux, in detail; the short table is in the [README](../README.md#hardware-status).

| | |
|---|---|
| System | NixOS, this repository's kernel 7.3.4 (stable v7.3.4 + linux-surface 7.3 + [patches](../kernel/README.md) 0001–0026)[^later-patches] |
| Firmware | UEFI 24.103.143, SAM 6.603.139 |
| Accessories | Type Cover attached, Slim Pen 2 |
| Options | The NixOS module's, `hardware.surface-pro-9.*` ([nixos.md](nixos.md)); other distributions install the same files ([install.md](install.md)) |
| Other distributions | The packaged routes were checked once each; see [install.md](install.md#support) and the tables at the end of [test-results.md](test-results.md) |

Evidence terms: **measured** (logs, numbers), **observed** (seen, heard or felt on the device), **inferred** (a
conclusion about a cause or effect that was not tried), **unverified** (not tested yet, which is not a failure; most
are manual tests). Test IDs (`PEN-1`, …) are defined in [test-plan.md](test-plan.md) and their results are in
[test-results.md](test-results.md); patch numbers refer to [kernel/PROVENANCE.md](../kernel/PROVENANCE.md). Where
documents disagree, those two win.

[^later-patches]: Most results were measured on kernels with 0001–0022. The later patches matter only for the pen's
    cover coupling and reconnection (0023, 0024, Bluetooth) and for the gain range of the camera sensors (0025,
    0026); each row of the test results names the patch range of its kernel.

## Display

Verdict: works.[^display-open] Tests: DSP-1 pass, DSP-2 pass, DSP-3 pass.

- **Measured.** Driver i915. With no `i915.enable_psr` parameter the panel runs PSR1 and `tainted` is 0; giving
  the parameter taints the kernel.[^psr-measurement] FBC is off because of PSR1. While the screen is updating the
  PSR state is almost always IDLE, and idle the package draws 1.6 W.[^display-power] The EDID and the runtime VBT are
  identical to the ones read on Windows. Modes 2880×1920@60 (preferred) and @120 are both exposed, `vrr_capable` = 1,
  `vrr_range` 30–120, DRRS is not enabled. Switching between 60 and 120 Hz works (10/10) and is a full modeset,
  since the link rate changes: the screen is dark for about 1.1 s each time.[^refresh-switch] At 120 Hz with VRR
  on there were no display errors over 16 boots. PSR depends on the desktop's VRR setting: with VRR on, i915
  keeps PSR disabled at both 60 and 120 Hz; with VRR off, PSR1 is enabled at both rates and the panel
  self-refreshes on a static screen (87 % of samples in SRDENT at 60 Hz; 42 % and 33 % of samples during
  ordinary use at 120 and 60 Hz). 120 Hz with VRR off also runs without i915 errors.[^psr-vrr] Backlight: only
  `intel_backlight` (type raw, max 96000); a written
  value is read back from `actual_brightness`, 0 and 1 included, and the driver leaves the backlight enabled at 0.
- **Observed.** With VRR off (PSR1) on battery, 5 minutes each at 120 and 60 Hz of typing, scrolling and a
  static screen showed no flicker (DSP-1); on AC no flicker was noticed in use.[^flicker-scope] The desktop's
  brightness keys go down and up in 20 steps, to 1920 (2 %) at the lowest, with no flicker while adjusting;
  at 2 % the screen is dark with the backlight on (DSP-3). In room light the screen content cannot be seen at
  raw brightness 0 and 1; in a dark room it is barely visible at 1 and 2, so the raw values go far below the
  desktop's lowest step.[^backlight-min]

- **Inferred.** That PSR2 is what causes the flicker reported for this device. What is confirmed is that
  Windows disables PSR2 on this panel (`PSR2Disable=1`).
- **Unverified.** The colour result of assigning the generated ICC profile. Whether PSR saves power here: the
  screen-on idle measurements so far vary by more than the difference (an open task,
  [research/gaps/display-power.md](research/gaps/display-power.md) difference 1).
- **Responsible.** Patch 0014 (i915 quirk that disables PSR2 for this panel, PSR1 stays). On a kernel without that
  patch the platform files limit PSR to PSR1 through i915 debugfs without tainting the kernel (unverified on the
  device).[^psr-fallback] The helper can set a PSR policy per power source (`sp9-ctl psr`: the AC side works, the
  battery side is unverified) and writes an ICC profile of the panel's native gamut (`sp9-ctl icc`: the file is
  produced).[^icc]

- **Research.** [research/gaps/display-power.md](research/gaps/display-power.md),
  [research/control/display.md](research/control/display.md).

[^display-open]: The colour result of the ICC profile is untested, and the power effect of PSR is not resolved
    (listed under Unverified).

[^psr-vrr]: Read from `i915_edp_psr_status` on battery with only the desktop's VRR setting changed; the kernel
    log had 0 i915 or PSR lines over the 5-minute runs.

[^refresh-switch]: HBR 2.7 Gbps at 60 Hz, HBR2 5.4 Gbps at 120 Hz. Dark for 1.03–1.14 s per switch, from the
    backlight off and on lines of the i915 KMS debug log; link training passed every time.

[^psr-measurement]: `PSR mode: PSR1 enabled`, and the boot log has "applying disable PSR2 quirk". The parameter sets
    taint flag 64 (an unsafe i915 option was set). The value read on the boot with the parameter was 4160, which is
    that 64 plus 4096 from an out-of-tree test module that was also loaded on that boot.

[^display-power]: While the screen is updating, 300 samples at 10 Hz: IDLE 295, SRDENT 3. Idle for 60 s: PkgWatt
    1.6, Pkg%pc8 24–28, pc10 0.0.

[^backlight-min]: The two readings differ in ambient light; whether the screen is lit at 0 could not be judged.

[^flicker-scope]: On AC at 60 Hz with PSR1, in use rather than the test plan's procedure.

[^psr-fallback]: Component `psr`, option `display.psr1`. The kernel parameter `i915.enable_psr=1` does the same
    with a taint and is left for a locked-down kernel (`psr-param`, `kernelParams.psr`, off by default). Both are
    unverified on the device.

[^icc]: From the factory calibration in UEFI variables.

## Touch

Verdict: works.[^touch-open] Tests: TOUCH-1, TOUCH-2, TOUCH-3, TOUCH-4 and PEN-8 pass.

Two daemons can turn the digitizer's raw data into input devices, selected with the option `touch.daemon`:
iptsd with this repository's patches (the default) and `sp9-digitizerd`, this repository's own daemon
([digitizerd.md](digitizerd.md)). The test device runs `sp9-digitizerd`; the measurements without contact
below were taken with iptsd.

- **Observed.** With `sp9-digitizerd`: touch works, as do pen writing, the eraser end, the barrel button and
  the pen haptics of the helper (TOUCH-1). In daily use with touch in Xournal++ and Rnote nothing wrong was
  noticed (TOUCH-1).
- **Measured.** `sp9-digitizerd` starts by itself after a reboot and finds the digitizer by its ID.[^hidraw-number]
  A pen setting changed through the helper reaches the daemon without a restart of the process. iptsd reads
  `/etc/iptsd.conf` and then the preset `/etc/iptsd.d/surface-pro-9.conf`, and runs in touchscreen mode. With the
  preset's thresholds there were no ghost touches without contact, and s2idle cycles left no ithc errors in the
  kernel log.[^touch-counts] With patch 0018 every Feature response belongs to the requested report.[^ithc-0018]
  Kernel-side input devices exist next to the virtual devices of the daemon, and the compositor opens both; during
  pen and finger contact only the daemon's devices delivered events (PEN-8).[^kernel-duplicates]
- **Measured, with `sp9-digitizerd`.** With its thresholds 24/20, exactly 10 light taps and 5 three-finger swipes
  were all caught (10/10, 5/5); with the iptsd defaults 40/36, 1/10 and 0/5 (TOUCH-1). With
  `DisableOnStylus` on, a palm rubbed on the screen while the pen hovers gives 0 touch contacts, against 4 with
  it off (TOUCH-2). On every resume the daemon opens the digitizer again, and touch and pen worked after every
  resume checked by hand (TOUCH-3).[^digitizerd-resume]
- **Measured + observed.** Finger taps and a pen stroke on the dark screen do not wake the device from s2idle
  (3/3, TOUCH-4), as expected.[^touch-wake]
- **Unverified.** Whether Windows wakes by touch.
- **Responsible.** linux-surface's ithc driver; iptsd or `sp9-digitizerd`. The preset
  [`userspace/platform/iptsd/surface-pro-9.conf`](../userspace/platform/iptsd/surface-pro-9.conf) (option
  `touch.enable`; `sp9-digitizerd` has the same values built in). Patch 0018 (ithc: match Feature responses to
  the requested report ID). The helper can override the thresholds and inhibit the touchscreen while the panel is
  off.[^touch-helper]

- **Research.** [research/gaps/pen-touch.md](research/gaps/pen-touch.md),
  [research/control/input.md](research/control/input.md), [digitizerd.md](digitizerd.md).

[^touch-open]: Touch does not wake the device from suspend, as expected (TOUCH-4); whether Windows wakes by
    touch is not known.

[^hidraw-number]: The hidraw number differed between two boots.

[^touch-counts]: Thresholds 24/20/5: 0 ghost touches in 150 s without contact. After 17 s2idle cycles the kernel
    log has 0 lines of ithc "DMA RX timeout" or "failed to reset".

[^ithc-0018]: The kernel log has 0 lines of `hid_input_report failed` or `too short`.

[^kernel-duplicates]: `Intel Touch Host Controller Touchscreen` and `… Stylus`; libinput takes them as a touch
    device and a tablet. During 8 s of pen and 8 s of finger input they had 0 events, the daemon's devices 5318
    pen events and 5 touch contacts.

[^touch-wake]: `power/wakeup` of the controller is disabled, and ithc takes the device down on suspend and has no
    wake set-up.

[^digitizerd-resume]: The daemon keeps its input devices while the digitizer's node is away. In 10 RTC-woken
    cycles it opened the digitizer again in the same second as the resume, with 0 lines of ithc "DMA RX
    timeout", "failed to reset" or `hid_input_report failed`. Checked by hand: after 4/4 lid resumes touch and
    pen, after 2/2 power-button resumes the pen. At one lid resume ithc logged 14 lines of `hid_input_report
    failed with -16`; touch and pen worked after it.

[^touch-helper]: `sp9-ctl iptsd`: works with iptsd; the hand-over to `sp9-digitizerd` is unverified for the touch
    thresholds. `sp9-ctl touch-off`: the setting toggles; the effect with the panel off is unverified.

## Pen (Slim Pen 2)

Verdict: works: writing, barrel button, eraser end, and with the device helper haptics, cover coupling and
reconnection.[^pen-open] Tests: PEN-1 to PEN-9 pass.

- **Observed, writing.** With the pen patches for iptsd ([userspace/iptsd](../userspace/iptsd/README.md),
  0001–0017) handwriting in a drawing application is good.[^writing-feel] Drawing with the barrel button held
  works (PEN-3) and the eraser end erases (PEN-4). The same holds with `sp9-digitizerd`, which the device runs
  now. After erasing, when the pen is taken away, one drawing application (Rnote) keeps showing its eraser
  cursor, while the daemon reports the eraser as gone (`BTN_TOOL_RUBBER` 0, 4/4 captures): that cursor is the
  application's and the desktop's cursor handling, and Xournal++ shows no pen hover cursor at all
  (PEN-4).[^eraser-cursor]
- **Measured, writing**, by replaying raw captures against what Windows reports for the same captures: 133
  position reports a second instead of 67, position within 0.04–0.05 mm and tilt within 2° in the
  median.[^replay-numbers] The stylus device declares X 0..27389 and Y 0..18259 (0.01 mm), pressure 0..4096, tilt
  ±9000, `BTN_STYLUS` and `BTN_TOOL_RUBBER`.
- **Measured + observed, pressure.** With `sp9-digitizerd`, pressing gradually harder reaches 4096, the end of the
  range, without pressing very hard: pressure saturates below full force (PEN-1).
- **Measured + observed, tilt.** With `sp9-digitizerd`, the pen held by hand upright and at about 45° in four
  directions: the direction and sign of the tilt are right, the magnitudes read 39–48°, and the jitter at rest
  is within 1° (PEN-2).[^tilt-check]

- **Optional, unverified on the device.** `sp9-digitizerd` can report the pen position a few milliseconds
  ahead (`sp9-ctl pen predict`, off by default; measured by replay only, [digitizerd.md](digitizerd.md)
  section 13).
- **Observed, haptics.** A click sent with output report 65 is felt. The continuous ink waveform is felt during
  strokes, almost as on Windows, once the host feeds the pen's speed back to the digitizer.[^haptic-reports] The
  device helper does this with `sp9-ctl pen ink on`: the pen vibrates while drawing and stops on lift (observed,
  with the helper alone); `surface-pen-feedback` is the standalone alternative. Faster strokes feel stronger
  (observed). The helper's strength setting is the intensity byte written to the pen, as on Windows
  (observed).[^haptic-strength] Without that feedback the continuous waveform is not felt.[^no-feedback]
- **Measured.** Tail button: single click Meta+F20, double click Meta+F19, long press Meta+F18. Pairing by
  hand works (2/2).[^manual-pairing] BlueZ and UPower then
  show the pen's battery level. The pen disconnects by itself after 50–80 s without use (reason 0x13, as on
  Windows). The touch controller's descriptor has the FFF4:0001 collection with input reports 84 and
  110, 12 B each; with the pen paired and connected, none of them arrived on the digitizer during 8 s of drawing
  (PEN-7).

- **Measured, with kernel patch 0023** (first loaded as a module, then on a booted kernel built with it).
  - Bonding through the cover's pen slot, without pairing mode, with LE legacy OOB pairing.[^oob-coupling] The
    device helper does this, automatically when a pen without a bond is put into the cover
    (`sp9-ctl pen auto-couple on`) or once with `sp9-ctl pen couple`: Paired, Bonded and Trusted in BlueZ, and the
    pen's HID attached without restarting bluetoothd. `pen couple` on a pen without a bond succeeded at the first
    attempt 12 times out of 12, and 6 times out of 6 in the state that used to fail.[^couple-order] A connected
    pen that has a bond gives `already-coupled`.
  - What coupling through the cover needs. The pen's coupling address is a value of the pen XOR the host
    address, and the helper learns that value from one bond made by a cover coupling: the pen coupled once on
    Windows (bond imported with `tools/linux/pen-bond-from-windows.py`) or earlier through this helper.
    Without such a bond it needs the 16-byte coupling prefix from the user (`/etc/sp9/pen-coupling-prefix`);
    with neither it reports `coupling-unavailable`. Pairing mode (hold the tail button) always works. Measured:
    the value learned from the bond of an earlier coupling, then `pen couple --force` → `coupled`.
  - Reconnection. A pen bonded that way, to which the host gave its identity key, disconnected by itself and was
    connected again when it was picked up and drawn with.[^reconnect-timing] A pen that is bonded again in the
    same boot reconnects as well with patch 0024 (measured on a kernel built with it).[^without-0024]
  - A bonded pen in the cover. It stays connected there after the bonding; after a resume from suspend it is
    disconnected and does not come back by itself.[^cover-key] The helper then probes instead of coupling
    again: by request and automatically the result was `connected` with the bond kept.[^probe] A host-side
    disconnect silences the pen for more than 30 s, so the helper never drops the link and removes a bond only
    once it is proven dead.

  - The battery level of a pen bonded this way reads 100 % (PEN-5).
  - Another operating system on the same device. The pen keeps one bond per host address, and all systems on
    the device share the Bluetooth controller. After the pen was coupled on an Ubuntu installation, NixOS's bond
    was dead and the helper's probe did not recognise it;[^dead-bond] the pen stayed disconnected until
    `sp9-ctl pen couple --force` with the pen in the cover coupled it again (measured). The same applies after
    coupling it on Windows ([helper.md](helper.md), the section on the pen).
  - Fail-closed pairing. With a TK stored, a pairing in which the pen answers without the OOB flag is refused
    by patch 0023 with Pairing Failed (OOB Not Available) instead of falling back to Just Works (measured with
    `btmon`, twice in a row).[^fail-closed]

- **With the eraser end** there is no continuous vibration, on Windows as on Linux (observed); only a click is
  felt, and the helper writes one at eraser touch-down (observed: one click, as intended).
- **Does not work.**
  - Reconnection of a pen bonded in pairing mode. After its idle disconnect the pen does not advertise
    again.[^no-advertise] To use the Bluetooth features again the pen has to be put in pairing mode again. The pen
    advertises for reconnection only to a host that gave it its identity key at bonding, which this pairing does
    not do.[^firmware-check]
  - Automatic bonding through the digitizer. Setting Features 86 and 112 on the digitizer leaves their values
    unchanged, input reports 84 and 110 do not arrive, and the pen does not appear over Bluetooth. The channel
    that works is the cover (above).
- **Unverified.** A count of false barrel button detections on the device (the replays are the
  measurement); the screen edges, the palm, another pen and a rotated screen. Whether the FastHostId is
  needed for the continuous waveforms. Several paths of the helper.[^pen-helper-unverified] The standalone unit
  `surface-pen-feedback.service` and the option `pen.feedbackService` as installed. The responder and Secure
  Connections paths of patch 0023 (compile-tested only). The helper's guard on a coupling link through the pen's
  private address (tested with fakes only).[^private-address]

- **Responsible.** iptsd with the patches of `userspace/iptsd`, or `sp9-digitizerd`, for drawing; the libinput
  quirk of `userspace/platform` for the tip threshold. Drawing does not need Bluetooth; the tail button, the
  battery level and haptics do. The device helper for haptics, speed feedback and cover coupling ([helper.md](helper.md), the
  section on the pen; pen haptics and automatic coupling are off by default). Kernel patches 0023 (LE legacy OOB
  pairing) and 0024 (resolving list entry replaced when a device is paired again) for the cover coupling and
  reconnection. The prototypes in [userspace/pen](../userspace/pen/README.md) (`pen-haptic`,
  `surface-pen-connect`, `surface-pen-feedback`; options `pen.enable`, `pen.connectService`,
  `pen.feedbackService`, off by default) for systems without the helper.
- **Research.** [research/gaps/pen-touch.md](research/gaps/pen-touch.md),
  [research/gaps/pen-ble-trace.md](research/gaps/pen-ble-trace.md),
  [research/gaps/pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md).

[^writing-feel]: Light strokes are not lost or broken, slow diagonals no longer wobble at a normal writing angle
    (a little remains with the pen laid down to about 45°), and the drawing application no longer drops into
    selection when the pen comes into range.

[^pen-open]: The screen edges and the palm are unverified. Replaying raw
    captures matches what Windows reports within 0.05 mm and 2° (below).

[^tilt-check]: Without a protractor; 30° and 60° were not done.

[^eraser-cursor]: The daemon reports the tool as gone; nothing is erased, and it is the same with both daemons.

[^replay-numbers]: [research/gaps/pen-touch.md](research/gaps/pen-touch.md) section 5. Position was off by up to
    0.94 mm at the edges before; tilt was 9–32° too large. Pressure at light force 1.0 times (it was 1.8 times);
    the stroke count of two handwriting captures 19 of 19 and 50 of 53. Deviation of slow strokes along a straight
    edge after patch 0015: 0.026–0.061 mm rms at a normal tilt.

[^haptic-reports]: The click is waveform ordinal 3; the ink waveform is ordinal 9 at intensity 127. The feedback is
    Feature 9 of the touch controller.

[^haptic-strength]: 30 % is not felt and 200 % is clearly stronger than 100 %.

[^no-feedback]: Neither with the bytes Windows was captured sending (`41 00 7f 09 00 00 00`) nor at intensity 255.
    Written as the Auto Trigger of Feature 65 it gives only a weak, intermittent vibration.

[^manual-pairing]: A long press of the tail button puts the pen in pairing mode and it bonds with a
    `NoInputNoOutput` agent; without an agent pairing fails with `AuthenticationFailed`.

[^oob-coupling]: After the host address is written to the cover, the cover announces the pen lying in its slot;
    the host gives the same 16 random bytes to the cover and to the kernel as the pairing key, connects to the
    pen's coupling address (a second address: a value of the pen XOR the host address) and pairs.

[^couple-order]: The helper hands the key to the pen first and connects with the controller's address resolution
    switched off; the counts are with that order, about 11 s each. The state that used to fail is the pen still on
    the controller's resolving list ([helper-reference.md](helper-reference.md)).

[^reconnect-timing]: It disconnected after 50 s and was connected again 3.5 s after it was picked up;
    reconnection was confirmed again on the booted kernel with 0023.

[^without-0024]: Without it the controller keeps the old identity key and the pen comes back only after a
    discovery scan or after the adapter is powered off and on.

[^cover-key]: It accepts a connection only after a key was written to the cover.

[^probe]: Key to the cover only, connect, the stored key encrypts.

[^dead-bond]: The link dropped without an encryption failure, result `no-answer`, bond kept.

[^fail-closed]: No User Confirmation Request and no New Long Term Key, twice in a row with the TK kept; pen in
    pairing mode ([pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md) section 2.5).

[^no-advertise]: Not when it is picked up, drawn with, or its tail button is pressed. A two-minute unfiltered scan
    (30 000 advertisements) does not contain the form seen on Windows, and `Device1.Connect()` times out.

[^firmware-check]: The 16 B exchange Windows performs on every connection is a firmware update check and has
    nothing to do with it.

[^pen-helper-unverified]: Whether the continuous waveform plays with the feedback frames changed to match what
    Windows sends and the scale of 300 mm/s; the choice of the inking waveform, the dead zone, zero-force contact
    and prediction set through the helper; `pen test`; the automatic stale-bond path (encryption refused) after
    another system couples the pen again; `unsupported-kernel` on a kernel without 0023.

[^private-address]: In the device run of [pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md) section
    2.5 such a link did not come up.

## Cameras

Stack: libcamera's simple pipeline with the software ISP, through PipeWire and WirePlumber for applications.
The early measurements were taken with libcamera 0.7.2 as nixpkgs ships it; the option `camera.libcamera`
(on by default) now installs libcamera 0.8.0 with the patches of
[userspace/libcamera](../userspace/libcamera/README.md), which is what the device runs.

With the unpatched 0.7.2 three defects showed in camera applications: purple lines at a fixed spacing, an
exposure that was off or swung, and no focus control for the rear camera (measured, CAM-7). They were in the
software ISP and in the gain ranges the sensor drivers advertised, not in the kernel's capture path
([research/gaps/camera.md](research/gaps/camera.md) section 6). With the patched libcamera and kernel patches
0025 and 0026 the lines are gone and the exposure settles (measured);[^exposure-settles] in a camera application
neither camera shows lines and the brightness regulates normally (observed, CAM-2).

[^exposure-settles]: Within 5–23 frames of the start, measured with `cam` and through PipeWire.

### Front (OV5693)

Verdict: works. Tests: CAM-2 pass, CAM-7 pass, CAM-5 pass, CAM-3 fail, CAM-6 unverified.

- **Measured.** Frames arrive for 640x480, 1280x720 and 1920x1080 requests with no dropped sequence numbers and
  no sync errors.[^front-frames] The image is upright and in focus. Whatever is requested, the sensor runs its
  2592x1944 mode. With the patched libcamera the exposure no longer blows the image out, and no column or row
  differs from its neighbours.[^front-libcamera] The analogue gain control ranges from 16 to 992 (patch 0025), the
  range in which the sensor follows it.
- **Observed.** In a camera application the image has no purple lines and the brightness regulates normally.
  While the front camera streams the LED on the front lights, and the rear camera lights the LED on the rear
  (CAM-5).[^privacy-led]
- **Does not work.** The binned 1296x972 mode: with either flip control on, every buffer fails with
  `csi2-2 error: Frame sync error`, and libcamera turns both flips on.[^binned-mode] The cause is unverified, so
  the mode is not fixed: patch 0020 stops the driver from offering it, and requests of 720p and below go to the
  full mode. The CSI-2 packed formats do not work either (every buffer has the error flag); libcamera patch 0001
  keeps to the unpacked ones.
- **Unverified.** Whether the image is mirrored. Behaviour on a kernel without the patches. Image quality against Windows (CAM-6): saturation is low and there is no noise
  reduction.[^image-quality] How the exposure follows a change of lighting (starting from wrong values was
  measured instead).
- **Responsible.** Patches 0001–0006 (upstream v7.4 backports: non-continuous MIPI clock for the OV5693 on
  IPU6), 0020 and 0025; the libcamera patches.

[^front-frames]: 1280x720 120/120 frames at 28.6 fps, 640x480 30/30, 1920x1080 90/90 at 28.65 fps, 0 lines of
    `Frame sync error`.

[^front-libcamera]: Exposure 33 ms × 2.4x where 0.7.2 settled at 10.4x and blew the image out; the column and row
    check covers 640x480 to 2560x1440.

[^binned-mode]: With both flips off 20/20 frames arrive at 57 fps. libcamera turns the flips on because the sensor
    is mounted rotated by 180°. What is lost is the 57 fps binned mode without flips.

[^privacy-led]: The privacy LED GPIO goes high while streaming and low afterwards: measured.

[^image-quality]: There is no colour calibration (grey-world white balance, no colour correction matrix), so
    saturation is low, and the software ISP has no noise reduction.

### Rear (OV13858)

Verdict: works. Tests: CAM-1 pass, CAM-4 pass.

- **Measured.** 1920x1080, 1280x720 and 3840x2160 frames arrive with no buffer errors.[^rear-frames] The lens
  device (`dw9800k`) is created and a written focus value is read back from the VCM registers.[^vcm-readback] The
  image changes only between about 400 and 800. When the camera is not in use the sensor and the VCM are
  runtime-suspended (patch 0021).[^rear-suspend] On the first boot of the kernel with patch 0022 the lens device
  was created (1/1 boots). The analogue gain control ranges from 128 to 1984 (patch 0026).[^rear-gain] With the
  patched libcamera the exposure settles at frame 7.
- **Measured, autofocus.** The patched libcamera gives the camera `AfMode`, `AfTrigger` and `LensPosition`.
  At start the lens steps through 400–800 and the metadata report the state.[^film]
- **Observed.** With the film removed the rear image is upright and sharp, and autofocus focuses (CAM-1,
  CAM-4). In a camera application the rear image has no purple lines.
- **Unverified.** The dioptre scale of the lens range, that is which lens position autofocus chooses for which
  distance ([`tools/linux/cam-af-check.sh`](../tools/linux/cam-af-check.sh)). Whether patch 0022 removes the boots
  without a lens device needs many boots.[^lens-missing]
- **Responsible.** linux-surface's camera patches for the sensor; patch 0011 (maps the firmware's VCM type
  0x11 to `dw9800k`),[^vcm-mapping] 0021 (the lens powers up on the first focus write, not on open), 0022
  (ov13858 enables runtime PM before registering), 0026; the libcamera patches (0007–0009 for autofocus). Option
  `camera.pipewire` turns WirePlumber's libcamera monitor off for standby measurements.

[^rear-frames]: 1920x1080 30/30 frames at 29.95 fps, 1280x720 120/120, 3840x2160 10/10.

[^vcm-readback]: `focus_absolute` (0–1023) exists; 900 reads as `03 84`.

[^rear-suspend]: With the patched libcamera the lens node is closed outside streaming, and the sensor is suspended
    again within 3 s of the end of a stream.

[^rear-gain]: Before, auto exposure drove it to 8191, where the sensor does not follow, and the brightness swung.

[^film]: While an opaque protective film still covered the rear camera of the test unit, the image did not get
    sharp at any lens position (CAM-4 fail row); that blur was the film.

[^lens-missing]: Seen in one of two boots before it.

[^vcm-mapping]: Measured with the dw9719 driver's DW9800K settings: the VCM binds, focus writes read back from its
    registers and the image changes over DAC 400–800. The exact part is not identified.

### IR (VD55G0)

Verdict: does not work: there is no Linux driver for the VD55G0 sensor, which Windows uses for Hello face
sign-in. The mainline `vd55g1` driver does not list this sensor, and with nothing to power it the sensor does
not answer on I2C. Patch 0015 prepares its 24 MHz clock selection; the clock output is unmeasured.

Research: [research/gaps/camera.md](research/gaps/camera.md),
[research/control/camera.md](research/control/camera.md).

## Audio

The codec (Realtek ALC274) is driven by legacy `snd_hda_intel`, not SOF (measured). The pin configuration the
firmware sets is already the one the optional codec pin patch writes, so the patch changes nothing here.

### Speakers

Verdict: works; the speaker filter does not limit the 100 Hz–1 kHz range (measured, below and
[research/gaps/audio.md](research/gaps/audio.md) G4). Tests: AUD-1 pass, AUD-2 pass.

- **Measured** (by recording with the internal microphones). Left and right each produce sound and sit on
  opposite sides; both speakers have the same polarity. Playing the left channel arrives first and louder on
  capture channel 1, while listening finds left and right correct (below). The
  level follows the volume curve. Below about 60 Hz the speakers return mostly harmonics, and each speaker has
  a rise between 3.4 and 4.2 kHz where distortion goes up. The filter chain cuts both: 50 Hz −5.9 dB and
  3.6–3.8 kHz −5.9 dB in its digital transfer, and around 3.7 kHz the recorded THD of each speaker roughly
  halves.[^filter-bands] It leaves 100 Hz–1 kHz alone, where a full-scale 100 Hz tone has 72 % THD.
- **Observed**, with the current filter. Music at 25, 50, 75 and 100 % volume has no audible distortion, and
  left and right are correct (`speaker-test`, AUD-1). Switching the filter off and on at 100 % makes no large
  audible difference; the filter makes nothing worse (AUD-2).[^speaker-listen]
- **Responsible.** Option `speakers.enable`: the PipeWire filter chain
  [`60-sp9-speakers.conf`](../userspace/platform/pipewire/pipewire.conf.d/60-sp9-speakers.conf) (high-pass,
  a cut at 3.7 kHz, limiter; each stage rests on a measurement of this device), the WirePlumber rule
  that names the raw sink, and a user unit that keeps the raw sink at 100 %.[^raw-sink] Details:
  [userspace/platform/README.md](../userspace/platform/README.md).

[^filter-bands]: Each speaker alone at −6 dBFS: THD between 3.2 and 4.2 kHz goes from 4.7–8.9 to 2.3–4.3 %
    (left) and from 1.8–6.2 to 0.8–3.2 % (right).

[^speaker-listen]: Listened to through a separate PipeWire instance of the same filter, with the raw sink at
    100 %; filter off and on alternated twice for 8 s each.

[^raw-sink]: WirePlumber starts it at 40 %, which caps the output at −23.9 dB whatever the volume control shows.

### Microphones

Verdict: works; the optional echo-cancelled source cuts the near-end voice while the speakers play.[^mic-open]
Tests: AUD-3 partial (speech clips at `Mic Boost` steps 1–3; the default step 0 meets the criterion), AUD-4
partial.

- **Measured.** Both channels carry signal, 0.6–0.8 dB apart, with the same polarity. `Mic Boost` has steps of
  0/10/20/30 dB. At WirePlumber's default of 100 % (`Capture` +30 dB plus `Mic Boost` +30 dB) a tone clips;
  at the lower boost steps it does not. Opening a capture while the codec is runtime-suspended gives a start
  transient.[^mic-transient] The echo-cancel source `sp9_echo_cancel` is listed and is not the default source,
  and all of its nodes are suspended when it is not in use. With the speakers playing speech it removes the echo
  down to its own noise floor, about 60 dB below the echo in the raw input, and it lowers room noise by about
  20 dB (AUD-4).[^aec-measure] Speech from about 50 cm in front at the default gain (`Mic Boost` 0) peaks at
  −3.6 dBFS with no clipping; boost steps 1–3 clip on speech (AUD-3).
- **Observed.** With the speakers playing speech and a person speaking at the same time (double talk), the
  echo-cancelled recording keeps the near-end voice audible but almost entirely cut or smeared (AUD-4). It
  stays so with WebRTC's noise suppression and automatic gain on or off and with the capture gain at 30, 24 or
  18 dB; PipeWire's WebRTC echo canceller has no parameter for its strength. The source is chosen per
  application, and the default source stays the raw microphone.
- **Unverified.** A call through the echo-cancel source (AUD-4).
- **Responsible.** Option `microphone.enable`: a WirePlumber rule that puts the first-use gain at +30 dB
  (`Capture` +30 dB, `Mic Boost` 0 dB); it applies only to accounts with no stored volume. Option
  `audio.echoCancel`: the PipeWire source "Microphone (echo cancelled)", chosen per application.

[^mic-transient]: The first 1.0 s sits at full scale and DC remains for about 2.5 s.

[^aec-measure]: 34 s of synthetic speech at 100 % sink volume; levels per second in the 100 Hz–8 kHz band: raw
    input −7.5 to −13.9 dBFS during playback, echo-cancelled source −68.0 to −73.7 dBFS (−70.6 dBFS in silence).

[^mic-open]: No call test. Windows also applies beamforming to the two microphones (Surface Capture APO); Linux
    does not.

### Other outputs

Verdict: works. Tests: AUD-5 pass, AUD-6 pass.

- **Measured + observed, Bluetooth.** A Bluetooth earset plays speech left and right over A2DP with AAC. On
  HFP with mSBC its microphone records (RMS −29.6 dBFS over 5 s), and the recording plays back normally (AUD-5).
- **Measured + observed, DisplayPort.** With a USB-C monitor attached (two screens through an MST hub), both
  ELDs report a monitor (LPCM 2 ch, 32–192 kHz), and one of the HDMI/DP PCMs plays through the monitor's
  speakers (AUD-6).
- **Unverified.** Left and right over DisplayPort.

Research: [research/gaps/audio.md](research/gaps/audio.md).

## Type Cover

Verdict: works.[^cover-open] Test: SAM-1 pass.

- **Measured.** Four SAM HID nodes (`01:15:02:0{1,2,3,5}:00`) sit under the KIP hub and are bound to
  `surface_hid`. The LED `surface::kbd_backlight` exists with `max_brightness` 3; writing 0/1/2/3 is read back
  from the LED. Over 6 detach and attach cycles the cover's keyboard and touchpad input devices disappeared and
  came back each time (SAM-1).
- **Observed.** After each of those attaches a key press and a touchpad stroke worked. The backlight is off at 0
  and on at 3. On Ubuntu 26.04 with `sp9-dkms`:[^dkms-hid] keys,
  touchpad and the function row work, and work again after detaching and reattaching the cover (one pass).
- **Unverified.** The backlight key in the detach cycles, and the SAM nodes listed in sysfs during them (they
  were followed through their input devices). Whether the level follows the
  cover's backlight key. For patch 0016: the resulting Linux input device and key codes, and the Call Mute
  key. For patch 0017: the format of the GET_FEATURE response on Linux and the level update when the key
  notification arrives.
- **Not provided.** Continuing a two-finger scroll at the touchpad edge, as Windows does, is left to the
  compositor: only the compositor knows the scroll direction setting.
- **Responsible.** linux-surface's SAM and `hid-surface` drivers; patch 0016 (ignore the Fn usages, map Call
  Mute Toggle to `KEY_MICMUTE`) and 0017 (keyboard backlight LED). The helper sets the level
  (`sp9-ctl kbd`: works through the LED).
- **Research.** [research/control/typecover.md](research/control/typecover.md).

[^cover-open]: The edge scroll is a compositor feature (below).

[^dkms-hid]: Patches 0016 and 0017 in its `hid-surface`.

## Buttons, tablet mode and lid

Verdict: works. Tablet mode on this device means the Type Cover detached: it has no 360° fold. Tests: SAM-3
pass, SAM-2 pass, PWR-2 pass.

- **Observed.** With the cover detached, the power and volume buttons work (SAM-3). Under a compositor that holds
  the power key (logind `handle-power-key` inhibitor) a short press turns the screen off or on, quickly, without
  suspending; that is the compositor's policy, not the device's.

- **Measured.** `pinctrl_tigerlake` is loaded in the initrd and the button devices exist: `gpio-keys` with
  volume down, volume up and power (power has wakeup enabled). The tablet mode switch reads 1 with the cover
  detached or folded back behind the screen (`folded-canvas`) and 0 (`laptop`) after reattaching or unfolding it
  (SAM-2, once each). A compositor with automatic rotation reads the switch through libinput.[^switch-compositor]
  The power button wakes the device from s2idle (8/8, entered with `systemctl suspend`). In all 24 boots kept in
  the journal both `gpio-keys` input devices were registered (SAM-3).
- **Measured + observed.** Lid close → open on battery, without `pci=hpiosize`: 4/4 entered and left s2idle, with
  no GPE errors.[^lid-cycles] While charging, 5/5: logind sees the lid close and open, the device enters and
  leaves s2idle, and the screen goes off as the cover comes close and on when it opens; over four of them
  `gpe52` rose by 2 per cycle with 0 GPE errors (PWR-2). At each lid resume the SAM serial hub logged one
  `rx: parser: invalid start of frame, skipping` line (none at power-button resumes), with no further errors.
  With an external monitor plugged in, logind treats the device as docked and ignores the lid
  (`HandleLidSwitchDocked` default).
- **Responsible.** `pinctrl_tigerlake` in the initramfs.[^pinctrl] linux-surface's GPE and button patches.

[^switch-compositor]: With the cover open it holds the panel upright, and with the switch forced on through the
    compositor's test hook the panel follows an injected orientation.

[^lid-cycles]: Four times: 0 suspend failures, `gpe52` counted about twice per close and open, 0 lines of
    "No handler or method for GPE".

[^pinctrl]: The module always adds it; without it the buttons can stay dead when `soc_button_array` probes first,
    per the linux-surface wiki.

## Ambient light and orientation sensors

Verdict: works.[^sensors-open] Tests: SAM-7 pass, DSP-4 pass.

- **Observed.** With `sensors.enable` (the default) the screen follows when the device is turned, in a
  compositor that reads iio-sensor-proxy (SAM-7).
- **Measured.** Seven iio devices.[^iio-devices] The sensor hub sends a report only when a reading moved by more
  than its change sensitivity, and the SAM HID transport cannot ask for an input report. On a device at rest
  reads therefore return 0 (6/6), and iio-sensor-proxy reads (0, 0, 0).[^at-rest] With the accelerometer's
  sensitivity written as 0, which the udev rule of `sensors.enable` does (0 after boot: measured), the hub
  reports while the sensor is on and iio-sensor-proxy reports the orientation within 0.7 s.[^sensitivity-zero]
  The light sensor reports about 81 lux in room light; polling it 35 times through sysfs gave a value only
  twice, because the hub reports on change, and iio-sensor-proxy takes it from the hub's reports through its
  buffer driver. Covering the light sensor drops it from 517–616 lux to 0.00–0.51 lux (DSP-4, through
  iio-sensor-proxy).
- **Unverified.** Holding the orientation by cover posture and the rotation lock of a desktop. The sensors
  after a suspend.[^sensors-resume] Automatic brightness.
- **Responsible.** linux-surface's SAM HID and the kernel's `hid-sensor-hub`; the rule
  [`61-sp9-sensors.rules`](../userspace/platform/udev/rules.d/61-sp9-sensors.rules) and iio-sensor-proxy
  (option `sensors.enable`).
- **Research.** [research/gaps/sam-platform.md](research/gaps/sam-platform.md), test 12.

[^sensors-open]: Automatic brightness is unverified. The light sensor's zero reads are an artefact of polling
    sysfs while the hub reports only on change, not a fault of the sensor.

[^iio-devices]: Gyroscope, accelerometer, inclinometer, ambient light, device rotation, gravity, magnetometer.

[^at-rest]: The change sensitivity is 0.05 g for the accelerometer. The accelerometer's buffer stays empty, and
    single sysfs `_raw` reads of acceleration, gravity and light take 5.3–5.6 s and return 0.

[^sensitivity-zero]: The hub reports at 8.5 Hz, and a single read returns a value in 0.09 s.

[^sensors-resume]: The kernel is written to restore the sensitivity; known from the code only.

## Battery and charging

Verdict: works.[^battery-open] Test: SAM-6 pass.

- **Measured.** `BAT1` reports status, capacity, energy and power; cycle count and health are readable. At
  100 % on a charger the status reads `Charging`, not `Full` (cosmetic). `ADP1` reports `online` and, with patch 0010,
  the adapter rating in `input_power_limit` (45 W with the charger used). There is no
  `charge_control_end_threshold`. Through the helper the smart charging state is readable (what the status
  value means is unverified) and the UEFI battery limit variable reads `off`. With smart charging engaged and
  the UEFI limit off, charging stopped at 79 %; the status then reads `Not charging` (power 0, `upower`:
  `pending-charge`), with single `Unknown` readings at the stop and a single `Discharging` reading after a
  resume (SAM-6). Over 6 plug-ins and 7 unplugs `ADP1/online` followed every change; at the smart charging stop a
  plug-in reads `Unknown` for about a second and then `Not charging`, an unplug reads `Discharging` at once, and
  the kernel logs no errors (SAM-6).
- **Observed.** Both USB-C ports charge.
- **Unverified.** Releasing smart charging through the helper. The desktop indicator during plugging.
- **Known issue.** Patch 0010: if the adapter rating request fails once while AC is connected,
  `input_power_limit` becomes 0 and a change event is sent.
- **Responsible.** linux-surface's SAM battery and charger drivers; patches 0009 and 0010 (from
  linux-surface/kernel#184). The helper's battery page (`sp9-ctl battery`). The 80 % limit itself is a UEFI
  setting; the helper only reads it.
- **Research.** [research/control/battery-charging.md](research/control/battery-charging.md).

[^battery-open]: The 80 % limit is a UEFI setting that applies to Windows and Linux alike; smart charging also
    stops charging near 80 %.

## USB-C

Verdict: works. Tests: DSP-5 pass, AUD-6 pass.

- **Observed.** An external monitor shows a picture on each of the two USB-C ports (DSP-5).[^usbc-monitor]
- **Measured + observed.** DP MST: a portable monitor with two screens behind a built-in MST hub shows both, each
  at 1920×1200 60 Hz, on either USB-C port and as an extended desktop each time it is plugged in
  (DSP-5).[^usbc-mst] The monitor's speakers play audio over DisplayPort (AUD-6). Hotplug leaves no GPE storm:
  `gpe6D` rose by 14 at the first plug-in and by 0 over three further detaches, `gpe52` did not change, and the
  kernel logs no GPE or i915 errors (DSP-5).
- **Measured.** A 45 W USB-C charger charges the device. Two `typec` ports and the partner appear. With patch
  0019 the PDOs read correctly.[^pdos]
  Without `pci=hpiosize=0`, the range `1800-18fe` is reserved by `PNP0C02:01` and the
  Thunderbolt root ports' I/O windows lie outside it; after boot and three s2idle cycles the GPE counters are 0.
- **Known defects.** The partner's PDO list starts at its second entry, so the 5 V entry is missing, and
  `voltage_now` reads 0.
- **Unverified.** USB devices through the ports, GPE behaviour when a Thunderbolt device is connected.
- **Responsible.** Patches 0007 and 0008 (UCSI over SAM HID, from linux-surface/kernel#184) and 0019 (fix-up of
  the GET_PDOS responses).
- **Research.** [research/gaps/sam-platform.md](research/gaps/sam-platform.md),
  [research/gaps/display-power.md](research/gaps/display-power.md) (item 7 of the differences table, on the
  GPE storm).

[^usbc-monitor]: One portable monitor with a built-in hub (model E1730A) showed no picture, on Windows as well,
    so the cause is that monitor or its set-up. On Linux i915 trained the link at every rate tried (RBR x4, HBR
    x2, HBR2 x1 and x4) with the monitor in SST mode (measured).

[^usbc-mst]: Two SST sinks under one branch device in `i915_dp_mst_info`. The same monitor once showed no
    picture before that; the log of that attempt has only the Type-C port attaching in `tbt-alt` mode and
    dropping 1 s later.

[^pdos]: Each port's source is 5 V 3 A; its sink is 5 V 3 A fixed, 5–20 V 60 W battery and 5–20 V 3 A variable;
    the charger's source is 9/12/15 V 3 A.

## Fan and temperatures

Verdict: reading works, and no fan floor is left over from Windows. Tests: SAM-4 pass; fan values under load from SAM-5 and PWR-9.

- **Measured.** `fan1_input` reads 0 rpm at idle (12 samples) and 4745–5960 rpm under load. Eleven SAM
  temperature channels are readable. The only thermal zones are `x86_pkg_temp` and `iwlwifi_1`, with no valid
  trip point; no ACPI thermal zone is created. SAM fan command 0x0b sets the speed of a fan that SAM has
  stopped[^fan-request] and is ignored while SAM drives the fan itself; command 0x0c does nothing. After a load
  under Windows and a reboot into Linux (kernel with patch 0013) the fan ran for the first two minutes while the device
  was still warm and then read 0 rpm: the minimum fan speed Windows can leave behind does not stay (SAM-4).[^fan-floor]
- **Unverified.** The same round trip on a kernel without patch 0013, so that the patch is what clears the
  floor is not shown by comparison. The request format rests on other users' observations.
- **Responsible.** linux-surface's `surface_fan` and `surface_thermal`; patch 0013. The helper reads the fan
  and can send the experimental fan request when explicitly allowed (`sp9-ctl fan`).
- **Research.** [research/control/power-thermal.md](research/control/power-thermal.md) section 1.4.

[^fan-request]: Requests of 1500–7500 gave 1588–7407 rpm.

[^fan-floor]: 2996–3001 rpm for the first two minutes (highest SAM sensor 42.4 → 39.1 °C) and 0 rpm from 38.8 °C
    on, for the rest of the six minutes.

## Performance profiles and heat

Verdict: works; under sustained load the back runs hotter than on Windows. Tests: SAM-5 pass, PWR-9
partial.

- **Measured, kernel alone.** `platform_profile` offers `low-power`, `balanced`, `balanced-performance` and
  `performance`. Under the same load (`stress-ng --cpu 12`, 120 s, AC) only `low-power` differs in power:

  | Profile | Average W | Package max °C | Fan max rpm | bogo ops/s |
  |---|---|---|---|---|
  | `low-power` | 6.0 | 42 | 0 | 5794 |
  | `balanced` | 33.2 | 91 | 4745 | 16199 |
  | `balanced-performance` | 33.1 | 92 | 4918 | 16381 |
  | `performance` | 33.1 | 94 | 5553 | 16314 |

  A ten-minute load in `performance` holds 29.9 W with no throttling oscillation; the skin sensor keeps rising
  to 51 °C, so under the same load the surface is hotter on Linux than on Windows.[^skin-sensor]
- **Measured, with the helper.** Each profile gets its own PL1 / PL2 and EPP, measured on Windows in the
  same power mode (on AC).[^profile-limits] The written values are read back from RAPL and cpufreq. The device has no `intel-rapl-mmio`, so only the MSR limits are written.
- **Observed.** Fan sound under 40 s of load: no change noticed in `low-power` (fan 0 rpm); in `performance`
  (fan up to 2913 rpm) the fan seemed slightly faster (SAM-5).
- **Unverified.** Whether the written limits take effect under load (no load comparison with the helper
  running). The feel of the back under load. The helper's skin temperature guard, which
  lowers the limits by that sensor: experimental, off by default, unverified.
- **Responsible.** linux-surface's `surface_platform_profile`; the helper (`sp9-ctl profile`, `limits`,
  `skin-guard`). Measurement tool: [`tools/linux/perf-profiles.sh`](../tools/linux/perf-profiles.sh).
- **Research.** [research/control/power-thermal.md](research/control/power-thermal.md).

[^skin-sensor]: The package reaches 94 °C; the skin sensor is `temp9`. Windows lowers PL1 to 21–25.5 W when that
    sensor is at 47–48 °C.

[^profile-limits]: `performance` 35/60 W and 64, `balanced-performance` 28/45 W and 115, `balanced` 28/35 W and
    153. `low-power` gets only EPP 184: Windows' battery saver limits were not measured, so the helper leaves RAPL
    as it is there.

## Suspend

Verdict: works; on battery standby draws about 190 mW, 2–4 times Windows' 52–102 mW at the same S0ix residency,
and only the shallower S0i2.0 state is reached, where Windows spends 97–99 % of standby in its deepest idle
state ([research/control/standby.md](research/control/standby.md)). Tests: PWR-1 pass, PWR-3 pass, PWR-4 pass,
PWR-5 pass, PWR-6 pass, PWR-7 pass, PWR-8 unverified.

- **Measured.** `mem_sleep` offers only `s2idle`. Ten of ten cycles woken by RTC entered and resumed, with 0
  suspend failures and no unmet LPI constraint.[^slp-s0] Residency accrues only in S0i2.0; S0i3.0 stays 0. Each
  cycle logs one `intel_ipu6.psys.40: Failed to get runtime PM` warning. The RTC (`rtc_cmos`) woke the device 17 of
  17 times.[^rtc-wake] After audio playback three consecutive cycles worked and playback continued
  afterwards. With neither kernel parameter, 3/3. Wi-Fi reconnects by itself after resume. Outside suspend, with
  the screen turned off, the package is in PC10 20 % of the time and draws 0.94 W (PWR-5).[^screen-off] Woken
  with the power button 8/8. On battery, 10/10 cycles woken by RTC with 0 ithc, i915, iwlwifi or GPE error
  lines, Wi-Fi connected again 4–7 s after each resume (PWR-1). One hour of s2idle on battery with the screen
  off: about 190 mW on average, `slp_s0` residency 99.86 % (PWR-4).[^standby-power]
- **Measured + observed, shutdown.** All 7 shutdowns and 16 reboots kept in the journal reached
  `systemd-shutdown`. No device switched itself on again after power-off, no LED or screen stayed on, and no
  battery was lost while off (PWR-7).[^shutdown-runs]
- **Unverified.** Entering suspend with the power button: it depends on the desktop's power-key setting (PWR-1).
  Which devices draw the extra standby power (an open task,
  [research/gaps/display-power.md](research/gaps/display-power.md) difference 6).
- **Not tested.** Hibernation (PWR-8) is a configuration choice: the tested set-up has only zram swap, so there
  is no disk swap to hold the image.
- **Responsible.** linux-surface's patches; nothing in this repository is specific to suspend except the lid
  and GPE notes above. Measurement tool: [`tools/linux/s2idle-trace.sh`](../tools/linux/s2idle-trace.sh).
- **Research.** [research/control/standby.md](research/control/standby.md),
  [research/gaps/display-power.md](research/gaps/display-power.md).

[^slp-s0]: Of 30.8–31.2 s per cycle, 27.86–28.35 s were spent in `slp_s0`.

[^screen-off]: 60 s each on AC with Wi-Fi in use: screen on pc10 0.00 %, PkgWatt 1.42, SysWatt 4.09; screen off
    pc10 20.1 %, PkgWatt 0.94, SysWatt 1.61. No latency tolerance short enough to block PC10 (CNV 20.5 ms, THC0
    99.6 ms).

[^rtc-wake]: 20.7 s for a 20 s request.

[^shutdown-runs]: Taken from ordinary use; a separate run of five shutdowns in a row was not done.

[^standby-power]: `energy_now` 30.29 → 30.10 Wh over 3601 s (gauge step 10 mWh), with a Bluetooth mouse left
    connected. Windows' sleepstudy: 52 mW over 36.9 h and 102 mW over 4.6 h, HW DRIPS 99.8–99.9 %.

## Wi-Fi

Verdict: works. Test: NET-1 partial.

- **Measured.** Intel AX211 (`8086:51f0`), `iwlwifi`. Connected on 5 GHz at 866.7 Mbit/s with no beacon loss
  and no failed transmission.[^wifi-link] Power save is on. After s2idle it reconnects by itself: a ping to the gateway passes 3.3 s after resume (2/2),
  and on battery it was connected again 4–7 s after each of 10 resumes (PWR-1).
- **Unverified.** Throughput against Windows from the same spot.

[^wifi-link]: An 80 MHz channel, 866.7 Mbit/s in both directions; no beacon loss or failed transmission in 854 s.

## Bluetooth

Verdict: works. Tests: PEN-5, PEN-6, NET-2 and AUD-5 pass.

- **Measured + observed.** AX211 Bluetooth (USB `8087:0033`), `btusb`/`btintel`; the firmware loads with no
  error lines. The pen pairs, bonds through the cover and reconnects (see Pen). Bluetooth also worked in the
  Ubuntu, Debian and Arch runs (observed).
- **Measured + observed, other devices.** A Bluetooth mouse (BLE HID) delivers motion, wheel and button events
  and works normally in use. A Bluetooth earset plays over A2DP (AAC) and records over HFP (mSBC) (AUD-5, see
  Audio). After each of 7 suspends both came back by themselves: the earset 1–28 s after resume, the mouse
  3–38 s, sooner when it is moved (NET-2).
- **Does not work.** Waking the device from suspend with the Bluetooth mouse (observed).

## Device helper

Verdict: works; the functions not yet checked are listed below and in [helper-reference.md](helper-reference.md).

Optional (`helper.enable`): the system service `sp9-helperd`, the command `sp9-ctl` and the GTK settings app
`sp9-settings`. Boot does not depend on it. Full status per function:
[helper-reference.md](helper-reference.md); usage: [helper.md](helper.md).

| Function | State on the device |
|---|---|
| Start-up, D-Bus objects, `sp9-ctl status` | Works |
| Per-profile PL1/PL2/tau and EPP, overrides | Works: the table values are read back from the kernel. Effect under load unverified |
| Fan rpm reading | Works |
| Fan rpm request | Experimental, must be allowed first. Works on a stopped fan; ignored while the firmware drives the fan |
| Battery state, adapter rating | Works |
| Smart charging state | Readable; the meaning of the status value is unverified. Release unverified |
| PSR policy | Works on AC; battery side unverified |
| Panel ICC | The file is produced; colour unverified |
| Keyboard backlight | Works through the LED; following the backlight key unverified |
| iptsd thresholds | Works with iptsd. With `sp9-digitizerd` the values are handed over on D-Bus without a restart: unverified for the touch thresholds |
| Zero-force contact and pressure floor of the pen | With `sp9-digitizerd`: a changed value reaches the running daemon (measured). Through iptsd's drop-in: unverified by the helper's own path |
| Pen position prediction | Unverified on the device (`sp9-digitizerd` only; off by default) |
| Touch off while the panel is off | The setting toggles; effect unverified |
| Pen haptics and speed feedback | Works: vibration while drawing, stop on lift (observed); strength and one click at eraser touch-down (observed);[^helper-strength] the digitizer state is restored after a suspend (measured). Unverified: several settings and the state after an iptsd restart[^helper-haptics-unverified] |
| Pen cover coupling | Works for a pen coupled once through the cover (on Windows, or earlier through the helper) or with a user-supplied coupling prefix; otherwise `coupling-unavailable` and pairing mode. `pen couple` on a pen without a bond at the first attempt (12/12, 6/6);[^helper-couple] a bonded pen that is not connected is probed and reconnected with its bond kept (measured); `--force` on a pen without a bond on this host → `coupled` (once, measured); plain `pen couple` after another installation's bond → `coupled-no-hid`, so `--force` is not needed there (once, measured).[^helper-other-install] Unverified: whether the HID attaches later after `coupled-no-hid`, the automatic coupling with the new order, replacing a dead bond after Windows coupled the pen, `unsupported-kernel` |
| Skin temperature guard | Experimental, off by default, unverified |
| Diagnostics report | Works |
| Settings app | The window opens (observed); individual controls unverified |

[^helper-strength]: 30 % is not felt, 200 % is clearly stronger than 100 %.

[^helper-haptics-unverified]: The frames changed to match Windows together with the scale of 300 mm/s, the choice
    of waveform, the dead zone, `pen test`.

[^helper-couple]: 12/12 on a pen without a bond, and 6/6 in the state that failed before address resolution was
    switched off for the connection.

[^helper-other-install]: `--force` was measured once, on the Debian 13 installation. The plain `pen couple` case
    is a pen without a bond on this host whose last bond was with another installation; `coupled-no-hid` means
    bonded, no Bluetooth HID within 10 s; measured once, on the Arch installation.

## Other

- **RTC.** With patch 0012 the SAM real-time clock appears as `rtc1` (`surface_rtc`) and agrees with system
  UTC within one second. It has no wake alarm; the wake source in the suspend tests is `rtc0` (`rtc_cmos`).
- **Firmware updates.** Through Windows Update.
