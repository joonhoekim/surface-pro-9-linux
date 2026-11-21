# dkms/patches/

Adjustments that exist only for the DKMS build. They are applied by `dkms/gen.sh` after the patches of
`kernel/patches/`, to the module's copy of the files, and never to a kernel tree.

| Patch | Module | Why |
|---|---|---|
| `ipu-bridge-genksyms.patch` | ipu-bridge | Keeps the MODVERSIONS CRCs of the stock `include/media/ipu-bridge.h`, so the stock IPU drivers load against the DKMS module. Applied only while the bundled header differs from the target's |
| `ucsi-surface-hid-generic.patch` | ucsi_surface_hid | Matches the UCSI device in the generic HID group, so the HID core patch 0007 is not needed, and lets a non-UCSI device on the same ID fall back to plain HID |
