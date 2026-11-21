# ==============================================================================
# B.E.E.R.C.A.D.E. Centralized Monorepo Build & Validation Orchestrator
# Domains: boss | astro | website | shared
# ==============================================================================

.PHONY: all check check-cargo check-types check-website check-kiosk gen-types test build-boss build-astro build-website clean image-x86 verify-x86 image-boss-x86 verify-boss-x86 image-boss-rpi

all: check test

# Validate syntax and tree integrity across all workspaces
check: check-cargo check-types check-website check-kiosk

check-cargo:
	@echo "=== Checking Cargo Workspaces (boss, astro/daemon, shared) ==="
	cargo check --workspace

check-types:
	@echo "=== Checking Shared TypeScript Wire Protocol ==="
	cargo run -p beercade-shared --locked --bin gen_ts_proto -- --check

gen-types:
	@echo "=== Generating Shared TypeScript Wire Protocol ==="
	cargo run -p beercade-shared --locked --bin gen_ts_proto -- --write

check-website:
	@echo "=== Checking Customer Website (SvelteKit) ==="
	npm --prefix website run check

check-kiosk:
	@echo "=== Checking B.O.S.S. Kiosk UI (SvelteKit) ==="
	npm --prefix boss/kiosk_ui run check

# Run unit and integration tests
test: test-cargo

test-cargo:
	@echo "=== Running Workspace Unit Tests ==="
	cargo test --workspace

# Build individual domain artifacts
build-boss:
	@echo "=== Building B.O.S.S. Daemon & Kiosk UI ==="
	cargo build --release -p boss-server
	npm --prefix boss/kiosk_ui run build

build-astro:
	@echo "=== Building A.S.T.R.O. Compute Node Daemon ==="
	cargo build --release -p astro-daemon

build-website:
	@echo "=== Building Customer Website ==="
	npm --prefix website run build

clean:
	@echo "=== Cleaning Build Artifacts ==="
	cargo clean

# x86_64 A.S.T.R.O. bootable image (Docker) and QEMU boot verification
image-x86:
	@echo "=== Building x86_64 A.S.T.R.O. Image (out/beercade-astro-x86_64.raw) ==="
	bash shared/buildfarm/x86_64/build_image.sh astro

verify-x86:
	@echo "=== Verifying x86_64 A.S.T.R.O. Image Boot (QEMU + OVMF, TCG) ==="
	bash shared/tests/verify_qemu_x86_64.sh astro

# x86_64 B.O.S.S. image (+ .zst) and QEMU gate: services, :8080, kiosk, /data persistence, A/B switch
image-boss-x86:
	@echo "=== Building x86_64 B.O.S.S. Image (out/beercade-boss-x86_64.raw) ==="
	bash shared/buildfarm/x86_64/build_image.sh boss

image-boss-rpi:
	@echo "=== Building Raspberry Pi 4/5 B.O.S.S. Image (out/beercade-boss-rpi-aarch64.raw) ==="
	bash shared/buildfarm/aarch64/build_image.sh boss

verify-boss-x86:
	@echo "=== Verifying x86_64 B.O.S.S. Image (QEMU + OVMF, TCG, two boots) ==="
	bash shared/tests/verify_qemu_x86_64.sh boss
