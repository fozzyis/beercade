# ==============================================================================
# A.S.T.R.O. Build Makefile
# Subsystems: daemon | rt | platform (RK3588)
# ==============================================================================

.PHONY: all daemon rt clean

all: daemon rt

daemon:
	@echo "=== Compiling A.S.T.R.O. Node Daemon ==="
	cargo build --release -p astro-daemon

daemon-arm64:
	@echo "=== Cross-compiling A.S.T.R.O. Node Daemon for ARM64 ==="
	cargo build --target aarch64-unknown-linux-gnu --release -p astro-daemon

rt:
	@echo "=== Building A.S.T.R.O. Real-Time C++ SPSC Engine ==="
	cmake -B rt/build -S rt
	cmake --build rt/build

clean:
	@echo "=== Cleaning ASTRO artifacts ==="
	cargo clean -p astro-daemon
	rm -rf rt/build
