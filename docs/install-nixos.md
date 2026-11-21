# NixOS

NixOS next to Windows, then this repository's kernel and NixOS module. **Supported**; the device this project is
developed on runs it, and [hardware-status.md](hardware-status.md) was measured on it. Overview and checks:
[install.md](install.md); module options: [nixos.md](nixos.md).

This page covers only what is specific to this device; it assumes the
[NixOS manual](https://nixos.org/manual/nixos/stable/#sec-installation)'s installation chapter and flakes. The
working setup below was read from the installed device; steps never run in this form are marked **Untested**
([list](#untested-parts-of-this-guide)).[^not-logged]

[^not-logged]: The individual commands of the installation were not logged.

## The setup in one table

| Item | Working setup | Why |
|---|---|---|
| Windows | Kept, dual boot | Surface firmware updates (UEFI, SAM, touch, PD controller) come through Windows Update |
| UEFI Secure Boot and Secure Core | Both off (Secure Boot "None") | NixOS's systemd-boot and this repository's kernel are unsigned |
| NixOS root | One ext4 partition in the free space made by shrinking C:, not encrypted | |
| `/boot` | The existing 260 MiB EFI system partition, shared with Windows | Nothing else had to be created. It is small: see [the boot partition](#the-boot-partition-and-the-generation-limit) |
| Boot loader | systemd-boot | |
| Swap | zram only | Hibernation is not used and cannot work without disk swap |
| First boot | The distribution kernel (nixpkgs) | No binary cache has this repository's kernel; it is built on the device after the first boot |
| Kernel afterwards | 7.3.4 from this flake (`uname -r` prints `7.3.4`) | |
| Kernel parameters | None for the device: `kernelParams.psr` and `kernelParams.hpiosize` are off | Measured as unnecessary on this kernel ([nixos.md](nixos.md), option table) |
| Device helper | On in the working setup; optional | |

## 1. Prepare the device

Windows, the disk and the UEFI settings: [install-prepare.md](install-prepare.md). Secure Boot has to be "None"
for this route. Leave room on the NixOS partition for a kernel build tree, and have the charger at hand: the kernel
is built on the device.

## 2. Partition, format, mount

Boot the NixOS installer[^installer] from USB, become root and connect to a network (`nmtui`).

The commands below produce the layout of the working setup. **Untested as written**.[^not-logged] Check every
device name against your own `lsblk` output before running anything that writes.

```sh
lsblk -o NAME,SIZE,FSTYPE,PARTLABEL /dev/nvme0n1
# Expect: p1 vfat "EFI system partition" 260M, p2 16M (MSR), p3 ntfs (C:), p4 ntfs about 1.4G (recovery),
# and free space between p3 and p4. If it looks different, stop.

nix-shell -p gptfdisk                               # provides sgdisk
sgdisk -p /dev/nvme0n1                              # "Total free space" must be the space you made
sgdisk -n 0:0:0 -t 0:8300 -c 0:root /dev/nvme0n1    # one partition over the largest free block
sgdisk -v /dev/nvme0n1 && sgdisk -p /dev/nvme0n1
udevadm settle
lsblk -o NAME,SIZE,FSTYPE,PARTLABEL /dev/nvme0n1   # the new partition: PARTLABEL root, no FSTYPE
readlink -f /dev/disk/by-partlabel/root            # must name that partition; if it names another disk, stop

mkfs.ext4 -L nixos /dev/disk/by-partlabel/root

mount /dev/disk/by-partlabel/root /mnt
mkdir -p /mnt/boot
mount -o fmask=0077,dmask=0077 /dev/nvme0n1p1 /mnt/boot    # the existing EFI system partition. Never format it
ls /mnt/boot/EFI                                           # Microsoft and Boot must be listed
```

[^installer]: The working setup was installed from the 26.05 minimal image.

### The boot partition and the generation limit

In this layout `/boot` is the 260 MiB EFI system partition that Windows also uses. systemd-boot keeps a kernel
and an initrd there for every generation, and it writes the new generation's files before it removes old ones,
so the partition has to hold the limit plus one set. When it overflows, `nixos-rebuild switch` fails with "No
space left". The working setup therefore limits the boot menu to two generations:[^esp-usage]

```nix
boot.loader.systemd-boot.configurationLimit = 2;
```

The cost is that only two generations are in the boot menu: before a rebuild that changes the kernel, make sure
the current generation is one you have booted successfully.

[^esp-usage]: With two generations the partition was 57 % full (146 MiB) on the test device.

**Better layout, untested**: a separate 2 GiB XBOOTLDR partition for `/boot`, with the EFI system partition
mounted at `/efi`. Kernels and initrds then go to the XBOOTLDR partition, the EFI system partition holds only
systemd-boot itself, and the generation limit is unnecessary.[^xbootldr-planned]

It replaces part of step 2: create the XBOOTLDR partition before the root partition, and mount it instead of
the EFI system partition at `/mnt/boot`.

```sh
sgdisk -n 0:0:+2G -t 0:ea00 -c 0:xboot /dev/nvme0n1     # before the root partition (sgdisk -n 0:0:0 ... root)
udevadm settle && readlink -f /dev/disk/by-partlabel/xboot   # must name the new partition on nvme0n1
mkfs.fat -F 32 -n xboot /dev/disk/by-partlabel/xboot
# then create, format and mount the root partition as above, but without the /mnt/boot mount
mkdir -p /mnt/boot /mnt/efi
mount -o fmask=0077,dmask=0077 /dev/disk/by-partlabel/xboot /mnt/boot
mount -o fmask=0077,dmask=0077 /dev/nvme0n1p1 /mnt/efi
```

```nix
boot.loader.efi.efiSysMountPoint = "/efi";
boot.loader.systemd-boot.xbootldrMountPoint = "/boot";
```

[^xbootldr-planned]: This layout was planned for the test device but not used.

**Disk encryption, untested**: the working setup is not encrypted. With LUKS, put the modules of
[install-prepare.md](install-prepare.md#3-disk-encryption) into `boot.initrd.availableKernelModules` and keep a
USB keyboard connected.

## 3. Install NixOS with the distribution kernel

Install first without this repository. The kernel is not built on the installation medium: the first boot
uses the nixpkgs kernel from the binary cache, and a system that already boots is something to go back to.

```sh
nixos-generate-config --root /mnt
```

Check `hardware-configuration.nix`: it should have `fileSystems` entries for `/` and `/boot` and an empty
`swapDevices`.

A minimal host configuration, **untested as written**.[^minimal-config] Replace the placeholders, and add your
desktop, users and the rest as on any NixOS machine. Put both files in `/mnt/etc/nixos`, next to the generated
`hardware-configuration.nix`.

[^minimal-config]: It is a reduction of the configuration the test device runs, which is part of a larger
    multi-host flake.

`flake.nix`:

```nix
{
  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }: {
    nixosConfigurations.sp9 = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [ ./configuration.nix ];
    };
  };
}
```

`configuration.nix`:

```nix
{ pkgs, ... }:
{
  imports = [ ./hardware-configuration.nix ];

  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.loader.systemd-boot.configurationLimit = 2;   # only when /boot is the 260 MiB EFI system partition

  boot.kernelPackages = pkgs.linuxPackages_latest;   # first boot; the device module replaces it in step 4

  zramSwap.enable = true;                            # no swap partition, no hibernation

  networking.networkmanager.enable = true;
  hardware.bluetooth.enable = true;                  # the pen's button, battery level and haptics
  services.pipewire = {                              # the device module only adds configuration to PipeWire
    enable = true;
    alsa.enable = true;
    pulse.enable = true;
  };

  users.users.YOU = { isNormalUser = true; extraGroups = [ "wheel" "networkmanager" ]; };

  system.stateVersion = "26.05";                     # the release of the installer you used
}
```

```sh
nixos-install --root /mnt --flake /mnt/etc/nixos#sp9
umount -R /mnt
reboot
```

At the first boot:

- systemd-boot should show the NixOS generation and "Windows Boot Manager"; systemd-boot adds the Windows
  entry by itself.[^windows-entry] The firmware's own boot entry for Windows is not changed by the installation.
- Do not expect touch, pen or cameras on the distribution kernel: it has no linux-surface patches.[^first-boot]

[^windows-entry]: The entry is present on the working setup. Starting Windows from that menu entry is listed
    as not yet confirmed in [nixos.md](nixos.md).

[^first-boot]: What does and does not work at this stage was not recorded.

## 4. Enable this repository's module and kernel

Add the flake as an input and import its module. The module is built with your `nixpkgs`, so let the input
follow it.

`flake.nix`:

```nix
{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    surface-pro-9-linux = {
      url = "github:joonhoekim/surface-pro-9-linux";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, surface-pro-9-linux, ... }: {
    nixosConfigurations.sp9 = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        surface-pro-9-linux.nixosModules.default
      ];
    };
  };
}
```

Added to `configuration.nix`. These are the options the working setup sets; everything else is at its default:

```nix
  hardware.surface-pro-9.enable = true;

  # Compile the kernel through ccache: a later change to a patch recompiles only the changed objects.
  hardware.surface-pro-9.kernel.ccacheDir = "/var/cache/ccache";

  # Not needed on this repository's kernel (measured). Keep both on with a distribution kernel.
  hardware.surface-pro-9.kernelParams.psr = false;
  hardware.surface-pro-9.kernelParams.hpiosize = false;

  # Optional: the device helper (sp9-helperd, sp9-ctl).
  hardware.surface-pro-9.helper.enable = true;
```

The defaults already turn on the platform files, the touch daemon, the patched camera stack and the diagnostic
tools.[^defaults] `hardware.surface-pro-9.touch.daemon = "digitizerd"` replaces iptsd with this repository's own
touch and pen daemon ([digitizerd.md](digitizerd.md)); the test device runs it. The full option table is in
[nixos.md](nixos.md). Do not import nixos-hardware's `microsoft-surface` modules together with this one: both
define the kernel.

[^defaults]: iptsd with this repository's pen patches and the device preset, the speaker filter chain, the
    microphone default gain, the echo-cancel source, iio-sensor-proxy with the accelerometer rule (automatic
    rotation in a desktop that supports it), PipeWire and WirePlumber built against the patched libcamera, and the
    diagnostic tools are on; the pen tools (`pen.enable`) and the codec patch are off. The first build also
    compiles libcamera, PipeWire and WirePlumber.

Build and boot:

```sh
# Once, for the first build that uses kernel.ccacheDir: the directory is created by the system
# that is being built, so it does not exist yet.
sudo install -d -m 2770 -o root -g nixbld /var/cache/ccache

sudo nixos-rebuild build --flake .#sp9 --option extra-sandbox-paths /var/cache/ccache
sudo nixos-rebuild boot  --flake .#sp9 --option extra-sandbox-paths /var/cache/ccache
sudo reboot
```

Later rebuilds need no `--option`: the module adds the directory to the Nix sandbox.

- **Build time.** The kernel is built locally: about 1.5 hours the first time, about 20 minutes after a change
  to the patches with `kernel.ccacheDir`.[^build-time] A change to the kernel configuration recompiles almost
  everything. Run it on AC with the lid open, so the device does not suspend in the middle.
- **ccache.** The cache grows to at most 10 GiB. With it, the kernel output depends on the contents of the
  cache, which Nix cannot check. Leave the option out if you prefer a pure build and accept the full build
  time on every change.
- **Building on another machine, untested**: `nixos-rebuild --build-host <x86_64 NixOS host>` should work; it
  was never run.
- **Updating.** `nix flake update surface-pro-9-linux`, then rebuild. A change under `kernel/` or to
  `nix/kernel.nix` rebuilds the kernel.

[^build-time]: On this CPU (i5-1235U).

## 5. Verify

```sh
uname -r                          # 7.3.4
cat /proc/cmdline                 # neither i915.enable_psr nor pci=hpiosize
cat /proc/sys/kernel/tainted      # 0
sudo cat /sys/kernel/debug/dri/0000:00:02.0/i915_edp_psr_status    # "PSR mode: PSR1 enabled"
journalctl -b | grep 'Loading config /etc/iptsd'                   # iptsd.conf, then iptsd.d/surface-pro-9.conf (touch.daemon = "iptsd")
cat /sys/bus/iio/devices/iio:device*/in_accel_hysteresis 2>/dev/null   # 0: the sensor rule applied
cam -l                            # both cameras, from the patched libcamera 0.8.0
bootctl status                    # Secure Boot: disabled, systemd-boot on the EFI system partition
```

Then run the post-install check from a clone of this repository, and with the helper `sp9-ctl status`
([install.md](install.md#check-the-installation)):

```sh
git clone https://github.com/joonhoekim/surface-pro-9-linux
sudo surface-pro-9-linux/tools/linux/post-install-check.sh
less ~/sp9-check-*/summary.txt
```

What to expect first is in [install.md](install.md#known-limits-you-will-meet-first).

## 6. Going back

- **A generation does not boot.** Choose the previous generation in the systemd-boot menu. The menu needs a
  keyboard: keep a USB keyboard at hand.[^cover-boot-menu] With the generation limit of 2 there is exactly one
  previous generation.
- **Back to the distribution kernel.** `hardware.surface-pro-9.kernel.enable = false;` keeps the rest of the
  module and returns to the host's own `boot.kernelPackages`. Set `kernelParams.psr` and
  `kernelParams.hpiosize` back to their defaults (on) in that case. This is also how to keep a
  distribution-kernel generation next to this kernel for comparison.
- **Without the module.** `hardware.surface-pro-9.enable = false;`.
- **Repair from the installer.** Mount as in step 2 and use `nixos-enter --root /mnt`.
- **Remove NixOS and keep only Windows, untested.**[^remove-untested] In Windows as administrator: delete the
  "Linux Boot Manager" firmware entry (`bcdedit /enum firmware`, then `bcdedit /delete {identifier}`); mount the
  EFI system partition (`mountvol S: /s`) and remove `S:\EFI\systemd`, `S:\loader` and `S:\EFI\nixos`;[^esp-dirs]
  delete the Linux partition with `diskpart`[^diskpart] and extend C: into the free space (possible only if
  nothing lies between C: and the free space); set Secure Boot back to "Microsoft Only".

[^cover-boot-menu]: The Type Cover in the boot menu is unverified.

[^remove-untested]: None of this was done on the test device. The Windows commands are general Windows knowledge.

[^esp-dirs]: The NixOS kernels and initrds are under `\EFI\nixos`. The directory names are those of the working
    setup: the boot loader is `\EFI\systemd\systemd-bootx64.efi` and the booted initrd is under `\EFI\nixos`, both
    read on the device; `\loader` is where systemd-boot keeps its entries.

[^diskpart]: `list partition`, and check by size and type that the selected one is the Linux partition before
    `delete partition override`.

## 7. After installation

- **Firmware updates** come through Windows Update ([install.md](install.md#known-limits-you-will-meet-first)).
- **Free space on `/boot`**: check `df -h /boot` now and then if `/boot` is the EFI system partition.
- **Working on the kernel.** Rebuilding the whole kernel for every driver change is slow;
  [nixos.md](nixos.md) describes a loop that rebuilds single modules against the booted kernel
  ([`tools/linux/kmod-dev.sh`](../tools/linux/kmod-dev.sh)).

## Untested parts of this guide

Statements above that describe something not done on the test device, or not recorded (the preparation steps
have [their own list](install-prepare.md#untested-parts-of-this-page)):

1. The partitioning, formatting and mounting commands exactly as written.
2. The XBOOTLDR layout and its two boot loader options.
3. Disk encryption and the initrd module list for it.
4. The minimal `flake.nix` and `configuration.nix` exactly as written, and `nixos-install` with them.[^item-config]
5. The `nixos-rebuild build` / `boot` commands exactly as written.
6. Starting Windows from the systemd-boot menu entry.
7. The Type Cover keyboard in the boot menu.
8. Building on another machine.
9. Removing NixOS again.
10. The boot order after a Windows firmware update.

[^item-config]: The module, its options and the kernel are the ones in use; the surrounding host configuration is
    different.
