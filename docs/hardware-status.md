# Hardware status

What each part of the Surface Pro 9 (Intel, SKU `Surface_Pro_9_2038`) does on Linux, in detail. The short table
is in the [README](../README.md).

Conditions: NixOS, kernel 7.3.4 = stable v7.3.4 + linux-surface 7.3 + [`kernel/patches`](../kernel/README.md)
0001–0022, firmware UEFI 24.103.143 and SAM 6.603.139, Type Cover attached, Slim Pen 2.

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

Verdict: unverified. Tests: TOUCH-1, TOUCH-2, TOUCH-4 unverified, TOUCH-3 partial, PEN-8 partial.

- **Measured.** iptsd reads `/etc/iptsd.conf` and then the preset `/etc/iptsd.d/surface-pro-9.conf`, and runs
  in touchscreen mode. With the preset's thresholds 24/20/5 there were 0 ghost touches in 150 s without
  contact. After 17 s2idle cycles the kernel log has 0 lines of ithc "DMA RX timeout" or "failed to reset".
  With patch 0018 every Feature response belongs to the requested report, and the kernel log has 0 lines of
  `hid_input_report failed` or `too short`. Kernel-side input devices
  (`Intel Touch Host Controller Touchscreen` and `… Stylus`) exist next to the iptsd virtual devices; libinput
  takes them as a touch device and a tablet, and the compositor opens both. In the same boot the iptsd devices
  held non-zero values and the kernel-side devices held only zeros.
- **Inferred.** Wake by touch will not work: `power/wakeup` of the controller is disabled, and ithc takes the
  device down on suspend and has no wake set-up (TOUCH-4). Not tried.
- **Unverified.** Missed taps and swipes against the iptsd defaults 40/36/15 (TOUCH-1). Palm rejection with
  `DisableOnStylus = true` (TOUCH-2). Whether touch and pen work after every resume (TOUCH-3). Whether the
  kernel-side duplicate devices deliver events during contact (PEN-8). No person has recorded a touch test.
- **Responsible.** linux-surface's ithc driver and iptsd. The preset
  [`userspace/platform/iptsd/surface-pro-9.conf`](../userspace/platform/iptsd/surface-pro-9.conf) (option
  `touch.enable`). Patch 0018 (ithc: match Feature responses to the requested report ID). The helper can
  override the thresholds (`sp9-ctl iptsd`: works) and inhibit the touchscreen while the panel is off
  (`sp9-ctl touch-off`: the setting toggles; the effect with the panel off is unverified).
- **Research.** [research/gaps/pen-touch.md](research/gaps/pen-touch.md),
  [research/control/input.md](research/control/input.md).

## Pen (Slim Pen 2)

Verdict: partial. Tests: PEN-5 partial, PEN-6 pass, PEN-8 partial, PEN-9 partial, PEN-1–4 and PEN-7 unverified.

- **Observed.** Strokes draw (the iptsd stylus device reports `BTN_TOUCH`). Haptics: a click (waveform
  ordinal 3) sent with output report 65 is felt. The continuous ink waveform (ordinal 9) sent the same way is
  not felt, neither with the bytes Windows was captured sending (`41 00 7f 09 00 00 00`) nor at intensity 255.
  Written as the Auto Trigger of Feature 65 it gives a weak, intermittent vibration during strokes, unlike the
  continuous vibration on Windows. No difference was felt between waveforms 9–13 or between intensities 100
  and 255.
- **Measured.** Tail button: single click Meta+F20, double click Meta+F19, long press Meta+F18. Pairing by
  hand works (2/2): a long press of the tail button puts the pen in pairing mode and it bonds with a
  `NoInputNoOutput` agent; without an agent pairing fails with `AuthenticationFailed`. BlueZ and UPower then
  show the pen's battery level. The pen disconnects by itself after about 80 s without use (reason 0x13, as on
  Windows). The stylus device declares `ABS_PRESSURE` 0..4096, `ABS_TILT_X/Y` −9000..9000, `BTN_STYLUS` and
  `BTN_TOOL_RUBBER`. The touch controller's descriptor has the FFF4:0001 collection with input reports 84 and
  110, 12 B each.
- **Does not work.**
  - Automatic bonding. Setting Features 86 and 112 on the digitizer leaves their values unchanged, input
    reports 84 and 110 do not arrive, and the pen does not appear over Bluetooth. The format written was built
    without knowing the bytes Windows writes.
  - Reconnection. After its idle disconnect the pen does not advertise again when it is picked up, drawn with,
    or its tail button is pressed; a two-minute unfiltered scan (30 000 advertisements) does not contain the
    form seen on Windows, and `Device1.Connect()` times out. To use the Bluetooth features again the pen has
    to be put in pairing mode again. On every reconnection Windows exchanges 16 B writes and notifications
    seven times through a vendor GATT service; that the pen advertises again only to a host that did this
    exchange is inferred, not confirmed.
  - Continuous haptics (above).
- **Unverified.** Reaching full pressure and the pressure at the start of a stroke (PEN-1), tilt against real
  angles (PEN-2), the side button (PEN-3), the eraser (PEN-4), whether reports 84 and 110 are received
  (PEN-7), haptics through the helper.
- **Responsible.** iptsd for drawing. Drawing does not need Bluetooth; the tail button, the battery level and
  haptics do. The prototypes in [userspace/pen](../userspace/pen/README.md) (`pen-haptic`,
  `surface-pen-connect`; option `pen.enable`, off by default). The helper's pen page (`sp9-ctl pen`, off by
  default, unverified on the device).
- **Research.** [research/gaps/pen-touch.md](research/gaps/pen-touch.md),
  [research/gaps/pen-ble-trace.md](research/gaps/pen-ble-trace.md).

## Cameras

Stack used in the tests: libcamera 0.7.2, simple pipeline with the software ISP, WirePlumber running.

### Front (OV5693)

Verdict: works. Tests: CAM-2 pass, CAM-3 fail, CAM-5 and CAM-6 unverified.

- **Measured.** Frames arrive for 640x480, 1280x720 and 1920x1080 requests: 1280x720 120/120 frames at
  28.6 fps, 640x480 30/30, 1920x1080 90/90 at 28.65 fps, no dropped sequence numbers, 0 lines of
  `Frame sync error`. The image is upright and in focus. Whatever is requested, the sensor runs its 2592x1944
  mode.
- **Does not work.** The binned 1296x972 mode. With either flip control on, every buffer fails with
  `csi2-2 error: Frame sync error`; with both flips off 20/20 frames arrive at 57 fps. libcamera turns both
  flips on because the sensor is mounted rotated by 180°. The cause is unverified, so the mode is not fixed:
  patch 0020 stops the driver from offering it, and requests of 720p and below go to the full mode. What is
  lost is the 57 fps binned mode without flips.
- **Unverified.** Whether the image is mirrored. Behaviour on a kernel without the patches. Which LED lights
  (the privacy LED GPIO goes high while streaming and low afterwards: measured; CAM-5). Image quality against
  Windows (CAM-6): frames from the uncalibrated software ISP look desaturated with a grey-green cast and
  clipped highlights, and auto exposure takes about 90 frames to settle.
- **Responsible.** Patches 0001–0006 (upstream v7.4 backports: non-continuous MIPI clock for the OV5693 on
  IPU6) and 0020.

### Rear (OV13858)

Verdict: works, focus unverified. Tests: CAM-1 partial, CAM-4 partial.

- **Measured.** 1920x1080 30/30 frames at 29.95 fps, 1280x720 120/120, 3840x2160 10/10, no buffer errors. The
  lens device (`dw9800k`) is created and `focus_absolute` (0–1023) exists; a written value is read back from
  the VCM registers (900 reads as `03 84`). The image changes only between about 400 and 800. When the camera
  is not in use the sensor and the VCM are runtime-suspended (patch 0021). On the first boot of the kernel with
  patch 0022 the lens device was created (1/1 boots).
- **Unverified.** Image orientation (libcamera offers only rot180 for this camera). Whether near and far
  subjects come into focus: the test scenes were too dark or had no edges to tell. Whether patch 0022 removes
  the boots without a lens device (seen in one of two boots before it) needs many boots. Image quality when
  focus is never written.
- **Does not work.** Autofocus: libcamera has no focus control for this camera, so focus stays where it is
  unless something writes `focus_absolute`.
- **Responsible.** linux-surface's camera patches for the sensor; patch 0011 (maps the firmware's VCM type
  0x11 to `dw9800k`; the mapping is confirmed from Windows, compatibility with the dw9719 driver's DW9800K
  settings is inferred), 0021 (the lens powers up on the first focus write, not on open), 0022 (ov13858 enables
  runtime PM before registering). Option `camera.pipewire` turns WirePlumber's libcamera monitor off for
  standby measurements.

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
  the cover attached, which matches.
- **Measured + observed.** Lid close → open on battery, four times: 4/4 entered and left s2idle, 0 suspend
  failures, `gpe52` counted about twice per close and open, 0 lines of "No handler or method for GPE" (without
  `pci=hpiosize`).
- **Unverified.** Pressing any button, waking by the power button, the tablet mode switch when the cover is
  folded or removed, the lid while charging.
- **Responsible.** `pinctrl_tigerlake` in the initramfs (the module always adds it; without it the buttons can
  stay dead when `soc_button_array` probes first, per the linux-surface wiki). linux-surface's GPE and button
  patches.

## Ambient light and orientation sensors

Verdict: partial. Tests: DSP-4 partial, SAM-7 partial.

- **Measured.** Seven iio devices (gyroscope, accelerometer, inclinometer, ambient light, device rotation,
  gravity, magnetometer). In buffer mode samples arrive: one acceleration sample in 8 s, magnitude 9.70 m/s².
  Single sysfs `_raw` reads of acceleration, gravity and light take 5.3–5.6 s and return 0 (6/6); polling the
  light sensor 35 times gave a value twice (about 81 lux).
- **Unverified.** Rotating the device, covering the light sensor, automatic rotation in a desktop
  (`iio-sensor-proxy` was not installed on the test system).
- **Research.** [research/gaps/sam-platform.md](research/gaps/sam-platform.md).

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

Verdict: reading works. Tests: SAM-4 unverified; fan values under load from SAM-5 and PWR-9.

- **Measured.** `fan1_input` reads 0 rpm at idle (12 samples) and 4745–5960 rpm under load. Eleven SAM
  temperature channels are readable. The only thermal zones are `x86_pkg_temp` and `iwlwifi_1`, with no valid
  trip point; no ACPI thermal zone is created. SAM fan command 0x0b sets the speed of a fan that SAM has
  stopped (requests of 1500–7500 gave 1588–7407 rpm) and is ignored while SAM drives the fan itself; command
  0x0c does nothing.
- **Unverified.** Whether patch 0013 clears the minimum fan speed that Windows can leave behind across a
  reboot: no Windows round trip was made. The request format rests on other users' observations.
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
  The pen pairs and connects (see Pen).
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
| iptsd thresholds | Works |
| Touch off while the panel is off | The setting toggles; effect unverified |
| Pen haptics | Unverified (the pen was not connected) |
| Skin temperature guard | Experimental, off by default, unverified |
| Diagnostics report | Works |
| Settings app | The window opens (observed); individual controls unverified |

## Other

- **RTC.** With patch 0012 the SAM real-time clock appears as `rtc1` (`surface_rtc`) and agrees with system
  UTC within one second. It has no wake alarm; the wake source in the suspend tests is `rtc0` (`rtc_cmos`).
- **Firmware updates.** Only through Windows.
