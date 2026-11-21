#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# run.sh KIND OUTDIR -- the whole DKMS check for one distribution kind (debian or arch), as CI runs
# it and as it can be run locally with rootless podman:
#   1. find linux-surface releases newer than targets.tsv (ci/new-targets.sh) and add them to a
#      working copy of the targets as candidates;
#   2. generate the sp9 and sp9-bluetooth trees for the listed targets, and separately for listed +
#      candidates (stops on a patch that does not apply, a reviewed file that changed, a new
#      linux-surface patch on a module's files);
#   3. build the packages from the listed targets' trees (Debian: ubuntu:24.04; Arch: archlinux);
#   4. for every target (listed and candidate): download linux-surface's header and kernel packages
#      of that release and run ci/container-test.sh in a fresh container, with the packages for
#      listed targets and with dkms by hand for candidates.
# Results: OUTDIR/packages/ (the .deb or .pkg.tar.zst files), OUTDIR/candidates.tsv (lines to add to
# targets.tsv once reviewed), OUTDIR/log/. Exit status 1 when anything failed.
#
# Environment: CONTAINER (docker or podman, default: docker if present), GH_TOKEN for gh,
# SP9_DKMS_CACHE (gen.sh cache), IMAGES_DEBIAN (default "ubuntu:24.04 ubuntu:26.04 debian:13"),
# IMAGE_ARCH (default archlinux:latest), NO_NEW=1 to skip step 1.
set -euo pipefail
export LC_ALL=C
[ $# = 2 ] || { sed -n '4,22p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
KIND=$1; mkdir -p "$2"; OUT=$(readlink -f "$2")
HERE=$(dirname "$(readlink -f "$0")"); DKMS=$(dirname "$HERE")
C=${CONTAINER:-$(command -v docker >/dev/null && echo docker || echo podman)}
IMAGES_DEBIAN=${IMAGES_DEBIAN:-ubuntu:24.04 ubuntu:26.04 debian:13}
IMAGE_ARCH=${IMAGE_ARCH:-archlinux:latest}
CACHE=${SP9_DKMS_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/sp9-dkms}
fail=0
mkdir -p "$OUT/log" "$OUT/packages" "$OUT/pkg"
gen() { "$DKMS/gen.sh" --cache "$CACHE" "$@"; }
run() { "$C" run --rm -e CI=1 -v "$DKMS:/w/dkms:ro" -v "$OUT:/w/out" "$@"; }

echo "::group::new linux-surface releases"
cp "$DKMS/targets.tsv" "$OUT/targets.tsv"
: >"$OUT/candidates.tsv"
if [ "${NO_NEW:-0}" != 1 ]; then
	"$HERE/new-targets.sh" | awk -F'\t' -v k="$KIND-" 'index($1, k) == 1' >"$OUT/candidates.tsv"
	cat "$OUT/candidates.tsv" >>"$OUT/targets.tsv"
fi
cat "$OUT/candidates.tsv"
echo "::endgroup::"
export SP9_DKMS_TARGETS=$OUT/targets.tsv
version=$(gen version)
listed=$(SP9_DKMS_TARGETS=$DKMS/targets.tsv gen targets "$KIND")
candidates=$(cut -f1 "$OUT/candidates.tsv")

echo "::group::generate ($version)"
rm -rf "$OUT/trees"
gen tree --out "$OUT/trees/listed/sp9" --version "$version" $listed
gen tree --out "$OUT/trees/listed/sp9-bt" --package bluetooth --version "$version" $listed
if [ -n "$candidates" ]; then
	if gen tree --out "$OUT/trees/all/sp9" --version "$version" $listed $candidates &&
		gen tree --out "$OUT/trees/all/sp9-bt" --package bluetooth --version "$version" $listed $candidates; then
		:
	else
		echo "::error::the DKMS sources cannot be generated for $candidates (see above)"; fail=1
	fi
fi
echo "::endgroup::"

echo "::group::packages"
case $KIND in
debian)
	run -v "$OUT/packages:/w/pkgout" docker.io/library/ubuntu:24.04 bash -c 'export DEBIAN_FRONTEND=noninteractive
		apt-get update -qq && apt-get install -y -qq --no-install-recommends build-essential dpkg-dev debhelper dh-dkms >/dev/null &&
		/w/dkms/pkg/build-deb.sh /w/pkgout /w/out/trees/listed/sp9 /w/out/trees/listed/sp9-bt' >"$OUT/log/build-packages.log" 2>&1 || fail=1 ;;
arch)
	run -v "$OUT/packages:/w/pkgout" "docker.io/library/$IMAGE_ARCH" bash -c 'pacman -Syu --noconfirm --needed base-devel >/dev/null &&
		useradd -m b && chown b /w/pkgout && runuser -u b -- /w/dkms/pkg/build-arch.sh /w/pkgout /w/out/trees/listed/sp9 /w/out/trees/listed/sp9-bt' >"$OUT/log/build-packages.log" 2>&1 || fail=1 ;;
esac
tail -n 5 "$OUT/log/build-packages.log"
echo "::endgroup::"

for tag in $listed $candidates; do
	d=$OUT/pkg/$tag; mkdir -p "$d"
	case $KIND in
	debian) gh release download "$tag" -R linux-surface/linux-surface -D "$d" --skip-existing -p 'linux-headers-*' -p 'linux-image-[0-9]*' ;;
	arch) gh release download "$tag" -R linux-surface/linux-surface -D "$d" --skip-existing -p 'linux-surface-*.pkg.tar.zst' ;;
	esac
	if echo " $listed " | grep -q " $tag "; then tree=listed; pk=$(cd "$OUT/packages" && ls | grep -E '\.(deb|pkg\.tar\.zst)$' | sed 's|^|/w/out/packages/|' | tr '\n' ' '); else tree=all; pk=; fi
	images=$IMAGES_DEBIAN; [ "$KIND" = arch ] && images=$IMAGE_ARCH
	for img in $images; do
		echo "::group::test $tag in $img"
		log=$OUT/log/test-$tag-${img//[:\/]/_}.log
		if run -e PACKAGES="$pk" "docker.io/library/$img" bash /w/dkms/ci/container-test.sh "$KIND" "/w/out/pkg/$tag" \
			"/w/out/trees/$tree/sp9" "/w/out/trees/$tree/sp9-bt" >"$log" 2>&1; then
			echo "passed: $tag in $img"
		else
			echo "::error::$tag in $img failed (log/${log##*/})"; fail=1
		fi
		grep -E '^(ok|FAIL|====)|BUILD_EXCLUSIVE|autoinstall' "$log" | tail -n 60
		echo "::endgroup::"
	done
done
exit $fail
