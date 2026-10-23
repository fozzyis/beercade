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
FE_SUB="fe-$PAYLOAD_NAME"
[[ -v FRONTEND_BUILDS ]] || FRONTEND_BUILDS=()
[[ -v NATIVE_BUILDS ]] || NATIVE_BUILDS=()
NATIVE_SUB="native-$PAYLOAD_NAME"

banner() { printf '\n==== %s ====\n' "$*"; }

banner "payload $PAYLOAD_NAME -> out/$IMAGE_NAME"
rm -rf "${REPO:?}/out/$BIN_SUB" "${REPO:?}/out/$FE_SUB" "${REPO:?}/out/$NATIVE_SUB"
mkdir -p "$REPO/out/$BIN_SUB" "$REPO/out/$FE_SUB" "$REPO/out/$NATIVE_SUB"

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

# Each entry src:build_subdir:dest; output lands in out/fe-<payload>/<n>/ and
# container_build.sh copies it to dest. The source tree is copied without
# node_modules/build so the Windows checkout is never written to.
if ((${#FRONTEND_BUILDS[@]})); then
  for i in "${!FRONTEND_BUILDS[@]}"; do
    IFS=: read -r fe_src fe_sub _ <<<"${FRONTEND_BUILDS[$i]}"
    banner "step 1b: frontend build $fe_src"
    docker run --rm       -v "$REPO_D:/src:ro"       -v "$OUT_D:/out"       -v beercade-x86-npm-cache:/root/.npm       node:22-bookworm       bash -euo pipefail -c '
        mkdir -p /build
        tar -C "/src/$1" --exclude=node_modules --exclude=.svelte-kit --exclude="./$2" -cf - . | tar -C /build -xf -
        cd /build && npm ci --no-audit --no-fund && npm run build
        [[ -f "$2/index.html" ]] || { echo "no $2/index.html" >&2; exit 1; }
        mkdir -p "/out/$3/$4" && cp -a "$2/." "/out/$3/$4/"' _ "$fe_src" "$fe_sub" "$FE_SUB" "$i"
  done
fi

# Each entry is a script run in archlinux:latest as `<script> <destdir>`;
# destdir (out/native-<payload>/<n>/) is overlaid onto the rootfs.
for i in "${!NATIVE_BUILDS[@]}"; do
  nb="${NATIVE_BUILDS[$i]}"
  banner "step 1c: native build $nb"
  docker run --rm     -v "$REPO_D:/src:ro"     -v "$OUT_D:/out"     -v beercade-x86-pacman-cache:/var/cache/pacman/pkg     -v beercade-x86-native-cache:/cache     archlinux:latest     bash -euo pipefail -c 'mkdir -p "/out/$2/$3" && bash "/src/$1" "/out/$2/$3"' _ "$nb" "$NATIVE_SUB" "$i"
done

banner "step 2: rootfs + image assembly (archlinux, privileged)"
docker run --rm --privileged \
  -v "$REPO_D:/src:ro" \
  -v "$OUT_D:/out" \
  -v beercade-x86-pacman-cache:/var/cache/pacman/pkg \
  -e PAYLOAD="$PAYLOAD" \
  archlinux:latest \
  bash /src/shared/buildfarm/x86_64/container_build.sh

banner "done"
ls -l "$REPO/out/$IMAGE_NAME"*
