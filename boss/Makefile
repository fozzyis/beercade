# ==============================================================================
# B.O.S.S. Build & Packaging Makefile
# ==============================================================================

.PHONY: all rock5c rpi4 rpi5 docker install ui check clean

all: rock5c

# ------------------------------------------------------------------------------
# 1. SBC OS Images (Rock 5C prioritized)
# ------------------------------------------------------------------------------
rock5c: ui
	@echo "=== Packaging B.O.S.S. for Radxa Rock 5C (RK3588S) ==="
	sudo bash ./build/package_boss_rock5c.sh

rpi4: ui
	@echo "=== Packaging B.O.S.S. for Raspberry Pi 4 (BCM2711) ==="
	sudo bash ./build/package_boss_rpi4.sh

rpi5: ui
	@echo "=== Packaging B.O.S.S. for Raspberry Pi 5 (BCM2712) ==="
	sudo bash ./build/package_boss_rpi5.sh

# ------------------------------------------------------------------------------
# 2. Frontend & Daemon Builds
# ------------------------------------------------------------------------------
ui:
	@echo "=== Building SvelteKit Kiosk UI (Static SPA) ==="
	cd kiosk_ui && npm install && npm run build

daemon:
	@echo "=== Compiling boss-server Release Binary ==="
	cargo build --release

daemon-arm64:
	@echo "=== Cross-compiling boss-server for ARM64 ==="
	cargo build --target aarch64-unknown-linux-gnu --release

# ------------------------------------------------------------------------------
# 3. Docker & Standalone Native
# ------------------------------------------------------------------------------
docker:
	@echo "=== Building B.O.S.S. Multi-Arch Docker Container ==="
	docker build -t beercade/boss:latest .

docker-run:
	@echo "=== Launching B.O.S.S. via Docker Compose ==="
	docker compose up -d

install: ui daemon
	@echo "=== Installing B.O.S.S. to Host System ==="
	sudo bash ./deploy/install_boss.sh

install-kiosk: ui daemon
	@echo "=== Installing B.O.S.S. with Touchscreen Kiosk UI ==="
	sudo bash ./deploy/install_boss.sh --with-kiosk

clean:
	@echo "=== Cleaning Workspace ==="
	cargo clean
	rm -rf dist/* /tmp/boss_*
