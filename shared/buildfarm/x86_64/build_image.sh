#!/usr/bin/env bash
# Host-side entry point (Git Bash on Windows, or Linux; Docker required).
#   bash shared/buildfarm/x86_64/build_image.sh [payload]   (default: astro)
# Output: out/<IMAGE_NAME> in the repo root.
set -euo pipefail
export MSYS_NO_PATHCONV=1

PAYLOAD="${1:-astro}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
PAYLOAD_FILE="$HERE/payloads/$PAYLOAD.conf"
[[ -f "$PAYLOAD_FILE" ]] || { echo "payload not found: $PAYLOAD_FILE" >&2; exit 1; }
# shellcheck source=/dev/null
source "$PAYLOAD_FILE"

# Docker Desktop on Windows needs D:/... paths; pwd -W gives that in Git Bash.
winpath() { (cd "$1" && { pwd -W 2>/dev/null || pwd; }); }
mkdir -p "$REPO/out"
REPO_D="$(winpath "$REPO")"
OUT_D="$(winpath "$REPO/out")"
BIN_SUB="bin-$PAYLOAD_NAME"

banner() { printf '\n==== %s ====\n' "$*"; }

banner "payload $PAYLOAD_NAME -> out/$IMAGE_NAME"
rm -rf "${REPO:?}/out/$BIN_SUB"
mkdir -p "$REPO/out/$BIN_SUB"

if ((${#CARGO_PACKAGES[@]})); then
  banner "step 1: cargo release build (${CARGO_PACKAGES[*]})"
  docker run --rm \
    -v "$REPO_D:/src:ro" \
    -v "$OUT_D:/out" \
    -v beercade-x86-cargo-target:/target \
    -v beercade-x86-cargo-registry:/usr/local/cargo/registry \
    -e CARGO_TARGET_DIR=/target \
    -e BIN_SUB="$BIN_SUB" \
    -w /src rust:1-bookworm \
    bash -euo pipefail -c '
      args=(); for p in "$@"; do args+=(-p "$p"); done
      cargo build --release --locked "${args[@]}"
      for p in "$@"; do install -m 0755 "/target/release/$p" "/out/$BIN_SUB/$p"; done
      ls -l "/out/$BIN_SUB"' _ "${CARGO_PACKAGES[@]}"
fi

banner "step 2: rootfs + image assembly (archlinux, privileged)"
docker run --rm --privileged \
  -v "$REPO_D:/src:ro" \
  -v "$OUT_D:/out" \
  -v beercade-x86-pacman-cache:/var/cache/pacman/pkg \
  -e PAYLOAD="$PAYLOAD" \
  archlinux:latest \
  bash /src/shared/buildfarm/x86_64/container_build.sh

banner "done"
ls -l "$REPO/out/$IMAGE_NAME"
