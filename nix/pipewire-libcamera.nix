{ pipewire, libcamera }:

# PipeWire whose libcamera plugin is built against the given libcamera. PipeWire 1.6 is written for
# libcamera 0.7: 0.8.0 renamed the enumerators of properties::Location and replaced libcamera::Span
# with std::span.

(pipewire.override { inherit libcamera; }).overrideAttrs (old: {
  postPatch = (old.postPatch or "") + ''
    sed -i 's/properties::CameraLocation\(Front\|Back\|External\)/properties::Location\1/' \
      spa/plugins/libcamera/libcamera-device.cpp
    sed -i 's/libcamera::Span</std::span</g; 0,/^#include </s//#include <span>\n#include </' \
      spa/plugins/libcamera/libcamera-source.cpp
  '';
})
