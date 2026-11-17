# Hardware inventory

The configuration of the Surface Pro 9 (Intel), read under Windows. The raw listings are in
[`reference/`](reference/) and are regenerated with
[`tools/windows/collect-hwinfo.ps1`](../tools/windows/collect-hwinfo.ps1) (no administrator rights needed).
Serials, MAC addresses and other values that identify a unit are not in the repository.

## Device

| Item | Value | Notes |
|---|---|---|
| Model / SKU | Surface Pro 9, `Surface_Pro_9_2038` | **Intel model** (not SQ3/ARM). The `surface-gpe` patch of linux-surface matches this SKU with `DMI_EXACT_MATCH` |
| CPU | i5-1235U (Alder Lake-P, 2P+8E, 12 threads) | |
| Memory | 16 GB LPDDR5 SK Hynix `H58G56AK6BX069`, 5200 MT/s | |
| Storage | Samsung `MZ9L4256HCJQ-00BMV` (PM9B1) 256 GB NVMe, GPT, 512 B logical sectors | |
| Display | 2880x1920, up to 120 Hz, panel LG Display `LGD0719` | The EDID is not in the repository; `tools/linux/post-install-check.sh` saves the one of your own device as `edid-linux.bin` |
| GPU | Iris Xe (`8086:46a8`) | `i915`/`xe` |
| Wi-Fi / BT | Intel AX211 CNVi (`8086:51f0`), BT `8087:0033` | `iwlwifi`, `btusb` |
| Audio | Realtek ALC274 (`10ec:0274`, HDA) + Intel SST (`8086:51c8`) | `snd_hda_intel`/SOF |
| Touch and pen | Intel THC (`8086:51d0`, "Precise Touch and Stylus") | linux-surface `ithc` + `iptsd` |
| Cameras | IPU6 (`8086:465d`), front OV5693 (`OVTI5693`), rear OV13858 (`OVTID858`), IR VD55G0 (`SMO55F0`), control `INT3472` x3 | Per the linux-surface wiki only the rear camera works. This repository's results are in [test-results.md](test-results.md) |
| Battery | Design capacity 46530 mWh | |
| UEFI | 24.103.143, Secure Boot on | The state read under Windows before the installation. Secure Boot and Secure Core were turned off for the installation ([install-nixos.md](install-nixos.md)) |
| Sleep | S0 Low Power Idle (Modern Standby) only, no S3 | s2idle on Linux |

### Surface-specific devices (ACPI)

| HID | Windows name | Linux side |
|---|---|---|
| `MSHW0084` | Surface Serial Hub (SAM) | `surface_aggregator` — Type Cover, battery, sensors and performance modes go through it |
| `MSHW0040` | Surface Button | `surface_button` / `soc_button_array` |
| `MSHW0153` | Surface Hot Plug - KIP | `surface_hotplug`/KIP hub (Type Cover) |
| `MSHW0187`, `MSHW0188` x11 | Thermal Policy / Thermal Zone Sensor | SAM temperature sensors |
| `MSHW0299` | ACPI Platform Extension | Related to `surface_platform_profile` |
| `MSHW0343` | Surface Integration | unverified |
| `MSHW0005` | GPIO Laptop or Slate Indicator | Tablet mode switch |
| `ACPI000E` (`MSHW0217`) | ACPI Wake Alarm | |

### Firmware component versions

| Component | Version |
|---|---|
| Surface UEFI | 24.103.143.0 |
| Surface System Aggregator (SAM) | 6.603.139.0 |
| Surface ME | 1.42.2872.1 |
| Surface SMF | 93.0.0.49 |
| Surface PD Controller | 3.10.0.0 |
| Surface Retimer | 3.10.0.3 |
| Surface TCON | 161.33.3.35 |
| Touch G6 / Type Cover v7 / Pen | 5.65.139.0 |
| NVMe | (Samsung update driver 1.10.139.0) |

Firmware updates are only possible from Windows. Take this into account when deciding whether to keep Windows
after installing Linux.

## What to check before installing Linux

- **BitLocker / device encryption**: check the state of the Windows volume before changing partitions
  (`manage-bde -status C:`). On the test device device encryption was pending activation; the volume was
  decrypted with `manage-bde -off C:` and then reported `Fully Decrypted`.
- **TPM 2.0** (Intel PTT, `INTC` 600.18.1042.2872) is present. **Secure Boot was on** when this was read, before the installation
  (`Confirm-SecureBootUEFI` = True).
- **Firmware boot order**: Windows Boot Manager → Internal Storage → USB Storage → PXE (+ SurfaceFrontPage = the
  UEFI setup screen).
- **Turn off Fast Startup and hibernation** in Windows (`powercfg /h off`) so that NTFS is closed cleanly.
- **Secure Boot / Secure Core**: according to the wiki, the firmware does not attempt USB boot while Secure Core
  is on. In the UEFI setup, change Secure Boot to "Microsoft & 3rd party CA" or turn it off, and turn Secure Core
  off. linux-surface provides its own MOK signing key. On the test device both Secure Boot and Secure Core were
  turned off.
- The hypervisor (VBS) is on in Windows (the reason given for Hybrid Sleep in `powercfg /a`). It has no effect on
  Linux.

## Status of the Surface Pro 9 (Intel) in the linux-surface wiki

A summary in this repository's words; the wiki is the authority and changes over time.

Feature table: keyboard, touchpad, tablet mode, touch, pen and buttons are listed as working (linux-surface
kernel required); Wi-Fi, Bluetooth, speakers and sensors as working; suspend as working; hibernation as working
(conditionally); cameras as not working (only the rear OV13858 is marked as working in
[Camera-Support](https://github.com/linux-surface/linux-surface/wiki/Camera-Support)).

Known issues and workarounds (wiki page
[Surface-Pro-9](https://github.com/linux-surface/linux-surface/wiki/Surface-Pro-9)):

- Screen flicker: `i915.enable_psr=0`
- ACPI interrupt storm (closing the lid while charging, Thunderbolt hotplug): `pci=hpiosize=0`
- Unstable touch: `iptsd` tuning values (`ActivationThreshold = 24` and others)
- Power and volume buttons not working: put `pinctrl_tigerlake` into the initramfs (probe-order race with
  `soc_button_array`)
- Shutdown does not complete and the device stays on: solved by the linux-surface `surface-shutdown` patch (since
  6.1.12-2)
- Hibernation: `HibernateMode=reboot` may be needed

linux-surface patches that deal with the Surface Pro 9 directly (7.3 series, base `v7.3.4`): `0007-surface-sam`,
`0011-surface-shutdown`, `0012-surface-gpe`, `0013-cameras`. Touch is `0006-ithc` (`8086:51d0` = ADL-P THC port 1).

## What else can be obtained from Windows

- Information that needs administrator rights (BitLocker, TPM, `bcdedit /enum firmware`, `reagentc`, VBS) is
  collected by [`tools/windows/collect-admin.ps1`](../tools/windows/collect-admin.ps1). Its full output contains
  GUIDs and is not in the repository.
- ACPI tables: Windows exposes the DSDT and 11 SSDTs in the registry
  ([`tools/windows/dump-acpi.ps1`](../tools/windows/dump-acpi.ps1) dumps the tables, without MSDM). They can be
  compared with what `acpidump` returns on Linux. The dumps are not in this repository; dumps for this model are published at
  <https://github.com/linux-surface/acpidumps> (directory `surface_pro_9_intel`). Decompiling (`iasl -d`) is done
  on Linux, because winget has no ACPICA package.
- The audio codec pin configuration, the camera sensor tuning values and the touch firmware are inside the
  Windows drivers. They are not in this repository.
