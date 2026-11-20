# Digitizer daemon (sp9-digitizerd)

`sp9-digitizerd` is a touch and pen daemon in Rust for the digitizer of the Surface Pro 9 (Intel), HID device `045E:0C52`.
The digitizer does no touch processing of its own in the mode Linux uses: it sends raw data (a capacitive heatmap for touch, DFT windows of
the pen's tones), and a user-space daemon has to turn that into input events. The daemon reads the raw reports from hidraw and feeds two
uinput devices, a touchscreen and a stylus.

That job is normally done by [iptsd](https://github.com/linux-surface/iptsd), and this repository also carries iptsd 3.1.0 with the
patches 0001–0017 ([userspace/iptsd](../userspace/iptsd/README.md)). `sp9-digitizerd` is a port of exactly that code, rewritten for this
one device and its configuration. It exists next to iptsd for three reasons: it keeps its input devices across suspend (iptsd's are
created anew at every resume, section 12), it takes setting changes over D-Bus without a restart (section 6), and it is where the defects
found in the reference are fixed (sections 7 and 11). iptsd stays the default; the daemon is selected with the NixOS option
`hardware.surface-pro-9.touch.daemon = "digitizerd"`.

The code is in [`helper/crates/sp9-digitizer`](../helper/crates/sp9-digitizer/) (the processing library, no I/O) and
[`helper/crates/sp9-digitizerd`](../helper/crates/sp9-digitizerd/) (the daemon and a replay tool). Both crates are GPL-3.0-or-later; the
files that derive from iptsd (GPL-2.0-or-later) name the iptsd source files they come from in their headers.

## 1. Status

| Item | Status |
|---|---|
| Processing (pen samples, touch contacts, uinput packets) | The port reproduced the C++ reference bit for bit in a replay of captures: 10 captures × 4 configurations, 366 549 dump lines identical (section 5). The defects of section 7 were fixed afterwards, so the output now differs on purpose (section 11); the reference dumps are now the ones this code produced |
| On the device (NixOS, `touch.daemon = "digitizerd"`, no iptsd) | Installed and in use. The unit starts at boot and the daemon finds the digitizer by its ID (the node number differed between two boots: measured). Touch, pen writing, the eraser end, the barrel button and the pen haptics of the helper work (observed). A pen setting changed with `sp9-ctl pen zero-force` changes the value in the daemon while the process stays the same (measured). The live evdev stream equals the replay of the raw reports received at the same time (measured, section 11) |
| Not verified on the device | Behaviour after a resume from suspend (finding the node again, section 12), re-entering raw mode, the sliders of the settings app with this daemon, position prediction (section 13: replay only) |
| Known issue | After rubbing with the eraser end and taking the pen away, the drawing application (Rnote) keeps showing the eraser's pointer; it does nothing. The daemon does send `BTN_TOOL_RUBBER 0` (4 of 4 captures), and the same happens with iptsd (patches 0001–0017) in the same configuration (observed), so it does not come from this daemon |
| NixOS | Package `packages.x86_64-linux.sp9-digitizerd`; option `hardware.surface-pro-9.touch.daemon = "digitizerd"` (the default is `"iptsd"`); flake checks `sp9-digitizerd` (build and tests) and `module-eval-digitizerd` |

The rows of the test log are TOUCH-1 and PEN-4 in [test-results.md](test-results.md). The C++ side is frozen at patch 0017: later
fixes are made here only.

## 2. Structure

```
hidraw report ─ frame::Parser ─┬─ Heatmap ─ touch::TouchPipeline ─ [Contact] ─ output::TouchOutput ─┐
                               └─ DftWindow ─ pen::Pen ─ PenSample ─ output::StylusOutput ───────────┴─ Sink (uinput / text)
```

`Digitizer::process(report, now, sink)` ties all of it together. The rule that switches touch off while the pen is in range
(`disable_on_stylus`) and for a short time after it left (`pen_holdoff_s`) is there too.

| Module | What it does |
|---|---|
| `frame` | Report parser: HID frame → heatmap / report frames (heatmap dimensions, DFT metadata, DFT windows). A structural error drops the rest of that report; the events emitted before it stay valid |
| `hid` | Finds the raw data reports and the feature reports for the mode switch and the metadata in the report descriptor |
| `touch::detect`, `touch::fit` | Heatmap → neutral level removed → 3×3 blur → local maxima → clusters → Gaussian fit → position, size, orientation |
| `touch::track` | Keeps contact numbers between frames, drops small changes, marks large ones as unstable, decides palm or finger |
| `pen` | The table below. One module per stage, each with its own state and its own settings |
| `output` | The shape of the two virtual devices (names, axes and properties are those of iptsd's devices: the libinput quirks and the helper find them by name) and the evdev events |
| `config` | The configuration types (serde) and the reader for iptsd configuration files |
| `control` | The settings that can change while running (`Param`, `set_params`), the pen state (`PenState`), the D-Bus names |
| `num` | `min`, `max` and `clamp` with the NaN behaviour of the C++ code |

Pen stages. Only `Pen::input` knows the order of the stages and the values that pass between them, and only `Pen::lift` knows what is
cleared when the pen leaves.

| Stage | Module | Input windows | State | Produces |
|---|---|---|---|---|
| Signal measurement | `pen::signal` | all | none | Parabola and centroid position, ring position, peak, sum of three antennas, tone bits |
| Position | `pen::position` | 0x06 (coarse), 0x07 and 0x08 (precise) | none | Position in antennas, mapping to screen coordinates |
| Tilt | `pen::tilt` | second row (ring) of 0x07 and 0x08 | two smoothed values, offset | Altitude and azimuth, tip offset |
| Contact | `pen::contact` | force tone of 0x06, 0x07, 0x08; tip signal | debounce, reference, zero-force contact, stroke level, lockout | Contact or not |
| Pressure | `pen::pressure` | 0x0B | measurement history, ramp, stroke-start history | Pressure of each sample |
| Lock | `pen::lock` | — | anchor point | Coordinates of a resting pen |
| Button | `pen::button` | 0x0A (bit), 0x09 (phase) | binary window decision, candidate | Barrel button, eraser |
| Prediction | `pen::predict` | — | the last two positions | Position moved ahead (off by default, section 13) |

In one frame (15 ms) the 0x07 and the 0x08 window produce one sample each (133 per second). The 0x06 window decides proximity and
reports by itself only when the frame before had no precise sample.

Floating point: everything is `f64` and the order of the operations is that of the C++ expressions. `min`, `max` and `clamp` come from
the `num` module because `f64::min` drops a NaN where the C++ code does not. Eigen is not used: the 2×2 inverse and eigenvalues are in
closed form and the 6×6 elimination works on arrays. The math functions are those of libm, which std calls.

## 3. Configuration

The built-in defaults are this device: iptsd's defaults plus the platform preset
([surface-pro-9.conf](../userspace/platform/iptsd/surface-pro-9.conf)). A unit test (`defaults_are_the_platform_preset`) compares every key
of the preset file with the built-in value.

Order of reading:

1. The built-in defaults.
2. TOML: `--config FILE`, otherwise `/etc/sp9/digitizer.toml` if it exists. The structure is that of the `Config` type, and an unknown
   key is an error.

   ```toml
   [touch]
   disable_on_stylus = true
   [touch.contacts]
   activation_threshold = 24
   deactivation_threshold = 20
   [pen.contact.zero_force]
   enabled = true
   enter = 0.85
   exit = 0.75
   reference = 0        # 0 = learned from the windows with force
   [pen.pressure]
   floor = 0.03
   ```

   The other sections: `pen.position` (antenna pitch and origin, correction coefficients of the centroid, coefficients of the edge
   estimate `edge_x` and `edge_y`, `edge_reach`), `pen.tilt`, `pen.lock`, `pen.button`, `pen.contact`.
   Values iptsd does not have: `touch.pen_holdoff_s` (how long touch stays off after the pen left, 0.2), `pen.position.edge_reach`
   (spreads the coordinates over the strip outside the edge antennas, on), `pen.prediction_ms` (position prediction, 0 = off,
   section 13).
3. iptsd configuration files (INI): `--iptsd-conf FILE` (may be repeated), otherwise `/etc/iptsd.d/zz-sp9-helper.conf` if it exists
   (the drop-in the helper writes). A file whose `[Device]` names another device is skipped. `[Stylus] PredictionMs` is a key iptsd does
   not have: the helper keeps the prediction horizon in the same drop-in, and iptsd does not read keys it does not know. Only keys with a
   meaning here are applied, and keys whose behaviour is fixed here are accepted only with the fixed value (`InkFilter = true` or
   `ExtraPositionWindows = false` is an error). A value that does not parse as a number is an error (iptsd silently uses its default).

`sp9-digitizer-replay --config FILE` and `Diagnostics.GetConfig` (section 6) show the configuration in force.

The helper keeps writing the drop-in (read at the next start), and while the daemon is on the bus it pushes the same values over D-Bus
(section 6). The daemon is not restarted.

## 4. Differences from iptsd

Not ported (paths the configuration of this device does not use):

| Left out | Why |
|---|---|
| Touchpads (button reports, touchpad device), legacy frames, MPP 1.0 and 1.51 stylus reports | This device does not send them |
| `InkFilter` (Kalman filter), phase coherence, the fade of the filter delay | The preset switches them off |
| Tilt from the 0x06 window (`TiltDistance`), the `Stylus.TipDistance` offset | The tilt of the MPP v2 windows replaces them |
| Contact from the pressure window (the path before patch 0012), pressure read from the pressure tone alone (pen tip) | `ForceToneContact` and `Mpp2PressureDecode` are on |
| The switches `ExtraPositionWindows`, `TiltSmoothing`, `TiltWhileHovering`, `PositionCentroidX/Y`, `PressureRamp`, `ZeroForcePressure`, `ZeroForceStrokeSum`, `AllowSplitEvents` | Fixed at the value of the preset |
| `HoverJitterRadius`, the tail asymmetry term (`CellCorrection*Asymmetry*`), the mapping without an antenna pitch | The preset does not use them |
| Neutral modes `average` and `constant`, peak suppression, the 5×5 convolution, inverting X or Y, `[Config] Width/Height` | Left at their defaults; the dimensions come from the metadata. If the metadata reports mirrored axes the daemon does not start |
| The tools (calibrate, dump, perf, plot, show) | The replay tool `sp9-digitizer-replay` writes dumps |

Where the behaviour differs without changing the result of a replay:

- A DFT window that claims more rows than there are bytes left ends the C++ daemon (a `gsl::span` contract violation). Here it is an
  error that drops that report.
- A packet (up to `SYN_REPORT`) is written to uinput with one `write`. The C++ code writes each event.
- When the device goes away (a read returns 0 bytes, `ENODEV`, `EIO` or `ENXIO`) the daemon lifts everything and looks for the node
  again (section 12); with a node given on the command line it waits for that path. The C++ daemon ends after 50 errors. Here 50 failed
  reports in a row close the node and open it again.
- An error in the configuration prevents the start.

What was first ported unchanged although it is a defect: section 7.

## 5. Verification

The reference is a replay harness built from the iptsd sources with patches 0001–0017: it uses iptsd's `Application`, `Daemon`,
`TouchDevice` and `StylusDevice` and replaces only `UinputDevice` with a writer of text. Reports are filtered by the device's real
report descriptor and cut to the daemon's read buffer. One line of a dump is a pen sample (P), the contacts of a frame (C), a uinput
packet (E) or a parse error (X); floating-point values are written in hexadecimal (`%a`), so a difference of one bit is a difference.
On the Rust side `sp9-digitizer-replay` writes the same format.

Result with the configuration of the device (preset plus the helper's drop-in), at the state of the port before any fix. All four
configurations: 0 differences.

The captures are named here by what they contain; the names are used in the rest of this document.

| Capture | Content | Pen samples | Touch frames | uinput packets | Differences |
|---|---|---|---|---|---|
| pen 1 | hovering, a resting and a slowly moved pen, small light writing, normal writing, lifting | 6895 | 2292 | 6900 | 0 |
| pen 2 | pen only | 1999 | 0 | 2000 | 0 |
| pen 3 | normal, tilted and light writing | 3217 | 729 | 3224 | 0 |
| slow 1 | slow horizontal, vertical, diagonal and free strokes | 4296 | 656 | 4315 | 0 |
| slow 2 | scribbles, slow diagonals, a tilted pen, very slow strokes | 5390 | 209 | 5639 | 0 |
| slow 3 | slow diagonals, a tilted pen, very slow strokes, writing | 5595 | 732 | 5663 | 0 |
| light 1 | light-pressure ticks, dots, hatching and a word | 4119 | 1542 | 4303 | 0 |
| light 2 | light-pressure cursive writing | 1766 | 870 | 2013 | 0 |
| reference pen | a scripted session in parts: barrel button, eraser, screen edges, a resting palm; with the evdev stream of the iptsd that ran | 4564 | 555 | 4696 | 0 |
| reference touch | a scripted session in parts: up to five fingers, light taps, screen edges; with the evdev stream of the iptsd that ran | 0 | 2885 | 2885 | 0 |

Configurations: `effective` (the device), `zf-off` (zero-force contact off, floor 0), `zf-learn` (learned reference, 0.6/0.4,
floor 0.05), `touch-alt` (thresholds 22/18, `DisableOnStylus` off, `DisableOnPalm` on).
The captures contain 403 samples with the barrel button, 580 eraser samples, up to 5 contacts, palm decisions and unstable frames.
No capture has a parse error.

**Difference from the packaged iptsd.** iptsd is built with `-O3 -march=x86-64-v3`, which contracts multiply-add into FMA instructions.
The reference above is built with `-O2 -ffp-contract=off`. Against the same harness built with the daemon's flags, the floating-point
values differ in the last bit on most lines, but **the uinput packets, which carry integers, are the same in all 40 replays** (no
difference in value or structure).

The captures, the harness and the reference dumps are not part of the repository. The comparison is the test
`helper/crates/sp9-digitizerd/tests/golden.rs`, which runs when two directories are named:

```sh
cd helper
SP9_CAPTURE_DIR=<dir with CAPTURE.bin> SP9_GOLDEN_DIR=<dir with rdesc.bin, conf/NAME.conf, NAME/CAPTURE.txt> \
  cargo test --release -p sp9-digitizerd --test golden
```

`SP9_GOLDEN_DIR` holds the report descriptor (`rdesc.bin`), the iptsd configuration files the reference was run with
(`conf/<name>.conf`) and one dump per configuration and capture (`<name>/<capture>.txt`). Without the two variables `cargo test` skips
the comparison and runs only the synthetic tests that are in the repository (parser, each stage, device packets, D-Bus service, node
search). For an intended change of behaviour, `SP9_GOLDEN_UPDATE=1` writes the new dumps as `<capture>.rs.txt` next to the reference
ones, for a diff to review.

A dump of one's own capture is made with `sp9-digitizer-replay [--config FILE] [--iptsd-conf FILE]... [--rdesc FILE] CAPTURE`.

## 6. D-Bus service

On the system bus as `io.github.joonhoekim.Sp9Digitizer1`, object `/io/github/joonhoekim/Sp9Digitizer1` (`service.rs`; the names and
the list of settings are in `sp9_digitizer::control`, and the helper uses the same).
The processing loop never waits for the bus: a changed setting is put into shared state and the loop takes it before its next report
(`Digitizer::set_config`; strokes, contacts and the input devices stay as they are). The daemon also runs without a bus and tries to
connect again every 5 seconds.
The daemon stores nothing. The settings belong to the helper (`helper.toml`); the daemon reads the drop-in when it starts, and after
that the helper pushes changes over D-Bus.

| Interface | Kind | Members |
|---|---|---|
| `.Pen` | properties (read) | Settings: `ZeroForce b`, `ZeroForceEnter d`, `ZeroForceExit d`, `ZeroForceReference d` (0 = learned), `ZeroForceStrokeLevel d`, `PressureFloor d`, `PredictionMs d` (0 = off, at most 30) |
| | properties (read) | State: `InRange b`, `Contact b`, `Tool s` (`pen`, `eraser` or empty), `Button b`, `TipLevel d` (tip energy / reference), `LearnedReference d` (the reference in force: the fixed value or what was learned) |
| | method | `SetParams(a{sv})`: by the names of the settings above; all are applied or all are refused (`InvalidArgs`) |
| | signal | `StateChanged(b in_range, b contact, s tool, b button)`: only when one of the four changes. Nothing is emitted per sample |
| `.Touch` | properties (read), `SetParams(a{sv})` | `ActivationThreshold d`, `DeactivationThreshold d`, `DisableOnStylus b`, `PenHoldoff d` |
| `.Diagnostics` | properties (read) | `Version s`, `Device s` (the hidraw node in use), `Geometry (yydd)` (antennas on X and Y, width and height in cm), `Reports t`, `ParseErrors t`, `PenSamples t`, `TouchFrames t`, `WriteErrors t` |
| | method | `GetConfig() → s`: the whole configuration in force (TOML) |

Permissions: the bus policy ([`io.github.joonhoekim.Sp9Digitizer1.conf`](../helper/data/io.github.joonhoekim.Sp9Digitizer1.conf)) lets
only root own the name and call `SetParams`. Reading (`Properties.Get/GetAll`, `GetConfig`, introspection) is open to everyone. The
methods check again that the caller is root.
A user's change goes through `sp9-helperd` as before, which asks polkit (action `touch`) and passes it on as root: the daemon knows
nothing about polkit.

Changing `ZeroForceReference` from a fixed value to 0 discards what was learned and starts learning anew. What defines the shape of the
devices (`touch.disable`, `pen.disable`, `axis_resolution`) cannot be changed this way.

## 7. Defects found in the C++ code and candidates for improvement

The port first carried all of them over unchanged, because the replay had to give the same result. They are ordered by effect, largest
first. File and function names are those of the iptsd sources after patch 0017 (`dft.hpp` is `src/core/generic/dft.hpp`).
Of these, #1, #2, #3, #4, #5, #6, #8, #9 and #15 are fixed in the Rust code (section 11; the cause of #2 is not the one in the table).
They remain in the C++ code.

| # | Defect | Evidence (captures) | Direction of a fix | How to measure |
|---|---|---|---|---|
| 1 | **The pen disappears at the screen edge.** When the strongest antenna of the 0x06 window is the first or the last one, the centre of the parabola is moved to the neighbour, whose amplitude is below `PositionMinAmp` (50): no position, so proximity ends (`interpolate_position` and `handle_position` in `dft.hpp`) | Reference pen: of 2367 0x06 windows with a magnitude above the threshold (2000), 241 have no position for this reason. 7 gaps of 0.17–0.8 s in the edge part, with raw magnitudes of 10 000–430 000 meanwhile | Decide proximity from the magnitude alone, take the position from the 0x07 and 0x08 windows, which are 15 times stronger (at the edge: an estimate from two antennas) | In the reference pen capture, the number of windows "magnitude above the threshold but not in range" → 0; the number of gaps in the edge part |
| 2 | **The eraser is always in contact while in range, with a constant pressure.** Eraser contact = `rubber && pressure > 0` (`update_contact`), pressure = interpolated tone position (`measure_pressure`), but the eraser's pressure tone is always in the same row. The eraser has no tone in 0x07 and 0x08, so its position comes only from the 0x06 window (67 per second, no correction, no tilt) | Reference pen: the eraser part of 8.6 s is one contact with pressure 2675/4096 ± 25 while it moves over the whole screen. Force tone: absent in all 592 windows of 0x07. Strongest row of the pressure tone: the same row in all 577 windows that have a tone | Find in captures where the eraser end encodes being pressed (section 11) | Contact samples while the eraser hovers → 0 |
| 3 | **Pressure is read as "no force" when its 6 digital bits are all 0** (`measure_pressure`). They are also 0 when the 10-bit value is a multiple of 64, so a single frame in the middle of a stroke becomes "no force". In that frame the pressure starts to ramp down, and a false entry goes into the stroke-start history, which raises the pressure of zero-force contact | A single 0 window between windows with force: 100 times in 9 captures (slow 3: 31, slow 1: 26, slow 2: 16, pen 3: 12, reference pen: 10). The tone level × 4 in those windows is near 448, 640, 128, 192, 256 (for example 641 → 0 → 641 with the tone at 640) | With 6 bits at 0 and the force tone on, take the nearest multiple of 16 from the tone level | Number of such windows → 0; entries of the stroke-start history = real strokes |
| 4 | **The palm becomes a touch when the pen drops out for a moment.** One sample out of proximity clears `active`, and touch is on again with the next heatmap (`on_touch` in `apps/daemon/daemon.hpp`). A palm that landed before the pen arrived is reported as well | Reference pen, palm part: 7 touch packets during 0.11 s in which the pen signal was near the threshold (1157–2437) | Keep touch off for some time after the pen left (for example 0.5 s). Hysteresis on the proximity threshold | Touch packets with a position over a whole pen capture |
| 5 | **A contact that was a palm becomes a finger again.** An unstable frame is recorded as valid without a decision (`check_contact` in `contacts/validation/validator.hpp`), which erases the "once a palm, always a palm" memory | Contacts reported as a finger after having been a palm, without lifting: reference touch 2, pen 1 2, light 1 2 | Keep the decision of the frame before in an unstable frame | The same count → 0 |
| 6 | **`BTN_TOUCH` flickers.** When the single-touch contact is skipped in an unstable frame, `BTN_TOUCH 0` is sent and 1 again in the next frame (`reset_singletouch` in `TouchDevice::process`, `apps/daemon/touch.hpp`) | Reference touch: lifted and pressed again within 35 ms 60 times (39 at the edge, 7 in taps). The slot is kept | Leave the single-touch state alone in an unstable frame | The same count → 0 |
| 7 | Touches at the edge are often unstable (the position is not updated in that frame) | Reference touch, edge part: 81 of 528 frames with a contact | Leave the size and orientation change of a blob cut by the edge out of the stability decision | Share of unstable frames per part |
| 8 | The slot is not the lowest free number (it depends on where in the list the new contact is: `track` and `find_new_index` in `contacts/tracking/tracker.hpp`). Above the axis maximum of 16 the kernel drops the slot selection and the events land in the wrong slot. The slot number is also used as the tracking ID (`emit_multitouch`) | Reference touch: five fingers reach slot 7 | Hand out free numbers after matching. A new tracking ID for every contact | Highest slot = contacts − 1 |
| 9 | Proximity has no hysteresis (one 0x06 magnitude above 2000 enters and leaves) | The gaps of #4. Reference pen: 6 gaps shorter than 0.6 s (5 at the edge, 1 in the palm part) | Different thresholds for entering and leaving, or the magnitude of the 0x07 window | Number of gaps shorter than 0.6 s |
| 10 | Every parse error is followed by a sleep of 100 ms (`run` in `core/linux/runner.hpp`): 6–7 frames are lost. A report frame of length 0 at the end of a report is an error (`Reader::skip` in `common/reader.hpp`) | The captures have no errors | Count instead of sleeping. Accept length 0 | — |
| 11 | 32-bit integer overflow: when the dot product of the button phase (`handle_button`) overflows, its sign flips and button and eraser are swapped. The same for the pressure tone estimate (`interpolate_frequency`) and the sum of the tone magnitudes (`force_tone`) | Largest values in the captures: the button dot product is at 1/200 of the limit (1.1e7), the pressure tone estimate at 1/12 (1.7e8). No overflow | 64 bits or floating point | Margin of the largest value |
| 12 | In a sample where zero-force contact cannot be used (no tip energy: at the edge) the stroke level is not cleared (`update_contact`), and the sum of three antennas survives a short window (`handle_position_mpp_2`) | The effect was not measured | Clear both together | Diff of the dumps |
| 13 | Gaussian fit: the share of overlapping blobs is divided by the square of the fitted Gaussian (`gaussian_like` in `contacts/detection/algorithms/gaussian.hpp`). The elimination runs on the transposed matrix (`ge_solve`; together with the doubled second row the result is right) | — | Leave it, or tidy it | Diff of the touch dumps |
| 14 | The neutral level is measured again every 16 frames (counted in frames, `detect` in `contacts/detection/detector.hpp`). The first heatmap after the start is dropped because its dimensions are not known yet | The live stream and the replay differ by one touch start in one part (8 against 7): the frame phase of the daemon | By time, or only in frames without a contact | Differences when the start of the replay is shifted |
| 15 | The size of the read buffer is computed without the report ID byte (`buffer_size` in `ipts/device.hpp`) | No effect on this device: the buffer is 7487 bytes, the largest report 4340 bytes | +1 | — |

## 8. What the live stream of the reference captures shows

The two reference captures (reference pen, reference touch) contain, next to the raw reports, the evdev stream of the iptsd that was running while
they were taken. The markers of their parts are approximate (the hand did not always follow the script).
Only what was confirmed in the raw windows is listed. Times are seconds from the first marker.

| Observation | Raw signal | Verdict |
|---|---|---|
| Six spans with the barrel button (−0.32–2.26, 14.56, 14.83, 15.11, 15.38, 15.68) | The bit of the binary window and the button tone (opposite phase) show the same six spans. The first span contains three contacts (three strokes with the button held) | As the signal says. Not a defect |
| No pen for 11 s after the button span ending at 2.26 s | 0x06 magnitude at most 2450 (near the threshold) | The pen was out of range. Not a defect |
| One eraser proximity and one contact (24.16–32.80) with constant pressure | Section 7 #2 | Looked like a defect (a hovering eraser reported as contact); a capture that separates hovering from touching showed the real causes (section 11) |
| Proximity gaps in the edge part: 36.61–37.14, 37.15–37.95, 40.13–40.63, 41.36–41.86, 44.06–44.73, 47.35–47.52, 48.82–49.23 | 0x06 magnitude of 10 000–430 000 meanwhile | Defect (section 7 #1) |
| Touch during the pen capture: 5.40–5.44, 33.48–33.52, 50.95, 56.93–57.02 | The first three while the pen was out of range (50.95 is 0.02 s before the pen arrived). 56.93–57.02 is in the palm part, during 0.11 s in which the pen signal fell below the threshold | The first three follow the rule. The last is a defect (section 7 #4) |
| Touch: `BTN_TOUCH` flickers 60 times (replay) / 62 times (live) | Coincides with the unstable frames | Defect (section 7 #6) |
| Touch: all 8 light taps are reported as contacts | 8 spans in which the strongest cell is at least 8 below the neutral level | Nothing was missed |
| The stylus stream as a whole | The spans of button, tool and contact are the same in the live stream and in the replay (patch 0017) | — |

## 9. Open decisions

- The rest of section 7 (#7, #10–#14) and the remaining items at the end of section 11.
- The time constants of proximity and of blocking touch (#4, #9).
- When `digitizerd` becomes the default daemon: after the unverified items of sections 1 and 12 were seen on the device.
- Whether the virtual devices keep their names (`IPTSD Virtual …`). A change needs the libinput quirks and the helper's search by name
  changed with it.

## 10. When the C++ side changes

1. Rebuild the reference harness from the new patch series.
2. If the preset changed, make the configuration files of the reference again (preset plus additions) and change the built-in defaults
   (`config.rs`) with it: the unit test `defaults_are_the_platform_preset` names the keys that disagree.
3. Produce the reference dumps again and run the comparison (section 5): it names the captures in which Rust differs. Carry the diff of
   `dft.hpp` over into the module of the one stage it belongs to (contact → `pen/contact.rs`, button → `pen/button.rs`, …).
   Patches 0016 and 0017 came in this way: 0016 is `TipSignal::sum` and `continues()` in `contact.rs`, 0017 is `binary_window()` in
   `button.rs`.

## 11. Fixes made since the port

Each defect was changed on its own, with the difference to the dump before the change and the values measured in the captures.
"Lines" is the number of changed dump lines over the 10 captures of section 5 with the device's configuration.

| Defect | Fix | Before → after | Changed lines |
|---|---|---|---|
| #3 pressure with 6 bits at 0 | With the 6 bits at 0 there is force if the force tone of the position windows was on up to the window just before: the level is then chosen from the tone like any other value (`pen/pressure.rs`, `ContactStage::force_steady`) | A single 0 window between windows with force: 104 → 1 (the remaining one also has the force tone off). Force spans 491 → 373, stroke-start history 452 → 354. Contact unchanged in every sample | P 2067 (pressure only) |
| #1 edge | The edge is decided by the antenna number (`signal::edge_of`), and the amplitude threshold applies to the strongest antenna. In the half cell at the edge the position is estimated from the amplitude ratios of the three inner antennas (`signal::edge_fraction`; coefficients fitted to 35 000 interior windows: 0.18 mm rms on the glass, where the parabola has 0.5 mm) and hands over to the parabola towards the next cell, continuous with the interior. `edge_reach` spreads it over the strip outside the edge antenna (X 0.44 mm, Y 0.94 mm) | Reference pen, edge part: gaps with a signal present 8 → 1 (0 after #9); windows above the threshold without proximity 255 → 17 (5 after #9). Reach: 0.80 / 0.44 / 0.94 / 0.94 mm short of the edge → 0.00 mm (four sides). Step at the hand-over, relative to the steps around it: X +0.005 mm, Y +0.23 mm (largest). Steps within the edge cell, p95: 0.15 → 0.10 mm (X), 0.25 → 0.24 mm (Y) | Reference pen P +232 (1555 → 1787), slow 1: 7 |
| #6 `BTN_TOUCH`, #5 palm | An unstable frame leaves the single-touch state as it is. When the single-touch contact lifts, another contact takes over in the same frame (the button is down while any finger is). An unstable frame keeps the palm decision of the frame before | Lift and press within 35 ms: reference touch 60 → 1 (a real gap: one frame without detection). Palm → finger 6 → 0. No delay on lifting (the lift is sent in the frame in which the contact is gone) | C 1369 (validity flag), E touch 163 |
| #4 touch after the pen | The time is an input (`Digitizer::process(report, now, sink)`). Touch is blocked while the pen is in range and for `pen_holdoff_s` after it left, and a contact that was on the screen in that time is not reported until it lifts | Pen captures: lift and press within 35 ms 33 → 0, slot starts 51 → 15. reference pen, palm part: 7 leaked packets → 0 | E touch −482 +46 |
| #9 proximity hysteresis | A pen in range does not leave when one 0x06 window gives no position, if the 0x07 and 0x08 windows of the frame before gave one and the 0x06 magnitude is above half the threshold | Gaps shorter than 0.15 s: 16 → 2 (both are a high hover in light 2); proximity spans 79 → 60. The tilt no longer starts from scratch after a gap (values such as 90°) | P −10370 +10643 (mostly the tilt smoothing carrying on: position difference at most 0.07 mm) |
| #8 slot numbers | After matching, the lowest number that neither this frame nor the frame before uses | Reference touch, five fingers: highest slot 7 → 4 | C 729, E touch 464 |
| #15 read buffer | Includes the report ID byte | No effect on the captures | 0 |
| #2 eraser | The eraser end is a switch, not a force sensor (below). The tone of the open switch leaking into the tone row is not read as force (`pen/pressure.rs`). With zero-force contact on, the strength of the eraser tone in the 0x06 window decides "touching the glass" (`ContactStage::eraser_touch`, thresholds `eraser_touch_enter` / `eraser_touch_exit` = 500 000 / 250 000) | Eraser capture: light rubbing 74 contact samples in 8 pieces → 533 samples in 1 piece. Three presses: 7 pieces (5 shorter than 35 ms) → 3. Taps: 9 pieces (3 shorter than 35 ms) → 8 (for 8 taps). Contact while hovering 0 → 0 | The 10 captures: reference pen P 2 |
| The tool on coming into range | Nothing is reported from the 0x06 window in which the pen comes into range. A tip is reported as the pen 3 ms later from the 0x07 window. An eraser is decided at once when the button window of the same frame shows a tone of the same phase as strong as the 0x06 window (skipping the two-frame confirmation) and is reported as the eraser from the next 0x06 window. A tone in the 0x07 and 0x08 windows that is weaker than in the 0x06 window is not a tip (the eraser has no tone there). A tool that comes into range is announced directly, without a packet out of range before it | Entries in which the eraser was first announced as a pen: eraser capture 18 → 0, a capture taken with this daemon 5 → 0, reference pen 1 → 0. Entries in which the pen was announced as an eraser: 0. First announcement: pen +2.5–2.8 ms; eraser 15 ms later than the earlier (wrong) pen announcement and earlier than the earlier eraser announcement (30–45 ms) | The 10 captures: one P line per entry (−60 in total); reference pen, eraser part: tilt of 578 samples (garbage → 0) |

Stylus packets (`output::StylusOutput`): a pen in range whose tool changes goes out of range for one packet. A tool that comes into
range touching, or leaves range touching, does so in two packets (tool first, then contact; contact end first, then out of range),
because clients keep the tool's cursor when contact and range change in one packet.

Why `pen_holdoff_s = 0.2` (captures): the spans in which the pen signal dropped out for a moment are within 0.01–0.14 s (16 of them),
and the fastest deliberate use of a finger after lifting the pen is a swipe that began 0.38 s later (slow 2). With 0.5 s that swipe would
be dropped as a whole.
A hand that lands before the pen it holds (reference pen at 50.95 s: 0.02 s earlier) is not prevented by this rule: the contact is only lifted
when the pen arrives.

The signal of the eraser end (eraser capture): the tone of the pressure window has only two places. With the switch open it is in the
row after the tone rows (row 6), with the switch closed in row 2, and pressing harder or softer never puts it in between (the ratio of
the magnitudes of rows 1 and 2 is fixed at 0.167). The digital rows do not change either. So the eraser has one pressure value (0.653),
and the 8.6 s of the reference pen capture that section 7 #2 reads as "always in contact" were rubbing with the eraser held down. The real defects were
two: the open tone leaks into row 5 and gives contacts of one or two samples, and light rubbing that does not close the switch erases
nothing.
Magnitude of the eraser tone in the 0x06 window (sum of both axes, in millions): 0.004–0.02 hovering at 5 mm, up to 0.14 just before
touching, 0.5–7 on the glass (light rubbing: p5 0.73, median 4.6).

Check on the device (a capture taken while this daemon ran, in a build with the fixes up to proximity hysteresis): comparing the replay
of the raw reports with the live evdev stream in the form a client sees (changed values only, no empty packets), all 1141 stylus
packets are the same in value, order and `SYN_REPORT` grouping. Of 276 touch packets 252 are the same; the others are the first 17
packets of a contact that was already down when the capture began, and 6 places with ±1 unit in a coordinate or size: the phase of the
16-frame neutral measurement differs between the replay and the running daemon (replayed with the phase at 1, everything after the
start is the same). The live event times differ from the times the reports were received by 0.05 ms in the median and 1.5 ms at most.

What cannot be measured at the edge: once the tip is at the edge antenna or beyond, the amplitude ratios of the inner antennas no
longer change (in interior windows with the tip 0.1–0.5 cells on the far side of an antenna, the ratio to the neighbour stays at
0.09–0.1). In that strip the coordinate is therefore a mapping, not an estimate (`edge_reach`): estimates of 0.03–0.2 cells are spread
from the screen edge to 0.2 cells. Without it the coordinates stop at the edge antenna (0.44 / 0.94 mm from the edge).

Remaining:

- Windows in which the pressure level is off by 16: the error of the level derived from the tone exceeds ±8 near tone rows 2.0–2.4
  (levels 114–141), so the 4 digital bits select the wrong value. 246 of 8364 windows differ from the smoothest sequence. A rule that
  uses the level of the window before as a prior was not added: once wrong it sticks (for up to 28 windows).
- In the half cell at the edge the tip energy cannot be measured, so there is no zero-force contact there (only contact by the force
  tone).
- Matching has no distance limit: when one finger lifts and another lands elsewhere in the same frame, they count as one contact that
  jumped.
- #7 (unstable frames of touches at the edge), #10–#14.

## 12. Installation and operation

NixOS: `hardware.surface-pro-9.touch.daemon = "digitizerd"` ([nixos.md](nixos.md)). iptsd is then neither built nor installed (no unit,
no udev rule, no preset file), and the single unit `sp9-digitizerd.service` starts with `multi-user.target`. The module loads `uinput`
and creates `/etc/iptsd.d` for the helper's drop-in. The virtual devices have the names iptsd gives them
(`sp9_digitizer::output::TOUCHSCREEN_NAME`, `STYLUS_NAME`, in one place, used by the helper). With `digitizerd.package = true` the
package is installed next to iptsd and nothing starts its unit.

**The node is found by its ID, and the daemon stays.** The ithc driver removes the HID device on suspend and creates it again on resume
(`ithc_suspend` releases its devres group, `ithc_resume` calls `ithc_start`). So the hidraw node disappears with every suspend and comes
back under another number: in the log of this device the HID instance number reached `.002F` within one boot and the node went from
hidraw0 to hidraw7.
iptsd runs one instance per node, so its virtual devices are created anew every time, and an open GTK application (Rnote) kept the state
of the old tablet device and put the pen at a wrong position (observed with iptsd).
Started without an argument, this daemon looks for `045E:0C52` in the `HID_ID` of `/sys/class/hidraw/*/device/uevent` and looks again
every 0.25 s while it is not there. When a read ends with `EIO`, `ENODEV`, `ENXIO` or 0 bytes it takes the pen out of range and lifts
every contact (`Digitizer::release`) and searches for the node again. **The uinput devices are created once and stay until the daemon
ends.**
For that reason the unit is not bound to a device unit (no `BindsTo`) and not started by a udev rule: bound, the daemon would go down at
every suspend and the devices would be created again. `Restart=on-failure` is for a daemon that ended with an error.

| Case | Design | Verified |
|---|---|---|
| Suspend and resume | When the node goes away, lift everything and wait; when it is back, open it and switch to raw mode | That ithc creates the device anew: driver code and the log of this device. The daemon finding the node again **was not run on the device** |
| The node number changes | Found again by ID | The search function has a unit test (a fake sysfs). Across a reboot: measured (hidraw7, then hidraw0). Within one boot: unverified |
| The device leaves raw mode (the node stays) | When something other than a raw report arrives, the mode is requested again, at most once per second | Unverified (it is not known whether this happens) |
| Stopping the daemon | Lift everything and switch back to single-touch mode | Stopping a transient unit |
| Changing settings | D-Bus, no restart, the devices stay | Unit tests, a test that changes settings in the middle of a capture replay; on the device `sp9-ctl pen zero-force` (measured, section 1) |
| The helper's haptics (evdev) | When the node of the virtual stylus disappears it is found again by name (`penwatch`). With this daemon the node does not disappear | Code. Haptics with this daemon: observed |

Unit hardening ([`sp9-digitizerd.service`](../helper/data/sp9-digitizerd.service)): no capabilities (`CapabilityBoundingSet=`), no
devices but `/dev/uinput` and hidraw (`DevicePolicy=closed`), `ProtectSystem=strict`, no network (the system bus is a Unix socket),
`SystemCallFilter=@system-service` and more. `systemd-analyze security` rates the exposure at 1.5. The daemon runs under this unit on
the device (section 1). If on another system it cannot open hidraw or uinput, look at `DevicePolicy` and `DeviceAllow` first.

The helper ([helper.md](helper.md)): when a setting changes, the helper writes the drop-in and, if `Sp9Digitizer1` is on the bus,
passes it on with `SetParams` and is done. Only when it is not on the bus does it restart iptsd as before. `sp9-ctl pen status` shows
the helper's pen settings together with the live state and the counters of the daemon.

## 13. Position prediction

`pen.prediction_ms` (default 0 = off). The last stage of the pen pipeline (`pen/predict.rs`, after the lock) reports the coordinates
ahead by that time along the pen's velocity. The distance by which a line trails the pen becomes shorter.
What is reported here is the ink: unlike the prediction an application draws temporarily at the end of a stroke, whatever is wrong
stays on the page. So it is off by default and the value is the user's choice.

```sh
sp9-ctl pen predict 7.5      # ms, 0..30; "off" switches it off; without an argument: the current value
```

The helper keeps the value in `helper.toml` (`input.pen_prediction_ms`) and in the drop-in (`[Stylus] PredictionMs`) and, while the
daemon runs, passes it on over D-Bus (`Pen.SetParams {PredictionMs}`). Nothing is restarted.
Only the tip is predicted (MPP v2 position windows, 133 Hz). The eraser's coordinates come once per frame from the weak position tone;
they were not measured and are reported as they are (in a capture replay the eraser output is the same with prediction on).
The stage always keeps two positions as they were before prediction, so a value changed in the middle of a stroke applies from the next
sample (unit tests and a pipeline test).

Method (all of it settled by measurements on captures; time is the timestamp of the DFT windows, not the wall clock, so the sample
intervals of 7.24 and 7.77 ms enter as they are):

| Rule | Value | Gain (tuning captures, 15 ms) |
|---|---|---|
| Velocity = mean of the last two intervals | 0.5 / 0.5 | Roughness 0.221 → 0.142 mm (0.080 without prediction). One interval alone carries the position noise times horizon / interval |
| Reduced by the change of the velocity vector | change / larger speed × 1 | Distance from the stroke p90 0.20 → 0.16 mm, overshoot at the end p90 1.06 → 0.89 mm |
| Reduced when the pressure falls | relative drop since the sample before × 3 | Overshoot at the end p50 0.34 → 0.20 mm, p90 0.89 → 0.60 mm |
| Half in contact without a steady force tone | 0.5 | Overshoot at the end p90 0.60 → 0.43 mm, largest 1.86 → 0.93 mm. Of 193 stroke ends, 162 had an unsteady force tone in the last sample |
| Ramped up over 3 samples after contact begins or ends | 3 | Deviation of the first three points of a stroke p90 0.37 → 0.05 mm |
| No prediction while the lock holds the coordinates | | A resting pen does not tremble |
| Starts anew after a gap of more than 20 ms | | |

Tried and left out: a speed threshold (no effect: a slow pen is predicted by little anyway), damping in proportion to deceleration (less
deviation, but the roughness grows from 0.166 to 0.202), a cap on the predicted distance (in fast strokes the lag is back in full).
There is no separate horizon for hovering (the cursor): at the same horizon the remaining error in the air is close to that in contact
(table below), which gives no reason for another setting.

On captures not used for tuning (91 strokes in contact, 11 000 samples), in mm:

| Horizon | Lag error p50 / p90 / p99 (without → with prediction) | Lag while hovering p50 / p90 | Deviation from the real path p90 / p99 / max | Length drawn beyond the end of a stroke p50 / p90 / max | Roughness (RMS of the second difference, median) |
|---|---|---|---|---|---|
| 0 (off) | — | — | — | — | 0.090 |
| 4 ms | 0.14 / 0.51 / 1.42 → 0.03 / 0.13 / 0.44 | 0.15 / 0.64 → 0.05 / 0.19 | 0.03 / 0.14 / 0.42 | 0.02 / 0.13 / 0.30 | 0.113 |
| 7.5 ms | 0.26 / 0.95 / 2.65 → 0.06 / 0.25 / 0.81 | 0.27 / 1.18 → 0.10 / 0.34 | 0.07 / 0.29 / 1.00 | 0.05 / 0.25 / 0.56 | 0.139 |
| 11 ms | 0.38 / 1.39 / 3.86 → 0.09 / 0.39 / 1.38 | 0.38 / 1.75 → 0.14 / 0.47 | 0.11 / 0.51 / 1.70 | 0.11 / 0.41 / 0.90 | 0.163 |
| 15 ms | 0.52 / 1.90 / 5.28 → 0.13 / 0.57 / 2.07 | 0.51 / 2.37 → 0.19 / 0.65 | 0.17 / 0.85 / 2.76 | 0.17 / 0.72 / 2.24 | 0.200 |

- Lag error: the distance between the predicted coordinates and the real coordinates that much later (interpolated from the coordinates
  before prediction). "Without" is the distance the pen travels in that time.
- Deviation, overshoot and roughness are what prediction leaves in the ink. On the same captures a straight-line prediction from two
  samples gives, at 15 ms, an overshoot of 0.50 / 1.53 / 6.12 mm and a roughness of 0.243. Its lag error is a little smaller
  (0.10 / 0.40 / 1.54): what the rules hold back is not followed.
- p99 and the maximum of the deviation are at the level of the straight-line prediction (strokes that turn fast). What the rules reduce
  is the start and the end of a stroke and the roughness.

Suggestion: the value to try is **7.5 ms** (one sample). About three quarters of the lag distance go away, and what stays in the ink is
mostly below 0.3 mm. 4 ms costs almost nothing and gains half as much. From 11 ms on, about one stroke in a hundred is off by more than
0.5 mm at a turn or at its end.
What to look at by hand: whether fast lines end in a hook or a flick, whether the corners of small letters (such as z or L) bulge or
grow a spike, whether a long slow line trembles, whether a dot gets a tail. The gain to look for is the gap between the pen tip and the
end of the ink in a fast line.

**Prediction was not run on the device.** All numbers above are from capture replays. With prediction off the output is unchanged (the
reference dumps stay the same).
