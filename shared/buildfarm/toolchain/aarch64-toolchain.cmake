set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR aarch64)

set(CMAKE_C_COMPILER aarch64-linux-gnu-gcc)
set(CMAKE_CXX_COMPILER aarch64-linux-gnu-g++)

# Point cmake's find_path / find_library at the custom rootfs (Mesa GLES3/Vulkan).
# Do NOT set CMAKE_SYSROOT — that overrides --sysroot for gcc and breaks glibc
# resolution from the cross-toolchain's own sysroot.
set(ASTRO_ROOTFS "${CMAKE_CURRENT_LIST_DIR}/../../ASTRO/rootfs")
get_filename_component(ASTRO_ROOTFS "${ASTRO_ROOTFS}" ABSOLUTE)
set(CMAKE_FIND_ROOT_PATH "${ASTRO_ROOTFS}" "/usr/aarch64-linux-gnu")

set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)

set(RK3588_OPT_FLAGS "-O3 -march=armv8.2-a+crypto+fp16+dotprod -mtune=cortex-a76 -ffunction-sections -fdata-sections -fomit-frame-pointer")
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} ${RK3588_OPT_FLAGS} -isystem ${ASTRO_ROOTFS}/usr/include" CACHE STRING "" FORCE)
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} ${RK3588_OPT_FLAGS} -isystem ${ASTRO_ROOTFS}/usr/include" CACHE STRING "" FORCE)
set(CMAKE_EXE_LINKER_FLAGS "${CMAKE_EXE_LINKER_FLAGS} -Wl,--gc-sections -Wl,-O1 -Wl,--as-needed -L${ASTRO_ROOTFS}/usr/lib -Wl,-rpath-link,${ASTRO_ROOTFS}/usr/lib" CACHE STRING "" FORCE)
set(CMAKE_SHARED_LINKER_FLAGS "${CMAKE_SHARED_LINKER_FLAGS} -L${ASTRO_ROOTFS}/usr/lib -Wl,-rpath-link,${ASTRO_ROOTFS}/usr/lib" CACHE STRING "" FORCE)
