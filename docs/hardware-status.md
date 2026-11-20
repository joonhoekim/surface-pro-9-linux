# Hardware status

What each part of the Surface Pro 9 (Intel, SKU `Surface_Pro_9_2038`) does on Linux, in detail. The short table
is in the [README](../README.md).

Conditions: NixOS, kernel 7.3.4 = stable v7.3.4 + linux-surface 7.3 + [`kernel/patches`](../kernel/README.md)
0001–0026, firmware UEFI 24.103.143 and SAM 6.603.139, Type Cover attached, Slim Pen 2. Most results were
measured on kernels with 0001–0022. The later patches matter only for the pen's cover coupling and reconnection
(0023, 0024, Bluetooth) and for the gain range of the camera sensors (0025, 0026); each row of the test
results names the patch range of its kernel.

How to read this page:

- **Measured** is something read from logs or numbers. **Observed** is something a person saw, heard or felt.
  **Inferred** is a conclusion about a cause or an effect that was not tried. **Unverified** is something
  nobody has checked; most of these need a person at the device.
- "Works" is used only for measured or observed behaviour. A driver that probed is not "works".
- Test IDs (`PEN-1`, `CAM-2`, …) are defined in [test-plan.md](test-plan.md). Their results are in
  [test-results.md](test-results.md), where the last row of an ID is its current state. Patch numbers refer to
  [kernel/PROVENANCE.md](../kernel/PROVENANCE.md). Where documents disagree, those two win.
- Options are those of the NixOS module, `hardware.surface-pro-9.*` ([nixos.md](nixos.md)). On another
  distribution the same files are installed by hand ([install-other.md](install-other.md)).

## Display

Verdict: works, with a caveat. Tests: DSP-1 partial, DSP-2 unverified, DSP-3 partial.

- **Measured.** Driver i915. With no `i915.enable_psr` parameter the panel runs PSR1: `PSR mode: PSR1 enabled`,
  the boot log has "applying disable PSR2 quirk", and `tainted` is 0. Giving the parameter taints the kernel: it sets
  taint flag 64 (an unsafe i915 option was set). The value read on the boot with the parameter was 4160, which is that 64 plus
  4096 from an out-of-tree test module that was also loaded on that boot. FBC is
  off because of PSR1. While the screen is updating, 300 samples at 10 Hz: IDLE 295, SRDENT 3. Idle for 60 s:
  PkgWatt 1.6, Pkg%pc8 24–28, pc10 0.0. The EDID and the runtime VBT are identical to the ones read on Windows.
  Modes 2880×1920@60 (preferred) and @120 are both exposed, `vrr_capable` = 1, `vrr_range` 30–120, DRRS is not
  enabled. Backlight: only `intel_backlight` (type raw, max 96000); a written value is read back from
  `actual_brightness`.
- **Observed.** No flicker was noticed in use. This was not the test plan's ten minutes of still image, typing
  and scrolling, and AC and battery were not tested separately.
- **Inferred.** That PSR2 is what causes the flicker reported for this device. What is confirmed is that
  Windows disables PSR2 on this panel (`PSR2Disable=1`).
- **Unverified.** Switching between 60 and 120 Hz and VRR (DSP-2). Brightness levels 0 and 1, the brightness
  keys, flicker while adjusting (DSP-3). The colour result of assigning the generated ICC profile.
- **Responsible.** Patch 0014 (i915 quirk that disables PSR2 for this panel, PSR1 stays). The kernel parameter
  `i915.enable_psr=1` (option `kernelParams.psr`) is not needed on this kernel; it is for a distribution kernel,
  where its effect is unverified. The helper can set a PSR policy per power source (`sp9-ctl psr`: the AC side
  works, the battery side is unverified) and writes an ICC profile of the panel's native gamut from the factory
  calibration in UEFI variables (`sp9-ctl icc`: the file is produced).
- **Research.** [research/gaps/display-power.md](research/gaps/display-power.md),
  [research/control/display.md](research/control/display.md).

## Touch

Verdict: works in everyday use; the defined tests are mostly open. Tests: TOUCH-1 partial, TOUCH-2 and TOUCH-4
unverified, TOUCH-3 partial, PEN-8 partial.

Two daemons can turn the digitizer's raw data into input devices, selected with the option `touch.daemon`:
iptsd with this repository's patches (the default) and `sp9-digitizerd`, this repository's own daemon
([digitizerd.md](digitizerd.md)). The test device runs `sp9-digitizerd`; the measurements without contact
below were taken with iptsd.

- **Observed.** With `sp9-digitizerd`: touch works, as do pen writing, the eraser end, the barrel button and
  the pen haptics of the helper (TOUCH-1).
- **Measured.** `sp9-digitizerd` starts by itself after a reboot and finds the digitizer by its ID (the hidraw
  number differed between two boots). A pen setting changed through the helper reaches the daemon without a
  restart of the process. iptsd reads `/etc/iptsd.conf` and then the preset
  `/etc/iptsd.d/surface-pro-9.conf`, and runs in touchscreen mode. With the preset's thresholds 24/20/5 there
  were 0 ghost touches in 150 s without contact. After 17 s2idle cycles the kernel log has 0 lines of ithc
  "DMA RX timeout" or "failed to reset". With patch 0018 every Feature response belongs to the requested
  report, and the kernel log has 0 lines of `hid_input_report failed` or `too short`. Kernel-side input devices
  (`Intel Touch Host Controller Touchscreen` and `… Stylus`) exist next to the virtual devices of the daemon;
  libinput takes them as a touch device and a tablet, and the compositor opens both. In the same boot the
  daemon's devices held non-zero values and the kernel-side devices held only zeros.
- **Inferred.** Wake by touch will not work: `power/wakeup` of the controller is disabled, and ithc takes the
  device down on suspend and has no wake set-up (TOUCH-4). Not tried.
- **Unverified.** Missed light taps and swipes counted against the iptsd defaults 40/36/15 (TOUCH-1). Palm
  rejection with `DisableOnStylus = true` (TOUCH-2). Whether touch and pen work after every resume (TOUCH-3);
  for `sp9-digitizerd`, which keeps its input devices while the digitizer's node is away, finding the node
  again after a resume has not been checked on the device. Whether the kernel-side duplicate devices deliver
  events during contact (PEN-8).
- **Responsible.** linux-surface's ithc driver; iptsd or `sp9-digitizerd`. The preset
  [`userspace/platform/iptsd/surface-pro-9.conf`](../userspace/platform/iptsd/surface-pro-9.conf) (option
  `touch.enable`; `sp9-digitizerd` has the same values built in). Patch 0018 (ithc: match Feature responses to
  the requested report ID). The helper can override the thresholds (`sp9-ctl iptsd`: works with iptsd; the
  hand-over to `sp9-digitizerd` is unverified for the touch thresholds) and inhibit the touchscreen while the
  panel is off (`sp9-ctl touch-off`: the setting toggles; the effect with the panel off is unverified).
- **Research.** [research/gaps/pen-touch.md](research/gaps/pen-touch.md),
  [research/control/input.md](research/control/input.md), [digitizerd.md](digitizerd.md).

## Pen (Slim Pen 2)

Verdict: works: writing, barrel button, eraser end, and with the device helper haptics, cover coupling and
reconnection. Tilt against real angles is unverified. Tests: PEN-3, PEN-5, PEN-6 and PEN-9 pass, PEN-1, PEN-4
and PEN-8 partial, PEN-2 and PEN-7 unverified.

- **Observed, writing.** With the pen patches for iptsd ([userspace/iptsd](../userspace/iptsd/README.md),
  0001–0017) handwriting in a drawing application is good: light strokes are not lost or broken, slow diagonals
  no longer wobble at a normal writing angle (a little remains with the pen laid down to about 45°), and the
  drawing application no longer drops into selection when the pen comes into range. Drawing with the barrel
  button held works (PEN-3) and the eraser end erases (PEN-4). The same holds with `sp9-digitizerd`, which
  the device runs now. One defect is open: after erasing, when the pen is taken away, the drawing application
  keeps showing its eraser cursor although the daemon reports the tool as gone; nothing is erased, and it is
  the same with both daemons (PEN-4).
- **Measured, writing** (by replaying raw captures, against the output of the Windows touch processor for the
  same captures; [research/gaps/pen-touch.md](research/gaps/pen-touch.md) section 5). 133 position reports a
  second instead of 67; position within 0.04–0.05 mm in the median (it was off by up to 0.94 mm at the edges);
  tilt within 2° in the median (it was 9–32° too large); pressure at light force 1.0 times (it was 1.8 times);
  the stroke count of two handwriting captures 19 of 19 and 50 of 53. Deviation of slow strokes along a
  straight edge after patch 0015: 0.026–0.061 mm rms at a normal tilt. The stylus device declares X 0..27389
  and Y 0..18259 (0.01 mm), pressure 0..4096, tilt ±9000, `BTN_STYLUS` and `BTN_TOOL_RUBBER`.
- **Optional, unverified on the device.** `sp9-digitizerd` can report the pen position a few milliseconds
  ahead (`sp9-ctl pen predict`, off by default; measured by replay only, [digitizerd.md](digitizerd.md)
  section 13).
- **Observed, haptics.** Haptics: a click (waveform
  ordinal 3) sent with output report 65 is felt. The continuous ink waveform (ordinal 9, intensity 127) is
  felt during strokes, almost as on Windows, once the host feeds the pen's speed back to the digitizer
  (Feature 9 of the touch controller). The device helper does this with `sp9-ctl pen ink on`: the pen vibrates
  while drawing and stops on lift (observed, with the helper alone); `surface-pen-feedback` is the standalone
  alternative. Faster strokes feel stronger (observed). The helper's strength setting is the intensity byte
  written to the pen, as on Windows: 30 % is not felt and 200 % is clearly stronger than 100 % (observed).
  Without that feedback the continuous waveform is not
  felt, neither with the bytes Windows was captured sending (`41 00 7f 09 00 00 00`) nor at intensity 255, and
  written as the Auto Trigger of Feature 65 it gives only a weak, intermittent vibration.
- **Measured.** Tail button: single click Meta+F20, double click Meta+F19, long press Meta+F18. Pairing by
  hand works (2/2): a long press of the tail button puts the pen in pairing mode and it bonds with a
  `NoInputNoOutput` agent; without an agent pairing fails with `AuthenticationFailed`. BlueZ and UPower then
  show the pen's battery level. The pen disconnects by itself after 50–80 s without use (reason 0x13, as on
  Windows). The touch controller's descriptor has the FFF4:0001 collection with input reports 84 and
  110, 12 B each.
- **Measured, with kernel patch 0023** (first loaded as a module, then on a booted kernel built with it).
  - Bonding through the cover's pen slot, without pairing mode. After the host address is written to the
    cover, the cover announces the pen lying in its slot; the host gives the same 16 random bytes to the cover
    and to the kernel as the pairing key, connects to the pen's coupling address (a second address, derived
    from the pen ID and the host address) and pairs with LE legacy OOB pairing. The device helper does this,
    automatically when a pen without a bond is put into the cover (`sp9-ctl pen auto-couple on`) or once with
    `sp9-ctl pen couple`: Paired, Bonded and Trusted in BlueZ, and the pen's HID attached without restarting
    bluetoothd. The helper hands the key to the pen first and connects with the controller's address
    resolution switched off; with that order `pen couple` on a pen without a bond succeeded at the first
    attempt 12 times out of 12 (about 11 s each) and 6 times out of 6 in the state that used to fail (the pen
    still on the controller's resolving list; [helper-reference.md](helper-reference.md)). A connected pen
    that has a bond gives `already-coupled`.
  - Reconnection. A pen bonded that way, to which the host gave its identity key, disconnected by itself
    after 50 s and was connected again 3.5 s after it was picked up and drawn with; reconnection was confirmed
    again on the booted kernel with 0023. A pen that is bonded again in the same boot reconnects as well with
    patch 0024 (measured on a kernel built with it); without it the controller keeps the old identity key and
    the pen comes back only after a discovery scan or after the adapter is powered off and on.
  - A bonded pen in the cover. It stays connected there after the bonding; after a resume from suspend it is
    disconnected and does not come back by itself, and it accepts a connection only after a key was written to
    the cover. The helper then probes instead of coupling again (key to the cover only, connect, the stored
    key encrypts): by request and automatically the result was `connected` with the bond kept. A host-side
    disconnect silences the pen for more than 30 s, so the helper never drops the link and removes a bond only
    once it is proven dead.
  - The battery level of a pen bonded this way reads 100 % (PEN-5).
- **With the eraser end** there is no continuous vibration, on Windows as on Linux (observed); only a click is
  felt, and the helper writes one at eraser touch-down (observed: one click, as intended).
- **Does not work.**
  - Reconnection of a pen bonded in pairing mode. After its idle disconnect the pen does not advertise again
    when it is picked up, drawn with, or its tail button is pressed; a two-minute unfiltered scan (30 000
    advertisements) does not contain the form seen on Windows, and `Device1.Connect()` times out. To use the
    Bluetooth features again the pen has to be put in pairing mode again. The pen advertises for reconnection
    only to a host that gave it its identity key at bonding, which this pairing does not do. (The 16 B
    exchange Windows performs on every connection is a firmware update check and has nothing to do with it.)
  - Automatic bonding through the digitizer. Setting Features 86 and 112 on the digitizer leaves their values
    unchanged, input reports 84 and 110 do not arrive, and the pen does not appear over Bluetooth. The channel
    that works is the cover (above).
- **Unverified.** Tilt against real angles (PEN-2); reaching full pressure and a count of false barrel button
  detections on the device (the replays are the measurement); the screen edges, the palm, another pen and a
  rotated screen; whether reports 84 and 110 are received on the digitizer (PEN-7). Whether the FastHostId is
  needed for the continuous waveforms. In the helper: whether the continuous waveform plays with the feedback
  frames changed to match what Windows sends and the scale of 300 mm/s, the choice of the inking waveform, the
  dead zone, zero-force contact and prediction set through the helper, `pen test`, the automatic coupling with
  the new order, the stale-bond path after Windows couples the pen again, `unsupported-kernel` on a kernel
  without 0023. The standalone unit `surface-pen-feedback.service` and the option `pen.feedbackService` as
  installed. The responder and Secure Connections paths of patch 0023 (compile-tested only).
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

## Cameras

Stack: libcamera's simple pipeline with the software ISP, through PipeWire and WirePlumber for applications.
The early measurements were taken with libcamera 0.7.2 as nixpkgs ships it; the option `camera.libcamera`
(on by default) now installs libcamera 0.8.0 with the patches of
[userspace/libcamera](../userspace/libcamera/README.md), which is what the device runs.

With the unpatched 0.7.2 three defects showed in camera applications: purple lines at a fixed spacing, an
exposure that was off or swung, and no focus control for the rear camera (measured, CAM-7). They were in the
software ISP and in the gain ranges the sensor drivers advertised, not in the kernel's capture path
([research/gaps/camera.md](research/gaps/camera.md) section 6). With the patched libcamera and kernel patches
0025 and 0026 the lines are gone and the exposure settles within 5–23 frames of the start (measured with `cam`
and through PipeWire); in a camera application neither camera shows lines and the brightness regulates normally
(observed, CAM-2).

### Front (OV5693)

Verdict: works. Tests: CAM-2 pass, CAM-7 pass, CAM-3 fail, CAM-5 and CAM-6 unverified.

- **Measured.** Frames arrive for 640x480, 1280x720 and 1920x1080 requests: 1280x720 120/120 frames at
  28.6 fps, 640x480 30/30, 1920x1080 90/90 at 28.65 fps, no dropped sequence numbers, 0 lines of
  `Frame sync error`. The image is upright and in focus. Whatever is requested, the sensor runs its 2592x1944
  mode. With the patched libcamera: exposure 33 ms × 2.4x where 0.7.2 settled at 10.4x and blew the image out;
  no column or row differs from its neighbours at 640x480 to 2560x1440. The analogue gain control ranges from
  16 to 992 (patch 0025), the range in which the sensor follows it.
- **Observed.** In a camera application the image has no purple lines and the brightness regulates normally.
- **Does not work.** The binned 1296x972 mode. With either flip control on, every buffer fails with
  `csi2-2 error: Frame sync error`; with both flips off 20/20 frames arrive at 57 fps. libcamera turns both
  flips on because the sensor is mounted rotated by 180°. The cause is unverified, so the mode is not fixed:
  patch 0020 stops the driver from offering it, and requests of 720p and below go to the full mode. What is
  lost is the 57 fps binned mode without flips. The CSI-2 packed formats do not work either (every buffer has
  the error flag); libcamera patch 0001 keeps to the unpacked ones.
- **Unverified.** Whether the image is mirrored. Behaviour on a kernel without the patches. Which LED lights
  (the privacy LED GPIO goes high while streaming and low afterwards: measured; CAM-5). Image quality against
  Windows (CAM-6): there is no colour calibration (grey-world white balance, no colour correction matrix), so
  saturation is low, and the software ISP has no noise reduction. How the exposure follows a change of
  lighting (starting from wrong values was measured instead).
- **Responsible.** Patches 0001–0006 (upstream v7.4 backports: non-continuous MIPI clock for the OV5693 on
  IPU6), 0020 and 0025; the libcamera patches.

### Rear (OV13858)

Verdict: frames arrive and the exposure works; focus is unverified, because an opaque protective film covers
the rear camera of the test unit. Tests: CAM-1 partial, CAM-4 partial.

- **Measured.** 1920x1080 30/30 frames at 29.95 fps, 1280x720 120/120, 3840x2160 10/10, no buffer errors. The
  lens device (`dw9800k`) is created and `focus_absolute` (0–1023) exists; a written value is read back from
  the VCM registers (900 reads as `03 84`). The image changes only between about 400 and 800. When the camera
  is not in use the sensor and the VCM are runtime-suspended (patch 0021); with the patched libcamera the lens
  node is closed outside streaming, and the sensor is suspended again within 3 s of the end of a stream. On
  the first boot of the kernel with patch 0022 the lens device was created (1/1 boots). The analogue gain
  control ranges from 128 to 1984 (patch 0026); before, auto exposure drove it to 8191, where the sensor does
  not follow, and the brightness swung. With the patched libcamera the exposure settles at frame 7.
- **Measured, autofocus.** The patched libcamera gives the camera `AfMode`, `AfTrigger` and `LensPosition`.
  At start the lens steps through 400–800 and the metadata report the state. **The image does not get sharp at
  any lens position**: held by a person towards a distant scene and towards an object 30 cm away, the contrast
  stays low and flat (a distant light is a diffuse blob), and autofocus ends as "nothing to focus on" in 4 of 6
  runs (CAM-4). The blur is not explained by the focus position.
- **Observed.** An opaque protective film covers the rear camera of the test unit. In a camera application the
  rear image has no purple lines.
- **Unverified.** Whether near and far subjects come into focus, and with it the dioptre scale of the lens
  range: to be measured again without the film
  ([`tools/linux/cam-af-check.sh`](../tools/linux/cam-af-check.sh)). Image orientation (libcamera offers only
  rot180 for this camera). Whether patch 0022 removes the boots without a lens device (seen in one of two boots
  before it) needs many boots.
- **Responsible.** linux-surface's camera patches for the sensor; patch 0011 (maps the firmware's VCM type
  0x11 to `dw9800k`; the mapping is confirmed from Windows, compatibility with the dw9719 driver's DW9800K
  settings is inferred), 0021 (the lens powers up on the first focus write, not on open), 0022 (ov13858 enables
  runtime PM before registering), 0026; the libcamera patches (0007–0009 for autofocus). Option
  `camera.pipewire` turns WirePlumber's libcamera monitor off for standby measurements.

### IR (VD55G0)

Verdict: does not work. There is no driver: the mainline `vd55g1` driver does not fit this sensor (different
status registers, firmware patch and exposure register block), and with nothing to power it the sensor does
not answer on I2C. Patch 0015 prepares its 24 MHz clock selection; the clock output is unmeasured.

Research: [research/gaps/camera.md](research/gaps/camera.md),
[research/control/camera.md](research/control/camera.md).

## Audio

The codec (Realtek ALC274) is driven by legacy `snd_hda_intel`, not SOF (measured). The pin configuration the
firmware sets equals the Windows driver's table, so the optional codec pin patch changes nothing here.

### Speakers

Verdict: works, listening unverified. Tests: AUD-1 partial, AUD-2 partial.

- **Measured** (by recording with the internal microphones). Left and right each produce sound and sit on
  opposite sides; both speakers have the same polarity. Playing the left channel arrives first and louder on
  capture channel 1: whether the speakers or the microphone channels are the swapped pair has to be heard. The
  level follows the volume curve. The filter chain is loaded and is the default sink. Against the raw sink it
  lowers the 40 Hz band (fundamental −62.8 → −69.3 dBFS) and the 3.7 kHz band (−18.9 → −26.3 dBFS, THD
  16.9 → 4.7 %); at 100 Hz, 400 Hz and 1 kHz level and THD do not change. Through the filter at 100 % volume a
  −1 dBFS tone has 4.4 / 3.1 / 4.7 % THD at 400 Hz / 1 kHz / 3.7 kHz. The filter's processing load is 0.53 ms
  per 42.7 ms period (about 1.2 %).
- **Unverified.** Audible distortion, stereo placement, how the filter chain sounds (nobody has listened
  critically).
- **Responsible.** Option `speakers.enable`: the PipeWire filter chain
  [`60-sp9-speakers.conf`](../userspace/platform/pipewire/pipewire.conf.d/60-sp9-speakers.conf), which carries
  the Windows speaker processing values (high-pass, EQ, two-band compression, limiter), the WirePlumber rule
  that names the raw sink, and a user unit that keeps the raw sink at 100 % (WirePlumber starts it at 40 %,
  which caps the output at −23.9 dB whatever the volume control shows). Details:
  [userspace/platform/README.md](../userspace/platform/README.md).

### Microphones

Verdict: works, calls unverified. Tests: AUD-3 partial (fails the clipping criterion at the WirePlumber
default gain), AUD-4 unverified.

- **Measured.** Both channels carry signal, 0.6–0.8 dB apart, with the same polarity. `Mic Boost` has steps of
  0/10/20/30 dB. At WirePlumber's default of 100 % (`Capture` +30 dB plus `Mic Boost` +30 dB) a tone clips;
  at the lower boost steps it does not. Opening a capture while the codec is runtime-suspended gives a start
  transient: the first 1.0 s sits at full scale and DC remains for about 2.5 s. The echo-cancel source
  `sp9_echo_cancel` appears, and all of its nodes are suspended when it is not in use.
- **Unverified.** A speech recording from the front (AUD-3). A call through the echo-cancel source (AUD-4; its
  row in the results table was recorded on a configuration without the source).
- **Responsible.** Option `microphone.enable`: a WirePlumber rule that puts the first-use gain at +30 dB
  (`Capture` +30 dB, `Mic Boost` 0 dB); it applies only to accounts with no stored volume. Option
  `audio.echoCancel`: the PipeWire source "Microphone (echo cancelled)", chosen per application.

### Other outputs

Bluetooth audio (AUD-5) and audio over a USB-C monitor (AUD-6) are unverified. Measured only: BlueZ has the
A2DP and HFP endpoints registered, and four HDMI/DP PCMs exist with no monitor present.

Research: [research/gaps/audio.md](research/gaps/audio.md).

## Type Cover

Verdict: unverified; the backlight works. Test: SAM-1 unverified.

- **Measured.** Four SAM HID nodes (`01:15:02:0{1,2,3,5}:00`) sit under the KIP hub and are bound to
  `surface_hid`. The LED `surface::kbd_backlight` exists with `max_brightness` 3; writing 0/1/2/3 is read back
  from the LED.
- **Observed.** The backlight is off at 0 and on at 3.
- **Unverified.** Keys, touchpad, detaching and attaching (no test record). Whether the level follows the
  cover's backlight key. For patch 0016: the resulting Linux input device and key codes, and the Call Mute
  key. For patch 0017: the format of the GET_FEATURE response on Linux and the level update when the key
  notification arrives.
- **Not provided.** Continuing a two-finger scroll at the touchpad edge, as Windows does, is left to the
  compositor: only the compositor knows the scroll direction setting.
- **Responsible.** linux-surface's SAM and `hid-surface` drivers; patch 0016 (ignore the Fn usages, map Call
  Mute Toggle to `KEY_MICMUTE`) and 0017 (keyboard backlight LED). The helper sets the level
  (`sp9-ctl kbd`: works through the LED).
- **Research.** [research/control/typecover.md](research/control/typecover.md).

## Buttons, tablet mode and lid

Verdict: buttons and tablet mode unverified; the lid works. Tests: SAM-2 and SAM-3 unverified, PWR-2 partial.

- **Measured.** `pinctrl_tigerlake` is loaded in the initrd and the button devices exist: `gpio-keys` with
  volume down, volume up and power (power has wakeup enabled). The tablet mode switch reads 0 (`laptop`) with
  the cover attached, which matches. A compositor with automatic rotation reads the switch through libinput:
  with the cover open it holds the panel upright, and with the switch forced on through the compositor's test
  hook the panel follows an injected orientation.
- **Measured + observed.** Lid close → open on battery, four times: 4/4 entered and left s2idle, 0 suspend
  failures, `gpe52` counted about twice per close and open, 0 lines of "No handler or method for GPE" (without
  `pci=hpiosize`).
- **Unverified.** Pressing any button, waking by the power button, the tablet mode switch when the cover is
  folded or removed, the lid while charging.
- **Responsible.** `pinctrl_tigerlake` in the initramfs (the module always adds it; without it the buttons can
  stay dead when `soc_button_array` probes first, per the linux-surface wiki). linux-surface's GPE and button
  patches.

## Ambient light and orientation sensors

Verdict: rotation works; the light sensor is partly checked. Tests: SAM-7 pass, DSP-4 partial.

- **Observed.** With `sensors.enable` (the default) the screen follows when the device is turned, in a
  compositor that reads iio-sensor-proxy (SAM-7).
- **Measured.** Seven iio devices (gyroscope, accelerometer, inclinometer, ambient light, device rotation,
  gravity, magnetometer). The sensor hub sends a report only when a reading moved by more than its change
  sensitivity (0.05 g for the accelerometer), and the SAM HID transport cannot ask for an input report. On a
  device at rest the accelerometer's buffer therefore stays empty, and single sysfs `_raw` reads of
  acceleration, gravity and light take 5.3–5.6 s and return 0 (6/6); iio-sensor-proxy then reads (0, 0, 0).
  With the accelerometer's sensitivity written as 0, which the udev rule of `sensors.enable` does (0 after
  boot: measured), the hub reports at 8.5 Hz while the sensor is on, a single read returns a value in 0.09 s,
  and iio-sensor-proxy reports the orientation within 0.7 s. Polling the light sensor 35 times gave a value
  twice (about 81 lux); iio-sensor-proxy takes it through its buffer driver.
- **Unverified.** Holding the orientation by cover posture and the rotation lock of a desktop. The sensors
  after a suspend (the kernel is written to restore the sensitivity; from the code only). Covering the light
  sensor, automatic brightness.
- **Responsible.** linux-surface's SAM HID and the kernel's `hid-sensor-hub`; the rule
  [`61-sp9-sensors.rules`](../userspace/platform/udev/rules.d/61-sp9-sensors.rules) and iio-sensor-proxy
  (option `sensors.enable`).
- **Research.** [research/gaps/sam-platform.md](research/gaps/sam-platform.md), test 12.

## Battery and charging

Verdict: partial. Test: SAM-6 partial.

- **Measured.** `BAT1` reports status, capacity, energy and power; cycle count and health are readable. At
  100 % on a charger the status reads `Charging`, not `Full`. `ADP1` reports `online` and, with patch 0010,
  the adapter rating in `input_power_limit` (45 W with the charger used). There is no
  `charge_control_end_threshold`. Through the helper the smart charging state is readable (what the status
  value means is unverified) and the UEFI battery limit variable reads `off`.
- **Unverified.** Plugging and unplugging the charger, the status text near the 80 % limit, releasing smart
  charging through the helper.
- **Known issue.** Patch 0010: if the adapter rating request fails once while AC is connected,
  `input_power_limit` becomes 0 and a change event is sent.
- **Responsible.** linux-surface's SAM battery and charger drivers; patches 0009 and 0010 (from
  linux-surface/kernel#184). The helper's battery page (`sp9-ctl battery`). The 80 % limit itself is a UEFI
  setting; the helper only reads it.
- **Research.** [research/control/battery-charging.md](research/control/battery-charging.md).

## USB-C

Verdict: partial (port information only). Tests: DSP-5 unverified, AUD-6 unverified.

- **Measured.** Two `typec` ports and the partner appear. With patch 0019 the PDOs read correctly: each port's
  source is 5 V 3 A; its sink is 5 V 3 A fixed, 5–20 V 60 W battery and 5–20 V 3 A variable; the charger's
  source is 9/12/15 V 3 A. Without `pci=hpiosize=0`, the range `1800-18fe` is reserved by `PNP0C02:01` and the
  Thunderbolt root ports' I/O windows lie outside it; after boot and three s2idle cycles the GPE counters are 0.
- **Known defects.** The partner's PDO list starts at its second entry, so the 5 V entry is missing, and
  `voltage_now` reads 0.
- **Unverified.** An external monitor, hotplug, audio over the monitor, GPE behaviour when a USB-C or
  Thunderbolt device is connected.
- **Responsible.** Patches 0007 and 0008 (UCSI over SAM HID, from linux-surface/kernel#184) and 0019 (fix-up of
  the GET_PDOS responses).
- **Research.** [research/gaps/sam-platform.md](research/gaps/sam-platform.md),
  [research/gaps/display-power.md](research/gaps/display-power.md) (item 7 of the differences table, on the
  GPE storm).

## Fan and temperatures

Verdict: reading works, and no fan floor is left over from Windows. Tests: SAM-4 pass; fan values under load from SAM-5 and PWR-9.

- **Measured.** `fan1_input` reads 0 rpm at idle (12 samples) and 4745–5960 rpm under load. Eleven SAM
  temperature channels are readable. The only thermal zones are `x86_pkg_temp` and `iwlwifi_1`, with no valid
  trip point; no ACPI thermal zone is created. SAM fan command 0x0b sets the speed of a fan that SAM has
  stopped (requests of 1500–7500 gave 1588–7407 rpm) and is ignored while SAM drives the fan itself; command
  0x0c does nothing. After a load under Windows and a reboot into Linux (kernel with patch 0013) the fan read
  2996–3001 rpm for the first two minutes while the device was still warm (highest SAM sensor 42.4 → 39.1 °C)
  and 0 rpm from 38.8 °C on, for the rest of the six minutes: the minimum fan speed Windows can leave behind
  does not stay (SAM-4).
- **Unverified.** The same round trip on a kernel without patch 0013, so that the patch is what clears the
  floor is not shown by comparison. The request format rests on other users' observations.
- **Responsible.** linux-surface's `surface_fan` and `surface_thermal`; patch 0013. The helper reads the fan
  and can send the experimental fan request when explicitly allowed (`sp9-ctl fan`).
- **Research.** [research/control/power-thermal.md](research/control/power-thermal.md) section 1.4.

## Performance profiles and heat

Verdict: works, with a caveat. Tests: SAM-5 partial, PWR-9 partial.

- **Measured, kernel alone.** `platform_profile` offers `low-power`, `balanced`, `balanced-performance` and
  `performance`. Under the same load (`stress-ng --cpu 12`, 120 s, AC) only `low-power` differs in power:

  | Profile | Average W | Package max °C | Fan max rpm | bogo ops/s |
  |---|---|---|---|---|
  | `low-power` | 6.0 | 42 | 0 | 5794 |
  | `balanced` | 33.2 | 91 | 4745 | 16199 |
  | `balanced-performance` | 33.1 | 92 | 4918 | 16381 |
  | `performance` | 33.1 | 94 | 5553 | 16314 |

  A ten-minute load in `performance` holds 29.9 W with no throttling oscillation; the package reaches 94 °C
  and the skin sensor (`temp9`) keeps rising to 51 °C. Windows lowers PL1 to 21–25.5 W when that sensor is at
  47–48 °C, so under the same load the surface is hotter on Linux.
- **Measured, with the helper.** Each profile gets its own PL1 / PL2 and EPP, taken from the Windows policy
  table (on AC): `performance` 35/60 W and 64, `balanced-performance` 28/45 W and 115, `balanced` 28/35 W and 153,
  `low-power` 10/11 W and 184. The written values are read back from
  RAPL and cpufreq. The device has no `intel-rapl-mmio`, so only the MSR limits are written.
- **Unverified.** Whether the written limits take effect under load (no load comparison with the helper
  running). Fan noise per profile. The feel of the back under load. The helper's skin temperature guard, which
  lowers the limits by that sensor: experimental, off by default, unverified.
- **Responsible.** linux-surface's `surface_platform_profile`; the helper (`sp9-ctl profile`, `limits`,
  `skin-guard`). Measurement tool: [`tools/linux/perf-profiles.sh`](../tools/linux/perf-profiles.sh).
- **Research.** [research/control/power-thermal.md](research/control/power-thermal.md).

## Suspend

Verdict: s2idle works; standby power is unverified; hibernation is not available. Tests: PWR-1 partial, PWR-3
pass, PWR-6 pass, PWR-4, PWR-5, PWR-7, PWR-8 unverified.

- **Measured.** `mem_sleep` offers only `s2idle`. Ten of ten cycles woken by RTC entered and resumed, with 0
  suspend failures; of 30.8–31.2 s per cycle, 27.86–28.35 s were spent in `slp_s0`, with no unmet LPI
  constraint. Residency accrues only in S0i2.0; S0i3.0 stays 0. Each cycle logs one
  `intel_ipu6.psys.40: Failed to get runtime PM` warning. The RTC (`rtc_cmos`) woke the device 17 of 17 times
  (20.7 s for a 20 s request). After audio playback three consecutive cycles worked and playback continued
  afterwards. With neither kernel parameter, 3/3. Wi-Fi reconnects by itself after resume.
- **Unverified.** Entering and leaving suspend with the power button (PWR-1). Drain over one hour on battery
  (PWR-4). Deep package C-state residency with the screen off (PWR-5; with the screen on, pc10 is 0.00).
  Shutdown (PWR-7).
- **Not available.** Hibernation (PWR-8): the tested setup has only zram swap, so there is no disk swap to hold
  the image, and it cannot be tested.
- **Responsible.** linux-surface's patches; nothing in this repository is specific to suspend except the lid
  and GPE notes above. Measurement tool: [`tools/linux/s2idle-trace.sh`](../tools/linux/s2idle-trace.sh).
- **Research.** [research/control/standby.md](research/control/standby.md),
  [research/gaps/display-power.md](research/gaps/display-power.md).

## Wi-Fi

Verdict: works. Test: NET-1 partial.

- **Measured.** Intel AX211 (`8086:51f0`), `iwlwifi`. Connected on 5 GHz with an 80 MHz channel at
  866.7 Mbit/s in both directions; in 854 s there was no beacon loss and no failed transmission. Power save is
  on. After s2idle it reconnects by itself: a ping to the gateway passes 3.3 s after resume (2/2).
- **Unverified.** Throughput against Windows from the same spot.

## Bluetooth

Verdict: partial. Tests: NET-2 unverified, AUD-5 unverified.

- **Measured.** AX211 Bluetooth (USB `8087:0033`), `btusb`/`btintel`; the firmware loads with no error lines.
  The pen pairs and connects, and reconnects when it was bonded through the cover (see Pen).
- **Unverified.** Mouse, earphones, audio, reconnection after resume.

## Device helper

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
| Pen haptics and speed feedback | Works: vibration while drawing, stop on lift (observed); strength (30 % not felt, 200 % clearly stronger than 100 %) and one click at eraser touch-down (observed); the digitizer state is restored after a suspend (measured). Unverified: the frames changed to match Windows together with the scale of 300 mm/s, the choice of waveform, the dead zone, `pen test`, the state after an iptsd restart |
| Pen cover coupling | Works: `pen couple` on a pen without a bond at the first attempt 12/12, and 6/6 in the state that failed before address resolution was switched off for the connection; a bonded pen that is not connected is probed and reconnected with its bond kept (measured). Unverified: the automatic coupling with the new order, replacing a dead bond after Windows coupled the pen, `--force` on a pen without a bond, `unsupported-kernel` |
| Skin temperature guard | Experimental, off by default, unverified |
| Diagnostics report | Works |
| Settings app | The window opens (observed); individual controls unverified |

## Other

- **RTC.** With patch 0012 the SAM real-time clock appears as `rtc1` (`surface_rtc`) and agrees with system
  UTC within one second. It has no wake alarm; the wake source in the suspend tests is `rtc0` (`rtc_cmos`).
- **Firmware updates.** Only through Windows.
