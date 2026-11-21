# Preparing the device

Windows, the disk and the UEFI settings: the same for every distribution, before its page
([install.md](install.md)). General advice (partitioning, Secure Boot, dual boot) is in the linux-surface wiki,
[Installation and Setup](https://github.com/linux-surface/linux-surface/wiki/Installation-and-Setup). Steps never run
on the test device are marked **Untested** ([list](#untested-parts-of-this-page)).

## Before you start

- A USB-C hub or adapter, a USB stick for the installer, and a **USB keyboard**.[^cover-keyboard]
- The charger. Building this repository's kernel on the device takes about 1.5 hours and should run on AC.[^build-time]
- In Windows, install pending firmware updates first (Settings → Windows Update, including the optional updates).
  Keeping Windows installed afterwards is recommended: Surface firmware updates come through Windows Update.

[^cover-keyboard]: Whether the Type Cover keyboard works in the installer and in the boot menu is unverified.

[^build-time]: The NixOS route and the source route build the kernel; the packaged routes use linux-surface's
    prebuilt kernel.

## 1. Prepare Windows

The steps below change the disk. Shrinking a partition can lose data if it is interrupted or goes wrong, and
nothing in this guide can undo it.

0. **Back up first.** Copy everything you want to keep off the device. Write down the BitLocker recovery key
   before you change anything.[^recovery-key] Until decryption has finished, a change of the UEFI settings can
   make Windows ask for this key.
1. **Turn BitLocker / device encryption off** and wait until decryption has finished: in an administrator
   terminal `manage-bde -status C:` must report `Fully Decrypted`.[^bitlocker]
2. **Turn off hibernation and fast startup**: `powercfg /h off` (administrator). This makes Windows close the
   NTFS volume cleanly on shutdown, which a Linux tool needs before it will touch the volume. Without
   hibernation, shut Windows down instead of leaving it in standby for long periods.
3. **Shrink C:** to make free space after it. Windows' own shrink (Disk Management, `Resize-Partition`) stops
   at the last unmovable file, which can leave C: far larger than its contents.[^shrink-limit] To go below that
   limit, use GParted (`ntfsresize`) from a live Linux session: it moves the MFT too.[^gparted] Before that, run
   `chkdsk C:` and **shut down** (not restart). Change nothing else: leave the EFI system partition, the MSR
   partition and the recovery partition where they are, and create nothing in the free space yet.
4. **Untested precaution**: make a Windows recovery drive (`RecoveryDrive.exe`) on a second USB stick, and
   copy the contents of the EFI system partition somewhere (`mountvol S: /s`, then copy `S:\EFI`), so that
   Windows can be repaired if the partition work or the boot loader installation goes wrong.

The amount of space is your choice. If the kernel is built on the device (NixOS, from source), leave room for a
kernel build tree in addition to the system; the size it needs was not measured.

[^recovery-key]: In an administrator terminal `manage-bde -protectors -get C:` prints it (the 48-digit
    "Numerical Password"), and a device signed in with a Microsoft account also keeps it in that account.

[^bitlocker]: It did on the test device. With BitLocker on, changing Secure Boot makes Windows ask for the
    recovery key. Linux tools also cannot resize an encrypted NTFS volume; that is general knowledge and was not
    tried on this device.

[^shrink-limit]: On the test device the limit was set by NTFS metadata, `$Mft::$BITMAP`, reported in event 259.

[^gparted]: On the test device the final shrink was made with GParted in an Ubuntu live session started from a
    Ventoy USB stick.

### Hardware clock

Windows keeps the hardware clock in local time and Linux expects UTC. Unless one side is changed, the clock is
off by the time zone offset at every boot until NTP corrects it.[^clock-measured] The two usual fixes are the
Windows registry value `RealTimeIsUniversal` or keeping the clock in local time on the Linux side (on NixOS
`time.hardwareClockInLocalTime = true`); use one, not both. **Untested**.[^clock-untested]

[^clock-measured]: Measured on the test device, four boots; neither side was changed there.

[^clock-untested]: Neither fix was applied on the test device, and it is unverified whether the Windows-side
    setting is enough, because the firmware may refill the clock at boot (inferred).

## 2. UEFI settings

Enter the Surface UEFI: with the device off, hold **volume up**, press and release the power button, and
release volume up when the UEFI screen appears.

To boot from USB: with the device off, hold **volume down** and press the power button briefly. The device
then starts from the USB stick.[^ventoy]

| Setting | Value | Note |
|---|---|---|
| Security → Secure Boot → Change configuration | "None" | Needed for NixOS and the source route, whose boot loader and kernel are unsigned.[^secure-boot-state] The packaged routes can in principle keep it on[^secure-boot-packaged] |
| Secure Core | Off | With it on, the firmware does not attempt USB boot.[^secure-core] |
| Boot configuration → Enable Boot from USB devices | On | |
| Boot configuration → boot configuration lock, if present | Off | With it on, `bootctl` cannot write EFI variables.[^boot-lock] |
| TPM | Unchanged | |

The names are those of UEFI 24.103.143 and can differ in other firmware versions. After changing Secure Boot,
boot Windows once to see that it still starts.

[^ventoy]: The test device was started from a Ventoy USB stick this way.

[^secure-boot-state]: Confirmed on the NixOS setup: `bootctl status` reports `Secure Boot: disabled (setup)`.

[^secure-boot-packaged]: On Debian and Ubuntu, linux-surface's key (for the kernel) and the DKMS key made on the
    machine (for the modules) are enrolled through MokManager
    ([Debian and Ubuntu](install-debian-ubuntu.md#4-kernel-modules-sp9-dkms-and-secure-boot)); Arch:
    [its page](install-arch.md#4-kernel-modules-sp9-dkms-and-secure-boot). Every run on the device had Secure
    Boot off, so Secure Boot on any of them is unverified.

[^secure-core]: From the linux-surface wiki. It was turned off on the test device.

[^boot-lock]: From the linux-surface wiki. Not recorded for the test device.

## 3. Disk encryption

Every installation tested here was unencrypted. With an encrypted root file system, the passphrase is typed
before the root file system is mounted, so the Type Cover keyboard has to work in the initramfs. The linux-surface
wiki ([Disk-Encryption](https://github.com/linux-surface/linux-surface/wiki/Disk-Encryption)) lists the modules
that have to be in it.[^luks-modules] Whether that list is sufficient is unverified; keep a USB keyboard
connected. Each distribution page says where its initramfs generator takes the list.

[^luks-modules]: `surface_aggregator`, `surface_aggregator_registry`, `surface_aggregator_hub`,
    `surface_hid_core`, `surface_hid`, `8250_dw`, `intel_lpss`, `intel_lpss_pci`, `pinctrl_tigerlake`.

## Untested parts of this page

1. The recovery drive and the copy of the EFI system partition.
2. Either fix for the hardware clock.
3. The state of the boot configuration lock in UEFI.
4. The Type Cover keyboard in the installer and in the boot menu.
5. Disk encryption and the initramfs module list for it.
