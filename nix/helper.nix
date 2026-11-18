# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
{ lib, rustPlatform, coreutils, bash, pkg-config, wrapGAppsHook4, gtk4, libadwaita }:

# helper/ (sp9-helperd, sp9-ctl, the GTK settings app sp9-settings) with its unit, D-Bus and polkit
# files, the app's desktop entry, icon and metainfo, and the two scripts the daemon runs from tools/
# (post-install-check.sh for diagnostics, sdc-blob.py for the panel ICC). The daemon finds the
# scripts in <its own directory>/sp9-helper, i.e. $out/libexec/sp9-helper.

let
  root = ../.;
in
rustPlatform.buildRustPackage {
  pname = "sp9-helper";
  version = "0.1.0";

  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      (root + "/helper/Cargo.toml")
      (root + "/helper/Cargo.lock")
      (root + "/helper/crates")
      (root + "/helper/data")
      (root + "/tools/linux/post-install-check.sh")
      (root + "/tools/sdc-blob.py")
    ];
  };

  cargoLock.lockFile = root + "/helper/Cargo.lock";
  cargoRoot = "helper";
  buildAndTestSubdir = "helper";

  # sp9-settings is outside the workspace's default members (so the daemon builds without GTK);
  # build and test every member here, except the digitizer daemon (nix/digitizerd.nix).
  cargoBuildFlags = [ "--workspace" "--exclude" "sp9-digitizer" "--exclude" "sp9-digitizerd" ];
  cargoTestFlags = [ "--workspace" "--exclude" "sp9-digitizer" "--exclude" "sp9-digitizerd" ];

  nativeBuildInputs = [ pkg-config wrapGAppsHook4 ];
  # patchShebangs (fixup) resolves post-install-check.sh's `#!/usr/bin/env bash` against bash.
  buildInputs = [ bash gtk4 libadwaita ];

  # Only the app needs the GTK environment; the daemon and the CLI stay unwrapped.
  dontWrapGApps = true;
  postFixup = ''
    wrapGApp $out/bin/sp9-settings
  '';

  postInstall = ''
    install -d $out/libexec/sp9-helper
    mv $out/bin/sp9-helperd $out/libexec/sp9-helperd
    install -m755 tools/linux/post-install-check.sh $out/libexec/sp9-helper/post-install-check.sh
    install -m644 tools/sdc-blob.py $out/libexec/sp9-helper/sdc-blob.py

    d=helper/data
    install -Dm644 $d/sp9-helperd.service $out/lib/systemd/system/sp9-helperd.service
    substituteInPlace $out/lib/systemd/system/sp9-helperd.service --replace-fail @LIBEXECDIR@ $out/libexec
    install -Dm644 $d/io.github.joonhoekim.OpenDeviceHelper1.conf \
      $out/share/dbus-1/system.d/io.github.joonhoekim.OpenDeviceHelper1.conf
    install -Dm644 $d/io.github.joonhoekim.OpenDeviceHelper1.service \
      $out/share/dbus-1/system-services/io.github.joonhoekim.OpenDeviceHelper1.service
    substituteInPlace $out/share/dbus-1/system-services/io.github.joonhoekim.OpenDeviceHelper1.service \
      --replace-fail /bin/false ${coreutils}/bin/false
    install -Dm644 $d/io.github.joonhoekim.opendevicehelper.sp9.policy \
      $out/share/polkit-1/actions/io.github.joonhoekim.opendevicehelper.sp9.policy
    install -Dm644 $d/helper.toml.example $out/share/doc/sp9-helper/helper.toml.example

    app=io.github.joonhoekim.OpenDeviceHelper.SurfacePro9
    install -Dm644 $d/$app.desktop $out/share/applications/$app.desktop
    install -Dm644 $d/$app.svg $out/share/icons/hicolor/scalable/apps/$app.svg
    install -Dm644 $d/$app.metainfo.xml $out/share/metainfo/$app.metainfo.xml
  '';

  meta = {
    description = "Surface Pro 9 (Intel) device helper: D-Bus service, CLI and GTK settings app";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "sp9-ctl";
  };
}
