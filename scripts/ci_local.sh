#!/usr/bin/env bash
# Local Linux/WSL2 CI runner mirroring .github/workflows/ci.yml.
# Usage: bash scripts/ci_local.sh [--with-frontends]
set -euo pipefail

WITH_FRONTENDS=0
for arg in "$@"; do
  case "$arg" in
    --with-frontends) WITH_FRONTENDS=1 ;;
    -h|--help)
      echo "usage: $0 [--with-frontends]"
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

# Keep Linux build artifacts on ext4 (fast & isolated from Windows target/).
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/var/cache/beercade-ci/target}"
export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}"
RT_BUILD_DIR="${RT_BUILD_DIR:-/tmp/beercade-astro-rt-build}"
mkdir -p "$CARGO_TARGET_DIR"

echo "==== 0. M4 governance, crate-boundary & x86_64 payload/VHDX lint ===="
python3 scripts/lint_governance.py
python3 shared/tests/test_x86_payload_and_vhdx.py

echo "==== 1. Cargo check (workspace, --locked, -D warnings) ===="
RUSTFLAGS="-D warnings" cargo check --workspace --locked --message-format=short

echo "==== 2. Cargo test (workspace, --locked, all 6 crates incl. astro-shell) ===="
cargo test --workspace --locked

echo "==== 2.5. TypeScript wire proto drift check ===="
cargo run -p beercade-shared --locked --bin gen_ts_proto -- --check

echo "==== 3. Cargo release build (astro-daemon, astro-shell, astro-tune, boss-server) ===="
cargo build --release --locked -p astro-daemon -p astro-shell -p astro-tune -p boss-server
ls -lh \
  "$CARGO_TARGET_DIR/release/astro-daemon" \
  "$CARGO_TARGET_DIR/release/astro-shell" \
  "$CARGO_TARGET_DIR/release/astro-tune" \
  "$CARGO_TARGET_DIR/release/boss-server"

echo "==== 4. astro_rt CMake build & CTest ===="
rm -rf "$RT_BUILD_DIR"
cmake -S astro/rt -B "$RT_BUILD_DIR" -G Ninja
cmake --build "$RT_BUILD_DIR"
ctest --test-dir "$RT_BUILD_DIR" --output-on-failure

echo "==== 5. Buildfarm shell syntax check ===="
bash -n shared/buildfarm/aarch64/build_image.sh shared/buildfarm/x86_64/build_image.sh shared/tests/verify_qemu_x86_64.sh scripts/ci_local.sh

if ((WITH_FRONTENDS)); then
  echo "==== 6. Frontend type checks ===="
  npm --prefix website run check
  npm --prefix boss/kiosk_ui run check
fi

echo "==== LOCAL CI PASSED ===="
