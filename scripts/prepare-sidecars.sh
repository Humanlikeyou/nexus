#!/usr/bin/env bash
# Stage the bundled sidecars into the git-ignored src-tauri/binaries/ (decision 0031).
#
#   scripts/prepare-sidecars.sh            copy ollama runtime + build amara-router
#   scripts/prepare-sidecars.sh --check    only verify the staged files exist (used by
#                                          beforeBuildCommand so `tauri build` fails clearly)
#
# Layout it produces (tauri.conf.json bundle.macOS.files maps it into the .app):
#   src-tauri/binaries/amara-router-aarch64-apple-darwin      -> Contents/MacOS/amara-router
#   src-tauri/binaries/ollama-runtime/{ollama,llama-server,*LICENSE*,*NOTICE*}
#                                                              -> Contents/Resources/ollama-runtime/
# Not copied: x86_64-only libggml-*/libllama*/libmtmd dylibs and .so CPU backends (the arm64
# ollama and llama-server link no non-system libraries), llama-quantize, icons, and the MLX
# engine dirs (439 MB) unless NEXUS_WITH_MLX=1.
#
# Env: OLLAMA_APP_RESOURCES (source, read-only; default the installed Ollama.app),
#      NEXUS_WITH_MLX=1 (also copy mlx_metal_v3/v4), NEXUS_THIN_ARM64=1 (lipo -thin arm64: smaller,
#      but the SHA-256 and Ollama's signature no longer match; Phase 14 re-signs anyway).
set -euo pipefail

EXPECTED_OLLAMA_SHA256="1e293ef2336e6f6be84e005ed539c6d2f3176d66b6c13350126f14fe0182024c"
TRIPLE="aarch64-apple-darwin"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$ROOT/src-tauri/binaries"
RUNTIME_DIR="$BIN_DIR/ollama-runtime"
ROUTER_OUT="$BIN_DIR/amara-router-$TRIPLE"
SRC="${OLLAMA_APP_RESOURCES:-/Applications/Ollama.app/Contents/Resources}"

fail() { echo "prepare-sidecars: $*" >&2; exit 1; }

check_staged() {
  local missing=0 f
  for f in "$ROUTER_OUT" "$RUNTIME_DIR/ollama" "$RUNTIME_DIR/llama-server"; do
    if [ ! -f "$f" ] || [ ! -x "$f" ]; then
      echo "prepare-sidecars: missing $f" >&2
      missing=1
    fi
  done
  if [ "$missing" -ne 0 ]; then
    fail "sidecars are not staged. Run: scripts/prepare-sidecars.sh   (then build again)"
  fi
}

if [ "${1:-}" = "--check" ]; then
  check_staged
  echo "prepare-sidecars: staged sidecars present"
  exit 0
fi
[ "$#" -eq 0 ] || fail "unknown argument: $1"

[ "$(uname -s)" = "Darwin" ] && [ "$(uname -m)" = "arm64" ] || fail "needs an Apple Silicon Mac"
[ -d "$SRC" ] || fail "source not found: $SRC"
[ -f "$SRC/ollama" ] || fail "no ollama in $SRC"
[ -f "$SRC/llama-server" ] || fail "no llama-server in $SRC"

sha256_of() { shasum -a 256 "$1" | awk '{print $1}'; }

src_sha="$(sha256_of "$SRC/ollama")"
[ "$src_sha" = "$EXPECTED_OLLAMA_SHA256" ] || fail "ollama SHA-256 mismatch: got $src_sha, want $EXPECTED_OLLAMA_SHA256"

# The arm64 slices must not need any library we are not copying.
for b in ollama llama-server; do
  extra="$(otool -arch arm64 -L "$SRC/$b" | tail -n +2 | awk '{print $1}' \
    | grep -v -e '^/usr/lib/' -e '^/System/' || true)"
  [ -z "$extra" ] || fail "$b needs non-system libraries ($extra); script must be updated"
done

# Stage into a temp dir next to the target, then swap, so a failed run leaves the old copy.
mkdir -p "$BIN_DIR"
STAGE="$(mktemp -d "$BIN_DIR/.stage.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/ollama-runtime"

for b in ollama llama-server; do
  if [ "${NEXUS_THIN_ARM64:-0}" = "1" ]; then
    lipo "$SRC/$b" -thin arm64 -output "$STAGE/ollama-runtime/$b"
  else
    cp -p "$SRC/$b" "$STAGE/ollama-runtime/$b"
  fi
  chmod 755 "$STAGE/ollama-runtime/$b"
done

# Licences: every LICENSE / NOTICE file the app ships next to the runtime (MIT et al.).
lic_count=0
for f in "$SRC"/*LICENSE* "$SRC"/*NOTICE*; do
  [ -f "$f" ] || continue
  cp -p "$f" "$STAGE/ollama-runtime/"
  chmod 644 "$STAGE/ollama-runtime/$(basename "$f")"
  lic_count=$((lic_count + 1))
done
[ "$lic_count" -gt 0 ] || fail "no licence files found in $SRC"

if [ "${NEXUS_WITH_MLX:-0}" = "1" ]; then
  for d in mlx_metal_v3 mlx_metal_v4; do
    [ -d "$SRC/$d" ] || fail "no $d in $SRC"
    cp -pR "$SRC/$d" "$STAGE/ollama-runtime/$d"
  done
fi

if [ "${NEXUS_THIN_ARM64:-0}" != "1" ]; then
  [ "$(sha256_of "$STAGE/ollama-runtime/ollama")" = "$EXPECTED_OLLAMA_SHA256" ] \
    || fail "copied ollama SHA-256 differs from the source"
fi

# Prove the copy runs on its own: version only (never serve/pull/run), scratch HOME, empty PATH.
scratch_home="$(mktemp -d)"
ver="$(env -i PATH=/usr/bin:/bin HOME="$scratch_home" NEXUS_OUTBOUND_DISABLED=1 \
  "$STAGE/ollama-runtime/ollama" --version 2>&1)" || { rm -rf "$scratch_home"; fail "copied ollama --version failed: $ver"; }
rm -rf "$scratch_home"
echo "$ver" | grep -q "0.40.14" || fail "unexpected ollama version output: $ver"

old="$BIN_DIR/ollama-runtime"
if [ -e "$old" ]; then
  case "$old" in */src-tauri/binaries/ollama-runtime) rm -rf "$old" ;; *) fail "refusing to remove $old" ;; esac
fi
mv "$STAGE/ollama-runtime" "$old"

# amara-router, release build (offline: the lockfile and cargo cache are enough).
cargo build --release --locked --offline --manifest-path "$ROOT/amara-router/Cargo.toml"
router_built="$ROOT/amara-router/target/release/amara-router"
[ -x "$router_built" ] || fail "amara-router build produced no binary"
cp -p "$router_built" "$ROUTER_OUT.tmp"
chmod 755 "$ROUTER_OUT.tmp"
mv "$ROUTER_OUT.tmp" "$ROUTER_OUT"

check_staged

echo "--- staged ---"
du -sh "$RUNTIME_DIR" "$ROUTER_OUT"
du -sh "$RUNTIME_DIR"/* | sed 's/^/  /'
echo "ollama      sha256 $(sha256_of "$RUNTIME_DIR/ollama")"
echo "llama-server sha256 $(sha256_of "$RUNTIME_DIR/llama-server")"
echo "amara-router sha256 $(sha256_of "$ROUTER_OUT")"
echo "ollama version: $ver"
