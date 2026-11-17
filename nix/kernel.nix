# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Other arguments go through to buildLinux: boot.kernelPackages calls kernel.override with
# kernelPatches, features and randstructSeed, and callPackage's override lands here.
{ lib, buildLinux, fetchurl, stdenv, ccache, writeShellScript, linux-surface
, ignoreConfigErrors ? false
, # Directory of a ccache shared by all builds of this kernel, or null. It has to exist, be writable by the
  # build users and be visible inside the sandbox (nix/module.nix kernel.ccacheDir does all three).
  ccacheDir ? null
, ...
}@args:

# v7.3.4 + linux-surface patches/7.3 + kernel/patches, the same tree tools/linux/build-kernel.sh builds.
# The config differs: the base is nixpkgs common-config.nix, not Ubuntu's mainline config. The two
# fragments only add to it, in merge_config.sh order (sp9.config wins).

let
  version = "7.3.4";
  series = lib.versions.majorMinor version;

  patchesIn = prefix: dir:
    map (n: {
      name = "${prefix}/${lib.removeSuffix ".patch" n}";
      patch = dir + "/${n}";
    }) (lib.sort lib.lessThan
      (lib.filter (lib.hasSuffix ".patch") (lib.attrNames (builtins.readDir dir))));

  # Kconfig fragment -> structuredExtraConfig. Keys lose the CONFIG_ prefix: generate-config.pl
  # matches answers against .config names without it, and a prefixed key is only an "unused
  # option" (fatal unless ignoreConfigErrors). "is not set" lines become optional: a symbol whose
  # dependencies are off never reaches .config, which means the same.
  parseFragment = text:
    let
      value = v:
        if v == "y" then lib.kernel.yes
        else if v == "m" then lib.kernel.module
        else if v == "n" then lib.kernel.no
        else
          let quoted = builtins.match ''"(.*)"'' v; in
          lib.kernel.freeform (if quoted == null then v else lib.head quoted);
      entry = line:
        let
          l = lib.trim line;
          set = builtins.match "CONFIG_([A-Za-z0-9_]+)=(.*)" l;
          unset = builtins.match "# CONFIG_([A-Za-z0-9_]+) is not set" l;
        in
        if set != null then [ (lib.nameValuePair (lib.elemAt set 0) (value (lib.elemAt set 1))) ]
        else if unset != null then [ (lib.nameValuePair (lib.head unset) (lib.kernel.option lib.kernel.no)) ]
        else [ ];
    in
    lib.listToAttrs (lib.concatMap entry (lib.splitString "\n" text));

  # A patch change rebuilds the whole kernel: the derivation changes and Nix keeps no objects between
  # builds. With the cache only the objects whose preprocessed source changed are compiled again.
  # Not ccacheStdenv: the kernel takes CC from stdenv.cc.cc (common-flags.nix), which under ccacheStdenv is
  # the links package without a main program. A later CC= on the make command line wins instead.
  # The fallback keeps the derivation buildable where the directory is not in the sandbox.
  ccacheCC = writeShellScript "sp9-kernel-cc" ''
    if [ -w ${lib.escapeShellArg ccacheDir} ]; then
      export CCACHE_DIR=${lib.escapeShellArg ccacheDir}
      export CCACHE_UMASK=007 CCACHE_COMPRESS=1 CCACHE_MAXSIZE=10G
      export CCACHE_SLOPPINESS=random_seed,time_macros
      exec ${ccache}/bin/ccache ${lib.getExe stdenv.cc.cc} "$@"
    fi
    exec ${lib.getExe stdenv.cc.cc} "$@"
  '';
in
buildLinux (removeAttrs args [ "linux-surface" "ccacheDir" "ccache" "writeShellScript" ] // {
  inherit version ignoreConfigErrors;
  modDirVersion = version;
  src = fetchurl {
    url = "mirror://kernel/linux/kernel/v${lib.versions.major version}.x/linux-${version}.tar.xz";
    # kernel.org sha256sums.asc: b11b6df8295e8e991ff040b71aaa4994a8b55caa23b6b092907f195b5840fd1f
    hash = "sha256-sRtt+Clejpkf8EC3GqpJlKi1XKojtrCSkH8ZW1hA/R8=";
  };

  # Applied with patch(1) in this order; kernel/README.md checks the same order applies with git am.
  kernelPatches =
    patchesIn "linux-surface" "${linux-surface}/patches/${series}"
    ++ patchesIn "sp9" ../kernel/patches
    ++ args.kernelPatches or [ ];

  structuredExtraConfig =
    parseFragment (builtins.readFile "${linux-surface}/configs/surface-${series}.config")
    // parseFragment (builtins.readFile ../kernel/config/sp9.config);

  extraMeta.branch = series;
} // lib.optionalAttrs (ccacheDir != null) {
  extraMakeFlags = (args.extraMakeFlags or [ ]) ++ [ "CC=${ccacheCC}" ];
})
