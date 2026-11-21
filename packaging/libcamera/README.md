# packaging/libcamera: sp9-libcamera (experimental)

**Experimental and opt-in.** `sp9-libcamera` is a native package for Debian, Ubuntu and Arch that puts the
patched libcamera of [`userspace/libcamera/`](../../userspace/libcamera/README.md) under PipeWire, beside the
distribution's own libcamera and PipeWire. No metapackage of this repository pulls it in; install it by hand
if you want it. Outside NixOS it was tried on the device once, under Ubuntu 26.04: what the patches do on the
device was measured with the NixOS build ([docs/research/gaps/camera.md](../../docs/research/gaps/camera.md),
section 6).

**Status: Ubuntu 26.04, Debian 13 and Arch checked once each on the device; the other targets unverified there.** Built and install-tested in
containers (the plugin loads into a PipeWire daemon there). On a Surface Pro 9 with Ubuntu 26.04 its `cam -l` lists
both cameras, the front image has no purple lines and a fine exposure, and the rear image is as expected (observed;
[docs/test-results.md](../../docs/test-results.md#ubuntu-2604-packaged-route)). The rear image and its autofocus work
with the same libcamera and patches on NixOS (observed, CAM-1 and CAM-4 in
[docs/test-results.md](../../docs/test-results.md)); the earlier blur was a protective film over the lens. Autofocus
through this package was not checked again after the film was removed. Reports:
[docs/install.md](../../docs/install.md#report-what-you-see).

## What it contains

| Path | What |
|---|---|
| `/usr/lib/sp9-libcamera/lib/` | libcamera 0.8.0 (`libcamera.so.0.8`, `libcamera-base.so.0.8`) with the ten patches; the IPA module `libcamera/ipa/ipa_softisp.so` and its signature |
| `/usr/lib/sp9-libcamera/libexec/libcamera/` | the IPA proxy (used when a module runs isolated) |
| `/usr/lib/sp9-libcamera/share/libcamera/ipa/softisp/` | tuning files, including `ov5693.yaml` and `ov13858.yaml` of patch 0010 |
| `/usr/lib/sp9-libcamera/bin/cam` | libcamera's test application, built against this libcamera |
| `/usr/lib/sp9-libcamera/spa-0.2/libcamera/libspa-libcamera.so` | PipeWire's libcamera SPA plugin, built from the source of the PipeWire version the distribution ships, against this libcamera |
| `/usr/lib/systemd/user/pipewire.service.d/sp9-libcamera.conf`, `.../wireplumber.service.d/sp9-libcamera.conf` | `SPA_PLUGIN_DIR=/usr/lib/sp9-libcamera/spa-0.2:<distribution SPA directory>` |

libcamera is built with `-Dpipelines=simple,uvcvideo -Dipas=softisp` and the GPU debayer: the two built-in
cameras use the simple pipeline, USB webcams keep working through `uvcvideo`. The IPA module, proxy and
tuning-file directories are compiled in for this prefix, and `cam`, the IPA module, the proxy and the plugin have
`RUNPATH=/usr/lib/sp9-libcamera/lib`, so nothing needs `LD_LIBRARY_PATH`. `libcamera.so` and `libcamera-base.so`
themselves have no RUNPATH: libcamera takes a RUNPATH on its own file as the sign that it runs from a build tree
(`isLibcameraInstalled()` in `src/libcamera/source_paths.cpp`) and then also searches IPA modules and tuning
files relative to it (`/usr/lib/src/ipa`, `/usr/lib/source`). Every user of libcamera needs both libraries
directly, so their RUNPATH covers `libcamera.so`'s own dependency on `libcamera-base.so`. Development files
(headers, `.pc`, `.so` links) and `libcamera-bug-report` (it runs the `cam` of `PATH`, the distribution's) are
not shipped.

**What it replaces:** nothing. No file of the distribution is diverted or overwritten; the distribution's
`libcamera0.7`/`libcamera`, its `libspa-0.2-libcamera`/`pipewire-libcamera` plugin and its `cam` stay installed
and unchanged.

**What it changes:** the two drop-ins. PipeWire looks a SPA plugin up in each directory of `SPA_PLUGIN_DIR` in
turn and loads the first file that opens (`load_spa_handle()` and `open_plugin()` in
`src/pipewire/pipewire.c`), so `libcamera/libspa-libcamera` is taken from `/usr/lib/sp9-libcamera/spa-0.2` and
every other plugin from the distribution's directory (`/usr/lib/x86_64-linux-gnu/spa-0.2` on Debian and
Ubuntu, `/usr/lib/spa-0.2` on Arch). If our plugin fails to load, the lookup goes on to the distribution's.

Both services need it: WirePlumber's libcamera monitor creates the enumerator and the device objects in its
own process (`SpaDevice` in `monitors/libcamera/enumerate-device.lua` and `create-device.lua`), but the camera
nodes are created through the `spa-node-factory` of the PipeWire daemon (`Node ("spa-node-factory", …)` in
`create-node.lua`), so the camera streams, and the IPA runs, in the `pipewire` process. With the drop-in on
WirePlumber alone the cameras would be listed by the patched libcamera and streamed by the distribution's.

## What you get, and what you lose without it

Distributions ship libcamera 0.7.x. With it, on this device (measured with 0.7.2 and an unpatched 0.8.0, see
camera.md section 6 and the [patch table](../../userspace/libcamera/README.md)):

- purple/magenta lines every 480 pixels in the processed image (GPU debayer; patch 0002);
- exposure metered on the top-left corner of the sensor instead of the visible image, so the image is blown out
  or too dark (0002);
- exposure that oscillates or takes long to settle (wrong gain ranges, control delays, loop delay, dark-frame
  estimate; 0003–0005);
- no autofocus or focus control for the rear camera (0007–0009);
- the lens and the rear sensor stay powered for as long as WirePlumber runs, even when no application uses the
  camera (0008).

Colour stays uncalibrated with or without the package (grey-world white balance, no colour correction matrix).

## Building

Each build runs inside the official container of its target, as root, from the repository root. The scripts
fetch the libcamera v0.8.0 tag and check its commit (`df8e348d64b4932a0676256ed24f0da3ee9438bc`; the Arch
PKGBUILD also checks the tree's sha256), fetch the PipeWire source, then build without the network (meson runs
with `--wrap-mode=nodownload`).

| Target | Container | PipeWire the plugin is built from | Command |
|---|---|---|---|
| Debian 13 (trixie) | `debian:13` | trixie-backports (1.6.9) | `SP9_VERSION=… DEB_SUFFIX=deb13 DEB_CODENAME=trixie packaging/libcamera/build-deb.sh OUT` |
| Debian testing (forky) | `debian:testing` | forky | `… DEB_SUFFIX=debtesting DEB_CODENAME=forky …` |
| Ubuntu 26.04 (resolute) | `ubuntu:26.04` | resolute (1.6.2) | `… DEB_SUFFIX=ubuntu26.04 DEB_CODENAME=resolute …` |
| Ubuntu 26.10 (stonking) | `ubuntu:26.10` | stonking (1.6.8) | `… DEB_SUFFIX=ubuntu26.10 DEB_CODENAME=stonking …` |
| Arch | `archlinux:latest` | upstream tag `_pwver` of the PKGBUILD (the current `pipewire`) | `SP9_ARCH_VERSION=$(packaging/version.sh --arch) packaging/libcamera/build-arch.sh OUT` |

Ubuntu 24.04 is not a target: its PipeWire predates the software ISP.

- **Debian and Ubuntu** ([`build-deb.sh`](build-deb.sh), [`debian/`](debian/)): the package version is
  `0.8.0+sp9.${SP9_VERSION}-1~${DEB_SUFFIX}`. The script enables `deb-src` (and trixie-backports on trixie),
  installs `libspa-0.2-modules`, fetches the PipeWire source package of exactly that version with
  `apt-get source` (Debian's and Ubuntu's own patches included) into `pipewire-src/`, turns the patches into
  the quilt series `debian/patches/`, installs the build dependencies and runs `dpkg-buildpackage -b`, then
  lintian. Output: the `.deb`, `.buildinfo` and `.changes`.
- **Arch** ([`build-arch.sh`](build-arch.sh), [`arch/PKGBUILD`](arch/PKGBUILD)): `pkgver` is
  `0.8.0_sp9.${SP9_ARCH_VERSION}` (default: `SP9_VERSION` with `-` replaced by `.`, else `0`). Pass
  `SP9_ARCH_VERSION=$(packaging/version.sh --arch)`, as the Packages workflow does: it gives the pacman spelling
  of the project version that the other Arch packages carry (`0.1.0pre.r91.g…`), where the default from
  `SP9_VERSION` keeps the Debian one (`0.1.0~git91.g…`) and without either the version is `0`. The script
  refuses to build when the installed `pipewire` is not `_pwver`: bump `_pwver` and its checksum
  (`updpkgsums`) when Arch moves on. It also checks that `_patches` lists exactly
  `userspace/libcamera/*.patch`. makepkg runs as an unprivileged user; namcap checks the PKGBUILD and the
  package.

The PipeWire 1.6 sources are written for libcamera 0.7. [`pipewire-libcamera-0.8.sh`](pipewire-libcamera-0.8.sh)
renames the `properties::CameraLocation*` enumerators to `properties::Location*` and replaces
`libcamera::Span` with `std::span` (already done upstream from 1.6.9), each only where the old spelling is
present, and fails if one remains. It is the same change as [`nix/pipewire-libcamera.nix`](../../nix/pipewire-libcamera.nix).

### IPA module signature

libcamera signs IPA modules at `meson install` with a key generated during the build and compiles the public
half into `libcamera.so`; a module whose signature does not verify is not refused but runs isolated in the
proxy process. Stripping changes the file, so both builds strip first and sign again with the build's key
(`src/ipa/ipa-sign.sh`, as distributions do), then run libcamera's `ipa_verify` on the result: Debian in
`override_dh_strip` (no `-dbgsym` package), Arch in `package()` with `options=(!strip !debug)`.
[`install-test.sh`](install-test.sh) verifies the installed signature again against the key extracted from
the installed `libcamera.so`.

### Dependencies on PipeWire

The plugin is compiled from one PipeWire release against the SPA headers of that release and runs inside the
host's PipeWire and WirePlumber. SPA interfaces do not change within a release series, so the package accepts
the version it was built from and every later release of the same series:

- Debian and Ubuntu: `libspa-0.2-modules (>= X.Y.Z), libspa-0.2-modules (<< X.(Y+1)~)`, with X.Y.Z the upstream
  version of the PipeWire source package (on trixie this can only be satisfied from trixie-backports);
  `Recommends: wireplumber (>= 0.5)`.
- Arch: `pipewire>=1:X.Y.Z`, `pipewire<1:X.(Y+1)`.

A newer release of the series is accepted, but our plugin then lacks that release's fixes to the plugin. A move
to the next series (1.7) makes the package uninstallable or blocks the upgrade until it is rebuilt.

## Install, turn off, remove

Install (Debian 13: the dependency on `libspa-0.2-modules` of PipeWire 1.6 is met only from trixie-backports; enable it
and install with `apt install -t trixie-backports`, which moves PipeWire to 1.6 and leaves WirePlumber at trixie's
0.5.8, enough for `Recommends: wireplumber (>= 0.5)`;
[docs/install-debian-ubuntu.md](../../docs/install-debian-ubuntu.md#5-cameras-experimental)):

    apt install ./sp9-libcamera_0.8.0+sp9.*.deb          # Debian, Ubuntu
    pacman -U sp9-libcamera-0.8.0_sp9.*.pkg.tar.zst       # Arch
    systemctl --user daemon-reload && systemctl --user restart pipewire wireplumber

Turn off for one user without uninstalling: mask the two drop-ins and restart the services.

    mkdir -p ~/.config/systemd/user/pipewire.service.d ~/.config/systemd/user/wireplumber.service.d
    ln -s /dev/null ~/.config/systemd/user/pipewire.service.d/sp9-libcamera.conf
    ln -s /dev/null ~/.config/systemd/user/wireplumber.service.d/sp9-libcamera.conf
    systemctl --user daemon-reload && systemctl --user restart pipewire wireplumber

For all users, put the same two symlinks in `/etc/systemd/user/pipewire.service.d/` and
`/etc/systemd/user/wireplumber.service.d/`. Removing the symlinks turns it on again.

Remove: `apt purge sp9-libcamera` or `pacman -Rns sp9-libcamera`, then restart the two services as above.

## Mains frequency (50 Hz)

The tuning files carry the flicker period of 60 Hz mains (`flickerPeriod: 8333`, in microseconds). Where the
mains are 50 Hz, give libcamera tuning files with `flickerPeriod: 10000`. libcamera looks a tuning file up in
this order (`ipaConfigurationFile()` in `src/libcamera/ipa_proxy.cpp`, `src/libcamera/global_configuration.cpp`):

1. `LIBCAMERA_SOFTISP_TUNING_FILE`: one file for every camera, which does not suit two different sensors;
2. each directory of `LIBCAMERA_IPA_CONFIG_PATH` (or `ipa: config_paths:` of a libcamera `configuration.yaml`),
   as `<dir>/softisp/<sensor>.yaml`;
3. `/etc/sp9-libcamera/libcamera/ipa/softisp/<sensor>.yaml` (the package's sysconfdir; nothing is installed
   there);
4. `/usr/lib/sp9-libcamera/share/libcamera/ipa/softisp/<sensor>.yaml` (the package's files).

System-wide, use step 3:

    mkdir -p /etc/sp9-libcamera/libcamera/ipa/softisp
    for s in ov5693 ov13858; do
        sed 's/flickerPeriod: 8333/flickerPeriod: 10000/' \
            /usr/lib/sp9-libcamera/share/libcamera/ipa/softisp/$s.yaml \
            > /etc/sp9-libcamera/libcamera/ipa/softisp/$s.yaml
    done

For one user, write the two files to a directory of your own (say `~/.config/sp9-libcamera/softisp/`) and
point `LIBCAMERA_IPA_CONFIG_PATH` at its parent in a drop-in of `pipewire.service` (the process that streams):
`systemctl --user edit pipewire`, then

    [Service]
    Environment=LIBCAMERA_IPA_CONFIG_PATH=%h/.config/sp9-libcamera

A per-user `~/.config/libcamera/configuration.yaml` would also work but is read by the distribution's
libcamera too. Copies in `/etc` or in your home do not follow later changes of the package's tuning files.
The lookup order is taken from the libcamera source; the 50 Hz files have not been tried on the device
(unverified).

## Risks and limits

- **Pinned to the host PipeWire.** The plugin is built from the sources of one PipeWire release. A PipeWire
  upgrade within the series keeps working with an older plugin (assumed from SPA's interface stability, not
  tested); an upgrade to another series needs a rebuilt package, and the dependency range makes the package
  manager say so.
- **Only PipeWire clients get the patched libcamera.** GStreamer's `libcamerasrc`, the distribution's `cam`,
  `qcam` and anything else linked against the distribution's libcamera keep libcamera 0.7.x. Use
  `/usr/lib/sp9-libcamera/bin/cam` for the patched one.
- **IPA signature.** The module is signed by a key made for each build; if the signature did not verify the
  module would run isolated (slower start, one more process), not fail.
- **Two libcamera versions in the system.** The sonames differ (0.7 and 0.8), and only processes that load our
  plugin load libcamera 0.8; a process that loaded both would be unsupported.
- **On the device outside NixOS, Ubuntu 26.04 only.** Building, installing, loading in a container and removing
  were tested for every target (below); on a Surface Pro 9 only the Ubuntu 26.04 package was used, in one run.

## Testing

[`install-test.sh`](install-test.sh) runs in a container of the target, as root, with the package file as
argument. It installs the package, checks `RUNPATH` and `ldd` of every ELF file (libcamera from
`/usr/lib/sp9-libcamera/lib`, everything else from the distribution, nothing missing), verifies the IPA
signature with the key in the installed `libcamera.so`, checks the tuning files, the compiled-in IPA module path
and the sysconfdir (libcamera names a broken `/etc/sp9-libcamera/libcamera/configuration.yaml` that the test
puts there for a moment), runs `cam --list` (no cameras in a container) and checks that libcamera does not
consider itself uninstalled, lets `spa-inspect` load the plugin and instantiate its factories,
starts a PipeWire daemon with the drop-in's `SPA_PLUGIN_DIR` and an enumerator object and checks that it loads
the libcamera plugin from `/usr/lib/sp9-libcamera` and its support plugin from the distribution, then removes
the package and checks that no file is left.

Results of the last run, with `SP9_VERSION=0.2.0`: all five targets built (plugin from PipeWire 1.6.9-2~bpo13+1 on
Debian 13, 1.6.9-2 on Debian testing, 1.6.2-1ubuntu1.2 on Ubuntu 26.04, 1.6.8-1ubuntu4 on Ubuntu 26.10, tag 1.6.11
on Arch), and `install-test.sh` passed every check on each of them. lintian printed nothing on Debian testing and
the two Ubuntu releases; on Debian 13 it printed `I: hardening-no-fortify-functions [usr/lib/sp9-libcamera/bin/cam]`
(informational; `cam` is compiled with `-D_FORTIFY_SOURCE=2`, it has no fortifiable call). namcap warns that
`libcamera.so` needs `libcamera-base.so.0.8` (its users load it, see above) and that `pipewire` "may not be
needed" (no ELF file links libpipewire; the dependency pins the release series the plugin was built from).
