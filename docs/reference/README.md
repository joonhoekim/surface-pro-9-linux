# Reference data

Device lists and version summaries read on Windows from one Surface Pro 9 (Intel, SKU `Surface_Pro_9_2038`,
i5-1235U, 16 GB, Surface UEFI 24.103.143). They are the raw material behind [../hardware.md](../hardware.md):
hardware IDs to match Linux drivers against, and the driver and firmware versions the research documents refer to.

The files are tool output, kept as the tools printed it (UTF-8 with a byte order mark where PowerShell wrote one).
Entries that describe one unit or its surroundings rather than the model were removed: the collection time, link
state, Secure Boot and BitLocker state, attached storage, paired displays and their audio endpoints. Device names
that Windows printed in the system language were replaced with the English names Windows uses for the same devices.
In the Type Cover files the per-device part of the instance path is replaced with the product ID in angle
brackets (`4&<09AF>&0&0001`), and the Slim Pen 2 address and the cover's instance suffix are masked by the tools
(`<addr>`, `<cover>`).

| File | Content | Produced by |
|---|---|---|
| [system.txt](system.txt) | Manufacturer, model, SKU, memory size, UEFI version and date, Windows build | [`tools/windows/collect-hwinfo.ps1`](../../tools/windows/collect-hwinfo.ps1) |
| [cpu.txt](cpu.txt) | Processor name, core and thread count, cache sizes | `collect-hwinfo.ps1` |
| [memory.txt](memory.txt) | Memory modules as SMBIOS reports them: part number, size, speed | `collect-hwinfo.ps1` |
| [display.txt](display.txt) | Graphics adapter, driver version, panel resolution and refresh rates | `collect-hwinfo.ps1` |
| [monitor.txt](monitor.txt) | Internal panel maker and product code from the EDID (no serial) | `collect-hwinfo.ps1` |
| [network.txt](network.txt) | Wi-Fi adapter and driver version | `collect-hwinfo.ps1` |
| [pnp-devices.csv](pnp-devices.csv) | Every present PnP device: class, name, service, driver version, INF, hardware and compatible IDs | `collect-hwinfo.ps1` |
| [platform-devices.txt](platform-devices.txt) | The ACPI, HID and PCI devices of the platform with class, service and first hardware ID | `collect-hwinfo.ps1` |
| [acpi-devices.txt](acpi-devices.txt) | ACPI devices by `_HID`, with the Surface subsystem ID and instance count | `collect-hwinfo.ps1` |
| [pci-devices.txt](pci-devices.txt) | PCI devices with vendor, device, subsystem and revision | `collect-hwinfo.ps1` |
| [usb-devices.txt](usb-devices.txt) | Built-in USB devices (the Bluetooth controller) | `collect-hwinfo.ps1` |
| [firmware-components.txt](firmware-components.txt) | Firmware components Windows Update services, with versions and resource IDs | `collect-hwinfo.ps1` |
| [driver-packages.txt](driver-packages.txt) | Third-party driver packages per device: INF, version, driver store folder, file names and sizes. The packages themselves are not in this repository | [`tools/windows/driver-map.ps1`](../../tools/windows/driver-map.ps1) |
| [typecover/devices.txt](typecover/devices.txt) | Type Cover (KIP) PnP devices: class, service, filters, INF and version | [`tools/windows/typecover-probe.ps1`](../../tools/windows/typecover-probe.ps1) |
| [typecover/layout.txt](typecover/layout.txt) | HID capabilities of every Type Cover collection: link collections, usages, ranges, bit position of each field | `typecover-probe.ps1` |
| [typecover/features.txt](typecover/features.txt) | Result of a GET on every Type Cover feature report, decoded per usage | `typecover-probe.ps1` |
| [typecover/ptp-registry.txt](typecover/ptp-registry.txt) | Precision Touchpad registry values: the machine defaults and one user's settings | `typecover-probe.ps1` |
| [firmware-hashes.tsv](firmware-hashes.tsv) | sha256 and size of the firmware-derived files this repository does not publish (ACPI tables, VBT, EDID, ICC profiles), for comparing your own dumps. [`tools/linux/post-install-check.sh`](../../tools/linux/post-install-check.sh) reads it | `sha256sum` over the output of [`tools/windows/dump-acpi.ps1`](../../tools/windows/dump-acpi.ps1), [`tools/vbt-extract.py`](../../tools/vbt-extract.py) and the Windows EDID and colour profile files |

To produce the same files from your own device, run the tools on Windows; they write to `hw/windows/` (not
tracked) and put everything unit-specific under `private/`. For the ACPI tables themselves, see
<https://github.com/linux-surface/acpidumps> (directory `surface_pro_9_intel`).
