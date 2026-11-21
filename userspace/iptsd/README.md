# userspace/iptsd

Patches for [iptsd](https://github.com/linux-surface/iptsd) 3.1.0, the daemon that turns the raw data of the
touch controller into a touchscreen and a stylus. They change how the Slim Pen 2 is read on the Surface Pro 9:
position, tilt, contact, pressure, the barrel button. Every patch adds configuration keys whose default is
iptsd's stock behaviour; the Surface Pro 9 preset
[`userspace/platform/iptsd/surface-pro-9.conf`](../platform/iptsd/surface-pro-9.conf) turns on the ones that
are meant to be used.

The NixOS module (`hardware.surface-pro-9.touch.enable` with `touch.daemon = "iptsd"`, the default) applies
the patches to `pkgs.iptsd`. On another distribution apply them to the iptsd 3.1.0 sources in the order of their
numbers (`patch -p1`). The contact thresholds are written by the device helper into a drop-in
([docs/helper.md](../../docs/helper.md)). The comparison with Windows and what was measured are in
[docs/research/gaps/pen-touch.md](../../docs/research/gaps/pen-touch.md), section 5. The patches are changes to
iptsd and carry its licence, GPL-2.0-or-later.

The Rust daemon `sp9-digitizerd` ([docs/digitizerd.md](../../docs/digitizerd.md)) does the same processing
without iptsd; these patches are its reference.

| File | What it does |
|---|---|
| `0001-zero-force-contact.patch` | The pen counts as touching by the strength of its tip signal even while it reports no force (`[DFT] ZeroForceContact`, `ZeroForceEnter`, `ZeroForceExit`, `ZeroForceReference`), and the pressure has a floor during contact (`ContactMinPressure`). All off by default |
| `0002-tilted-pen-in-range-button-debounce.patch` | With a strongly tilted pen the neighbour of the strongest antenna is in antiphase, and iptsd dropped the position of such a frame: the pen kept leaving and entering range (measured: 63 % of the position frames). Such a neighbour now counts as no signal. And the barrel button (or the eraser) is accepted only in two frames in a row: the first frame in range used to press it for one frame |
| `0003-extra-position-windows.patch` | `[DFT] ExtraPositionWindows`: the pen position is taken from the two MPP v2 position windows of a frame (0x07, 0x08) and reported per window. 133 reports a second instead of 67, and the offset of up to 1.7 mm that the 0x06 window has with a tilted pen is gone (measured by replaying captures). Off by default; the preset turns it on |
| `0004-antenna-mapping.patch` | Antenna numbers are mapped to the screen with the panel's real antenna pitch and the position of the first antenna (`AntennaPitchX/Y`, `AntennaOriginX/Y`). Before, the position was off by up to 0.44 mm horizontally and 0.94 mm vertically towards the edges |
| `0005-mpp2-tilt.patch` | Tilt from the distance between the pen tip and the ring in the MPP v2 position windows (`Mpp2TiltDistance`, `TiltSmoothing`, `TiltWhileHovering`). Before, tilt came out two to three times too large and jittered by 3–10° from sample to sample |
| `0006-tilt-offset.patch` | Position correction for a tilted pen (`TiltOffsetLinear/Quadratic`) |
| `0007-centroid.patch` | Per axis, the position can be the weighted centroid of three antennas (`PositionCentroidX/Y`) |
| `0008-axis-resolution.patch` | Stylus axes in units of 0.01 mm (`[Stylus] AxisResolution`) |
| `0009-jitter-lock.patch` | Holds the coordinates of a pen that stands still (`JitterRadius`, `JitterCount`) |
| `0010-ink-filter.patch` | A Kalman filter for ink (`InkFilter`). Roughness is halved and the line lags by 0.16–0.24 mm. The preset leaves it off |
| `0011-button-tone.patch` | No button when another tone is stronger in the button window (`ButtonToneRatio`) |
| `0012-force-tone-contact.patch` | Contact is decided from the force tone of every position window (`ForceToneContact`), with conditions for the start, the end and a lockout of zero-force contact (`ZeroForceStartSamples`, `ZeroForceStrokeLevel`, `ZeroForceLockoutPressure/Samples`) |
| `0013-mpp2-pressure-decode.patch` | Pressure decoded from the pen's tone and digital bits (`Mpp2PressureDecode`). Before, light pressure came out about 1.8 times too high |
| `0014-pressure-ramp-zero-force-pressure.patch` | The two samples of a frame are joined by a half step (`PressureRamp`), and the pressure of a zero-force contact follows the pressure at which recent strokes started (`ZeroForcePressure`) |
| `0015-cell-correction.patch` | Interpolating between antennas from three of them leaves an error that repeats with every antenna cell, so a slow diagonal line comes out wavy (up to 0.45 mm with a tilted pen). Both axes use the weighted centroid, and that error, which depends on the level of the tip signal, is subtracted (`TipSignalReference`, `CellCorrection*`). At a high hover, where the signal is weak, the interpolation goes over to the parabola (`CentroidFadeMin/Max`). No added delay |
| `0016-zero-force-stroke-sum.patch` | A lightly pressed pen reports no force for up to about 0.2 s within a stroke, which used to break the stroke. A stroke that had force now continues while the sum of the three antenna amplitudes stays above `ZeroForceStrokeLevel` times its level with force (`ZeroForceStrokeSum`). The earlier measure of signal strength (the energy of the interpolated peak) varied by a factor of 1.0–3.2 within one stroke of a tilted pen |
| `0017-mpp2-button-purity.patch` | While the pen comes into range, both rows of the binary window carry signal for a few frames, which was read as the barrel button. A window whose stronger row is not `Mpp2ButtonPurity` times the weaker one is not decided |

The figures of 0004–0017 were measured by replaying raw frame captures of this device; the header of each patch
gives them. What was seen on the device is in [docs/test-results.md](../../docs/test-results.md) (PEN-1, PEN-3,
PEN-4) and [docs/hardware-status.md](../../docs/hardware-status.md).
