# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
{ lib, rustPlatform }:

# helper/crates/sp9-digitizer and sp9-digitizerd: the touch and pen daemon for the digitizer
# (hidraw in, uinput out), the capture replay tool, the unit and the D-Bus policy of the daemon's
# service. The other crates of the workspace are only read for the lock file.

let
  root = ../.;
in
rustPlatform.buildRustPackage {
  pname = "sp9-digitizerd";
  version = "0.1.0";

  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      (root + "/helper/Cargo.toml")
      (root + "/helper/Cargo.lock")
      (root + "/helper/crates")
      (root + "/helper/data/sp9-digitizerd.service")
      (root + "/helper/data/io.github.joonhoekim.Sp9Digitizer1.conf")
      # A test holds the built-in configuration and this preset together.
      (root + "/userspace/platform/iptsd/surface-pro-9.conf")
    ];
  };

  cargoLock.lockFile = root + "/helper/Cargo.lock";
  cargoRoot = "helper";
  buildAndTestSubdir = "helper";

  cargoBuildFlags = [ "-p" "sp9-digitizerd" ];
  cargoTestFlags = [ "-p" "sp9-digitizer" "-p" "sp9-digitizerd" ];

  postInstall = ''
    install -Dm644 helper/data/sp9-digitizerd.service $out/lib/systemd/system/sp9-digitizerd.service
    substituteInPlace $out/lib/systemd/system/sp9-digitizerd.service --replace-fail @BINDIR@ $out/bin
    install -Dm644 helper/data/io.github.joonhoekim.Sp9Digitizer1.conf \
      $out/share/dbus-1/system.d/io.github.joonhoekim.Sp9Digitizer1.conf
  '';

  meta = {
    description = "Surface Pro 9 (Intel) touch and pen daemon: raw digitizer data to uinput devices";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "sp9-digitizerd";
  };
}
