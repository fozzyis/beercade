#!/usr/bin/env bash
# Builds the project's RetroArch for the x86_64 A.S.T.R.O. image.
# Runs in archlinux:latest as an assembler NATIVE_BUILDS step:
#   build_x86_64.sh <destdir>   (repo at /src read-only, /cache persistent)
# Upstream tag + patches/*.patch, linked against astro/rt (HAVE_ASTRO_RT_INPUT).
set -euo pipefail
DEST="$1"
RA_TAG=v1.22.2
SRC=/src
CACHE=/cache
# Cabinet CPU floor is x86-64-v3 (AVX2/FMA/BMI2: Intel Haswell 2013+, AMD
# Zen). astro-frontend.service refuses to start on anything older.
ARCH_FLAGS="-march=x86-64-v3 -mtune=generic"

pacman -Syu --noconfirm --needed base-devel git cmake pkgconf \
  mesa libdrm libglvnd vulkan-headers vulkan-icd-loader alsa-lib systemd-libs \
  sdl2-compat zlib freetype2 libxkbcommon >/dev/null

echo "== astro_rt"
cmake -S "$SRC/astro/rt" -B /build/rt -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_CXX_FLAGS="$ARCH_FLAGS" >/dev/null
cmake --build /build/rt -j"$(nproc)"

echo "== RetroArch $RA_TAG"
mkdir -p "$CACHE"
[[ -d "$CACHE/retroarch-$RA_TAG/.git" ]] ||
  git clone -q --depth 1 -b "$RA_TAG" https://github.com/libretro/RetroArch.git "$CACHE/retroarch-$RA_TAG"
rm -rf /build/ra
cp -a "$CACHE/retroarch-$RA_TAG" /build/ra
cd /build/ra
for p in "$SRC"/astro/emulators/retroarch/patches/*.patch; do
  echo "  patch $(basename "$p")"
  git apply "$p"
done

export CFLAGS="-O2 $ARCH_FLAGS -DHAVE_ASTRO_RT_INPUT -I$SRC/astro/rt/include"
export CXXFLAGS="$CFLAGS"
# RetroArch links LDFLAGS after its objects, so the static lib resolves here.
export LDFLAGS="-L/build/rt -lastro_rt -lstdc++ -lpthread"
./configure --prefix=/usr \
  --disable-qt --disable-discord --disable-materialui --disable-xmb --disable-ozone \
  --disable-cheevos --disable-online_updater \
  --disable-x11 --disable-wayland --disable-pulse --disable-jack \
  --enable-kms --enable-plain_drm --enable-egl --enable-opengl --enable-vulkan \
  --enable-udev --enable-alsa --enable-sdl2 >/dev/null
make -j"$(nproc)" >/dev/null
make install DESTDIR="$DEST" >/dev/null
grep -q astro_input_drain "$DEST/usr/bin/retroarch" || { echo "astro_rt hook not linked" >&2; exit 1; }
ls -l "$DEST/usr/bin/retroarch"
