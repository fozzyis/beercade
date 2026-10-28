#!/usr/bin/env bash
# Builds ES-DE (EmulationStation Desktop Edition) for the x86_64 A.S.T.R.O.
# image; ES-DE is not in the Arch repos. Runs in archlinux:latest as an
# assembler NATIVE_BUILDS step: build_x86_64.sh <destdir>  (/cache persistent)
# Runtime libraries are listed in payloads/astro.conf.
set -euo pipefail
DEST="$1"
ESDE_TAG=v3.5.0
# Cabinet CPU floor, same as RetroArch (see build_x86_64.sh there).
ARCH_FLAGS="-march=x86-64-v3 -mtune=generic"
# FreeImage left the Arch repos; build the AUR recipe (pinned) with makepkg.
FREEIMAGE_AUR=https://aur.archlinux.org/freeimage.git
FREEIMAGE_COMMIT=66732f09db170483348f14f3d02815181c4d04d0
CACHE=/cache

pacman -Syu --noconfirm --needed base-devel git cmake pkgconf \
  bluez-libs curl ffmpeg freetype2 gettext harfbuzz icu libgit2 \
  poppler pugixml sdl2-compat mesa libglvnd >/dev/null

mkdir -p "$CACHE"

echo "== FreeImage (AUR $FREEIMAGE_COMMIT)"
pacman -S --noconfirm --needed libjpeg-turbo openexr openjpeg2 libwebp libraw libtiff jxrlib glu >/dev/null
id builder >/dev/null 2>&1 || useradd -m builder
sed -i "s/-march=x86-64 -mtune=generic/$ARCH_FLAGS/" /etc/makepkg.conf
rm -rf /build/freeimage && mkdir -p /build && git clone -q "$FREEIMAGE_AUR" /build/freeimage
git -C /build/freeimage checkout -q "$FREEIMAGE_COMMIT"
chown -R builder /build/freeimage
su builder -c 'cd /build/freeimage && makepkg --noconfirm --skippgpcheck' >/dev/null
pkg=$(ls /build/freeimage/freeimage-*.pkg.tar.zst | head -1)
pacman -U --noconfirm "$pkg" >/dev/null
# The library ships in the image too (pacstrap can't install it).
tar -C "$DEST" -xf "$pkg" --exclude='.PKGINFO' --exclude='.BUILDINFO' --exclude='.MTREE'

echo "== ES-DE $ESDE_TAG"
[[ -d "$CACHE/es-de-$ESDE_TAG/.git" ]] ||
  git clone -q --depth 1 -b "$ESDE_TAG" https://gitlab.com/es-de/emulationstation-de.git "$CACHE/es-de-$ESDE_TAG"
rm -rf /build/es-de
cp -a "$CACHE/es-de-$ESDE_TAG" /build/es-de

cmake -S /build/es-de -B /build/es-de/build -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX=/usr -DCMAKE_CXX_FLAGS="$ARCH_FLAGS" >/dev/null
# Capped: ES-DE's heavy template units segfaulted cc1plus at -j12 in 8 GB Docker.
cmake --build /build/es-de/build -j"$(( $(nproc) < 4 ? $(nproc) : 4 ))" >/dev/null
DESTDIR="$DEST" cmake --install /build/es-de/build >/dev/null
ls -l "$DEST/usr/bin/"
