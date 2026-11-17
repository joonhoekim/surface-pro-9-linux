# kernel/

A patch series on top of a pinned base. There is no kernel fork. Fetch the base, apply the linux-surface series and
then `patches/` in order with `git am`, and build.
Origin, author, status and evidence of each patch are in [PROVENANCE.md](PROVENANCE.md).

## Base

| Layer | Value |
|---|---|
| Upstream | stable `v7.3.4` (`7595a62d45ee1a2297eafa5f2ef5d0a45142d84d`) |
| linux-surface | `patches/7.3/0001`–`0015` of [linux-surface/linux-surface](https://github.com/linux-surface/linux-surface) at `ea852b4eff5ed641571e13766bda5bd9cf121983` (47 commits, `patches/7.3/BASE` = `v7.3.4`) |
| This repository | `patches/0001`–`0023` |

The linux-surface series is not copied but pinned by commit: it is a public git repository, so the commit hash
alone reproduces it exactly, and copying 15 files would only add provenance tracking and update work.

## Making the tree

```sh
git clone --depth 1 -b v7.3.4 https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git linux-sp9
git clone https://github.com/linux-surface/linux-surface.git
git -C linux-surface checkout ea852b4eff5ed641571e13766bda5bd9cf121983
cd linux-sp9
git am ../linux-surface/patches/7.3/*.patch                    # 15 files, 47 commits (whitespace warnings only)
git am /path/to/surface-pro-9-linux/kernel/patches/*.patch     # 0001-0023, in order
```

In exactly this order (v7.3.4 → 47 linux-surface commits → 0001–0017) everything has been confirmed to apply
cleanly without 3-way merging. 0018–0022 were exported from commits on the same tree as the booted kernel
(`tools/linux/kmod-dev.sh patch`). 0023 comes from the same module development tree, where it was built as
`bluetooth.ko` and loaded into the booted kernel.

Build and boot are confirmed through the Nix path: the Surface Pro 9 boots the kernel (`uname -r` = `7.3.4`) that
[`nix/kernel.nix`](../nix/kernel.nix) builds by applying the 15 linux-surface files and 0001–0022 (37 files in
all) with `patch -p1` to the same tarball. The measurements on the device per patch are in PROVENANCE.md. For
0022 the creation of the rear lens device was checked on one boot only (there is no confirmation across several
boots). 0023 is in the series (38 files with it) but no kernel built with it had been booted at the time of
writing: it was exercised on the device as a module loaded into that 7.3.4 kernel. Results on the device per
function are in [docs/research/gaps](../docs/research/gaps/README.md) and
[docs/test-results.md](../docs/test-results.md).
No build has been run to completion through the `git am` tree or the `build-kernel.sh` (.deb) path described
below.

## Configuration

`config/sp9.config` is merged after `configs/surface-7.3.config` of linux-surface. It contains only the options
the series uses that are missing from the linux-surface fragment (IPU6, OV13858, Type-C UCSI and
`UCSI_SURFACE_HID`, the LED class and `LEDS_BRIGHTNESS_HW_CHANGED`).
Start from a distribution configuration (`x86_64_defconfig` lacks most laptop devices).
In the running kernel of the Nix build all 41 options of the linux-surface fragment and all 8 options of
`sp9.config` are in effect (compared against `/proc/config.gz`).

```sh
cp /boot/config-"$(uname -r)" .config        # or the kernel configuration of the distribution to install on
scripts/kconfig/merge_config.sh -m .config \
    ../linux-surface/configs/surface-7.3.config \
    /path/to/surface-pro-9-linux/kernel/config/sp9.config
scripts/config --set-str LOCALVERSION -sp9 \
    --set-str SYSTEM_TRUSTED_KEYS "" --set-str SYSTEM_REVOCATION_KEYS ""   # for a Debian/Ubuntu configuration
make olddefconfig
make -j"$(nproc)" bzImage modules
sudo make modules_install install
```

## Build

[`tools/linux/build-kernel.sh`](../tools/linux/build-kernel.sh) does exactly what the two sections above
describe. Run it on an x86-64 Linux host. The results go to `out/kernel/dist/` (with SHA256SUMS).

```sh
tools/linux/build-kernel.sh                  # linux-image, linux-headers .deb (make bindeb-pkg)
tools/linux/build-kernel.sh --tarball        # .tar.zst of boot/ + lib/modules/ for other distributions (make dir-pkg)
tools/linux/build-kernel.sh --prepare-only   # fetch, apply the patches, verify the configuration, then stop
```

- The pins (the `v7.3.4` hash, the linux-surface commit) are in the script, and it stops when they disagree with
  this document. It stops when any `git am` fails or when the number of applied commits differs from the number
  of commits in the patch files. The source tree is rebuilt only when the patches change. `--clean` removes only
  the build tree and the results.
- The starting configuration is the Ubuntu mainline configuration that linux-surface uses for its Debian package
  (`pkg/debian/kernel/config.amd64`). On top of it `ubuntu.config` (debug info and module compression off, trusted
  key lists emptied), `surface-7.3.config`, `sp9.config` and the build settings (LOCALVERSION, Rust off) are
  merged with `merge_config.sh`. If any option requested by a fragment has a different value after
  `olddefconfig`, the script prints the list and stops. `--base-config /boot/config-…` starts from the
  configuration of the distribution to install on instead.
- The release is `7.3.4-sp9`, the package version `7.3.4-<--revision>`. `linux-libc-dev` is discarded so that it
  does not replace the distribution's. ccache is used when present.

### Signing

With Secure Boot on, shim and GRUB refuse an unsigned kernel. Only linux-surface holds the private key of the
linux-surface MOK, so enrol one more MOK of your own. The key is not kept in the repository: the script refuses a
key path inside the repository directory, for `--make-key`, `--sign-key` and `KBUILD_SIGN_KEY` alike. The one
exception is the git-ignored `private/` directory of a working copy.

```sh
tools/linux/build-kernel.sh --make-key        # ~/.local/share/sp9-kernel/mok/MOK.key (0600), MOK.crt, MOK.der
sudo mokutil --import ~/.local/share/sp9-kernel/mok/MOK.der   # one-time password → reboot → MokManager: Enroll MOK → Continue → Yes → password
KBUILD_SIGN_KEY=~/.local/share/sp9-kernel/mok/MOK.key KBUILD_SIGN_CERT=~/.local/share/sp9-kernel/mok/MOK.crt \
    tools/linux/build-kernel.sh               # or --sign-key, --sign-cert
```

When a key is given, vmlinuz is signed with `sbsign` and put back into the .deb (or the one inside the tarball is
signed), and the modules are signed with the same key (`MODULE_SIG_KEY`; the certificate is built into the
kernel).
Without a key, the modules are signed with a temporary key generated per build; that key is also built into the
kernel, so the modules load under lockdown. A .deb built without signing is signed after installation with
`sudo sbsign --key MOK.key --cert MOK.crt --output /boot/vmlinuz-7.3.4-sp9 /boot/vmlinuz-7.3.4-sp9` (every time
the package is reinstalled).
The machine-keyring revert patch that the linux-surface Debian package carries
(`pkg/debian/kernel/0001-Partially-revert-integrity-…`) is not included, so DKMS modules signed with a MOK may
fail to load under Secure Boot. The Surface Pro 9 needs no DKMS module.

### Installing on the Surface Pro 9, and reverting

This path has not been run. On Ubuntu with the linux-surface kernel already installed:

```sh
sudo apt install ./linux-image-7.3.4-sp9_*.deb ./linux-headers-7.3.4-sp9_*.deb   # the package hooks create the initramfs and the GRUB entry
```

GRUB picks the kernel with the highest version as the default, so `7.3.4-sp9` comes before `7.3.x-N-surface`. At
first, pin the linux-surface kernel as the default and boot sp9 only once.
Whether the Type Cover keyboard works in the GRUB menu is not confirmed, so this method does not depend on operating
the menu:

```sh
sudo sed -i 's/^GRUB_DEFAULT=.*/GRUB_DEFAULT=saved/' /etc/default/grub && sudo update-grub
grep -o "menuentry '[^']*'" /boot/grub/grub.cfg                                 # check the entry names
sudo grub-set-default "Advanced options for Ubuntu>Ubuntu, with Linux <linux-surface release>"
sudo grub-reboot "Advanced options for Ubuntu>Ubuntu, with Linux 7.3.4-sp9"     # for the next boot only
sudo reboot
```

If the sp9 kernel hangs, hold the power button for 10 seconds to turn the device off; turning it on again boots
the linux-surface kernel. To remove: `sudo apt remove linux-image-7.3.4-sp9 linux-headers-7.3.4-sp9`.
On other distributions, unpack the `--tarball` result with `sudo tar -xf … -C /`, run `sudo depmod 7.3.4-sp9`, and
create the initramfs and the boot loader entry with the distribution's tools
(`kernel-install add 7.3.4-sp9 /boot/vmlinuz-7.3.4-sp9` or similar).

## Patches by area

| Area | Patches | Content |
|---|---|---|
| Camera: front OV5693 | 0001–0006 | Upstream (v7.4) backports. ipu-bridge adds `clock-noncontinuous` to the OV5693 endpoint on IPU6 (TGL, ADL-P, ADL-N), and ov5693 then sets bit 5 of `MIPI_CTRL00` (0x4800) before stream-on. 0004 is adjusted to the position of the `OVTI5693` entry in linux-surface |
| Camera: rear autofocus | 0011 | Maps SSDB VCM type 0x11 to `dw9800k`. The table is indexed directly by the SSDB value and empty slots are treated as unknown types |
| Camera: sensor clock | 0015 | Sets the frequency-select argument of the INT3472 clock `_DSM` from the SSDB mclk (24 MHz → 0, anything else → 1). For the IR sensor (24 MHz); no change for front and rear |
| USB-C, charger | 0007–0010 | linux-surface/kernel#184: a UCSI driver over SAM HID (`ucsi_surface_hid`), a return-value fix in `surface_charger` and the adapter rating (`input_power_limit`) |
| SAM: RTC | 0012 | Adds `ssam:01:01:01:00:00` (rtc-surface) to `ssam_node_group_sp9`. It also applies to SP10, SP11 and SP12 (Intel), which use this group |
| SAM: fan | 0013 | Sets the minimum fan RPM (FAN 0x0b) to 0 at probe and resume of `surface_platform_profile` |
| Display | 0014 | i915 `quirk_disable_psr2` for `46a8:1414:0063`. PSR1 stays |
| Type Cover keys | 0016 | In linux-surface `hid-surface`, does not map Fn, Fn lock and the lock indicator (0001:0097–0099) (so that `BTN_1` and `BTN_2` do not leak), and maps Call Mute Toggle (0001:00E1) to `KEY_MICMUTE` |
| Touch: ithc | 0018 | Hands a Feature response to the waiting request only when its first byte is the requested report ID. A response that does not match, or that arrives with no request pending, is dropped (otherwise it goes to `hid_input_report`, which logs `-22`, or the next GET receives the wrong data) |
| USB-C: PDO | 0019 | `ucsi_surface_hid` strips the length byte that SAM prepends to GET_PDOS responses and corrects the length. Partner PDOs are cut to 3 |
| Camera: front binning | 0020 | On a `clock-noncontinuous` endpoint ov5693 does not offer the binned 1296x972 mode. Requests of 720p and below go to the full mode |
| Camera: rear lens power | 0021 | dw9719 powers up on the first `focus_absolute` write, not on open. A lens node that is merely held open no longer keeps the rear sensor awake |
| Camera: rear VCM creation | 0022 | The ov13858 probe enables runtime PM before it registers the subdev. This prevents boots without a lens device, which happen when the VCM creation work fails with -EACCES while waking the sensor |
| Bluetooth: LE legacy OOB pairing | 0023 | LE legacy pairing with a Temporary Key supplied out of band: the 16 B given as `hash192` in the extended form of Add Remote OOB Data are used as the TK (the Slim Pen 2's coupling through the Type Cover's pen slot). In that pairing the host also offers its identity key; a pen that did not receive it does not advertise for reconnection. Exercised on the device as a module: cover coupling with LTK and IRK stored, and reconnection after the pen's idle disconnect. The responder path and the path with Secure Connections OOB data are compile-tested only |
| Type Cover backlight | 0017 | `surface::kbd_backlight` LED in `hid-surface`. The levels 0/3/6/12 of Feature 213 (0x517) map to 0–3, writes go through Output 213, reads through Feature 214. When the backlight key (0x515) arrives, Feature 214 is read again and reported through `brightness_hw_changed` |

## What was left out

- Restricting INT3472 GPIO type 0x08 to `OVTID858`: the OV13858 patch of linux-surface requests the non-standard
  name `pwr1`, so changing only the map would require fixing that patch as well. On the device this rail registers
  as `pwr1` and the rear sensor delivers frames. Not urgent, deferred.
- The upstream Surface Pro 9 rotation quirk (`db44b1792898`) and the OV13858 sensor configuration
  (`2ee01a094aba`): linux-surface `0013-cameras.patch` already does the same.
- A VD55G0 (IR) driver: its status registers, firmware patch and exposure register block differ from `vd55g1`,
  so adding a model ID is not enough ([camera.md](../docs/research/gaps/camera.md), the section on VD55G0 and
  vd55g1).
- The OV5693 binned mode: the link frequency 358.4 MHz has been derived, but window placement, flip and Bayer
  phase have to be settled on the device, so it is on hold ([camera.md](../docs/research/gaps/camera.md), the
  section on the binned mode's link frequency). With flip on, this mode yields no frames (`Frame sync error`),
  which is why patch 0020 does not offer it.
- An rtc-surface wake alarm, a notification driver for `MSHW0299`, turning PSR off on AC power: the evidence
  (payloads, register values) is insufficient, or it is not a job for a kernel patch. Each is covered in
  [docs/research/gaps](../docs/research/gaps/README.md).
