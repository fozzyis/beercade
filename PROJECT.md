# Project: B.O.S.S. OS Transition to Raspberry Pi 5

## Architecture
B.O.S.S. OS is an embedded arcade and emulation operating system transitioning from Rockchip RK3588 to Broadcom BCM2712 (Raspberry Pi 5).

### Architecture Components
1. **Bootloader & Firmware Architecture**:
   - Raspberry Pi 5 on-board SPI EEPROM bootloader loads Partition 1 (FAT32 BOOT) assets: `kernel_2712.img`, `bcm2712-rpi-5-b.dtb`, `config.txt`, and `cmdline.txt`.
   - Boot firmware does not utilize legacy `start4.elf` or `fixup4.dat`.
2. **I/O Subsystem (RP1 Southbridge)**:
   - PCIe bus connection between BCM2712 PCIe Gen 2/3 host controller (`pcie-brcmstb.c`) and RP1 ASIC (PCI ID `1de4:0001`).
   - RP1 MFD (`rp1_pci.c`) multiplexes GPIO, I2C, SPI, UART, Ethernet (Cadence GEM), DWC3 USB, and PWM.
   - MSI-X interrupts from RP1 are routed via `irq-bcm2712-mip.c` to the BCM2712 GIC-400 (SPI range 128..191).
3. **Memory Architecture**:
   - Kernel compiled with `CONFIG_ARM64_16K_PAGES=y` and `CONFIG_ARM64_PAGE_SHIFT=14`.
   - User-space applications and libraries utilize 64KB ELF segment alignment.
   - JIT dynamic recompilers (e.g. Mupen64Plus-Next) patched from 4096-byte alignment to 16384-byte page alignment for POSIX `mprotect()` validity.
4. **Graphics & Multimedia Subsystem**:
   - Display: Broadcom VC4 KMS DRM driver (`vc4`) controlling dual independent micro-HDMI outputs with separate MAI audio serializers (`dtoverlay=vc4-kms-v3d-pi5`).
   - 3D Acceleration: Broadcom V3D 7.1 driver (`v3d`) and Mesa Gallium `v3d,vc4` + Vulkan `broadcom`.
   - Camera Pipeline: Raspberry Pi PiSP backend hardware accelerator (`pisp_be`) memory-to-memory engine registered as a V4L2 device.
5. **Emulation & Verification Platform**:
   - QEMU AArch64 emulation (`-M virt,gic-version=2 -cpu cortex-a76`) for validating boot, device tree, memory page size, and driver stacks.

---

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | BCM2712 Defconfig & Kernel Image | Create `bcm2712_boss_defconfig` and compile `kernel_2712.img` & `bcm2712-rpi-5-b.dtb` | M1 | Survey 1 |
| 2 | EEPROM Bootloader & Packaging | Update `package_boss_rpi5.sh` to stage kernel, DTB blobs, overlays, and config.txt in Partition 1 | M1 | Survey 1, 3 |
| 3 | RP1 PCIe Bus Enumeration | Configure PCIe root complex (`pcie-brcmstb`) for RP1 Southbridge discovery (1de4:0001) | M2 | Survey 1 |
| 4 | RP1 MSI-X Interrupt Routing | Map RP1 MSI-X interrupts through `irq-bcm2712-mip` to BCM2712 GIC SPI lines | M2 | Survey 1 |
| 5 | 16KB Page Kernel Configuration | Configure `CONFIG_ARM64_16K_PAGES=y`, verifying `getconf PAGE_SIZE == 16384` | M3 | Survey 2 |
| 6 | User-Space 16KB Dynarec Patch | Patch Mupen64Plus-Next `ALIGN(4096)` to `ALIGN(16384)` and audit ELF alignments | M3 | Survey 2 |
| 7 | VideoCore VII Mesa Pipeline | Update Mesa drivers to Gallium `v3d,vc4` and Vulkan `broadcom` with dual HDMI support | M4 | Survey 3 |
| 8 | PiSP Camera Pipeline Driver | Configure and probe `pisp_be` backend driver as V4L2 device | M4 | Survey 3 |
| 9 | QEMU Emulation & E2E Validation | Update QEMU launch scripts and automate execution of all 8 acceptance criteria | M5 | Survey 1, 2, 3 |

---

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | BCM2712 SoC Firmware & Bootloader | Kernel defconfig, `kernel_2712.img`, `bcm2712-rpi-5-b.dtb`, EEPROM bootloader packaging in `package_boss_rpi5.sh` | None | PLANNED |
| M2 | RP1 I/O Controller Integration | PCIe root complex enumeration, RP1 MFD/driver integration, MSI-X to BCM2712 GIC interrupt routing | M1 | PLANNED |
| M3 | 16KB Page Size Transition | `CONFIG_ARM64_16K_PAGES=y`, user-space dynarec buffer alignment patches, toolchain flags | M1 | PLANNED |
| M4 | VideoCore VII & PiSP Pipeline | Mesa V3D 7.1 build script, split HDMI KMS overlay, PiSP backend driver integration (`pisp_be` V4L2) | M1, M2 | PLANNED |
| M5 | E2E Integration & Verification | QEMU BCM2712 launch automation, execution of full test suite across all 8 acceptance criteria | M1, M2, M3, M4 | PLANNED |

---

## Interface Contracts

### BCM2712 Bootloader ↔ Kernel Interface
- **Boot Media**: Partition 1 FAT32 filesystem (`LABEL=BOOT`).
- **Kernel Image**: `/kernel_2712.img` (uncompressed 64-bit ARM Linux kernel).
- **Device Tree**: `/bcm2712-rpi-5-b.dtb` loaded by SPI EEPROM bootloader at firmware handoff address.
- **Kernel Command Line** (`cmdline.txt`): `console=serial0,115200 console=tty1 root=LABEL=ROOT rootfstype=ext4 rw rootwait fsck.repair=yes quiet splash vt.global_cursor_default=0`.
- **Config Parameters** (`config.txt`): `arm_64bit=1`, `enable_uart=1`, `dtoverlay=vc4-kms-v3d-pi5`.

### BCM2712 PCIe Host ↔ RP1 Southbridge Interface
- **PCIe Endpoint**: Vendor ID `0x1de4` (Raspberry Pi Ltd), Device ID `0x0001` (RP1 C0/D0).
- **Bus Address**: Root complex at `0x1000120000`.
- **MSI-X Routing**: `irq-bcm2712-mip` at `0x1000130000` translates RP1 MSI-X vectors into BCM2712 GICv2 SPI interrupts (SPI 128..191).
- **Peripheral Subdevices**: RP1 MFD instantiates GPIO/pinctrl, I2C, SPI, UART, Ethernet, USB DWC3, and PWM sub-nodes.

### Memory & User-Space Interface
- **Page Size**: System page size strictly `16384` bytes (`PAGE_SHIFT = 14`).
- **ELF Segment Alignment**: System loader requires all PT_LOAD segment virtual addresses and file offsets to satisfy `p_vaddr % 16384 == p_offset % 16384`. Standard binaries use 64KB (0x10000) alignment.
- **Dynamic Memory Allocation**: POSIX `mprotect(addr, len, prot)` requires `addr` to be a multiple of 16384.

### VideoCore VII & PiSP Pipeline Interface
- **DRM/KMS**: DRM card device nodes `/dev/dri/card0` and `/dev/dri/renderD128` backed by `vc4` and `v3d` kernel drivers.
- **Dual Display**: Independent HDMI0 and HDMI1 connectors exposed via KMS with dedicated audio endpoints `hw:CARD=vc4hdmi0` and `hw:CARD=vc4hdmi1`.
- **V4L2 PiSP**: `/dev/video*` and `/dev/media*` interfaces providing memory-to-memory hardware ISP processing for RetroArch / camera capture.

---

## Code Layout
- `d:\data\dev\beercade\build\linux-rockchip\`: Upstream Linux kernel source tree containing BCM2712 DTS and drivers.
- `d:\data\dev\beercade\ASTRO\configs\`: Kernel defconfigs (`bcm2712_boss_defconfig`).
- `d:\data\dev\beercade\BOSS\build\`: OS image assembly and packaging scripts (`package_boss_rpi5.sh`).
- `d:\data\dev\beercade\BOSS\`: BOSS system daemon, emulation launch scripts (`emulate_boss.sh`).
- `d:\data\dev\beercade\buildfarm\scripts\`: Cross-compilation scripts for Mesa (`build_mesa.sh`) and kernel (`build_kernel.sh`).
- `d:\data\dev\beercade\build\mupen64plus-libretro-nx\`: Mupen64Plus-Next source code.
- `d:\data\dev\beercade\tests\`: Verification test harnesses and E2E acceptance test runner.
