# userspace/libcamera

Patches for [libcamera](https://libcamera.org) 0.8.0: what the two cameras of the Surface Pro 9 need from the
simple pipeline handler and its software ISP (SoftISP). With an unpatched libcamera the processed image has
purple lines at a fixed spacing, the exposure is off or swings, the rear camera has no focus control, and
0.8.0 delivers no frames at all on this device. The measurements behind each patch are in
[docs/research/gaps/camera.md](../../docs/research/gaps/camera.md), section 6, and in the patch headers.

The NixOS module (`hardware.surface-pro-9.camera.libcamera`, on by default) builds libcamera 0.8.0 with these
patches and PipeWire and WirePlumber against it ([`nix/libcamera.nix`](../../nix/libcamera.nix),
[`nix/pipewire-libcamera.nix`](../../nix/pipewire-libcamera.nix)); `camera.mainsFrequency` selects the 50 Hz
flicker period. On another distribution apply them to the libcamera 0.8.0 sources in the order of their numbers
(`git am` or `patch -p1`). The sensor gain ranges are also corrected in the kernel (patches 0025 and 0026 of
[`kernel/`](../../kernel/README.md)); the tuning files of 0010 carry the same limits, so the patched libcamera
behaves the same on a kernel without them.

The patches are changes to libcamera and carry the licence of the files they touch: LGPL-2.1-or-later for the
library and the IPA module, BSD-2-Clause for the debayer shader `bayer_unpacked.frag`, CC0-1.0 for the meson
build files and the tuning files ([REUSE.toml](../../REUSE.toml) has it per patch).

| Patch | What it does |
|---|---|
| `0001` Revert "libcamera: pipeline: simple: Prefer CSI2 packed bayer formats over unpacket formats" | Reverts the preference for CSI-2 packed Bayer formats that 0.8.0 introduced. On this device the IPU6 ISYS returns every packed buffer with the error flag, for both sensors, and no frame reaches the application. The unpacked formats work |
| `0002` libcamera: software_isp: debayer_egl: Debayer pixel for pixel and scale in a second pass | The GPU debayer scaled the whole input into the output with nearest-texel lookups; where a lookup fell on a texel boundary the pixel got the colours of the wrong Bayer site: magenta lines every 480 pixels at 1920x1080. It now debayers the centred area with the aspect ratio of the output pixel for pixel and scales in a second pass (bilinear, mipmaps below 70 %). The statistics, which were taken from the top-left corner of the input, come from the same area, so exposure and white balance follow what the output shows |
| `0003` libcamera: sensor: Add control delays for ov5693 and ov13858 and the ov13858 black level | Control delays of two frames for exposure and analogue gain of both sensors (measured; the default assumed one frame for the gain), and the black level of the OV13858 |
| `0004` libipa: agc: Take analogue gain limits and a flicker period from the tuning data | `minAnalogueGain` and `maxAnalogueGain` in the tuning data narrow the gain range to what the sensor follows. `flickerPeriod` keeps an exposure time that reaches the mains flicker period at a whole number of periods |
| `0005` ipa: softisp: agc: Fix the luminance estimate of dark frames and the loop delay | Three defects that made the exposure oscillate after a start or a change of light: the luminance estimate of dark frames (the gain it asked for grew without bound), a loop delay of one frame that split the regulation into two alternating loops, and the black first frame of a stream sending the exposure to its maximum |
| `0006` libcamera: software_isp: swstats_cpu: Keep the histogram index in range | Clamps the histogram index: a corrupt frame can carry values beyond its bit depth |
| `0007` libcamera: software_isp: Add focus statistics | A contrast figure for autofocus in the SoftISP statistics: squared differences of neighbouring green pairs over the central half of the image, relative to the brightness |
| `0008` libcamera: software_isp: Let the IPA move the focus lens | The IPA module gets the controls of the focus lens and can move it. The lens device is closed while the camera is not streaming, so that an idle session (WirePlumber) does not keep the lens and the rear sensor powered |
| `0009` ipa: softisp: Add a contrast detection autofocus | The `Af` algorithm: a coarse scan from the far end, a fine scan around the peak, the interpolated maximum as the result; continuous mode scans again when the scene changes. Controls `AfMode`, `AfTrigger`, `LensPosition`; metadata `AfState`, `LensPosition`, `FocusFoM` |
| `0010` ipa: softisp: Add tuning files for ov5693 and ov13858 | Tuning files for the two sensors: gain limits (OV5693 1x–62x, OV13858 1x–15.5x), the lens range of the rear camera (400–800), the flicker period of 60 Hz mains |

On the device: with these patches the lines are gone and the exposure settles within a few frames (measured
with `cam` and through PipeWire; observed in a camera application). The autofocus sweeps the lens and reports
its state (measured). With it the rear camera focuses (observed); the dioptre scale of the lens range is not
measured. Colour is uncalibrated (grey-world white balance, no colour correction matrix).
[docs/hardware-status.md](../../docs/hardware-status.md) has the status,
[docs/test-results.md](../../docs/test-results.md) the rows CAM-2, CAM-4 and CAM-7. The capture tools used for
the measurements are `cam-raw.sh`, `cam-raw-stats.py`, `cam-ctrl-delay.c`, `cam-focus-sweep.c` and
`cam-af-check.sh` in [`tools/linux/`](../../tools/README.md).
