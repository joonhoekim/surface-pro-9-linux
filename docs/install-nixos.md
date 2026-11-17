# Installing NixOS on the Surface Pro 9 (Intel)

How to put NixOS on a Surface Pro 9 with an Intel CPU (SKU `Surface_Pro_9_2038`) next to Windows, and then
switch it to this repository's kernel and device module. The result is the setup the status in the
[README](../README.md) and [hardware-status.md](hardware-status.md) was measured on.

This is not a general NixOS installation manual; it covers what is specific to this device and assumes you
know the [NixOS manual](https://nixos.org/manual/nixos/stable/#sec-installation)'s installation chapter and
flakes.

**What is verified and what is not.** The working setup described here exists and boots: its layout, options
and measurements were read from the installed device. The individual commands of the installation were not
logged, and some steps of the procedure were never run in this form. Those are marked **Untested**. A list of
them is at the [end](#untested-parts-of-this-guide).

## The setup in one table

| Item | Working setup | Why |
|---|---|---|
| Windows | Kept, dual boot | Firmware updates (UEFI, SAM, touch, PD controller) are delivered only through Windows |
| UEFI Secure Boot and Secure Core | Both off (Secure Boot "None") | NixOS's systemd-boot and this repository's kernel are unsigned |
| NixOS root | One ext4 partition in the free space made by shrinking C:, not encrypted | |
| `/boot` | The existing 260 MiB EFI system partition, shared with Windows | Nothing else had to be created. It is small: see [the boot partition](#the-boot-partition-and-the-generation-limit) |
| Boot loader | systemd-boot | |
| Swap | zram only | Hibernation is not used and cannot work without disk swap |
| First boot | The distribution kernel (nixpkgs) | No binary cache has this repository's kernel; it is built on the device after the first boot |
| Kernel afterwards | 7.3.4 from this flake (`uname -r` prints `7.3.4`) | |
| Kernel parameters | None for the device: `kernelParams.psr` and `kernelParams.hpiosize` are off | Measured as unnecessary on this kernel ([nixos.md](nixos.md), option table) |
| Device helper | On in the working setup; optional | |

## Before you start

- A USB-C hub or adapter, a USB stick for the installer, and a **USB keyboard**. Whether the Type Cover
  keyboard works in the installer and in the boot menu is unverified.
- The charger. The kernel build takes about 1.5 hours and should run on AC.
- In Windows, first install all pending firmware updates (Settings → Windows Update, including the optional
  updates). After Linux is installed, firmware still only comes through Windows.

## 1. Prepare Windows

1. **Turn BitLocker / device encryption off** and wait until decryption has finished. In an administrator
   terminal `manage-bde -status C:` must report `Fully Decrypted` (as it did on the test device). With BitLocker
   on, changing Secure Boot makes Windows ask for the recovery key. Linux tools also cannot resize an encrypted
   NTFS volume; that is general knowledge and was not tried on this device.
2. **Turn off hibernation and fast startup**: `powercfg /h off` (administrator). This makes Windows close the
   NTFS volume cleanly on shutdown, which a Linux tool needs before it will touch the volume. Without
   hibernation, shut Windows down instead of leaving it in standby for long periods.
3. **Shrink C:** to make free space after it. Windows' own shrink (Disk Management, `Resize-Partition`) stops
   at the last unmovable file, which can leave C: far larger than its contents (on the test device the limit
   was set by NTFS metadata, `$Mft::$BITMAP`, reported in event 259). To go below that limit, use GParted
   (`ntfsresize`) from a live Linux session: it moves the MFT too. On the test device the final shrink was
   made with GParted in an Ubuntu live session started from a Ventoy USB stick. Before that, run `chkdsk C:`
   and **shut down** (not restart). Change nothing else: leave the EFI system partition, the MSR partition and
   the recovery partition where they are, and create nothing in the free space yet.
4. **Untested precaution**: make a Windows recovery drive (`RecoveryDrive.exe`) on a second USB stick, and
   copy the contents of the EFI system partition somewhere (`mountvol S: /s`, then copy `S:\EFI`), so that
   Windows can be repaired if the partition work or the boot loader installation goes wrong.

The amount of space is your choice. Leave room on the NixOS partition for a kernel build tree in addition to
the system; the size it needs was not measured.

**Hardware clock.** Windows keeps the hardware clock in local time and Linux expects UTC. On the test device
neither side was changed, and the clock is off by the time zone offset at every boot until NTP corrects it
(measured, four boots). The two usual fixes are the Windows registry value `RealTimeIsUniversal` or the NixOS
option `time.hardwareClockInLocalTime = true` (use one, not both). **Untested**: neither was applied on the
test device, and it is unverified whether the Windows-side setting is enough, because the firmware may refill
the clock at boot (inferred).

## 2. UEFI settings

Enter the Surface UEFI: with the device off, hold **volume up**, press and release the power button, and
release volume up when the UEFI screen appears.

To boot from USB: with the device off, hold **volume down** and press the power button briefly. The device
then starts from the USB stick. The test device was started from a Ventoy USB stick this way.

| Setting | Value | Note |
|---|---|---|
| Security → Secure Boot → Change configuration | "None" | Confirmed on the working setup: `bootctl status` reports `Secure Boot: disabled (setup)` |
| Secure Core | Off | From the linux-surface wiki: with it on, the firmware does not attempt USB boot. It was turned off on the test device |
| Boot configuration → Enable Boot from USB devices | On | |
| Boot configuration → boot configuration lock, if present | Off | From the linux-surface wiki: with it on, `bootctl` cannot write EFI variables. Not recorded for the test device |
| TPM | Unchanged | |

The names are those of UEFI 24.103.143 and can differ in other firmware versions. After changing Secure Boot,
boot Windows once to see that it still starts.

## 3. Partition, format, mount

Boot the NixOS installer (the working setup was installed from the 26.05 minimal image) from USB, become root
and connect to a network (`nmtui`).

The commands below produce the layout of the working setup. **Untested as written**: the commands typed
during the original installation were not logged. Check every device name against your own `lsblk` output
before running anything that writes.

```sh
lsblk -o NAME,SIZE,FSTYPE,PARTLABEL /dev/nvme0n1
# Expect: p1 vfat "EFI system partition" 260M, p2 16M (MSR), p3 ntfs (C:), p4 ntfs about 1.4G (recovery),
# and free space between p3 and p4. If it looks different, stop.

nix-shell -p gptfdisk                               # provides sgdisk
sgdisk -p /dev/nvme0n1                              # "Total free space" must be the space you made
sgdisk -n 0:0:0 -t 0:8300 -c 0:root /dev/nvme0n1    # one partition over the largest free block
sgdisk -v /dev/nvme0n1 && sgdisk -p /dev/nvme0n1
udevadm settle

mkfs.ext4 -L nixos /dev/disk/by-partlabel/root

mount /dev/disk/by-partlabel/root /mnt
mkdir -p /mnt/boot
mount -o fmask=0077,dmask=0077 /dev/nvme0n1p1 /mnt/boot    # the existing EFI system partition. Never format it
ls /mnt/boot/EFI                                           # Microsoft and Boot must be listed
```

### The boot partition and the generation limit

In this layout `/boot` is the 260 MiB EFI system partition that Windows also uses. systemd-boot keeps a kernel
and an initrd there for every generation, and it writes the new generation's files before it removes old ones,
so the partition has to hold the limit plus one set. When it overflows, `nixos-rebuild switch` fails with "No
space left". The working setup therefore limits the boot menu to two generations:

```nix
boot.loader.systemd-boot.configurationLimit = 2;
```

With two generations the partition was 57 % full (146 MiB) on the test device. The cost is that only two
generations are in the boot menu: before a rebuild that changes the kernel, make sure the current generation
is one you have booted successfully.

**Better layout, untested**: a separate 2 GiB XBOOTLDR partition for `/boot`, with the EFI system partition
mounted at `/efi`. Kernels and initrds then go to the XBOOTLDR partition, the EFI system partition holds only
systemd-boot itself, and the generation limit is unnecessary. This layout was planned for the test device but
not used.

```sh
sgdisk -n 0:0:+2G -t 0:ea00 -c 0:xboot /dev/nvme0n1     # before creating the root partition
mkfs.fat -F 32 -n xboot /dev/disk/by-partlabel/xboot
mkdir -p /mnt/boot /mnt/efi
mount -o fmask=0077,dmask=0077 /dev/disk/by-partlabel/xboot /mnt/boot
mount -o fmask=0077,dmask=0077 /dev/nvme0n1p1 /mnt/efi
```

```nix
boot.loader.efi.efiSysMountPoint = "/efi";
boot.loader.systemd-boot.xbootldrMountPoint = "/boot";
```

**Disk encryption, untested**: the working setup is not encrypted. With LUKS, the Type Cover keyboard has to
work in the initrd to type the passphrase; the linux-surface wiki
([Disk-Encryption](https://github.com/linux-surface/linux-surface/wiki/Disk-Encryption)) lists the modules
(`surface_aggregator`, `surface_aggregator_registry`, `surface_aggregator_hub`, `surface_hid_core`,
`surface_hid`, `8250_dw`, `intel_lpss`, `intel_lpss_pci`, `pinctrl_tigerlake`) for
`boot.initrd.availableKernelModules`. Whether that list is sufficient is unverified; keep a USB keyboard
connected.

## 4. Install NixOS with the distribution kernel

Install first without this repository. The kernel is not built on the installation medium: the first boot
uses the nixpkgs kernel from the binary cache, and a system that already boots is something to go back to.

```sh
nixos-generate-config --root /mnt
```

Check `hardware-configuration.nix`: it should have `fileSystems` entries for `/` and `/boot` and an empty
`swapDevices`.

A minimal host configuration. It is a reduction of the configuration the test device runs, which is part of a
larger multi-host flake; **untested as written**. Replace the placeholders, and add your desktop, users and
the rest as on any NixOS machine. Put both files in `/mnt/etc/nixos`, next to the generated
`hardware-configuration.nix`.

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

  boot.kernelPackages = pkgs.linuxPackages_latest;   # first boot; the device module replaces it in step 5

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
  entry by itself (the entry is present on the working setup). Starting Windows from that menu entry is
  listed as not yet confirmed in [nixos.md](nixos.md). The firmware's own boot entry for Windows is not
  changed by the installation.
- Do not expect touch, pen or cameras on the distribution kernel: it has no linux-surface patches. What does
  and does not work at this stage was not recorded.

## 5. Enable this repository's module and kernel

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

What the defaults already do: iptsd with the device preset, the speaker filter chain, the microphone default
gain, the echo-cancel source, and the diagnostic tools are on; the pen tools (`pen.enable`) and the codec
patch are off. The full option table is in [nixos.md](nixos.md). Do not import nixos-hardware's
`microsoft-surface` modules together with this one: both define the kernel.

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

- **Build time.** The kernel is built locally. On this CPU (i5-1235U) the first build takes about 1.5 hours;
  with `kernel.ccacheDir`, a rebuild after a change to the patches only takes about 20 minutes. A change to
  the kernel configuration recompiles almost everything. Run it on AC with the lid open, so the device does
  not suspend in the middle.
- **ccache.** The cache grows to at most 10 GiB. With it, the kernel output depends on the contents of the
  cache, which Nix cannot check. Leave the option out if you prefer a pure build and accept the full build
  time on every change.
- **Building on another machine, untested**: `nixos-rebuild --build-host <x86_64 NixOS host>` should work; it
  was never run.
- **Updating.** `nix flake update surface-pro-9-linux`, then rebuild. A change under `kernel/` or to
  `nix/kernel.nix` rebuilds the kernel.

## 6. Verify

```sh
uname -r                          # 7.3.4
cat /proc/cmdline                 # neither i915.enable_psr nor pci=hpiosize
cat /proc/sys/kernel/tainted      # 0
sudo cat /sys/kernel/debug/dri/0000:00:02.0/i915_edp_psr_status    # "PSR mode: PSR1 enabled"
journalctl -b | grep 'Loading config /etc/iptsd'                   # iptsd.conf, then iptsd.d/surface-pro-9.conf
bootctl status                    # Secure Boot: disabled, systemd-boot on the EFI system partition
```

The collector script reads everything that can be checked without a person and writes one file per area plus
`summary.txt` into `~/sp9-check-<host>/`. It is read-only and masks MAC addresses, UUIDs and serial numbers.
Run it from a clone of this repository:

```sh
git clone https://github.com/joonhoekim/surface-pro-9-linux
sudo surface-pro-9-linux/tools/linux/post-install-check.sh
less ~/sp9-check-*/summary.txt
```

Each line of the summary has a status: `MATCH` (as the research documents expect), `DIFF`, `MISSING`, `INFO`,
or `SKIP` (needs root or a tool that is not installed). `MATCH` is not "works": the tests a person has to do
are in [test-plan.md](test-plan.md).

With the helper enabled:

```sh
sp9-ctl status          # every property the daemon exports; Device is surface-pro-9
sp9-ctl diagnostics     # runs the same collector and prints the path of a tar.gz report
```

## 7. Going back

- **A generation does not boot.** Choose the previous generation in the systemd-boot menu. The menu needs a
  keyboard: keep a USB keyboard at hand (the Type Cover in the boot menu is unverified). With the generation
  limit of 2 there is exactly one previous generation.
- **Back to the distribution kernel.** `hardware.surface-pro-9.kernel.enable = false;` keeps the rest of the
  module and returns to the host's own `boot.kernelPackages`. Set `kernelParams.psr` and
  `kernelParams.hpiosize` back to their defaults (on) in that case. This is also how to keep a
  distribution-kernel generation next to this kernel for comparison.
- **Without the module.** `hardware.surface-pro-9.enable = false;`.
- **Repair from the installer.** Mount as in step 3 and use `nixos-enter --root /mnt`.
- **Remove NixOS and keep only Windows, untested.** In Windows as administrator: delete the "Linux Boot
  Manager" firmware entry (`bcdedit /enum firmware`, then `bcdedit /delete {identifier}`); mount the EFI
  system partition (`mountvol S: /s`) and remove `S:\EFI\systemd`, `S:\loader` and `S:\EFI\nixos`
  (the NixOS kernels and initrds); delete the Linux partition with `diskpart` and extend C: into the free
  space (possible only if nothing lies between C: and the free space); set Secure Boot back to "Microsoft
  Only". The directory names are those of the working setup (the boot loader is
  `\EFI\systemd\systemd-bootx64.efi` and the booted initrd is under `\EFI\nixos`, both read on the device;
  `\loader` is where systemd-boot keeps its entries). The Windows commands are general Windows knowledge.
  None of this was done on the test device.

## 8. After installation

- **Firmware updates** are installed from Windows. A firmware update can put Windows Boot Manager first in the
  boot order again (not observed on the test device); `efibootmgr -o` or the UEFI boot configuration restores
  it.
- **Free space on `/boot`**: check `df -h /boot` now and then if `/boot` is the EFI system partition.
- **Working on the kernel.** Rebuilding the whole kernel for every driver change is slow;
  [nixos.md](nixos.md) describes a loop that rebuilds single modules against the booted kernel
  ([`tools/linux/kmod-dev.sh`](../tools/linux/kmod-dev.sh)).

## 9. Known limits you will meet first

The full list is [hardware-status.md](hardware-status.md).

- **Pen.** Drawing works without Bluetooth. The Bluetooth features (tail button, battery level, haptics) need
  the pen to be paired by hand: hold the tail button until the pen is in pairing mode and pair it with a
  `NoInputNoOutput` agent. The pen disconnects after about 80 s without use and does not reconnect; it has to
  be put in pairing mode again. There is no automatic bonding as on Windows.
- **Rear camera.** Frames arrive, but there is no autofocus through libcamera: the lens stays where it is
  unless `focus_absolute` is written by hand, and whether the image then focuses is unverified.
- **Front camera.** Works; the sensor always runs its full-resolution mode at about 28.6 fps.
- **IR camera.** No driver, so no face unlock.
- **Speakers.** The volume control you move is the "Speakers" filter sink, which the module makes the default.
  The raw sink behind it is kept at 100 % by the module; leave it there.
- **Microphones.** The module sets the first-use gain to +30 dB because the PipeWire default (+60 dB) clips. It
  applies only to an account with no stored volume; on an account that already recorded at 100 % run
  `wpctl set-volume @DEFAULT_AUDIO_SOURCE@ 32%`. For calls there is a separate source, "Microphone (echo
  cancelled)", which has to be selected in the application; call quality is unverified.
- **Touch, Type Cover keys and touchpad, buttons, tablet mode, rotation, external monitor.** Unverified: no
  test by a person is on record.
- **Hibernation.** Not available with zram swap only.
- **Heat.** Under sustained load the surface gets hotter than on Windows, which lowers the power limit at a
  lower skin temperature.

## Untested parts of this guide

Statements above that describe something not done on the test device, or not recorded:

1. The recovery drive and the copy of the EFI system partition.
2. Either fix for the hardware clock.
3. The state of the boot configuration lock in UEFI.
4. The partitioning, formatting and mounting commands exactly as written.
5. The XBOOTLDR layout and its two boot loader options.
6. Disk encryption and the initrd module list for it.
7. The minimal `flake.nix` and `configuration.nix` exactly as written, and `nixos-install` with them (the
   module, its options and the kernel are the ones in use; the surrounding host configuration is different).
8. The `nixos-rebuild build` / `boot` commands exactly as written.
9. Starting Windows from the systemd-boot menu entry.
10. The Type Cover keyboard in the installer and in the boot menu.
11. Building on another machine.
12. Removing NixOS again.
13. The boot order after a Windows firmware update.
