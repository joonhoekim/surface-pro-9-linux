# Security policy

## Reporting a vulnerability

Report security problems privately through GitHub's
[private vulnerability reporting](https://github.com/joonhoekim/surface-pro-9-linux/security/advisories/new) (Security, Report a vulnerability), not in a public
issue. Say what is affected (file and commit), how to reproduce it, and what an attacker gains. This is a one-person
project: expect an answer within about two weeks. Fixes land on `main`; there are no release branches.

## What runs with privileges

This is the attack surface the project adds to a system. Reports about any of it are in scope.

- **`sp9-helperd`** (`helper/`), the device helper. It runs as root and owns the system bus name
  `io.github.joonhoekim.OpenDeviceHelper1`. The bus policy lets any local user call it; every method that changes
  something asks polkit for one of the 13 actions `io.github.joonhoekim.opendevicehelper.sp9.*`, with the caller's
  bus name as the subject. Changing the power limits per profile, the skin temperature guard, smart charging, the
  panel ICC profile, the keyboard backlight, touch and pen settings and running diagnostics are allowed without a
  password to the user of the active local session; reloading the configuration, the fan request, the panel self
  refresh policy and pen coupling need an administrator (`auth_admin_keep`); inactive and remote sessions get
  nothing. A way to reach a changing method without the authorization its action asks for is a vulnerability.
  - It writes to `/sys` (RAPL power limits, EPP, `platform_profile`, input inhibit, debugfs for PSR),
    `/etc/sp9`, `/var/lib/sp9`, `/etc/iptsd.d` and `/run/lock`. Its unit has `ProtectSystem=strict` with those
    paths writable, `ProtectHome` and `PrivateTmp`, and no capability or device restrictions: it opens
    `/dev/surface/aggregator`, hidraw nodes, Bluetooth management and raw HCI sockets, talks to BlueZ over D-Bus and
    reads EFI variables.
  - Diagnostics run `tools/linux/post-install-check.sh` (bash) as root, and the ICC profile runs
    `tools/sdc-blob.py` (python3) as root, both from the helper's data directory; the diagnostics archive is
    handed to the caller. The check masks MAC addresses, UUIDs and serial lines by pattern, which is a design
    limit: an identifier in an unexpected format can pass. A leak of something the check is meant to mask is in
    scope.
  - `--no-polkit`, `--session` and `--fake` are for tests on a private bus. A packaged unit that starts the
    daemon with `--no-polkit` is a bug.
- **`sp9-digitizerd`** (`helper/crates/sp9-digitizerd/`), the touch and pen daemon. It runs as root, reads the
  digitizer's hidraw node and creates the virtual touch and pen devices through `/dev/uinput`. Its system bus name
  `io.github.joonhoekim.Sp9Digitizer1` lets anyone read properties, introspect and call `Diagnostics.GetConfig`;
  only root may call the setters (the bus policy says so and each setter checks the caller's uid again), so users
  change its settings through `sp9-helperd` and polkit. Its unit is hardened (no capabilities,
  `DevicePolicy=closed` with only `/dev/uinput` and hidraw, no network, `SystemCallFilter=@system-service`).
  Input injected into the session, or a crash on crafted digitizer reports, is in scope.
- **D-Bus and polkit policies** (`helper/data/*.conf`, `helper/data/*.policy`), as installed by
  `helper/install.sh` or the NixOS module.
- **systemd units**: `sp9-helperd.service`, `sp9-digitizerd.service`, and the pen tool units in
  `userspace/pen/systemd/`, which run as root with `ProtectSystem=strict`.
- **udev rules**: `userspace/platform/udev/rules.d/` has one rule, which writes a sensor attribute of the
  accelerometer and grants no device access. A rule that gives users access to a device is in scope.
- **Install scripts**: `userspace/platform/install.sh` and `helper/install.sh` run as root and write to `/etc` and
  `/usr/local`, including the kernel command line and the initramfs configuration. Unsafe handling of paths or temporary files there is in scope.
- **The NixOS module** (`nix/`), for what it installs and enables.

The kernel patches change drivers that talk to the device's firmware (the Surface Aggregator Module, the touch
controller, the cameras' power control, Bluetooth pairing). A memory-safety or privilege problem in one of this
repository's patches is in scope; for the linux-surface series and upstream code, see below.

## Out of scope

The upstream Linux kernel, the linux-surface patch series and packages, iptsd and libcamera apart from this
repository's patches, the device's firmware (UEFI, SAM, touch, PD controller), and distributions' own packages.
Report those to their maintainers. Someone with physical access to an unlocked device is outside what this project
can protect against.
