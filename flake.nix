# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
{
  description = "Surface Pro 9 (Intel) on NixOS: linux-surface kernel with this repo's patches, iptsd preset, speaker DSP, pen tools, device helper";

  inputs = {
    # The module builds with the consumer's pkgs; this nixpkgs only serves `packages`.
    # Consumers should set `inputs.nixpkgs.follows` so flake.lock does not pin a second nixpkgs.
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    # Must be the commit in kernel/README.md and tools/linux/build-kernel.sh (patches/7.3, BASE = v7.3.4).
    # A flake input rather than fetchFromGitHub: the config fragment and the patch list are read at
    # evaluation time, which on a fetcher derivation would be import-from-derivation.
    linux-surface = {
      url = "github:linux-surface/linux-surface/ea852b4eff5ed641571e13766bda5bd9cf121983";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, linux-surface }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      nixosModules = {
        surface-pro-9 = import ./nix/module.nix { inherit linux-surface; };
        default = self.nixosModules.surface-pro-9;
      };

      packages.${system} = {
        kernel = pkgs.callPackage ./nix/kernel.nix { inherit linux-surface; };
        surface-pen-tools = pkgs.callPackage ./nix/pen-tools.nix { };
        sp9-helper = pkgs.callPackage ./nix/helper.nix { };
      };

      # tools/linux/kmod-dev.sh: module rebuilds against the booted kernel. The kernel derivation's own
      # build inputs (stdenv gcc, flex, bison, bc, perl, elfutils, openssl, pahole, python3, kmod, ...)
      # plus what the script uses. The compiler must be the one the host built the kernel with
      # (CONFIG_CC_VERSION_TEXT, checked); --override-input nixpkgs to the host's revision otherwise.
      devShells.${system}.kernel-dev = pkgs.mkShell {
        inputsFrom = [ self.packages.${system}.kernel ];
        packages = with pkgs; [
          gnumake flex bison bc perl elfutils openssl pahole python3 kmod
          git jq gnupatch xz zstd bzip2 cpio
        ];
        # buildLinux disables these too; the cc wrapper would otherwise add -fstack-protector-strong,
        # -D_FORTIFY_SOURCE and PIC flags to every kernel object.
        hardeningDisable = [ "all" ];
      };

      checks.${system} = {
        inherit (self.packages.${system}) surface-pen-tools sp9-helper;
        # The module with every option on. Only the system's drvPath is written, so building this
        # check evaluates the kernel without building it.
        module-eval = pkgs.writeText "surface-pro-9-module-eval" (builtins.unsafeDiscardStringContext
          (nixpkgs.lib.nixosSystem {
            modules = [
              self.nixosModules.default
              {
                nixpkgs.hostPlatform = system;
                system.stateVersion = nixpkgs.lib.trivial.release;
                boot.loader.grub.enable = false;
                fileSystems."/" = { device = "/dev/disk/by-label/nixos"; fsType = "ext4"; };
                services.pipewire.enable = true;
                hardware.surface-pro-9 = {
                  enable = true;
                  audio = { legacy = true; codecPatch = "pins"; };
                  hibernateReboot = true;
                  pen.connectService = true;
                  helper.enable = true;
                };
              }
            ];
          }).config.system.build.toplevel.drvPath);
      };
    };
}
