# Debian and Ubuntu

linux-surface's kernel from its repository, and this project's packages from its own signed apt repository. Before
this: [prepare the device](install-prepare.md) and install the distribution with its own installer. Overview,
checks and reporting: [install.md](install.md).

| Release | Support | Tested |
|---|---|---|
| Ubuntu 26.04 | Supported | Once on the device[^run] ([results](test-results.md#ubuntu-2604-packaged-route)) |
| Debian 13 | Supported | Once on the device[^run]; cameras need trixie-backports ([results](test-results.md#debian-13-packaged-route)) |
| Ubuntu 24.04 | Works without cameras; not a target | Once on the device[^run]; no cameras, no speaker protection filter ([results](test-results.md#ubuntu-2404-packaged-route)) |
| Ubuntu 26.10, Debian testing | Best effort | Containers only |

Notes for one release are marked **Ubuntu** or **Debian 13**.

[^run]: From an SD card, Secure Boot off, no MOK enrolled, packages built locally. All 12 DKMS modules loaded
    and the kernel command line, PSR1 and initramfs drop-ins took effect (measured); booting, touch, pen, rotation,
    the cameras (not on 24.04), the Type Cover, sound, suspend, pen coupling, Wi-Fi and Bluetooth worked when
    checked (observed). Unverified on every release: Secure Boot with the DKMS key enrolled through MokManager,
    the signed repository, and moving to a new linux-surface kernel release.

## 1. linux-surface's repository and kernel

Follow the linux-surface wiki, [Installation and Setup](https://github.com/linux-surface/linux-surface/wiki/Installation-and-Setup),
for your distribution. At the time of writing it comes down to the following; the wiki is authoritative.
linux-surface's key has the fingerprint `87DE FA4A B94A 99A4 C8C3 1125 56C4 64BA AC42 1453`. A minimal Debian
installation needs `sudo apt install wget gpg` first.

```sh
wget -qO - https://raw.githubusercontent.com/linux-surface/linux-surface/master/pkg/keys/surface.asc \
    | gpg --dearmor | sudo dd of=/etc/apt/trusted.gpg.d/linux-surface.gpg
echo "deb [arch=amd64] https://pkg.surfacelinux.com/debian release main" \
    | sudo tee /etc/apt/sources.list.d/linux-surface.list
sudo apt update
sudo apt install linux-surface linux-headers-surface
sudo apt install linux-surface-secureboot-mok      # with Secure Boot: enrol linux-surface's key (password "surface")
sudo update-grub
```

- The kernel headers are needed: `sp9-dkms` builds against them.
- **Ubuntu:** with recommends on (apt's default), installing `sp9` or `sp9-dkms` also installs a distribution
  kernel's headers, which nothing here needs.[^dkms-headers] To avoid it, install dkms on its own first,
  `sudo apt install --no-install-recommends dkms`, and the packages of section 3 after it. If they came in already,
  list the header packages that are not linux-surface's, `dpkg -l 'linux-headers-*' | grep '^ii' | grep -v surface`,[^headers-example]
  remove them by name with `sudo apt remove`, then `sudo apt autoremove`.
- **Debian 13:** do not use that workaround: install dkms with recommends (`sudo apt install dkms`, or let `sp9`
  pull it in).[^debian-dkms] With `--no-install-recommends` there is no compiler, and the `sp9-dkms` build fails
  (`gcc: not found`, dkms exit status 10). To recover: `sudo apt install gcc fakeroot && sudo dpkg --configure -a`.
- **Debian** has no `linux-firmware` package: the firmware for Wi-Fi, Bluetooth, i915 and the cameras comes from the
  `non-free-firmware` component (section 8).
- `linux-image-surface` recommends `iptsd`, so with recommends on apt installs it here. `sp9` depends on
  `sp9-digitizerd`, which conflicts with iptsd: installing `sp9` (section 3) removes it again. To keep iptsd
  instead, see the end of section 3.
- Do not install `libwacom-surface` on Ubuntu 26.04 or Debian testing: the wiki warns of a bug there
  ([linux-surface#2076](https://github.com/linux-surface/linux-surface/issues/2076)).
- Reboot and check that `uname -r` contains `surface`.

[^dkms-headers]: dkms recommends `linux-headers-generic | linux-headers-amd64 | linux-headers`, and
    `linux-headers-surface` provides none of them. Seen on Ubuntu 26.04 and 24.04.

[^headers-example]: For example `linux-headers-generic` and `linux-headers-6.8.0-NN-generic` on 24.04.

[^debian-dkms]: Debian's dkms recommends no kernel headers, only the compiler (`gcc | c-compiler`), `fakeroot`
    and `sudo`.

## 2. This project's repository

The repository is signed with the key in [packaging/keyring/](../packaging/keyring/README.md); its fingerprint is
in [SECURITY.md](../SECURITY.md#package-signing). Compare it with what `gpg --show-keys` prints below before you trust
the key. The suite is your release's codename: `noble`, `resolute`, `stonking`, `trixie`, `forky`.

```sh
curl -fsSL https://joonhoekim.github.io/surface-pro-9-linux/packages/sp9-archive-keyring.gpg -o /tmp/sp9.gpg
gpg --show-keys /tmp/sp9.gpg                       # compare the fingerprint with SECURITY.md
sudo install -m 644 /tmp/sp9.gpg /usr/share/keyrings/sp9-archive-keyring.gpg
printf '%s\n' 'Types: deb' \
    'URIs: https://joonhoekim.github.io/surface-pro-9-linux/packages/apt' \
    "Suites: $(. /etc/os-release && echo "$VERSION_CODENAME")" \
    'Components: main' 'Architectures: amd64' \
    'Signed-By: /usr/share/keyrings/sp9-archive-keyring.gpg' | sudo tee /etc/apt/sources.list.d/sp9.sources
sudo apt update
sudo apt install sp9-archive-keyring              # from now on key changes arrive as updates of this package
```

The repository holds the packages of the latest release. Every release also attaches them, with the source
tarball and a signed `SHA256SUMS`, to its [GitHub release](https://github.com/joonhoekim/surface-pro-9-linux/releases).

Instead of this repository you can build the packages yourself[^build-locally] and install the files in place
of the package names of sections 3 and 5: `sudo apt install ./sp9_*.deb ./sp9-platform_*.deb …` (apt resolves the
distribution's dependencies). The on-device runs above were made that way, before the repository was published.

[^build-locally]: [packaging/README.md](../packaging/README.md#building-locally); `sp9-dkms` and
    `sp9-dkms-bluetooth`: [dkms/README.md](../dkms/README.md#ci).

## 3. Install the packages

```sh
sudo apt install sp9                               # removes linux-surface's iptsd (section 1)
```

`sp9` pulls in `sp9-platform`, `sp9-helper` and `sp9-digitizerd`, and with recommends on also `sp9-settings`,
`sp9-dkms` and `sp9-cmdline`; `sp9-dkms-bluetooth` (needed for `sp9-ctl pen couple`) and `sp9-libcamera` (section 5)
are installed by name. What each package is: [install.md](install.md#the-packages). The units of
`sp9-helperd` and `sp9-digitizerd` are enabled and started by the packages.

`sp9-cmdline` adds `pci=hpiosize=0` to the kernel command line through GRUB (`/etc/default/grub.d`, then
`update-grub`); other boot loaders take it by hand
([userspace/platform/README.md](../userspace/platform/README.md)).

To keep linux-surface's iptsd as the touch and pen daemon, install the packages one by one without `sp9` and
`sp9-digitizerd`, which `sp9` depends on: `sudo apt install iptsd sp9-platform sp9-helper sp9-settings sp9-dkms
sp9-cmdline`. linux-surface's iptsd has none of the pen patches of [userspace/iptsd](../userspace/iptsd/README.md);
`sp9-digitizerd` has that processing built in.

PSR (panel self refresh): linux-surface's kernel enables PSR2 on this panel, which Windows keeps off. `sp9-platform`
limits it to PSR1 through i915 debugfs at boot (`sp9-psr1.service`), which needs a writable debugfs. Kernel
lockdown makes it read-only;[^lockdown] the unit then only logs that. In that case add `i915.enable_psr=1` to the
kernel command line by hand; it taints the kernel (flag 64)
([userspace/platform/README.md](../userspace/platform/README.md)).

Initramfs: Ubuntu 24.04 and Debian 13 build it with initramfs-tools, Ubuntu 26.04 with dracut.[^dracut]
`sp9-platform` has drop-ins for initramfs-tools, dracut and mkinitcpio, and its installation triggers a rebuild.
What the images held was read on the device.[^initramfs-contents] On Debian 13 none of the DKMS modules are in the
image: Debian's initramfs-tools copies directories of the kernel's own module tree, and those modules load from the
root file system after it is mounted.

This matters only with disk encryption ([install-prepare.md](install-prepare.md#3-disk-encryption)). No encrypted
installation was tried on any release here, and no image above was checked against the linux-surface wiki's module
list: whether the Type Cover works at the passphrase prompt is unverified. On Debian 13 only the kernel's own
modules could serve there. Add the wiki's modules to the generator's module list and keep a USB keyboard at hand.

[^lockdown]: Many distribution kernels turn lockdown on under Secure Boot.

[^dracut]: `ubuntu-minimal` conflicts with initramfs-tools.

[^initramfs-contents]: Ubuntu 26.04, dracut: `pinctrl_tigerlake` and, with `sp9-dkms` installed, the DKMS build of
    `surface_aggregator_registry`. Ubuntu 24.04, initramfs-tools: `pinctrl_tigerlake` and the DKMS builds of
    `surface_aggregator_registry`, `hid-surface` and `ithc`. Debian 13, initramfs-tools: `pinctrl_tigerlake` and the
    stock `surface_aggregator`, nothing from `updates/dkms` (read from the image, and with `lsinitramfs` on the
    device).

Reboot once: the kernel command line, the initramfs, PSR and the kernel modules take effect at boot. Then:

```sh
sp9-ctl packages                                   # versions, the sp9-dkms modules of the running kernel, the update command
sudo /usr/libexec/sp9-helper/post-install-check.sh # report in ~/sp9-check-<host>/
```

What the check reports, and what to expect first: [install.md](install.md#check-the-installation).

## 4. Kernel modules (sp9-dkms) and Secure Boot

`sp9-dkms` carries this project's kernel patches as DKMS modules for linux-surface's kernel, and
`sp9-dkms-bluetooth` the Bluetooth core with patches 0023 and 0024 (the pen's cover coupling); which patches
ship and why is in [dkms/README.md](../dkms/README.md). DKMS builds them for every installed linux-surface
kernel with its headers (`linux-headers-surface`), now and when a kernel arrives later
([installing](../dkms/README.md#installing)). Two things to know:

- **A new linux-surface kernel release runs the stock modules until `sp9-dkms` is updated for it.** Each
  `sp9-dkms` release covers the linux-surface kernel releases it was generated and tested against
  (`cat /usr/src/sp9-*/kernels`); for any other release DKMS skips the build (it does not fail) and the kernel's own
  modules load, without this project's camera, USB-C, Type Cover, touch and pen-coupling fixes
  ([kernel updates](install.md#kernel-updates)). An `sp9-dkms` update that covers the new release comes through the
  repository when there is one; there is no schedule for it. `sp9-ctl packages` shows the state
  as "not covered". To keep a covered kernel, hold linux-surface's packages and release them once `sp9-dkms` lists
  the new release:

  ```sh
  sudo apt-mark hold linux-surface linux-image-surface linux-headers-surface     # apt upgrade keeps the kernel
  apt-mark showhold
  sudo apt-mark unhold linux-surface linux-image-surface linux-headers-surface   # after an sp9-dkms update lists it
  ```

  Without a hold, the previous kernel stays installed next to the new one (its package, `linux-image-<release>`,
  is named after the release) with its DKMS modules, so it can be picked under "Advanced options" in GRUB until
  `apt autoremove` removes it (untested).
- **Under Secure Boot the DKMS modules load only once their signing key is enrolled, and the stock module does not
  take their place.** Until then the devices those modules drive (cameras, Type Cover keys, touch) stop working.
  DKMS signs with a key it creates on the machine; enrol it once through MokManager before rebooting into a kernel
  with the modules ([Secure Boot](../dkms/README.md#secure-boot) has the details):

  ```sh
  sudo mokutil --import /var/lib/shim-signed/mok/MOK.der    # Ubuntu; Debian: /var/lib/dkms/mok.pub
  sudo reboot                                               # MokManager: Enroll MOK, the password you chose
  ```

  This key is the second one next to linux-surface's (`linux-surface-secureboot-mok`, section 1): that one is for
  the kernel, this one for the modules built on your machine.

`sp9-ctl packages` and `sp9-dkms-status` show per module whether the DKMS build, the kernel's own copy or nothing
is loaded, and name a failed build with its log. `dkms` itself is in `/usr/sbin`, which a normal user's `PATH`
lacks on **Debian**: run `sudo dkms status` there.

## 5. Cameras (experimental)

`sp9-libcamera` installs libcamera 0.8.0 with the patches of [userspace/libcamera](../userspace/libcamera/README.md)
into its own prefix, with a PipeWire libcamera plugin built against it, and makes WirePlumber load that plugin instead
of the distribution's. Nothing of the distribution is replaced. It is opt-in, and nothing pulls it in:

```sh
sudo apt install sp9-libcamera                     # Ubuntu 26.04, 26.10, Debian testing
systemctl --user restart wireplumber
```

**Debian 13:** `sp9-libcamera` needs PipeWire 1.6 (`libspa-0.2-modules (>= 1.6.9)`), which only trixie-backports has;
a plain `apt install sp9-libcamera` stops at that dependency against trixie's 1.4. Enable trixie-backports once and
install from it:

```sh
printf '%s\n' 'Types: deb' 'URIs: http://deb.debian.org/debian' 'Suites: trixie-backports' \
    'Components: main' 'Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg' \
    | sudo tee /etc/apt/sources.list.d/trixie-backports.sources
sudo apt update
sudo apt install -t trixie-backports sp9-libcamera
systemctl --user restart wireplumber
```

This moves PipeWire to 1.6 and the distribution's libcamera to 0.7.1 from trixie-backports; WirePlumber stays
trixie's 0.5.8.

**Ubuntu 24.04:** there is no `sp9-libcamera` for it,[^no-2404-libcamera] and its own libcamera 0.2 finds neither
camera (measured), so on Ubuntu 24.04 the cameras do not work.

Elsewhere, without the package the cameras go through the distribution's libcamera.[^distro-libcamera] With 0.7.2
(measured)[^libcamera-072] the processed image has magenta lines every 480 pixels, exposure and white balance follow
the top-left corner of the frame, the exposure oscillates after a start, the rear camera has no autofocus, and the
rear lens and sensor stay powered while WirePlumber runs. What the package changes, how to turn it off without
removing it, and its risks are in [packaging/libcamera/README.md](../packaging/libcamera/README.md).[^libcamera-risk]

[^no-2404-libcamera]: Its PipeWire 1.0's libcamera plugin does not build against libcamera 0.8.

[^distro-libcamera]: Debian 13: 0.4.0, 0.7.1 from trixie-backports; elsewhere 0.7.

[^libcamera-072]: Debian's 0.4.0 was not tried on the device.

[^libcamera-risk]: It is tied to the PipeWire version it was built for.

## 6. Updates

Updates come through the package manager, together with the distribution's:

```sh
sudo apt update && sudo apt full-upgrade
```

- A new linux-surface kernel arrives from linux-surface's repository. Until an `sp9-dkms` update covers it, DKMS
  skips it and the kernel's own modules load (section 4); `sp9-ctl packages` says "not covered".
- `sp9-ctl packages` (also part of `sp9-ctl status`) lists the installed versions and prints the update command. The
  helper only reads the package database and DKMS's state; it never downloads or installs anything. The desktop's
  update notifier tells you of new packages.
- Changes of the repository key (a renewed signing subkey) arrive as updates of `sp9-archive-keyring`, well before
  the old one expires.

## 7. Remove everything

Purge also removes the helper's settings in `/etc/sp9`, its reports in `/var/lib/sp9` and the iptsd drop-in it wrote:

```sh
sudo apt purge 'sp9*'
sudo rm -f /etc/apt/sources.list.d/sp9.sources /usr/share/keyrings/sp9-archive-keyring.gpg
sudo apt update
```

Removing `sp9-dkms` (and `sp9-dkms-bluetooth`) removes its modules from every kernel. The DKMS signing key and its MOK enrolment stay
(`sudo mokutil --delete /var/lib/dkms/mok.pub` revokes it). linux-surface's kernel and repository are removed as its
wiki says; keep another kernel installed, and boot it, before you remove that one.

## 8. What the packages do not cover

- Firmware for Wi-Fi, Bluetooth, i915 and the cameras. **Ubuntu:** `linux-firmware`. **Debian** has no
  `linux-firmware` package; with the `non-free-firmware` component enabled,[^non-free-firmware] install
  `firmware-iwlwifi` (Wi-Fi and Bluetooth), `firmware-intel-graphics` (i915, and the IPU6 firmware the cameras need)
  and `intel-microcode`.
- The opt-in files of `userspace/platform` (codec pin patch, legacy audio, reboot after hibernation): installed under
  `/usr/share/sp9-platform/optional/` and copied by hand when needed; none is needed with the current firmware.
- `i915.enable_psr=1` where debugfs is locked down (section 3).
- The tools the post-install check calls (the `diagnostics.enable` row in
  [install-from-source.md](install-from-source.md#3-settings-the-nixos-module-makes-that-you-must-reproduce-by-hand)):
  install those you want.
- The pen prototypes of [userspace/pen](../userspace/pen/README.md): the helper does what they did.

[^non-free-firmware]: The Debian installer adds it, debootstrap does not: add it to `Components:` in
    `/etc/apt/sources.list.d/debian.sources` or to the `deb` lines of `/etc/apt/sources.list`.

## 9. Verify

```sh
uname -r                                           # contains "surface"
sp9-ctl packages
systemctl status sp9-helperd sp9-digitizerd sp9-psr1
sudo cat /sys/kernel/debug/dri/0000:00:02.0/i915_edp_psr_status    # PSR mode: PSR1 enabled
```

More checks and the post-install check: [install.md](install.md#check-the-installation). Hardware expectations are
in [hardware-status.md](hardware-status.md). A test report is welcome ([install.md](install.md#report-what-you-see)).
