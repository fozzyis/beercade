# GPU support (A.S.T.R.O. x86_64)

| GPU | Packages |
|-----|----------|
| AMD (GCN and newer) | `mesa`, `vulkan-radeon` (radeonsi / RADV) |
| Intel | `mesa`, `vulkan-intel` (iris / ANV) |
| NVIDIA GTX 16xx, RTX and newer | `nvidia-open`, `nvidia-utils` |

Older NVIDIA cards (pre-Turing) are not supported: `nvidia-open` does not
cover them and nouveau is not an arcade target.

## How the driver is chosen at boot

1. udev autoloads the kernel module from the PCI modalias (`amdgpu`, `i915`,
   `xe`, `nvidia`). `/etc/modprobe.d/beercade-gpu.conf` blacklists `nouveau`
   and sets `nvidia_drm modeset=1 fbdev=1`, which NVIDIA KMS needs.
2. `astro/launcher/astro_kms_launcher.sh` probes `/sys/class/drm`: the first
   card with a `connected` connector whose driver is not `simpledrm` / `efifb`
   wins; otherwise the first non-framebuffer card; otherwise `card0`. It
   exports `SDL_VIDEODRIVER=kmsdrm`, `DRM_DEVICE` and
   `SDL_KMSDRM_DEVICE_INDEX`, plus `GBM_BACKEND=nvidia-drm` and
   `__GLX_VENDOR_LIBRARY_NAME=nvidia` for the NVIDIA driver. The choice is
   logged to stderr.

## Overriding

Set `ASTRO_DEV=/dev/dri/cardN` in the environment of the launcher to skip the
probe (for example on a hybrid-graphics machine where the wrong card drives the
display).

## Status

Only QEMU (`bochs` / `virtio-gpu`) is tested so far. AMD, Intel and NVIDIA
paths are untested on real hardware.
