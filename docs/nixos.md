# NixOS

This repository provides the **device layer** for a Surface Pro 9 (Intel) running NixOS: the flake output
`nixosModules.default`, which a host configuration imports. The host itself (users, desktop, disks, boot loader)
belongs to the reader's own flake. The installation steps are in [install-nixos.md](install-nixos.md); section 2
shows how a host consumes this flake.

The Nix code here has been evaluated and built on Linux, and the device boots the resulting kernel (7.3.4).
`nix flake check` passes and `flake.lock` is committed. On the device the options `enable`, `helper.enable` and
`kernel.ccacheDir` have been used switched on, with `kernelParams.psr` and `kernelParams.hpiosize` switched off
(2.2); the helper there also does the pen's haptics, speed feedback and cover coupling
([helper.md](helper.md)). The other options whose default is off (`audio.codecPatch`, `audio.legacy`, `hibernateReboot`, `pen.*`)
have never been run on Linux; they are listed in section 6.

## 1. The flake of this repository

| Output | Content |
|---|---|
| `nixosModules.default` (= `nixosModules.surface-pro-9`) | [`nix/module.nix`](../nix/module.nix). Options are `hardware.surface-pro-9.*` |
| `packages.x86_64-linux.kernel` | [`nix/kernel.nix`](../nix/kernel.nix): v7.3.4 + linux-surface `patches/7.3` + `kernel/patches` |
| `packages.x86_64-linux.surface-pen-tools` | [`nix/pen-tools.nix`](../nix/pen-tools.nix): `userspace/pen` as is: `pen-haptic`, `surface-pen-connect` and `surface-pen-feedback` with their units |
| `packages.x86_64-linux.sp9-helper` | [`nix/helper.nix`](../nix/helper.nix): `sp9-helperd`, `sp9-ctl` and the settings app `sp9-settings` from `helper/`, the unit, D-Bus and polkit files, and the two scripts the daemon calls (`post-install-check.sh`, `sdc-blob.py`) |
| `devShells.x86_64-linux.kernel-dev` | The build inputs of the kernel derivation (`inputsFrom`) plus the tools `kmod-dev.sh` uses, `hardeningDisable = [ "all" ]`. See 5.1 |
| `checks.x86_64-linux` | `surface-pen-tools`, `sp9-helper`, `module-eval` (a minimal NixOS system with the module and its off-by-default options switched on; only the system's drvPath is written to a file, so the kernel is evaluated and not built) |

CI is [`.github/workflows/nix.yml`](../.github/workflows/nix.yml): `nix flake check --no-build` (which also
evaluates the dev shell), `nix build .#surface-pen-tools` and `nix build .#sp9-helper`. The kernel is not built in
CI (a host builds the same derivation).

The only inputs are `nixpkgs` (nixos-unstable) and `linux-surface` (`flake = false`, pinned in the URL to the
commit `ea852b4e…` named in [kernel/README.md](../kernel/README.md)). `flake.lock` is committed.
The module builds with the consumer's `pkgs`, so this repository's `nixpkgs` only serves the `packages` output.
A consumer sets `inputs.nixpkgs.follows`.

| Option | Default | What it does | Source file (referenced, not copied) |
|---|---|---|---|
| `enable` | false | The switch for everything below. On, it sets `hardware.enableRedistributableFirmware` (mkDefault) and puts `pinctrl_tigerlake` into the initrd (this kernel has `PINCTRL_TIGERLAKE=m`, so the initrd entry is valid and the module is loaded there) | the intent of `userspace/platform/initramfs/` |
| `kernel.enable` | true | `boot.kernelPackages = mkOverride 90 (linuxPackagesFor kernel)`. It beats a definition without a priority (a host's plain `boot.kernelPackages = pkgs.linuxPackages_latest`) and yields to `mkForce`. Off, the host's own `boot.kernelPackages` stays | `kernel/` |
| `kernel.ignoreConfigErrors` | false | A fragment option that is missing from `.config` or has another value stops the build (as strict as `build-kernel.sh`). true turns that into a warning. The running kernel was built with the strict check, and all 49 options of the two fragments (linux-surface 41, `sp9.config` 8) are in its configuration | `kernel/config/sp9.config` |
| `kernel.ccacheDir` | null | Given a path, the kernel is compiled through ccache (a wrapper that overrides `CC=` on the make command line, `nix/kernel.nix`). A rebuild after a patch change compiles only the changed objects; a configuration change still misses almost everything. The module creates the directory through tmpfiles (root:nixbld 2770) and adds it to `nix.settings.extra-sandbox-paths`. The first build that has this option does not have the directory yet: create it by hand once and pass `--option extra-sandbox-paths <dir>`. When the directory is not in the sandbox the wrapper calls the compiler directly. The kernel output then depends on what the cache holds, which Nix cannot check. Has an effect only with `kernel.enable` | option description |
| `kernelParams.enable` | true | Adds `i915.enable_psr=1 pci=hpiosize=0` to `boot.kernelParams`. `i915.enable_psr` is an unsafe i915 option, so the kernel is tainted on every boot (taint flag 64). With this repository's kernel neither parameter is needed (measured; the next two rows and [research/gaps/display-power.md](research/gaps/display-power.md) differences 1 and 7). They are for a distribution kernel | `cmdline/sp9-platform.cmdline` |
| `kernelParams.psr` | true | false drops only `i915.enable_psr=1`. With this repository's kernel patch 0014 turns PSR2 off, so the parameter is not needed. Boot measurement (7.3.4, patches 0001–0022, false): `tainted` 0, `PSR mode: PSR1 enabled` unchanged (on the boot with the parameter `tainted` read 4160: the 64 the parameter sets plus 4096 from an out-of-tree test module that was loaded on that boot), "applying disable PSR2 quirk" in the boot log. No flicker was noticed while using the device in that state (observed, DSP-1). Leave it on with a distribution kernel (`kernel.enable = false`) | option description |
| `kernelParams.hpiosize` | true | false drops only `pci=hpiosize=0`. 7.3.4 reserves the 0x1800 range through `PCHRESV`, so the parameter is not needed with this kernel ([research/gaps/display-power.md](research/gaps/display-power.md) difference 7). Boot measurement (false): `1800-18fe` is reserved by `PNP0C02:01`, and the I/O windows of the Thunderbolt root ports `00:07.0` and `00:07.3` are `0x3000-0x3fff` and `0x4000-0x4fff`, outside it. After boot and 3 s2idle cycles `gpe_all` and `gpe52` are 0 and there are 0 lines of "No handler or method for GPE". Closing and opening the lid 4 times also gave 0 lines of GPE errors (PWR-2). GPEs on a USB-C connection are unverified (DSP-5) | option description |
| `touch.enable` | true | `services.iptsd.enable`, and the preset as `/etc/iptsd.d/surface-pro-9.conf`. iptsd 3.1.0 reads this file after `/etc/iptsd.conf` (journal). Changing only the preset does not restart `iptsd@` (the NixOS module uses only `/etc/iptsd.conf` as a restart trigger) | `iptsd/surface-pro-9.conf` |
| `speakers.enable` | true | `services.pipewire.configPackages`, `services.pipewire.wireplumber.configPackages`. The LADSPA plugins (`ladspaPlugins` = swh-plugins) reach PipeWire's `LADSPA_PATH` through `passthru.requiredLadspaPackages`. The filter sink "Speakers" and `sp9_speaker_raw` appear. The WirePlumber rule arrives through the service's `XDG_DATA_DIRS`, not `/etc/wireplumber/`. A user unit, `sp9-speaker-raw-volume`, puts the raw sink at 100 % after every WirePlumber start | `pipewire/…/60-sp9-speakers.conf`, `wireplumber/…/60-sp9-speaker-raw.conf`, `pipewire/sp9-speaker-raw-volume` |
| `microphone.enable` | true | A WirePlumber rule sets the first-use gain of the internal microphones to +30 dB (`Capture` +30 dB, `Mic Boost` 0 dB). WirePlumber's default of 100 % is +60 dB and clips (measured, AUD-3). It applies only to an account with no stored value | `wireplumber/…/60-sp9-mic.conf` |
| `audio.codecPatch` | `"none"` | `"pins"`: the firmware (the pin defaults of the Windows driver) goes into `hardware.firmware` (uncompressed) and the modprobe line into `boot.extraModprobeConfig`. This device comes up on legacy `snd_hda_intel` without `audio.legacy`, so that is a path on which the patch is read as is. `"pins"` changes nothing (the BIOS pin values already equal the Windows table, [research/gaps/audio.md](research/gaps/audio.md) G2) | `optional/firmware/sp9-alc274-pins.fw`, `optional/modprobe.d/sp9-alc274-pins.conf` |
| `audio.legacy` | false | `snd_intel_dspcfg dsp_driver=1`. Meaningful only on a firmware that comes up with SOF. This device defaults to legacy ([research/gaps/audio.md](research/gaps/audio.md) section 2.1) | `optional/modprobe.d/sp9-audio-legacy.conf` |
| `audio.echoCancel` | true | The PipeWire echo-cancel source `sp9_echo_cancel` (the internal microphones minus what the speakers play). A source chosen per application; the default sink and source do not change. Needs `speakers.enable`, which names the sink the reference comes from. Confirmed: a temporary instance creates the source, and everything is suspended when idle. Call quality is unverified (AUD-4) | `userspace/platform/pipewire/` |
| `camera.pipewire` | true | false disables WirePlumber's libcamera monitor. While it is on, libcamera keeps the rear lens (VCM) node open for the whole session, so the rails and the clock of the rear sensor stay on ([research/gaps/camera.md](research/gaps/camera.md)). Off, the cameras disappear from PipeWire applications (programs that use libcamera themselves keep them), so this is for measuring standby power | option description |
| `diagnostics.enable` | true | Puts the tools that the test plan and the collector call (`cam`, `v4l2-ctl`, `amixer`, `libinput`, `hid-decode`, `drm_info`, the iptsd tools and others) into the system PATH. Without them the corresponding sections of the collector stay empty | the tools section of [test-plan.md](test-plan.md) |
| `hibernateReboot` | false | `/etc/systemd/sleep.conf.d/sp9-hibernate-reboot.conf` | `optional/sleep.conf.d/` |
| `pen.enable` | false | The pen tools in PATH (`pen-haptic`, `surface-pen-connect`, `surface-pen-feedback`) | `userspace/pen/` |
| `pen.connectService` | false | The unit through `systemd.packages` (only ExecStart is rewritten to the store path), `wantedBy = multi-user.target`, `hardware.bluetooth.enable` (mkDefault). Implies `pen.enable` | `userspace/pen/systemd/surface-pen-connect.service` |
| `pen.feedbackService` | false | The unit `surface-pen-feedback` through `systemd.packages` (ExecStart rewritten to the store path), `wantedBy = multi-user.target`. It feeds the pen's speed back to the digitizer, which the Slim Pen 2 needs for its continuous haptic waveforms, and starts and stops the inking waveform on contact ([research/gaps/pen-vendor-exchange.md](research/gaps/pen-vendor-exchange.md) section 1). Together with `helper.enable` the unit is only installed, not started at boot, and a warning is printed: the helper (`sp9-ctl pen ink on`) writes the same digitizer report. Implies `pen.enable` | `userspace/pen/systemd/surface-pen-feedback.service` |
| `helper.enable` | false | `sp9-helperd` through `systemd.packages` and `services.dbus.packages` (started with `multi-user.target`), the package (`sp9-ctl`, `sp9-settings`) in PATH, `security.polkit.enable`, `surface_aggregator_cdev` in `boot.kernelModules`. The tools the diagnostics script uses go into the unit's `path`, and `SP9_PYTHON` is set (ICC generation). Runs on the device; per-feature status is in [helper-reference.md](helper-reference.md) It also does the pen: inking haptics and the pen speed to the digitizer (`sp9-ctl pen ink on`; takes the place of `pen.feedbackService`) and coupling a pen put into the cover's pen slot (`sp9-ctl pen auto-couple on`, or `pen couple` once), which needs Bluetooth and this repository's kernel (patch 0023) | `helper/`, [helper.md](helper.md) |

The options map one to one to the components of [`install.sh`](../userspace/platform/install.sh) (`iptsd` →
`touch`, `cmdline` → `kernelParams`, `speakers`, `initramfs` always, `hda-pins` → `audio.codecPatch`,
`audio-legacy`, `hibernate-reboot`). File contents are not duplicated; the module refers to them by path.

### 1.1 The kernel: why not nixos-hardware

nixos-hardware (`microsoft/surface/common`, last change `ecdce73e`) works the same way as this repository:
`buildLinux` on the kernel.org tarball, with linux-surface `patches/<version>` as `kernelPatches`. It has no binary
cache either, so both are local builds. This repository still uses its own derivation (`nix/kernel.nix`).

| | nixos-hardware | This repository |
|---|---|---|
| Version | `hardware.microsoft-surface.kernelVersion` enum: `longterm` = 6.18.60, `stable` = 7.3.1 | 7.3.4 |
| linux-surface | `668a0208…` | `ea852b4e…` (`patches/7.3/BASE` = v7.3.4). The combination patches 0001–0023 of this repository apply to |
| Config fragment | `structuredExtraConfig` in `patches.nix` prefixes the keys with `CONFIG_`. `generate-config.pl` matches names without the prefix, so every one is an "unused option", and `ignoreConfigErrors = true` hides that error (judged from reading the code; unverified against a build log). What remains is only what nixpkgs `common-config.nix` and autoModules switch on | linux-surface `configs/surface-7.3.config` and `sp9.config` are parsed at evaluation time and passed without the prefix, with the strict check on. `# … is not set` becomes optional (a symbol that is absent from `.config` because its dependencies are off means the same) |
| `boot.kernelPackages` | A definition without a priority. It collides with a host that also sets `boot.kernelPackages` without a priority; that host needs `mkForce` | `mkOverride 90` |
| Extras | surface-pro-intel enables `iptsd`, `surface-control`, a thermald `thermal-conf.xml` made for other models, and `mem_sleep_default=deep` (mkDefault) | Only what has evidence on the SP9 (`userspace/platform/README.md`). thermald is the host's choice |

Do **not import** nixos-hardware's `surface-pro-intel` together with this module (the kernel definitions overlap.
This repository's wins because of the priority, but the intent becomes unclear).
If `surface-control` is wanted, add only `pkgs.surface-control` in the host.

One difference remains, the starting configuration: `build-kernel.sh` starts from Ubuntu's mainline configuration
and the Nix build from nixpkgs `common-config.nix`. The options the fragments ask for are the same; the rest of
the driver selection (built-in or module) differs. The reference for this device is the Nix build;
`build-kernel.sh` (.deb) matters only on another distribution.
`LOCALVERSION` is not set (`modDirVersion` = `7.3.4`, and `uname -r` is `7.3.4`, so the release string does not
contain `surface`). Secure Boot is off, so there is no module signing configuration either.

`boot.kernelPatches` still applies on top of this kernel (NixOS appends it through `kernel.override`). A host adds
test patches that way.

## 2. The host

A host is a NixOS configuration in the reader's own flake that imports this module. The snippets below are an
example to adapt, not a layout to copy. The author's public `nixos-config` repository has a host built this way.

### 2.1 Disk and boot loader (example: dual boot on the factory ESP)

The layout of one installation that keeps Windows. There is no XBOOTLDR partition; `/boot` is the EFI System
Partition that Windows created.

| Partition | Size | Use |
|---|---|---|
| p1 EFI System (vfat) | 260 MiB (file system 256 MiB) | `/boot`. Shared with Windows. systemd-boot and the NixOS kernels and initrds are here |
| p2 MSR | 16 MiB | |
| p3 NTFS | 64 GiB | Windows C: |
| p4 WinRE | 1.4 GiB | |
| p5 ext4 (partition name `root`) | 172.8 GiB | `/`. Not encrypted |

Swap is zram only (7.7 GiB). `hardware-configuration.nix` has two `fileSystems`, `/` and `/boot`, and an empty
`swapDevices`.

With `/boot` on a 260 MiB ESP there is little room for the kernel and initrd of each generation. This host forces
`boot.loader.systemd-boot.configurationLimit` to 2. systemd-boot writes the kernel and initrd of the new
generation before it removes the old ones, so the limit + 1 sets have to fit; when they do not, `switch` fails
with "No space left". With 2 generations the ESP is 57 % full (146 MiB). It also means the boot menu keeps only
two generations: before a rebuild that changes this repository's kernel, make sure the current generation boots.

The boot loader is systemd-boot 261.2 (`\EFI\systemd\systemd-bootx64.efi`).

### 2.2 This repository as an input

A minimal host flake:

```nix
{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    # The module builds with this flake's pkgs; following nixpkgs avoids locking a second copy.
    surface-pro-9-linux = {
      url = "github:joonhoekim/surface-pro-9-linux";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, surface-pro-9-linux, ... }: {
    nixosConfigurations.sp9 = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        surface-pro-9-linux.nixosModules.default
        ./configuration.nix
      ];
    };
  };
}
```

The module can also be imported from a host module's `imports` when the flake inputs are passed through
`specialArgs` (an argument from `specialArgs` may be used in `imports` without infinite recursion; one from
`_module.args` may not).

The device part of `configuration.nix`, with the options the author's host sets:

```nix
{ lib, ... }:
{
  imports = [ ./hardware-configuration.nix ];

  hardware.surface-pro-9.enable = true;
  # Compile the kernel through ccache: a patch change recompiles only the changed objects.
  hardware.surface-pro-9.kernel.ccacheDir = "/var/cache/ccache";
  # Not needed with this repository's kernel: patch 0014 disables PSR2, and the parameter taints the kernel.
  hardware.surface-pro-9.kernelParams.psr = false;
  # Not needed with this repository's kernel: 7.3.4 reserves the 0x1800 I/O range itself.
  hardware.surface-pro-9.kernelParams.hpiosize = false;
  # The device helper: sp9-helperd, sp9-ctl, sp9-settings (docs/helper.md).
  hardware.surface-pro-9.helper.enable = true;

  # Only when /boot is the 260 MiB factory ESP (2.1). mkForce is needed only when another module of the host
  # also sets the limit (the author's host shares modules with other machines); otherwise a plain `= 2` does.
  boot.loader.systemd-boot.configurationLimit = lib.mkForce 2;
}
```

[install-nixos.md](install-nixos.md) builds the same configuration step by step (first with the distribution
kernel, then with this module) and uses the same host name `sp9`.

The first build with `kernel.ccacheDir` needs the directory to exist already and to be visible in the sandbox
(the option table in section 1):

```sh
sudo install -d -m 2770 -o root -g nixbld /var/cache/ccache
sudo nixos-rebuild switch --flake .#sp9 --option extra-sandbox-paths /var/cache/ccache
```

Leave `kernelParams.psr` and `kernelParams.hpiosize` at their default (true) with a distribution kernel
(`kernel.enable = false`).

What gets locked is a commit. To pick up a newer revision of this repository, run
`nix flake update surface-pro-9-linux` in the host flake and then `nixos-rebuild switch --flake .#sp9`.
When `kernel/` or `nix/kernel.nix` changed, the kernel derivation is built again (there is no binary cache; with
`kernel.ccacheDir` only the changed objects are recompiled). To build on another machine, the form is
`nixos-rebuild switch --flake .#sp9 --build-host <x86_64 NixOS host> --target-host <SP9>` (never run).
If something goes wrong, boot the previous generation or set `hardware.surface-pro-9.kernel.enable = false`.

**Testing commits that are not pushed.** Point the input at a local clone for the time being:

```nix
      url = "git+file:///path/to/surface-pro-9-linux";
```

- `git+file` sees only the files git tracks: ignored and untracked files in the clone (a kernel build tree, for
  example) are not copied into the store. A `path:` input copies the whole directory; do not use it.
- A machine that does not have this path cannot fetch the input, and `nix flake lock`/`update` fail there. Do not
  commit a `git+file` input to a flake that other machines use.

## 3. Secure Boot

This repository assumes Secure Boot is **off**. The NixOS systemd-boot and this repository's kernel are not
signed, so UEFI Secure Boot on the SP9 is set to "None"; the module does not set up lanzaboote. Secure Core is
off in UEFI as well.
State on the device (measured): `bootctl status` prints `Secure Boot: disabled (setup)`, the EFI variable
`SecureBoot` = 0, `SetupMode` = 1, and the lockdown LSM is not included.

- Windows boots with Secure Boot off. BitLocker was off on the measured device. Had it been on, the Secure Boot
  change would alter the TPM measurements and Windows would ask for the recovery key.
- With Secure Boot off there is no lockdown, so hibernation, debugfs writes and unsigned modules (the module
  development loop in 5.1) are possible.
- lanzaboote enrols its own keys into the firmware db (sbctl, Setup Mode). The Secure Boot screen of the Surface
  UEFI offers "Microsoft Only", "Microsoft & 3rd party CA" and "None"; a menu for enrolling user keys was not
  checked. With "None" the firmware reports `SetupMode` = 1, so enrolling keys from the OS may be possible
  (inferred; enrolment was not attempted).
  Going through shim and MOK (the Ubuntu way) is not lanzaboote's default path.

## 4. Status

1. The device runs NixOS from a host flake as in section 2 and boots this repository's kernel (7.3.4).
2. Test results are in [test-results.md](test-results.md).
3. The checks that remain on the Nix side are in section 6.

With only two boot generations (2.1), keeping a generation with the nixpkgs kernel next to this one for comparison
takes one generation built with `hardware.surface-pro-9.kernel.enable = false`.

## 5. Changing things in this repository

- When the kernel pin (v7.3.4, the linux-surface commit) moves, change `kernel/README.md`,
  `tools/linux/build-kernel.sh`, the `linux-surface` URL in `flake.nix`, and `version` and `hash` in
  `nix/kernel.nix` together. The hash is the hex value from kernel.org `sha256sums.asc` converted to SRI (base64).
- When a file is added to `userspace/platform/`, give `install.sh` and `nix/module.nix` the same component.

### 5.1 Module development loop

Rebuilding the kernel derivation takes 1–2 hours on the i5-1235U. Most patches of this repository touch only
modules (`ov5693`, `ipu-bridge`, `dw9719`, `int3472`, `surface_*`, `i915`, `ucsi`), so the loop is: in a tree
identical to the booted kernel, rebuild only that directory and `insmod` the result.
[`tools/linux/kmod-dev.sh`](../tools/linux/kmod-dev.sh) does it. Patches 0018–0022 were made and exported with this
loop ([kernel/README.md](../kernel/README.md)); 0023 was built and loaded with it first, and a kernel built with it
has been booted since.

```sh
cd ~/git/surface-pro-9-linux
nix develop .#kernel-dev
tools/linux/kmod-dev.sh prepare              # once (unpack the tree, patch, git commit, modules_prepare)
$EDITOR ~/.cache/sp9-kdev/src/drivers/media/i2c/ov5693.c
tools/linux/kmod-dev.sh build ov5693         # = make M=drivers/media/i2c modules; prints the .ko paths and vermagic
tools/linux/kmod-dev.sh load ov5693          # rmmod (holders included) → insmod → kernel log
tools/linux/kmod-dev.sh git commit -as       # author, subject and body in upstream form
tools/linux/kmod-dev.sh patch                # kernel/patches/<next number>-….patch
```

The reference is not the repository but **the derivation of the booted kernel**
(`nix-store -q --deriver /run/booted-system/kernel`; `--drv` selects another).

- Source: the derivation's `src` (the v7.3.4 tarball) and its `patches` list (linux-surface, `kernel/patches`, the
  host's `boot.kernelPatches`), applied with `patch -p1` in the derivation's order.
  Shebangs (`/bin/bash` and the like) are changed to `/usr/bin/env`, everything is committed as one commit and
  tagged `kdev-base`. The work directory is `$KDEV` (default `~/.cache/sp9-kdev`); the git directory is
  `$KDEV/git`, outside the tree, which is why commits go through `kmod-dev.sh git`.
- Configuration and symbols: `.config` and `Module.symvers` come from `lib/modules/7.3.4/build` in the derivation's
  `dev` output. With RANDSTRUCT on, the seed (nixpkgs fixes it per derivation) is used too, and so is `vmlinux`
  when `dev` has it (for module BTF). `dev` is built together with the kernel but is not in the system closure, so
  garbage collection can remove it. `$KDEV/kernel-dev` is a GC root; to keep it from the start, add
  `system.extraDependencies = [ config.boot.kernelPackages.kernel.dev ];` to the host.
  Recreating it after it was removed is a full kernel build, so the script stops unless `KDEV_REBUILD_DEV=1`.
- Checks (any mismatch stops the script): the derivation's `out` = `/run/booted-system/kernel`, `uname -r`,
  `/proc/config.gz` = the `.config` in `dev`, `CONFIG_CC_VERSION_TEXT` = the shell's `gcc --version`, `.config`
  unchanged after `modules_prepare`, `kernel.release` = `uname -r`.
  `make` always gets `LOCALVERSION=` (empty). Without it `setlocalversion` appends `+` in a git tree and the
  vermagic no longer matches.
- Compiler: the dev shell's `nixpkgs` is the one in this repository's `flake.lock`, while the host kernel is built
  with the host's `nixpkgs` (follows). When the two differ the gcc check stops the script. Then use
  `nix develop .#kernel-dev --override-input nixpkgs github:nixos/nixpkgs/<rev in the host's flake.lock>`.

`build` takes a tree directory (`drivers/media/pci/intel`) or a module name. A name is resolved to its directory
through the path of the installed module (a built-in, `=y`, stops it).
The whole directory is built with `M=`, so `i915` takes a few minutes. When a change alters the signature of an
exported symbol that modules in another directory use, build those modules as well
(`Module.symvers` is the booted kernel's).

`load` needs root (`sudo` is used otherwise). It follows `/sys/module/<m>/holders` and unloads the holders first
(for example `ipu-bridge` → `intel-ipu6`), `insmod`s the new `.ko`, and loads the rest in reverse order (the `.ko`
built in the tree if there is one, `modprobe` otherwise). Device relations that are not symbol dependencies are
not in holders: when changing `int3472`, name the sensor and the VCM too, as in `load int3472 ov5693 dw9719`.
A refcount that remains although there is no holder means a process has the device open (PipeWire and WirePlumber
open `/dev/video*`). The script writes a marker to `/dev/kmsg` and shows the kernel log after it.

Signing: `nix/kernel.nix` sets no module signing configuration and Secure Boot is off (section 3), so an unsigned
`.ko` loads as is.
With any of `CONFIG_MODULE_SIG_FORCE=y`, `module.sig_enforce=1` or lockdown (`/sys/kernel/security/lockdown` not
`none`), `prepare` prints the reason and `load` stops. With only `CONFIG_MODULE_SIG=y` the module loads and the
kernel is tainted with `E`.

`patch` writes the commits after `kdev-base` (or after the last export, `kdev-exported`) to the next numbers in
`kernel/patches/`, in the format of the existing series (`--zero-commit --no-signature`, `[PATCH]` without a
number). Uncommitted changes stop it. The rest is manual: add a row to
[PROVENANCE.md](../kernel/PROVENANCE.md), update the patch range in [kernel/README.md](../kernel/README.md), check
that the series applies with `build-kernel.sh --prepare-only`, commit, then `nix flake update surface-pro-9-linux`
in the host flake and rebuild (only this step builds the whole kernel). After booting the new kernel, recreate the
tree with `prepare --force` (the derivation changed, so the existing tree is refused).

## 6. Remaining checks

These have not been checked on Linux yet. The numbers are fixed; the missing ones (N1, N3, N4, N6, N7, N8, N14)
are checks that are done and reflected in the text above.

| # | Command | What to look at |
|---|---|---|
| N2 | `nix build .#surface-pen-tools` | Whether the shebang is the Python that includes dbus-next. Confirmed: the build, `tests/test_reports.py` passing its 16 tests, and `--help` of both tools |
| N5 | In the host flake, add one test patch to `boot.kernelPatches` and run `nixos-rebuild build --flake .#sp9` | Whether it lands after this repository's 38 (15 linux-surface files + 23 in `kernel/patches`) (the path from `kernel.override` through `nix/kernel.nix` to `buildLinux`), and whether a package taken from `config.boot.kernelPackages` (`cpupower`, for example) builds against this kernel. Confirmed: `mkOverride 90` beats a host's plain `linuxPackages_latest` definition, and `pinctrl_tigerlake` is in the initrd |
| N9 | Build with `audio.codecPatch = "pins"`, `audio.legacy = true` | `/run/current-system/firmware/sp9-alc274-pins.fw` (uncompressed), two lines in `/etc/modprobe.d/nixos.conf` |
| N10 | `hibernateReboot = true` | The drop-in in `/etc/systemd/sleep.conf.d/`, `systemd-analyze cat-config systemd/sleep.conf` |
| N11 | `pen.connectService = true` | The ExecStart path in `systemctl status surface-pen-connect`, the unit being enabled |
| N15 | `pen.feedbackService = true`, without `helper.enable` | The ExecStart path in `systemctl status surface-pen-feedback`, the unit being enabled, the state file `/var/lib/surface-pen/fast-host-id`, and the continuous waveform during a stroke with the pen connected |
| N12 | `bootctl list` | Whether the menu has Windows Boot Manager and Windows boots from it. Confirmed: systemd-boot is on the ESP (`/boot`) and the current entry is a NixOS generation |
| N13 | `nix flake check --no-build` on every nixpkgs update | The 49 keys of the two fragments are not set by `common-config.nix` at present (compared by name). Once it sets one of them to another value, evaluation fails with "conflicting definition values" for `settings.<KEY>.tristate`. Decide then whether to use `lib.mkForce` on the `parseFragment` values |
