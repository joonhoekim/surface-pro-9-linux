# Arch Linux

linux-surface's kernel from its repository, and this project's packages from its own signed pacman repository.
Before this: [prepare the device](install-prepare.md) and install Arch as usual. Overview, checks and reporting:
[install.md](install.md).

**Works; best effort.** Checked once on the device[^arch-run] ([results](test-results.md#arch-linux-packaged-route)).
Arch is a rolling release and `sp9-dkms` is built for exact linux-surface kernel releases, so after a kernel update
the device can run without this project's kernel patches unless the kernel is held
([section 4](#4-kernel-modules-sp9-dkms-and-secure-boot)).

[^arch-run]: From an SD card: linux-surface's `7.3.4.arch1-2`, mkinitcpio 42.2, systemd-boot, a unified kernel
    image, packages built locally, Secure Boot off. `sp9-cmdline`'s parameter reached the kernel and all 12 DKMS
    modules loaded (measured); booting, touch, pen, rotation, both cameras, the Type Cover, sound, suspend, Wi-Fi and
    Bluetooth worked when checked (observed). The packages also build and install in the official `archlinux`
    container ([install.md](install.md#what-was-verified)).

Where Arch differs from Debian and Ubuntu:

- the initramfs generator is chosen at install time;
- optional dependencies are not installed by themselves;
- the packages do not enable their units;
- `sp9-cmdline` works only with unified kernel images.

## 1. linux-surface's repository and kernel

Follow the linux-surface wiki, [Installation and Setup](https://github.com/linux-surface/linux-surface/wiki/Installation-and-Setup).
At the time of writing it comes down to the following; the wiki is authoritative. linux-surface's key has the
fingerprint `87DE FA4A B94A 99A4 C8C3 1125 56C4 64BA AC42 1453`.

```sh
curl -s https://raw.githubusercontent.com/linux-surface/linux-surface/master/pkg/keys/surface.asc \
    | sudo pacman-key --add -
sudo pacman-key --finger 56C464BAAC421453
sudo pacman-key --lsign-key 56C464BAAC421453
printf '\n[linux-surface]\nServer = https://pkg.surfacelinux.com/arch/\n' | sudo tee -a /etc/pacman.conf
sudo pacman -Syu mkinitcpio linux-surface linux-surface-headers \
    linux-firmware-intel wireless-regdb intel-ucode
```

- The kernel headers are needed: `sp9-dkms` builds against them.
- **The initramfs generator: mkinitcpio.** The kernel needs an initramfs provider, and pacman asks which one
  (mkinitcpio, dracut, booster) unless one is installed or named, as above. Take mkinitcpio: `sp9-platform`'s
  pacman hook rebuilds with mkinitcpio only, and `sp9-cmdline` works only through mkinitcpio's unified kernel
  images (section 3).
- mkinitcpio writes `/etc/mkinitcpio.d/linux-surface.preset` when the kernel is installed. As shipped it builds a
  plain initramfs image, with the unified kernel image line commented out; section 3 says when to change that.
- **Firmware.** `linux-firmware` is a meta package of twelve vendor packages; `linux-firmware-intel` alone carries
  every firmware file this device was seen loading.[^firmware-files] `wireless-regdb` provides `regulatory.db` for
  Wi-Fi, and `intel-ucode` the CPU microcode. `sof-firmware` is not needed: this device's sound binds
  `snd_hda_intel`, not SOF ([userspace/platform/README.md](../userspace/platform/README.md)).
- linux-surface's Arch kernel does not pull in `iptsd`, so nothing is replaced when `sp9-digitizerd` is installed
  (section 3).
- Reboot and check that `uname -r` contains `surface`.

[^firmware-files]: Wi-Fi `iwlwifi-so-a0-gf-a0-89` and its `.pnvm`, Bluetooth `ibt-0040-0041`, i915 `adlp_dmc`,
    `adlp_guc_70` and `tgl_huc`, the cameras' `ipu6ep_fw`.

## 2. This project's repository

**Not published yet.** The repository is published with the first release, so until then the key file and `sp9.db`
below do not exist on the website, and the commands of this section fail (HTTP 404).
Until then, build the packages yourself,[^build-locally] skip this section, and install the files in place of the
package names of sections 3 and 5: `sudo pacman -U sp9-*.pkg.tar.zst` (pacman resolves the distribution's
dependencies from the sync repositories).

[^build-locally]: [packaging/README.md](../packaging/README.md#building-locally); `sp9-dkms` and
    `sp9-dkms-bluetooth`: [dkms/README.md](../dkms/README.md#ci); `sp9-libcamera`:
    [packaging/libcamera/README.md](../packaging/libcamera/README.md).

The repository is signed with the key in [packaging/keyring/](../packaging/keyring/README.md); its fingerprint is
in [SECURITY.md](../SECURITY.md#package-signing). Compare it with what `gpg --show-keys` prints below before you trust
the key.

```sh
curl -fsSL https://joonhoekim.github.io/surface-pro-9-linux/packages/sp9-archive-keyring.asc -o /tmp/sp9.asc
gpg --show-keys /tmp/sp9.asc                       # compare the fingerprint with SECURITY.md
sudo pacman-key --add /tmp/sp9.asc
sudo pacman-key --lsign-key FINGERPRINT            # the fingerprint from SECURITY.md
printf '\n[sp9]\nServer = https://joonhoekim.github.io/surface-pro-9-linux/packages/arch/$arch\n' | sudo tee -a /etc/pacman.conf
sudo pacman -Syu sp9-keyring                       # key changes arrive as updates of this package
```

The repository holds the packages of the latest release. Every release also attaches them, with the source
tarball and a signed `SHA256SUMS`, to its [GitHub release](https://github.com/joonhoekim/surface-pro-9-linux/releases).

## 3. Install the packages

`sp9` depends on `sp9-platform`, `sp9-helper` and `sp9-digitizerd`; everything else is an optional dependency,
which pacman does not install by itself. Name what you want:

```sh
sudo pacman -S sp9 sp9-settings sp9-dkms sp9-dkms-bluetooth sp9-cmdline
sudo systemctl enable --now sp9-helperd sp9-digitizerd    # the packages do not enable them
```

`sp9-dkms-bluetooth` is needed for `sp9-ctl pen couple`; `sp9-cmdline` is recommended but needs the unified kernel
image setup below, and does nothing without it. `sp9-libcamera` is section 5. What each package is:
[install.md](install.md#the-packages).

To keep linux-surface's iptsd as the touch and pen daemon, install the packages one by one without `sp9` and
`sp9-digitizerd`, which `sp9` depends on: `sudo pacman -S iptsd sp9-platform sp9-helper sp9-settings sp9-dkms
sp9-cmdline`. linux-surface's iptsd has none of the pen patches of [userspace/iptsd](../userspace/iptsd/README.md);
`sp9-digitizerd` has that processing built in.

### Kernel command line (sp9-cmdline)

`sp9-cmdline` writes `/etc/cmdline.d/sp9-platform.conf` (the parameters of `/usr/share/sp9-platform/cmdline`, now
`pci=hpiosize=0`). mkinitcpio reads that directory only when it builds a unified kernel image (UKI). For the
parameters to reach the kernel:

1. In `/etc/mkinitcpio.d/linux-surface.preset`, comment out `default_image=` and uncomment `default_uki=` (and
   the same for `fallback_` lines, if the preset has them). The UKI must land in `EFI/Linux/` on the EFI system
   partition: adjust the `/efi` of the shipped path to where yours is mounted.
2. Put your own parameters, `root=…` and whatever else the boot loader passes today, into `/etc/kernel/cmdline` or
   another file in `/etc/cmdline.d/`. mkinitcpio 42 joins both. Without your own `root=` the UKI does not find its
   root file system.[^cmdline-fallback]
3. Use a boot loader that finds UKIs in `EFI/Linux/*.efi` by itself: systemd-boot does. The old entry in
   `/boot/loader/entries/` for the plain image does not get the parameters; remove it or leave it as a fallback.
4. Run `sudo mkinitcpio -P` after installing or removing `sp9-cmdline`, and after changing the files above.
   `sp9-cmdline` triggers no rebuild by itself,[^no-rebuild] so the next kernel update would be the first to pick
   it up.

[^cmdline-fallback]: Only when neither exists does mkinitcpio embed the running system's `/proc/cmdline`, which
    is wrong when the image is built from a chroot (an installation medium's command line), and once
    `sp9-cmdline`'s file is there, that fallback no longer happens.

[^no-rebuild]: Read from the package: its install script only prints a note, and `sp9-platform`'s pacman hook
    watches only its mkinitcpio drop-in.

With GRUB, or systemd-boot entries for the plain image, `sp9-cmdline` does nothing. Leave it out and add the
parameters of `/usr/share/sp9-platform/cmdline` by hand: GRUB, `GRUB_CMDLINE_LINUX_DEFAULT` in `/etc/default/grub`,
then `sudo grub-mkconfig -o /boot/grub/grub.cfg`; systemd-boot, the `options` line of the entry in
`/boot/loader/entries/`. Other boot loaders: [userspace/platform/README.md](../userspace/platform/README.md).

### PSR

linux-surface's kernel enables PSR2 on this panel, which Windows keeps off. `sp9-platform` limits it to PSR1 through
i915 debugfs at boot (`sp9-psr1.service`), which needs a writable debugfs. Kernel lockdown makes it read-only; the
unit then only logs that. In that case add `i915.enable_psr=1` to the kernel command line by hand; it taints the
kernel (flag 64) ([userspace/platform/README.md](../userspace/platform/README.md)).

### Initramfs

`sp9-platform` installs a mkinitcpio drop-in (`/etc/mkinitcpio.conf.d/sp9-platform.conf`, `pinctrl_tigerlake`
for the power and volume buttons) and a pacman hook that runs `mkinitcpio -P` when the drop-in changes. On the
first installation mkinitcpio therefore runs twice, once from dkms's hook and once from this one; that is
harmless. In linux-surface's Arch kernel `pinctrl_tigerlake` is built in, so the drop-in changes nothing there.

The image holds no `surface_aggregator` and none of the DKMS modules,[^image-read] so the Type Cover keyboard does
not work until the root file system is mounted (inferred; not tried). That matters only with disk encryption
([install-prepare.md](install-prepare.md#3-disk-encryption)): add the modules the linux-surface wiki lists to
`MODULES=` in a file under `/etc/mkinitcpio.conf.d/`, run `sudo mkinitcpio -P`, and keep a USB keyboard at hand;
whether that list is enough is unverified.

[^image-read]: Read from the image on the device.

### Check

Reboot once: the kernel command line, the initramfs, PSR and the kernel modules take effect at boot. Then:

```sh
sp9-ctl packages                                   # versions, the sp9-dkms modules of the running kernel, the update command
sudo /usr/lib/sp9-helper/post-install-check.sh     # report in ~/sp9-check-<host>/
```

What the check reports, and what to expect first: [install.md](install.md#check-the-installation).

## 4. Kernel modules (sp9-dkms) and Secure Boot

`sp9-dkms` carries this project's kernel patches as DKMS modules for linux-surface's kernel, and
`sp9-dkms-bluetooth` the Bluetooth core with patches 0023 and 0024 (the pen's cover coupling); which patches
ship and why is in [dkms/README.md](../dkms/README.md). DKMS builds them for every installed linux-surface
kernel with its headers (`linux-surface-headers`), now and when a kernel arrives later (dkms's pacman hook;
[installing](../dkms/README.md#installing)). Arch's `dkms` depends on `gcc` and `make`, so nothing else is needed
for the build, and it is in `/usr/bin` (on Arch `/usr/sbin` is a link to it), so `dkms status` works as a normal
user.

- **A new linux-surface kernel release runs the stock modules until `sp9-dkms` is updated for it.** Each
  `sp9-dkms` release covers the linux-surface kernel releases it was generated and tested against
  (`cat /usr/src/sp9-*/kernels`); for any other release DKMS skips the build (it does not fail) and the kernel's own
  modules load, without this project's camera, USB-C, Type Cover, touch and pen-coupling fixes
  ([kernel updates](install.md#kernel-updates)). An `sp9-dkms` update that covers the new release comes through the
  repository when there is one; there is no schedule for it. `sp9-ctl packages` shows the state as "not covered".
  pacman replaces `linux-surface` in place, so the previous kernel is not kept to fall back to. To keep a covered
  kernel, hold linux-surface's packages in `/etc/pacman.conf` and remove the line once `sp9-dkms` lists the new
  release:

  ```ini
  # /etc/pacman.conf, section [options]
  IgnorePkg = linux-surface linux-surface-headers
  ```

  `pacman -Syu` then prints "ignoring package upgrade" for them. Arch does not support partial upgrades in general;
  holding the kernel together with its headers is the usual exception.
- **Secure Boot.** Arch's linux-surface kernel loads unsigned modules, so no key has to be enrolled for them
  ([Secure Boot](../dkms/README.md#arch) has what dkms does with its own key there).

`sp9-ctl packages` and `sp9-dkms-status` show per module whether the DKMS build, the kernel's own copy or nothing
is loaded, and name a failed build with its log.

## 5. Cameras (experimental)

`sp9-libcamera` installs libcamera 0.8.0 with the patches of [userspace/libcamera](../userspace/libcamera/README.md)
into its own prefix, with a PipeWire libcamera plugin built against it, and makes WirePlumber load that plugin instead
of the distribution's. Nothing of the distribution is replaced. It is opt-in, and nothing pulls it in:

```sh
sudo pacman -S sp9-libcamera
systemctl --user restart wireplumber
```

Without the package the cameras go through Arch's libcamera (0.7). With 0.7.2 (measured) the processed image has
magenta lines every 480 pixels, exposure and white balance follow the top-left corner of the frame, the exposure
oscillates after a start, the rear camera has no autofocus, and the rear lens and sensor stay powered while
WirePlumber runs. What the package changes, how to turn it off without removing it, and its risks are in
[packaging/libcamera/README.md](../packaging/libcamera/README.md).[^libcamera-risk]

[^libcamera-risk]: It is tied to the PipeWire version it was built for, so a PipeWire update to the next series
    waits for a rebuilt package.

## 6. Updates

Updates come through the package manager, together with the distribution's:

```sh
sudo pacman -Syu
```

- A new linux-surface kernel arrives from linux-surface's repository. Until an `sp9-dkms` update covers it, DKMS
  skips it and the kernel's own modules load (section 4); `sp9-ctl packages` says "not covered".
- `sp9-ctl packages` (also part of `sp9-ctl status`) lists the installed versions and prints the update command. The
  helper only reads the package database and DKMS's state; it never downloads or installs anything.
- Optional: `sudo systemctl enable --now sp9-update-check.timer` checks once a day whether the
  repositories have newer `sp9` packages and leaves a notice that `sp9-ctl packages` shows ("ReleaseNotice"): no
  root code, no partial upgrade, nothing installed.[^update-check]
- Changes of the repository key (a renewed signing subkey) arrive as updates of `sp9-keyring`, well before the old
  one expires.

[^update-check]: It runs `checkupdates` (packages `pacman-contrib` and `fakeroot`) as an unprivileged dynamic user
    against a private copy of the sync databases.

## 7. Remove everything

`pacman -R` keeps `.pacsave` copies of changed files in `/etc` and does not disable units:

```sh
sudo systemctl disable --now sp9-helperd sp9-digitizerd
sudo pacman -Rns $(pacman -Qq | grep '^sp9')
sudo rm -rf /etc/sp9 /var/lib/sp9 /etc/iptsd.d/zz-sp9-helper.conf \
    /var/lib/private/sp9-update-check /var/cache/private/sp9-update-check   # the last two: the release check
sudo pacman-key --delete FINGERPRINT               # then remove the [sp9] section from /etc/pacman.conf
sudo mkinitcpio -P                                 # if sp9-cmdline was installed: UKIs without its parameters
```

Removing `sp9-dkms` (and `sp9-dkms-bluetooth`) removes its modules from every kernel. Parameters you added to
another boot loader by hand stay. linux-surface's kernel and repository are removed as its wiki says; keep another
kernel installed, and boot it, before you remove that one.

## 8. What the packages do not cover

- Firmware and microcode: `linux-firmware-intel`, `wireless-regdb` and `intel-ucode` (section 1).
- The opt-in files of `userspace/platform` (codec pin patch, legacy audio, reboot after hibernation): installed under
  `/usr/share/sp9-platform/optional/` and copied by hand when needed; none is needed with the current firmware.
- `i915.enable_psr=1` where debugfs is locked down (section 3), and the kernel command line with a boot loader other
  than a UKI one (section 3).
- The tools the post-install check calls (the `diagnostics.enable` row in
  [install-from-source.md](install-from-source.md#3-settings-the-nixos-module-makes-that-you-must-reproduce-by-hand);
  `sp9-helper`'s optional dependencies name the Arch packages): install those you want.
- The pen prototypes of [userspace/pen](../userspace/pen/README.md): the helper does what they did.

## 9. Verify

```sh
uname -r                                           # contains "surface"
cat /proc/cmdline                                  # contains pci=hpiosize=0 when sp9-cmdline took effect
sp9-ctl packages
systemctl status sp9-helperd sp9-digitizerd sp9-psr1
sudo cat /sys/kernel/debug/dri/0000:00:02.0/i915_edp_psr_status    # PSR mode: PSR1 enabled
```

More checks and the post-install check: [install.md](install.md#check-the-installation). Hardware expectations are
in [hardware-status.md](hardware-status.md). A test report is welcome ([install.md](install.md#report-what-you-see)).
