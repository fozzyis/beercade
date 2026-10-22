# ==============================================================================
# B.E.E.R.C.A.D.E. Centralized Monorepo Build & Validation Orchestrator
# Domains: boss | astro | website | shared
# ==============================================================================

.PHONY: all check test build-boss build-astro build-website clean

all: check test

# Validate syntax and tree integrity across all workspaces
check: check-cargo check-website check-kiosk

check-cargo:
	@echo "=== Checking Cargo Workspaces (boss, astro/daemon, shared) ==="
	cargo check --workspace

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
