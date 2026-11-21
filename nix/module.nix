# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
{ linux-surface }:
{ config, lib, pkgs, ... }:

# Surface Pro 9 (Intel, Surface_Pro_9_2038). Every file comes from userspace/platform/ or
# userspace/pen/ unchanged (the helper is built from helper/, nix/helper.nix); what each one fixes
# and its evidence: userspace/platform/README.md.
# Host integration: docs/nixos.md

let
  cfg = config.hardware.surface-pro-9;
  platform = ../userspace/platform;

  kernel = pkgs.callPackage ./kernel.nix {
    inherit linux-surface;
    inherit (cfg.kernel) ignoreConfigErrors ccacheDir;
  };
  penTools = pkgs.callPackage ./pen-tools.nix { };
  helper = pkgs.callPackage ./helper.nix { };
  digitizerd = pkgs.callPackage ./digitizerd.nix { };
  libcamera = pkgs.callPackage ./libcamera.nix { inherit (cfg.camera) mainsFrequency; };
  pipewire = pkgs.callPackage ./pipewire-libcamera.nix { inherit libcamera; };

  # The file is the one install.sh applies; a parameter switched off below is dropped by its name.
  # i915.enable_psr=1 is install.sh's opt-in component psr-param.
  kernelParams = lib.filter
    (p: builtins.isString p && p != ""
      && (cfg.kernelParams.hpiosize || !lib.hasPrefix "pci=hpiosize=" p))
    (builtins.split "[[:space:]]+" (builtins.readFile (platform + "/cmdline/sp9-platform.cmdline")))
    ++ lib.optional cfg.kernelParams.psr "i915.enable_psr=1";

  # install.sh component psr: the script, its unit (ExecStart pointed at the store) and the udev
  # rule that starts it when the i915 card appears.
  psr1 = pkgs.runCommand "sp9-psr1" { } ''
    install -Dm755 ${platform + "/psr/sp9-psr1"} $out/lib/sp9-platform/sp9-psr1
    install -Dm644 ${platform + "/psr/sp9-psr1.service"} $out/lib/systemd/system/sp9-psr1.service
    substituteInPlace $out/lib/systemd/system/sp9-psr1.service \
      --replace-fail /usr/lib/sp9-platform/sp9-psr1 $out/lib/sp9-platform/sp9-psr1
    install -Dm644 ${platform + "/udev/rules.d/61-sp9-psr1.rules"} $out/lib/udev/rules.d/61-sp9-psr1.rules
  '';

  # One file at $out/<dest>.
  fileAt = name: dest: src: passthru: pkgs.runCommand name { inherit passthru; } ''
    install -Dm644 ${src} $out/${dest}
  '';

  speakers = fileAt "sp9-speakers"
    "share/pipewire/pipewire.conf.d/60-sp9-speakers.conf"
    (platform + "/pipewire/pipewire.conf.d/60-sp9-speakers.conf")
    # fastLookaheadLimiter (swh). The NixOS pipewire module puts these on PipeWire's LADSPA_PATH;
    # without them the module is skipped (nofail) and only the raw speaker sink remains.
    { requiredLadspaPackages = [ pkgs.ladspaPlugins ]; };
  speakerRaw = fileAt "sp9-speaker-raw"
    "share/wireplumber/wireplumber.conf.d/60-sp9-speaker-raw.conf"
    (platform + "/wireplumber/wireplumber.conf.d/60-sp9-speaker-raw.conf")
    { };
  echoCancel = fileAt "sp9-echo-cancel"
    "share/pipewire/pipewire.conf.d/61-sp9-echo-cancel.conf"
    (platform + "/pipewire/pipewire.conf.d/61-sp9-echo-cancel.conf")
    { };
  mic = fileAt "sp9-mic"
    "share/wireplumber/wireplumber.conf.d/60-sp9-mic.conf"
    (platform + "/wireplumber/wireplumber.conf.d/60-sp9-mic.conf")
    { };

  sensors = fileAt "sp9-sensors"
    "lib/udev/rules.d/61-sp9-sensors.rules"
    (platform + "/udev/rules.d/61-sp9-sensors.rules")
    { };

  codecFirmware = fileAt "sp9-alc274-pins"
    "lib/firmware/sp9-alc274-pins.fw"
    (platform + "/optional/firmware/sp9-alc274-pins.fw")
    { compressFirmware = false; };
in
{
  options.hardware.surface-pro-9 = {
    enable = lib.mkEnableOption "Surface Pro 9 (Intel) platform support";

    kernel = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Boot v7.3.4 + linux-surface patches/7.3 + kernel/patches (nix/kernel.nix). Built locally:
          no binary cache has it. Off: the host's own boot.kernelPackages stays.
        '';
      };
      ignoreConfigErrors = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Turn generate-config.pl errors (a fragment option missing from .config or set to another
          value) into warnings. Off, the build stops like tools/linux/build-kernel.sh does.
        '';
      };
      ccacheDir = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        example = "/var/cache/ccache";
        description = ''
          Compile the kernel through ccache with this cache directory. Without it every change to a patch
          compiles all of the kernel again; with it only the changed objects are compiled (a config change
          still misses almost everything: every object includes autoconf.h). Sets the directory up
          (root:nixbld 2770) and adds it to the Nix sandbox. The first build that has this option must
          already see the directory: the system that creates it is the one being built. Create it by hand
          once and pass --option extra-sandbox-paths <dir> to that build (docs/nixos.md).
          The kernel output then depends on what the cache holds, which Nix cannot check.
        '';
      };
    };

    display.psr1 = lib.mkOption {
      type = lib.types.bool;
      default = !cfg.kernel.enable;
      defaultText = lib.literalExpression "!config.hardware.surface-pro-9.kernel.enable";
      description = ''
        PSR1 only on the internal panel (as on Windows) on a kernel without patch 0014: a oneshot
        unit, started by a udev rule when the i915 card appears, writes 3 (FORCE_PSR1) to debugfs
        i915_edp_psr_debug. Same effect as i915.enable_psr=1 but without its taint. A value other
        than 0 is left alone (the helper's PSR policy). Not possible under kernel lockdown; see
        kernelParams.psr. With kernel.enable it changes nothing (the quirk already gives PSR1).
      '';
    };

    kernelParams = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Kernel command line from userspace/platform/cmdline/sp9-platform.cmdline.";
      };
      psr = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Pass i915.enable_psr=1 (PSR1 only). The parameter is an unsafe i915 option and taints the
          kernel on every boot (U, 64: "Setting dangerous option enable_psr"). It is not needed:
          with kernel.enable patch 0014 turns PSR2 off by a quirk (a boot without the parameter gave
          tainted 0 and "PSR mode: PSR1 enabled", measured, docs/nixos.md), and with another kernel
          display.psr1 sets the same limit through i915 debugfs without a taint. Use it only where
          debugfs cannot be written (kernel lockdown).
        '';
      };
      hpiosize = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Pass pci=hpiosize=0. It keeps hotplug bridge I/O windows off the ACPI PM1/GPE registers at
          0x1800 (GPE storm on lid close and Thunderbolt hotplug); 7.3.4 reserves that range through
          PNP0C02 "PCHRESV" anyway, so with kernel.enable it is not needed: a boot with this set to
          false had no bridge window inside 1800-18fe and no GPE errors over s2idle and lid cycles
          (measured, docs/nixos.md; a Thunderbolt or USB-C hotplug was not tested). To see the same
          on your device, set this to false, rebuild, reboot, then
            journalctl -k -b | grep 'Reserved \[io  0x1800'    # the reservation is still made
            sudo cat /proc/ioports                              # no bridge window inside 1800-18fe
            grep -v '^ *0 ' /sys/firmware/acpi/interrupts/gpe*  # before and after closing the lid
                                                                # and plugging a Thunderbolt device
          No window may overlap and no GPE counter may run away.
        '';
      };
    };

    touch.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Touch and pen from the digitizer's raw data, by the daemon chosen with touch.daemon.";
    };

    touch.daemon = lib.mkOption {
      type = lib.types.enum [ "iptsd" "digitizerd" ];
      default = "iptsd";
      description = ''
        Which daemon turns the digitizer's raw data into the touchscreen and the stylus.
        "iptsd": pkgs.iptsd with the patches of userspace/iptsd and the preset
        userspace/platform/iptsd/surface-pro-9.conf, one instance per hidraw node, started by
        its udev rule. "digitizerd": sp9-digitizerd (helper/crates/sp9-digitizerd), one service that finds the node by its ID, keeps its input devices
        across suspend, and takes setting changes from sp9-helperd over D-Bus without a restart;
        iptsd is then neither built nor installed. The input devices have the same names with
        both.'';
    };

    sensors.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        iio-sensor-proxy (hardware.sensor.iio.enable, mkDefault) for automatic rotation and
        brightness, and the udev rule that sets the accelerometer's change sensitivity to 0:
        without it the proxy finds the buffer empty, falls back to polled reads, and those
        take 5 s and return 0 on this hub.
      '';
    };

    diagnostics.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        The tools docs/test-plan.md and tools/linux/post-install-check.sh call, in the system PATH:
        iptsd-check-device, iptsd-calibrate, iptsd-dump (iptsd), cam (libcamera), v4l2-ctl, media-ctl,
        edid-decode (v4l-utils), amixer, arecord, speaker-test (alsa-utils), hda-verb (alsa-tools),
        pactl, libinput, hid-decode, evtest, drm_info, i2ctransfer, intel_vbt_decode, acpidump, iasl,
        sensors, lspci, lsusb. Without them the collector leaves those sections empty.
        iptsd-dump, iptsd-calibrate and iptsd-show put the digitizer back in single-touch mode when
        they exit and the daemon does not set it again: stop 'iptsd@*' before running them and start
        it afterwards.
      '';
    };

    speakers.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        PipeWire "Speakers" filter chain, the WirePlumber rule naming the raw sink, and the user
        unit that puts the raw sink at 100 % after every WirePlumber start
        (userspace/platform/pipewire/sp9-speaker-raw-volume).
      '';
    };

    microphone.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        WirePlumber rule for the internal microphones: first-use gain +30 dB (Capture +30 dB, Mic
        Boost 0 dB) instead of +60 dB, which clips. A volume WirePlumber has stored for the user is
        kept; to get the default on an account that already recorded at 100 %:
        wpctl set-volume @DEFAULT_AUDIO_SOURCE@ 32%.
      '';
    };

    audio = {
      echoCancel = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          PipeWire source "Microphone (echo cancelled)" (sp9_echo_cancel): the internal microphones
          minus what the internal speakers play. Selected per application; the default source and
          sink do not change. Needs speakers.enable, which names the sink the reference comes from.
        '';
      };
      codecPatch = lib.mkOption {
        type = lib.types.enum [ "none" "pins" ];
        default = "none";
        description = ''
          ALC274 patch firmware for snd_hda_intel patch= (install.sh component hda-pins): the pin
          defaults of the Windows driver. snd_hda_intel drives the codec on this device without
          audio.legacy, so the patch is read as is. "pins" changes nothing here: /sys/class/sound/hwC0D0/init_pin_configs already equals
          the Windows table.
        '';
      };
      legacy = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Force legacy HDA (snd_intel_dspcfg dsp_driver=1). Only for a firmware on which SOF binds
          00:1f.3: with the current one snd_hda_intel binds without this.
        '';
      };
    };

    camera.pipewire = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Leave WirePlumber's libcamera monitor on, which is what gives PipeWire applications
        (browsers, portals) the two cameras. nixpkgs' libcamera opens the rear lens sub-device
        (dw9719) when it enumerates and keeps it open; after the first focus write the lens and,
        through its runtime-PM link, the rear sensor then stay powered until WirePlumber exits
        (kernel patch 0021). With camera.libcamera the lens device is only open while the camera
        streams and this cost is gone.
        false disables the monitor: PipeWire applications lose both cameras, programs that use
        libcamera themselves (cam) keep them. Compare
          cat /sys/bus/i2c/devices/i2c-OVTID858:00/power/runtime_status
        Taking the lens node away from the user does not work: libcamera then rejects the whole
        media device ("Device node /dev/v4l-subdev6 can't be accessed") and lists no camera at
        all.
      '';
    };

    camera.libcamera = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        libcamera 0.8.0 with userspace/libcamera/*.patch (nix/libcamera.nix) for the PipeWire
        session: services.pipewire.package and services.pipewire.wireplumber.package become
        builds against it (mkDefault), and `cam` in the system profile is the patched one. The
        patches: GPU debayering without the magenta lines and with a centred, filtered
        reduction, exposure regulation that settles in a few frames and stays on the gain range
        the sensors follow, flicker-free exposure times, contrast autofocus for the rear camera,
        and a rear lens that is only powered while the camera streams.
        Only the two PipeWire packages are rebuilt. Programs that link libcamera themselves
        (GStreamer's libcamerasrc) keep nixpkgs' libcamera.
        false leaves PipeWire and libcamera as nixpkgs has them.
      '';
    };

    camera.mainsFrequency = lib.mkOption {
      type = lib.types.enum [ 50 60 ];
      default = 60;
      description = ''
        Mains frequency of the lighting. With camera.libcamera the exposure time stays at a
        multiple of its flicker period (8.33 ms at 60 Hz, 10 ms at 50 Hz) once it reaches one,
        which keeps bands and brightness beats out of the picture under mains-powered light.
      '';
    };

    hibernateReboot = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Reboot instead of powering off after writing the hibernation image.";
    };

    pen = {
      enable = lib.mkEnableOption "pen-haptic, surface-pen-connect and surface-pen-feedback in PATH (prototypes, userspace/pen)";
      connectService = lib.mkEnableOption "the surface-pen-connect system service (implies pen.enable)";
      feedbackService = lib.mkEnableOption ''
        the surface-pen-feedback system service: pen speed to the digitizer, which the Slim Pen 2
        needs for its continuous haptic waveforms, and inking haptics on contact (implies
        pen.enable). The helper does the same (sp9-ctl pen ink on) and both write the same
        digitizer report, so with helper.enable the unit is installed but not started and a
        warning says so: use the helper, or leave its pen haptics off and start the unit by hand
        (systemctl start surface-pen-feedback)'';
    };

    helper.enable = lib.mkEnableOption ''
      sp9-helperd (system D-Bus service io.github.joonhoekim.OpenDeviceHelper1) and sp9-ctl:
      performance limits by platform_profile, battery and smart charging, PSR policy, keyboard
      backlight, touch and pen settings, diagnostics (helper/, docs/helper.md). Settings live in
      /etc/sp9/helper.toml, written by the daemon. Pen: inking haptics and the pen speed to
      the digitizer (sp9-ctl pen ink on; replaces pen.feedbackService), and coupling a pen put
      into the cover slot (sp9-ctl pen auto-couple on, or pen couple once), which needs
      hardware.bluetooth and kernel.enable (kernel/patches 0023)'';

    digitizerd.package = lib.mkEnableOption ''
      the sp9-digitizerd package (helper/crates/sp9-digitizerd) next to
      iptsd: the daemon, its replay tool and its unit, which nothing starts. To run it instead
      of iptsd set touch.daemon = "digitizerd"'';
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      # Wi-Fi (AX211), i915 DMC, SOF topology, IPU6.
      hardware.enableRedistributableFirmware = lib.mkDefault true;

      # Power and volume buttons stay dead when soc_button_array probes before the GPIO controller.
      boot.initrd.kernelModules = [ "pinctrl_tigerlake" ];
    }

    (lib.mkIf cfg.kernel.enable {
      # Hosts commonly set boot.kernelPackages without a priority;
      # 90 beats that and still yields to mkForce.
      boot.kernelPackages = lib.mkOverride 90 (pkgs.linuxPackagesFor kernel);
    })

    (lib.mkIf (cfg.kernel.enable && cfg.kernel.ccacheDir != null) {
      nix.settings.extra-sandbox-paths = [ cfg.kernel.ccacheDir ];
      systemd.tmpfiles.rules = [ "d ${cfg.kernel.ccacheDir} 2770 root nixbld -" ];
    })

    (lib.mkIf cfg.kernelParams.enable {
      boot.kernelParams = kernelParams;
    })

    (lib.mkIf cfg.display.psr1 {
      services.udev.packages = [ psr1 ];
      systemd.packages = [ psr1 ];
      systemd.services.sp9-psr1.path = [ pkgs.coreutils pkgs.gnugrep ];
    })

    (lib.mkIf (cfg.touch.enable && cfg.touch.daemon == "digitizerd") {
      environment.systemPackages = [ digitizerd ];
      systemd.packages = [ digitizerd ];
      services.dbus.packages = [ digitizerd ];
      # NixOS ignores [Install] of packaged units.
      systemd.services.sp9-digitizerd.wantedBy = [ "multi-user.target" ];
      # /dev/uinput is opened once, when the digitizer is first seen.
      boot.kernelModules = [ "uinput" ];
      # sp9-helperd keeps the touch and pen settings in a drop-in there, which the daemon reads
      # when it starts; without iptsd's preset nothing else creates the directory.
      systemd.tmpfiles.rules = [ "d /etc/iptsd.d 0755 root root -" ];
    })

    (lib.mkIf cfg.sensors.enable {
      hardware.sensor.iio.enable = lib.mkDefault true;
      services.udev.packages = [ sensors ];
    })

    (lib.mkIf cfg.touch.enable {
      # libinput reads one override file. `text` is of type lines, so other modules can add to it.
      environment.etc."libinput/local-overrides.quirks".text =
        builtins.readFile (platform + "/libinput/sp9-stylus.quirks");
    })

    (lib.mkIf (cfg.touch.enable && cfg.touch.daemon == "iptsd") {
      services.iptsd.enable = true;
      # services.iptsd takes pkgs.iptsd. The pen processing patches of userspace/iptsd (each behaviour is
      # off unless a config turns it on: the preset and the helper's drop-in do).
      nixpkgs.overlays = [
        (final: prev: {
          iptsd = prev.iptsd.overrideAttrs (old: {
            patches = (old.patches or [ ]) ++ (
              # every patch of userspace/iptsd, in the order of their numbers
              map (n: ../userspace/iptsd + "/${n}") (
                builtins.filter (lib.hasSuffix ".patch") (
                  builtins.attrNames (builtins.readDir ../userspace/iptsd)
                )
              )
            );
          });
        })
      ];
      # A drop-in, loaded after /etc/iptsd.conf (services.iptsd.config) and only for 045E:0C52.
      # The preset directory is inside the iptsd store path.
      environment.etc."iptsd.d/surface-pro-9.conf".source = platform + "/iptsd/surface-pro-9.conf";
    })

    (lib.mkIf cfg.diagnostics.enable {
      environment.systemPackages = with pkgs; [
        (if cfg.camera.libcamera then libcamera else pkgs.libcamera)
        v4l-utils alsa-utils alsa-tools pulseaudio libinput hid-tools evtest drm_info
        i2c-tools intel-gpu-tools acpica-tools lm_sensors pciutils usbutils
      ]
      # services.iptsd only registers the unit and the udev rule of the package.
      ++ lib.optional (cfg.touch.enable && cfg.touch.daemon == "iptsd") iptsd;
    })

    (lib.mkIf cfg.speakers.enable {
      services.pipewire.configPackages = [ speakers ];
      services.pipewire.wireplumber.configPackages = [ speakerRaw ];
      systemd.user.services.sp9-speaker-raw-volume = {
        description = "Raw speaker sink at 100 % (the Speakers filter sink is the volume control)";
        # wantedBy and not a login-time unit: a restarted WirePlumber restores the stored route again.
        wantedBy = [ "wireplumber.service" ];
        after = [ "wireplumber.service" ];
        path = [
          config.services.pipewire.package
          config.services.pipewire.wireplumber.package
          pkgs.coreutils
          pkgs.gnused
        ];
        serviceConfig = {
          Type = "oneshot";
          ExecStart = "${pkgs.runtimeShell} ${platform + "/pipewire/sp9-speaker-raw-volume"}";
        };
      };
    })

    (lib.mkIf (cfg.audio.echoCancel && cfg.speakers.enable) {
      services.pipewire.configPackages = [ echoCancel ];
    })

    (lib.mkIf cfg.microphone.enable {
      services.pipewire.wireplumber.configPackages = [ mic ];
    })

    (lib.mkIf (cfg.audio.codecPatch == "pins") {
      hardware.firmware = [ codecFirmware ];
      boot.extraModprobeConfig =
        builtins.readFile (platform + "/optional/modprobe.d/sp9-alc274-pins.conf");
    })

    (lib.mkIf cfg.audio.legacy {
      boot.extraModprobeConfig = builtins.readFile (platform + "/optional/modprobe.d/sp9-audio-legacy.conf");
    })

    (lib.mkIf cfg.camera.libcamera {
      services.pipewire.package = lib.mkDefault pipewire;
      services.pipewire.wireplumber.package =
        lib.mkDefault (pkgs.wireplumber.override { inherit pipewire; });
    })

    (lib.mkIf (!cfg.camera.pipewire) {
      services.pipewire.wireplumber.extraConfig."60-sp9-no-libcamera"."wireplumber.profiles".main = {
        "monitor.libcamera" = "disabled";
      };
    })

    (lib.mkIf cfg.hibernateReboot {
      environment.etc."systemd/sleep.conf.d/sp9-hibernate-reboot.conf".source =
        platform + "/optional/sleep.conf.d/sp9-hibernate-reboot.conf";
    })

    (lib.mkIf (cfg.pen.enable || cfg.pen.connectService || cfg.pen.feedbackService) {
      environment.systemPackages = [ penTools ];
    })

    (lib.mkIf cfg.helper.enable {
      environment.systemPackages = [ helper ];
      systemd.packages = [ helper ];
      services.dbus.packages = [ helper ];
      security.polkit.enable = true;
      # /dev/surface/aggregator: smart charging and the experimental fan request.
      boot.kernelModules = [ "surface_aggregator_cdev" ];
      systemd.services.sp9-helperd = {
        wantedBy = [ "multi-user.target" ];
        # Diagnostics run tools/linux/post-install-check.sh; it skips what is not in PATH.
        path = with pkgs; [
          bash coreutils findutils gawk gnugrep gnused gnutar gzip iproute2 kmod pciutils procps
          usbutils util-linux
        ];
        environment.SP9_PYTHON = "${pkgs.python3}/bin/python3";
      };
    })

    (lib.mkIf cfg.digitizerd.package {
      environment.systemPackages = [ digitizerd ];
      systemd.packages = [ digitizerd ];
    })

    (lib.mkIf cfg.pen.feedbackService {
      systemd.packages = [ penTools ];
      # sp9-helperd feeds the digitizer too (pen.ink_feedback with pen.speed_feedback); two
      # writers of feature report 9 undo each other's speed value.
      systemd.services.surface-pen-feedback.wantedBy = lib.mkIf (!cfg.helper.enable) [ "multi-user.target" ];
      warnings = lib.optional cfg.helper.enable ''
        hardware.surface-pro-9: pen.feedbackService and helper.enable are both set. Both feed the
        pen speed to the digitizer, so surface-pen-feedback.service is not started at boot. Use
        the helper (sp9-ctl pen ink on), or drop helper.enable to keep the service.
      '';
    })

    (lib.mkIf cfg.pen.connectService {
      hardware.bluetooth.enable = lib.mkDefault true;
      # The unit is userspace/pen/systemd/surface-pen-connect.service with ExecStart rewritten;
      # NixOS ignores [Install] of packaged units.
      systemd.packages = [ penTools ];
      systemd.services.surface-pen-connect.wantedBy = [ "multi-user.target" ];
    })
  ]);
}
