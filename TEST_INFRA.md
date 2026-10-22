# E2E Test Infra: B.O.S.S. OS Transition to Raspberry Pi 5

## Test Philosophy
- Opaque-box, requirement-driven verification derived directly from `ORIGINAL_REQUEST.md`.
- Automated test execution through QEMU BCM2712 emulation environment and test suites.
- Methodology: Category-Partition + Boundary Value Analysis (BVA) + Pairwise Combinatorial Testing + Real-World Workload Testing across 4 tiers.

## Feature Inventory & Test Mapping
| # | Requirement | Source | Tier 1 (Isolated) | Tier 2 (Boundary/Edge) | Tier 3 (Cross-Feature) | Tier 4 (Workload) |
|---|-------------|--------|:-----------------:|:----------------------:|:----------------------:|:-----------------:|
| 1 | BCM2712 Firmware & Boot | ORIGINAL_REQUEST §R1 | 5 | 5 | ✓ | ✓ |
| 2 | RP1 I/O Controller & MSI-X | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ | ✓ |
| 3 | 16KB Page Size Transition | ORIGINAL_REQUEST §R3 | 5 | 5 | ✓ | ✓ |
| 4 | VideoCore VII & PiSP Pipeline | ORIGINAL_REQUEST §R4 | 5 | 5 | ✓ | ✓ |

## Test Architecture
- **Test Runner**: `d:\data\dev\beercade\tests\run_acceptance_tests.sh` / `d:\data\dev\beercade\tests\run_acceptance_tests.py`
- **Invocation**: Executes QEMU BCM2712 boot sequence, captures serial output, inspects dmesg/sysfs/procfs, runs user-space benchmarks and test suites, and validates exit codes.
- **Pass/Fail Semantics**: Strict exit code 0; all assertions must evaluate to true with zero segmentation faults or alignment errors.

## Test Directory Layout
- `d:\data\dev\beercade\tests\tier1_features\`: Individual feature smoke tests (dtb loading, kernel_2712 boot, getconf PAGE_SIZE, lspci RP1, v4l2 pisp_be probe).
- `d:\data\dev\beercade\tests\tier2_boundaries\`: Boundary & edge tests (misaligned page accesses, mprotect limits, CMA memory exhaustion, PCIe config space boundaries).
- `d:\data\dev\beercade\tests\tier3_interactions\`: Cross-feature pairwise interactions (RP1 interrupts under 16KB pages, VideoCore VII rendering with 16KB buffers, PiSP streaming with RP1 audio).
- `d:\data\dev\beercade\tests\tier4_scenarios\`: Full real-world scenarios (RetroArch game launch with Mupen64Plus-Next JIT on 16KB pages, dual HDMI display output, and BOSS network telemetry mesh).

## Real-World Application Scenarios (Tier 4)
| # | Scenario | Features Exercised | Complexity |
|---|----------|--------------------|------------|
| 1 | Full B.O.S.S. System Boot in QEMU BCM2712 | R1, R2, R3, R4 | High |
| 2 | N64 Emulation (Mupen64Plus-Next JIT) on 16KB Pages | R1, R3 | High |
| 3 | VideoCore VII OpenGL/EGL Rendering Pipeline | R1, R4 | High |
| 4 | PiSP V4L2 Hardware Processing & Stream Capture | R1, R4 | Medium |
| 5 | Full BOSS Mesh Telemetry & Pacing Integration | R1, R2, R3, R4 | High |

## Coverage Thresholds
- Tier 1: ≥5 per feature (20 test cases)
- Tier 2: ≥5 per feature (20 test cases)
- Tier 3: ≥4 cross-feature interaction suites
- Tier 4: ≥5 realistic end-to-end application scenarios
- Total Target: ≥49 test cases
