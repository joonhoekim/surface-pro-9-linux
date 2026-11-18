# sp9-digitizer

Touch and pen processing for the digitizer of the Surface Pro 9 (Intel, `045E:0C52`): the parser
for its raw reports, contact detection on the capacitive image, the pen pipeline on the DFT
windows, and the evdev events of the two virtual input devices. No I/O; `sp9-digitizerd` connects
it to hidraw and uinput.

## Origin

The processing derives from [iptsd](https://github.com/linux-surface/iptsd) 3.1.0
(GPL-2.0-or-later, Copyright (C) the iptsd authors) with the pen patches of this repository
(`userspace/iptsd`), rewritten for this one device and its configuration. Files that derive from
iptsd name the source files they come from in their header. This crate is distributed under
GPL-3.0-or-later, which the "or later" of iptsd's licence permits.

In this phase the emitted events are those of the patched iptsd, bit for bit: a test replays
captures of the device and compares the result with dumps of the C++ code
(`sp9-digitizerd/tests/golden.rs`, run with `SP9_CAPTURE_DIR` and `SP9_GOLDEN_DIR`).
