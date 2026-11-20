# sp9-digitizer

Touch and pen processing for the digitizer of the Surface Pro 9 (Intel, `045E:0C52`): the parser
for its raw reports, contact detection on the capacitive image, the pen pipeline on the DFT
windows, and the evdev events of the two virtual input devices. No I/O; `sp9-digitizerd` connects
it to hidraw and uinput. Design, verification and the list of known defects:
[docs/digitizerd.md](../../../docs/digitizerd.md).

## Origin

The processing derives from [iptsd](https://github.com/linux-surface/iptsd) 3.1.0
(GPL-2.0-or-later, Copyright (C) the iptsd authors) with the pen patches of this repository
(`userspace/iptsd`), rewritten for this one device and its configuration. Files that derive from
iptsd name the source files they come from in their header. This crate is distributed under
GPL-3.0-or-later, which the "or later" of iptsd's licence permits.

The port was checked against the patched iptsd bit for bit, by replaying captures of the device
and comparing every emitted event with dumps of the C++ code. Defects found in that code have
been fixed here since, so the output now differs on purpose where they applied. The comparison
is the test `sp9-digitizerd/tests/golden.rs`; the captures and the reference dumps are not part
of the repository, and the test is skipped unless `SP9_CAPTURE_DIR` and `SP9_GOLDEN_DIR` name
them.
