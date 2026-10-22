#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BUILD_DIR="${ROOT_DIR}/build"
ROOTFS_DIR="${ROOT_DIR}/ASTRO/rootfs"
CROSS_FILE="${ROOT_DIR}/buildfarm/toolchain/aarch64-linux-gnu.txt"
CMAKE_TOOLCHAIN="${ROOT_DIR}/buildfarm/toolchain/aarch64-toolchain.cmake"

mkdir -p "${BUILD_DIR}" "${ROOTFS_DIR}/usr/lib/pkgconfig" "${ROOTFS_DIR}/usr/include"

export CC="aarch64-linux-gnu-gcc"
export CXX="aarch64-linux-gnu-g++"
export AR="aarch64-linux-gnu-ar"
export STRIP="aarch64-linux-gnu-strip"
export RANLIB="aarch64-linux-gnu-ranlib"

export PKG_CONFIG_PATH=""
export PKG_CONFIG_LIBDIR="${ROOTFS_DIR}/usr/lib/pkgconfig:${ROOTFS_DIR}/usr/share/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR="${ROOTFS_DIR}"

BASE_CFLAGS="-O3 -march=armv8.2-a+crypto+fp16+dotprod -mtune=cortex-a76 -ffunction-sections -fdata-sections -fomit-frame-pointer -funroll-loops -I${ROOTFS_DIR}/usr/include"
BASE_LDFLAGS="-Wl,--gc-sections -Wl,-O1 -Wl,--as-needed -L${ROOTFS_DIR}/usr/lib -Wl,-rpath-link=${ROOTFS_DIR}/usr/lib"

export CFLAGS="${BASE_CFLAGS}"
export CXXFLAGS="${BASE_CFLAGS}"
export LDFLAGS="${BASE_LDFLAGS}"

echo "======================================================================"
echo "   Building ES-DE Cross-Compilation Dependencies for A.S.T.R.O.      "
echo "======================================================================"

# 1. pugixml
PUGIXML_SRC="${BUILD_DIR}/pugixml"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libpugixml.so" ] && [ ! -f "${ROOTFS_DIR}/usr/lib/libpugixml.a" ]; then
    echo "[+] Building pugixml..."
    if [ ! -d "${PUGIXML_SRC}" ]; then
        git clone --depth 1 -b v1.14 https://github.com/zeux/pugixml.git "${PUGIXML_SRC}"
    fi
    cd "${PUGIXML_SRC}"
    rm -rf build_cross
    cmake -B build_cross \
      -DCMAKE_TOOLCHAIN_FILE="${CMAKE_TOOLCHAIN}" \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX=/usr \
      -DBUILD_SHARED_LIBS=ON
    make -C build_cross -j"$(nproc)"
    make -C build_cross install DESTDIR="${ROOTFS_DIR}"
fi

# 2. SDL2 (KMSDRM / ALSA / GLES / Vulkan)
SDL2_SRC="${BUILD_DIR}/SDL2"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libSDL2.so" ]; then
    echo "[+] Building SDL2..."
    if [ ! -d "${SDL2_SRC}" ]; then
        git clone --depth 1 -b release-2.30.12 https://github.com/libsdl-org/SDL.git "${SDL2_SRC}"
    fi
    cd "${SDL2_SRC}"
    rm -rf build_cross
    cmake -B build_cross \
      -DCMAKE_TOOLCHAIN_FILE="${CMAKE_TOOLCHAIN}" \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX=/usr \
      -DSDL_KMSDRM=ON \
      -DSDL_KMSDRM_SHARED=OFF \
      -DSDL_ALSA=ON \
      -DSDL_ALSA_SHARED=OFF \
      -DSDL_PULSEAUDIO=OFF \
      -DSDL_JACK=OFF \
      -DSDL_PIPEWIRE=OFF \
      -DSDL_X11=OFF \
      -DSDL_WAYLAND=OFF \
      -DSDL_OPENGL=OFF \
      -DSDL_OPENGLES=ON \
      -DSDL_VULKAN=ON \
      -DSDL_STATIC=OFF \
      -DSDL_SHARED=ON
    make -C build_cross -j"$(nproc)"
    make -C build_cross install DESTDIR="${ROOTFS_DIR}"
fi

# 3. libpng
PNG_SRC="${BUILD_DIR}/libpng"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libpng.so" ] && [ ! -f "${ROOTFS_DIR}/usr/lib/libpng16.so" ]; then
    echo "[+] Building libpng..."
    if [ ! -d "${PNG_SRC}" ]; then
        git clone --depth 1 -b v1.6.43 https://github.com/pnggroup/libpng.git "${PNG_SRC}"
    fi
    cd "${PNG_SRC}"
    rm -rf build_cross
    cmake -B build_cross \
      -DCMAKE_TOOLCHAIN_FILE="${CMAKE_TOOLCHAIN}" \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX=/usr \
      -DPNG_SHARED=ON \
      -DPNG_STATIC=OFF \
      -DPNG_TESTS=OFF \
      -DPNG_TOOLS=OFF \
      -DPNG_EXECUTABLES=OFF \
      -DPNG_HARDWARE_OPTIMIZATIONS=ON
    make -C build_cross -j"$(nproc)"
    make -C build_cross install DESTDIR="${ROOTFS_DIR}"
fi

# 4. libjpeg-turbo
JPEG_SRC="${BUILD_DIR}/libjpeg-turbo"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libjpeg.so" ]; then
    echo "[+] Building libjpeg-turbo..."
    if [ ! -d "${JPEG_SRC}" ]; then
        git clone --depth 1 -b 3.0.3 https://github.com/libjpeg-turbo/libjpeg-turbo.git "${JPEG_SRC}"
    fi
    cd "${JPEG_SRC}"
    rm -rf build_cross
    cmake -B build_cross \
      -DCMAKE_TOOLCHAIN_FILE="${CMAKE_TOOLCHAIN}" \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX=/usr \
      -DENABLE_SHARED=ON \
      -DENABLE_STATIC=OFF \
      -DWITH_JPEG8=ON
    make -C build_cross -j"$(nproc)"
    make -C build_cross install DESTDIR="${ROOTFS_DIR}"
fi

# 5. FreeType
FREETYPE_SRC="${BUILD_DIR}/freetype"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libfreetype.so" ]; then
    echo "[+] Building FreeType..."
    if [ ! -d "${FREETYPE_SRC}" ]; then
        git clone --depth 1 -b VER-2-13-3 https://gitlab.freedesktop.org/freetype/freetype.git "${FREETYPE_SRC}"
    fi
    cd "${FREETYPE_SRC}"
    rm -rf build_cross
    cmake -B build_cross \
      -DCMAKE_TOOLCHAIN_FILE="${CMAKE_TOOLCHAIN}" \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX=/usr \
      -DBUILD_SHARED_LIBS=ON \
      -DFT_DISABLE_HARFBUZZ=TRUE \
      -DFT_DISABLE_BZIP2=TRUE \
      -DFT_DISABLE_BROTLI=TRUE \
      -DFT_DISABLE_PNG=FALSE \
      -DFT_DISABLE_ZLIB=FALSE
    make -C build_cross -j"$(nproc)"
    make -C build_cross install DESTDIR="${ROOTFS_DIR}"
fi

# 6. HarfBuzz
HARFBUZZ_SRC="${BUILD_DIR}/harfbuzz"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libharfbuzz.so" ]; then
    echo "[+] Building HarfBuzz..."
    if [ ! -d "${HARFBUZZ_SRC}" ]; then
        git clone --depth 1 -b 8.5.0 https://github.com/harfbuzz/harfbuzz.git "${HARFBUZZ_SRC}"
    fi
    cd "${HARFBUZZ_SRC}"
    rm -rf build_cross
    meson setup build_cross \
      --cross-file "${CROSS_FILE}" \
      --prefix=/usr \
      -Dbuildtype=release \
      -Dglib=disabled \
      -Dgobject=disabled \
      -Dcairo=disabled \
      -Dicu=disabled \
      -Dtests=disabled \
      -Ddocs=disabled \
      -Dbenchmark=disabled \
      -Dintrospection=disabled \
      -Dfreetype=enabled
    ninja -C build_cross -j"$(nproc)"
    DESTDIR="${ROOTFS_DIR}" ninja -C build_cross install
fi

# 7. OpenSSL
OPENSSL_SRC="${BUILD_DIR}/openssl"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libssl.so" ]; then
    echo "[+] Building OpenSSL..."
    if [ ! -d "${OPENSSL_SRC}" ]; then
        git clone --depth 1 -b openssl-3.3.1 https://github.com/openssl/openssl.git "${OPENSSL_SRC}"
    fi
    cd "${OPENSSL_SRC}"
    git clean -fdx
    ./Configure linux-aarch64 \
      --prefix=/usr \
      --openssldir=/etc/ssl \
      shared no-tests no-unit-test no-docs \
      ${BASE_CFLAGS} ${BASE_LDFLAGS}
    make -j"$(nproc)"
    make install_sw DESTDIR="${ROOTFS_DIR}"
fi

# 8. cURL
CURL_SRC="${BUILD_DIR}/curl"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libcurl.so" ]; then
    echo "[+] Building cURL..."
    if [ ! -d "${CURL_SRC}" ]; then
        git clone --depth 1 -b curl-8_8_0 https://github.com/curl/curl.git "${CURL_SRC}"
    fi
    cd "${CURL_SRC}"
    rm -rf build_cross
    cmake -B build_cross \
      -DCMAKE_TOOLCHAIN_FILE="${CMAKE_TOOLCHAIN}" \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX=/usr \
      -DCURL_USE_OPENSSL=ON \
      -DCURL_ZLIB=ON \
      -DENABLE_ARES=OFF \
      -DHTTP_ONLY=ON \
      -DBUILD_CURL_EXE=OFF \
      -DBUILD_SHARED_LIBS=ON
    make -C build_cross -j"$(nproc)"
    make -C build_cross install DESTDIR="${ROOTFS_DIR}"
fi

# 9. libgit2
LIBGIT2_SRC="${BUILD_DIR}/libgit2"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libgit2.so" ]; then
    echo "[+] Building libgit2..."
    if [ ! -d "${LIBGIT2_SRC}" ]; then
        git clone --depth 1 -b v1.8.1 https://github.com/libgit2/libgit2.git "${LIBGIT2_SRC}"
    fi
    cd "${LIBGIT2_SRC}"
    rm -rf build_cross
    cmake -B build_cross \
      -DCMAKE_TOOLCHAIN_FILE="${CMAKE_TOOLCHAIN}" \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX=/usr \
      -DUSE_SSH=OFF \
      -DUSE_HTTPS=OpenSSL \
      -DBUILD_TESTS=OFF \
      -DBUILD_CLI=OFF \
      -DBUILD_SHARED_LIBS=ON
    make -C build_cross -j"$(nproc)"
    make -C build_cross install DESTDIR="${ROOTFS_DIR}"
fi

# 10. ICU (Host tool build + target cross build)
ICU_SRC="${BUILD_DIR}/icu"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libicuuc.so" ]; then
    echo "[+] Building ICU (Host tools + AArch64 target)..."
    if [ ! -d "${ICU_SRC}" ]; then
        git clone --depth 1 -b release-75-1 https://github.com/unicode-org/icu.git "${ICU_SRC}"
    fi
    cd "${ICU_SRC}"
    if [ ! -f "build_host/bin/icupkg" ]; then
        echo "[+] Building ICU host tools..."
        rm -rf build_host
        mkdir -p build_host
        cd build_host
        env -i PATH="${PATH}" HOME="${HOME:-/home/outl4w}" USER="${USER:-outl4w}" \
          CC=gcc CXX=g++ \
          ../icu4c/source/runConfigureICU Linux --disable-samples --disable-tests
        make -j"$(nproc)"
        cd ..
    fi

    echo "[+] Building ICU cross-compiled target..."
    rm -rf build_cross
    mkdir -p build_cross
    cd build_cross
    ../icu4c/source/configure \
      --host=aarch64-linux-gnu \
      --prefix=/usr \
      --with-cross-build="$(pwd)/../build_host" \
      --disable-samples \
      --disable-tests \
      --enable-static=no \
      --enable-shared=yes \
      CFLAGS="${BASE_CFLAGS}" \
      CXXFLAGS="${BASE_CFLAGS}" \
      LDFLAGS="${BASE_LDFLAGS}"
    make -j"$(nproc)"
    make install DESTDIR="${ROOTFS_DIR}"
fi

# 11. BlueZ (libbluetooth)
BLUEZ_SRC="${BUILD_DIR}/bluez"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libbluetooth.so" ]; then
    echo "[+] Building BlueZ (libbluetooth)..."
    if [ ! -d "${BLUEZ_SRC}" ]; then
        git clone --depth 1 -b 5.76 https://github.com/bluez/bluez.git "${BLUEZ_SRC}"
    fi
    cd "${BLUEZ_SRC}"
    rm -rf bluetooth lib/bluetooth 2>/dev/null || true
    mkdir -p "${ROOTFS_DIR}/usr/include/bluetooth"
    cp "${BLUEZ_SRC}/lib/"*.h "${ROOTFS_DIR}/usr/include/bluetooth/"
    aarch64-linux-gnu-gcc -shared -fPIC ${BASE_CFLAGS} -I. -I"${ROOTFS_DIR}/usr/include" \
      lib/bluetooth.c lib/hci.c lib/sdp.c lib/uuid.c \
      -Wl,-soname,libbluetooth.so.3 \
      ${BASE_LDFLAGS} \
      -o "${ROOTFS_DIR}/usr/lib/libbluetooth.so.3.19.2"
    cd "${ROOTFS_DIR}/usr/lib"
    ln -sf libbluetooth.so.3.19.2 libbluetooth.so.3
    ln -sf libbluetooth.so.3 libbluetooth.so
fi

# 12. FreeImage
FREEIMAGE_SRC="${BUILD_DIR}/freeimage"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libfreeimage.so" ]; then
    echo "[+] Building FreeImage..."
    if [ ! -d "${FREEIMAGE_SRC}" ]; then
        git clone --depth 1 https://github.com/WinMerge/freeimage.git "${FREEIMAGE_SRC}"
    fi
    cd "${FREEIMAGE_SRC}"
    sed -i '1i #ifndef _byteswap_ulong\n#define _byteswap_ulong(x) __builtin_bswap32(x)\n#endif\n#ifndef _byteswap_ushort\n#define _byteswap_ushort(x) __builtin_bswap16(x)\n#endif\n#ifndef _byteswap_uint64\n#define _byteswap_uint64(x) __builtin_bswap64(x)\n#endif' Source/LibJXR/image/decode/segdec.c 2>/dev/null || true
    sed -i '1i #include <wchar.h>' Source/LibJXR/jxrgluelib/JXRGlueJxr.c 2>/dev/null || true
    if ! grep -q "tif_hash_set.c" Makefile.srcs; then
        sed -i 's/tif_flush.c/tif_flush.c Source\/LibTIFF4\/tif_hash_set.c/' Makefile.srcs
    fi
    env -u CXXFLAGS -u CFLAGS make -f Makefile.gnu -j"$(nproc)" \
      CC="aarch64-linux-gnu-gcc" \
      CXX="aarch64-linux-gnu-g++" \
      AR="aarch64-linux-gnu-ar"
    mkdir -p "${ROOTFS_DIR}/usr/include" "${ROOTFS_DIR}/usr/lib"
    cp Dist/FreeImage.h "${ROOTFS_DIR}/usr/include/"
    cp Dist/libfreeimage-*.so "${ROOTFS_DIR}/usr/lib/"
    cd "${ROOTFS_DIR}/usr/lib"
    ln -sf libfreeimage-*.so libfreeimage.so.3
    ln -sf libfreeimage.so.3 libfreeimage.so
fi

# 13. FFmpeg
FFMPEG_SRC="${BUILD_DIR}/ffmpeg"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libavcodec.so" ]; then
    echo "[+] Building FFmpeg..."
    if [ ! -d "${FFMPEG_SRC}" ]; then
        git clone --depth 1 -b release/6.1 https://github.com/FFmpeg/FFmpeg.git "${FFMPEG_SRC}"
    fi
    cd "${FFMPEG_SRC}"
    ./configure \
      --enable-cross-compile \
      --cc="aarch64-linux-gnu-gcc" \
      --cxx="aarch64-linux-gnu-g++" \
      --ar="aarch64-linux-gnu-ar" \
      --ranlib="aarch64-linux-gnu-ranlib" \
      --strip="aarch64-linux-gnu-strip" \
      --arch=aarch64 \
      --cpu=cortex-a76 \
      --target-os=linux \
      --prefix=/usr \
      --enable-shared \
      --disable-static \
      --disable-programs \
      --disable-doc \
      --disable-everything \
      --enable-decoder=h264,hevc,vp8,vp9,mp3,aac,flac,vorbis,opus,png,mjpeg \
      --enable-demuxer=mov,mp4,m4a,3gp,matroska,webm,mp3,ogg,flac,wav,aac,image2 \
      --enable-parser=h264,hevc,vp8,vp9,mpegaudio,aac,vorbis,opus,png,mjpeg \
      --enable-protocol=file,http,https \
      --enable-openssl \
      --enable-swscale \
      --enable-swresample \
      --enable-avfilter \
      --enable-avformat \
      --enable-avcodec \
      --enable-avutil \
      --disable-autodetect \
      --enable-zlib \
      --extra-cflags="${BASE_CFLAGS}" \
      --extra-ldflags="${BASE_LDFLAGS}"
    make -j"$(nproc)"
    make install DESTDIR="${ROOTFS_DIR}"
fi

# 14. Poppler (PDF Rendering Library for game manuals)
POPPLER_SRC="${BUILD_DIR}/poppler"
if [ ! -f "${ROOTFS_DIR}/usr/lib/libpoppler.so" ] || [ ! -f "${ROOTFS_DIR}/usr/lib/libpoppler-cpp.so" ]; then
    echo "[+] Building Poppler..."
    if [ ! -d "${POPPLER_SRC}" ]; then
        git clone --depth 1 -b poppler-24.08.0 https://gitlab.freedesktop.org/poppler/poppler.git "${POPPLER_SRC}"
    fi
    cd "${POPPLER_SRC}"
    rm -rf build_cross
    cmake -B build_cross \
      -DCMAKE_TOOLCHAIN_FILE="${CMAKE_TOOLCHAIN}" \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX=/usr \
      -DFONT_CONFIGURATION=generic \
      -DENABLE_CPP=ON \
      -DENABLE_GLIB=OFF \
      -DENABLE_GOBJECT_INTROSPECTION=OFF \
      -DENABLE_QT5=OFF \
      -DENABLE_QT6=OFF \
      -DENABLE_UTILS=OFF \
      -DENABLE_BOOST=OFF \
      -DENABLE_LIBOPENJPEG=none \
      -DENABLE_CMS=none \
      -DENABLE_LCMS=OFF \
      -DENABLE_DCTDECODER=libjpeg \
      -DENABLE_LIBTIFF=OFF \
      -DENABLE_LIBPNG=ON \
      -DENABLE_ZLIB=ON \
      -DENABLE_ZLIB_NG=OFF \
      -DENABLE_NSS3=OFF \
      -DENABLE_GPGME=OFF \
      -DENABLE_CAIRO=OFF \
      -DBUILD_SHARED_LIBS=ON \
      -DBUILD_GTK_TESTS=OFF \
      -DBUILD_QT5_TESTS=OFF \
      -DBUILD_QT6_TESTS=OFF \
      -DBUILD_CPP_TESTS=OFF \
      -DBUILD_MANUAL_TESTS=OFF
    make -C build_cross -j"$(nproc)"
    make -C build_cross install DESTDIR="${ROOTFS_DIR}"
fi

echo "======================================================================"
echo "   [✓] All ES-DE Dependencies Successfully Built & Installed!        "
echo "======================================================================"
