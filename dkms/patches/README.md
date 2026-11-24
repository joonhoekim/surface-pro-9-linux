# dkms/patches/

Adjustments that exist only for the DKMS build. They are applied by `dkms/gen.sh` after the patches of
`kernel/patches/`, to the module's copy of the files, and never to a kernel tree.

| Patch | Module | Why |
|---|---|---|
| `ipu-bridge-genksyms.patch` | ipu-bridge | Keeps the MODVERSIONS CRCs of the stock `include/media/ipu-bridge.h`, so the stock IPU drivers load against the DKMS module. Applied only while the bundled header differs from the target's |
| `0012-registry-sp9-dptf.patch` | surface_aggregator_registry | Kernel patch 0012 for linux-surface releases from 7.3.5 on, where the Surface Pro 9, 10 and 11 (Intel) have their own group `ssam_node_group_sp9_dptf` (the Surface Pro 12 keeps the old group, without the RTC node). Used only where 0012 does not apply (`ALT_PATCHES`) |
| `ucsi-surface-hid-generic.patch` | ucsi_surface_hid | Matches the UCSI device in the generic HID group, so the HID core patch 0007 is not needed, and lets a non-UCSI device on the same ID fall back to plain HID |
