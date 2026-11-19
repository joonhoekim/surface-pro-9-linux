{ lib, libcamera, fetchgit, mainsFrequency ? 60 }:

# libcamera 0.8.0 with userspace/libcamera/*.patch: what the two cameras need from the simple
# pipeline and its software ISP (docs/gaps/camera.md 6). nixpkgs' recipe with another source.
#
# mainsFrequency (50 or 60): the exposure time stays at a multiple of the flicker period of the
# lighting (10 ms or 8.33 ms). The tuning files carry the 60 Hz value.

assert lib.assertOneOf "mainsFrequency" mainsFrequency [ 50 60 ];

libcamera.overrideAttrs (old: rec {
  version = "0.8.0";

  src = fetchgit {
    url = "https://git.libcamera.org/libcamera/libcamera.git";
    rev = "v${version}";
    hash = "sha256-HPQZEzxH1QqmNpMVvKeYl2o7rxfNNP+npCyzYHYyFP8=";
  };

  patches = (old.patches or [ ]) ++ (
    # every patch of userspace/libcamera, in the order of their numbers
    map (n: ../userspace/libcamera + "/${n}") (
      builtins.filter (lib.hasSuffix ".patch") (
        builtins.attrNames (builtins.readDir ../userspace/libcamera)
      )
    )
  );

  postPatch = (old.postPatch or "") + lib.optionalString (mainsFrequency == 50) ''
    sed -i 's/flickerPeriod: 8333/flickerPeriod: 10000/' \
      src/ipa/softisp/data/ov5693.yaml src/ipa/softisp/data/ov13858.yaml
  '';
})
