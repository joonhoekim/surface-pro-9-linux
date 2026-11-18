# userspace/iptsd

`0001-zero-force-contact.patch` applies to iptsd 3.1.0: the pen counts as touching when its tip signal reaches a
fraction of the level it has while it writes with force, even while it reports no force, and a pressure floor
is reported meanwhile (`[DFT] ZeroForceContact`, `ZeroForceEnter`, `ZeroForceExit`, `ZeroForceReference`,
`ContactMinPressure`; all off by default). The NixOS module applies it to `pkgs.iptsd`; the device helper writes
the values.

`0002-tilted-pen-in-range-button-debounce.patch`: a neighbour antenna in antiphase with the peak (a strongly tilted pen) no longer
makes iptsd drop the pen out of range, and the magnitude based button decision needs two frames in a row (the first
frame in range used to press the barrel button).

`0003-extra-position-windows.patch`: `[DFT] ExtraPositionWindows` takes the pen position from the two MPP v2 position windows of
a frame and reports each: 133 samples per second instead of 67, and no offset with a tilted pen. Off by default; the
Surface Pro 9 preset turns it on.

Patches 0004 to 0014 (antenna mapping, tilt from the MPP v2 position windows, tilt offset of the position, centroid
interpolation, 0.01 mm stylus axes, jitter lock, ink filter, button tone condition, contact from the force tone, pressure
decode, pressure ramp and zero-force pressure) each add `[DFT]` keys that default to the stock behaviour; the Surface Pro 9
preset in `userspace/platform/iptsd/` enables the recommended ones. Their headers give the measurements.
