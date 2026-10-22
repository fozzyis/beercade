# Beercade A.S.T.R.O. - RT Input & Locked Audio Deployment Pack

Branch: `beercade-input-audio` (RetroArch worktree)

## What ships here

1. **Sub-frame input integration (`gfx/drivers/drm_gfx.c`)**
   The plain KMSDRM driver now drains the lock-free SPSC evdev ring
   (`astro_rt::SpscRing` behind the C ABI `astro_input_drain()`) on the
   driver thread immediately before the blit + atomic page flip, i.e. right
   before VBLANK. Gated behind the `HAVE_ASTRO_RT_INPUT` build flag; build
   with `-DHAVE_ASTRO_RT_INPUT`, `-I<path-to-astro_rt>/include` and link
   `-lastro_rt`. Without the flag the hook compiles to an empty function, so
   stock RetroArch builds are unaffected.

2. **1000 Hz real-time encoder rules (`deploy/udev/92-beercade-input-rt.rules`)**
   Direct non-root evdev access plus `BEERCADE_RT` tagging for Sanwa/Xin-Mo
   arcade encoders and gamepads. The RT itself is granted by
   `deploy/security/limits.d/50-beercade-rt.conf` (`rtprio 90`,
   `memlock unlimited`, `nice -20`), which the `astro_rt` producer thread
   uses to raise itself to `SCHED_FIFO(90)` on the Cortex-A55 cluster.

3. **rockchipdp0 ALSA hardware endpoint lock (`deploy/udev/93-beercade-audio.rules`)**
   Locks `hw:CARD=rockchipdp0,DEV=0` exclusively for the runtime and keeps
   plug/dmix emulation out of the path, eliminating the context-switch
   deadlock class on the on-board DP/HDMI audio. RetroArch config pins the
   same endpoint (`deploy/retroarch_rk3588.cfg`, `audio_device`).

## Install

```
sudo ./deploy/install-rt.sh
```
