# ==============================================================================
# B.E.E.R.C.A.D.E. Shared Components Makefile
# Modules: crates/beercade-shared | arch | proto
# ==============================================================================

.PHONY: all test check types

all: check test types

check:
	@echo "=== Checking beercade-shared crate ==="
	cargo check -p beercade-shared

test:
	@echo "=== Testing beercade-shared crate ==="
	cargo test -p beercade-shared

types:
	@echo "=== Verifying Shared Protocol Definitions ==="
	test -f proto/telemetry.ts
