#!/usr/bin/env bash
# Host-side entry point for Raspberry Pi 4/5 images (Git Bash on Windows, or
# Linux; Docker with arm64 emulation required: Docker Desktop has it, on Linux
# install qemu-user-static/binfmt).
#   bash shared/buildfarm/aarch64/build_image.sh [payload]   (default: boss)
# Output: out/<IMAGE_NAME> in the repo root.
set -euo pipefail
export MSYS_NO_PATHCONV=1

PAYLOAD="${1:-boss}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
PAYLOAD_FILE="$HERE/payloads/$PAYLOAD.conf"
[[ -f "$PAYLOAD_FILE" ]] || { echo "payload not found: $PAYLOAD_FILE" >&2; exit 1; }
# shellcheck source=/dev/null
source "$PAYLOAD_FILE"

winpath() { (cd "$1" && { pwd -W 2>/dev/null || pwd; }); }
mkdir -p "$REPO/out"
REPO_D="$(winpath "$REPO")"
OUT_D="$(winpath "$REPO/out")"
BIN_SUB="bin-$PAYLOAD_NAME-aarch64"
FE_SUB="fe-$PAYLOAD_NAME-aarch64"
[[ -v FRONTEND_BUILDS ]] || FRONTEND_BUILDS=()

banner() { printf '\n==== %s ====\n' "$*"; }

banner "payload $PAYLOAD_NAME (aarch64) -> out/$IMAGE_NAME"
rm -rf "${REPO:?}/out/$BIN_SUB" "${REPO:?}/out/$FE_SUB"
mkdir -p "$REPO/out/$BIN_SUB" "$REPO/out/$FE_SUB"

# Native arm64 build under emulation: slow (tens of minutes cold) but needs no
# cross toolchain, and picks up .cargo/config.toml's aarch64 CPU floor.
if ((${#CARGO_PACKAGES[@]})); then
  banner "step 1: cargo release build (${CARGO_PACKAGES[*]}, linux/arm64)"
  docker run --rm --platform linux/arm64 \
    -v "$REPO_D:/src:ro" \
    -v "$OUT_D:/out" \
    -v beercade-arm-cargo-target:/target \
    -v beercade-arm-cargo-registry:/usr/local/cargo/registry \
    -e CARGO_TARGET_DIR=/target \
    -e BIN_SUB="$BIN_SUB" \
    -w /src rust:1-bookworm \
    bash -euo pipefail -c '
      args=(); for p in "$@"; do args+=(-p "$p"); done
      cargo build --release --locked "${args[@]}"
      for p in "$@"; do install -m 0755 "/target/release/$p" "/out/$BIN_SUB/$p"; done
      ls -l "/out/$BIN_SUB"' _ "${CARGO_PACKAGES[@]}"
fi

# Frontends are architecture-independent: built natively, as for x86.
if ((${#FRONTEND_BUILDS[@]})); then
  for i in "${!FRONTEND_BUILDS[@]}"; do
    IFS=: read -r fe_src fe_sub _ <<<"${FRONTEND_BUILDS[$i]}"
    banner "step 1b: frontend build $fe_src"
    docker run --rm -v "$REPO_D:/src:ro" -v "$OUT_D:/out" -v beercade-x86-npm-cache:/root/.npm node:22-bookworm \
      bash -euo pipefail -c '
        mkdir -p /build
        tar -C "/src/$1" --exclude=node_modules --exclude=.svelte-kit --exclude="./$2" -cf - . | tar -C /build -xf -
        cd /build && npm ci --no-audit --no-fund && npm run build
        [[ -f "$2/index.html" ]] || { echo "no $2/index.html" >&2; exit 1; }
        mkdir -p "/out/$3/$4" && cp -a "$2/." "/out/$3/$4/"' _ "$fe_src" "$fe_sub" "$FE_SUB" "$i"
  done
fi

banner "step 2: rootfs + image assembly (linux/arm64, privileged)"
docker run --rm --privileged --platform linux/arm64 \
  -v "$REPO_D:/src:ro" \
  -v "$OUT_D:/out" \
  -v beercade-arm-alarm-cache:/cache \
  -e PAYLOAD="$PAYLOAD" \
  -e BIN_SUB="$BIN_SUB" \
  -e FE_SUB="$FE_SUB" \
  debian:bookworm-slim \
  bash /src/shared/buildfarm/aarch64/container_build.sh

banner "done"
ls -l "$REPO/out/$IMAGE_NAME"*
