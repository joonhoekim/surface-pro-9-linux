# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
{ lib, stdenvNoCC, python3 }:

# userspace/pen as is. The scripts put their own realpath directory on sys.path, so they stay next
# to surface_pen/ in libexec and bin/ only holds symlinks.

let
  python = python3.withPackages (ps: [ ps.dbus-next ]);
  root = ../userspace/pen;
in
stdenvNoCC.mkDerivation {
  pname = "surface-pen-tools";
  version = "0-unstable";

  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      (root + "/pen-haptic")
      (root + "/surface-pen-connect")
      (root + "/surface-pen-feedback")
      (root + "/surface_pen")
      (root + "/systemd")
      (root + "/tests")
    ];
  };

  # patchShebangs resolves `#!/usr/bin/env python3` against this.
  buildInputs = [ python ];

  doCheck = true;
  checkPhase = ''
    runHook preCheck
    ${python}/bin/python3 -I -B tests/test_reports.py
    runHook postCheck
  '';

  installPhase = ''
    runHook preInstall
    install -d $out/bin $out/libexec/surface-pen
    cp -r surface_pen $out/libexec/surface-pen/
    install -m755 pen-haptic surface-pen-connect surface-pen-feedback $out/libexec/surface-pen/
    ln -s ../libexec/surface-pen/pen-haptic $out/bin/pen-haptic
    ln -s ../libexec/surface-pen/surface-pen-connect $out/bin/surface-pen-connect
    ln -s ../libexec/surface-pen/surface-pen-feedback $out/bin/surface-pen-feedback
    install -Dm644 systemd/surface-pen-connect.service \
      $out/lib/systemd/system/surface-pen-connect.service
    substituteInPlace $out/lib/systemd/system/surface-pen-connect.service \
      --replace-fail /opt/surface-pen/surface-pen-connect $out/libexec/surface-pen/surface-pen-connect
    install -Dm644 systemd/surface-pen-feedback.service \
      $out/lib/systemd/system/surface-pen-feedback.service
    substituteInPlace $out/lib/systemd/system/surface-pen-feedback.service \
      --replace-fail /opt/surface-pen/surface-pen-feedback $out/libexec/surface-pen/surface-pen-feedback
    runHook postInstall
  '';

  meta = {
    description = "Surface Slim Pen 2 haptics and reconnect helper (prototype)";
    platforms = lib.platforms.linux;
    mainProgram = "pen-haptic";
  };
}
