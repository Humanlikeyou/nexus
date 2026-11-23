#!/usr/bin/env bash
# Local release (handover Phase 14): build -> sign every nested binary -> notarize -> staple -> DMG -> verify.
# No secrets in this file. The signing identity comes from the keychain / APPLE_SIGNING_IDENTITY, the
# notary credentials from the keychain profile `nexus-notary` (created once with `xcrun notarytool
# store-credentials`, never in the repo). Never touches /Applications.
#
#   scripts/release-local.sh --unsigned   build the app (ad-hoc signed, like the build we install) and an
#                                         UNSIGNED, un-notarized DMG, then stop. Needs no Developer ID.
#   scripts/release-local.sh              signed + notarized release. Refuses (exit 2) when no
#                                         "Developer ID Application" identity exists.
#   scripts/release-local.sh --check      only report what is missing; build nothing.
#
# Env: APPLE_SIGNING_IDENTITY (full name or hash; default: the only Developer ID Application found),
#      NEXUS_NOTARY_PROFILE (default nexus-notary).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
APP_NAME="Amara OS"
PROFILE="${NEXUS_NOTARY_PROFILE:-nexus-notary}"
ENTITLEMENTS="$ROOT/src-tauri/Entitlements.plist"
BUNDLE_DIR="$ROOT/src-tauri/target/release/bundle"
APP="$BUNDLE_DIR/macos/$APP_NAME.app"
DMG_DIR="$BUNDLE_DIR/dmg"

mode="signed"
case "${1:-}" in
  "") ;;
  --unsigned) mode="unsigned" ;;
  --check) mode="check" ;;
  *) echo "release-local: unknown argument: $1" >&2; exit 1 ;;
esac

fail() { echo "release-local: $1" >&2; exit "${2:-1}"; }
log() { echo "release-local: $*"; }

[ "$(uname -s)" = "Darwin" ] && [ "$(uname -m)" = "arm64" ] || fail "needs an Apple Silicon Mac"

version="$(node -p "require('./src-tauri/tauri.conf.json').version")"
DMG="$DMG_DIR/${APP_NAME}_${version}_aarch64.dmg"

# Identity: never printed beyond its public name.
find_identity() {
  if [ -n "${APPLE_SIGNING_IDENTITY:-}" ] && [ "$APPLE_SIGNING_IDENTITY" != "-" ]; then
    echo "$APPLE_SIGNING_IDENTITY"; return 0
  fi
  local ids
  ids="$(security find-identity -v -p codesigning | grep 'Developer ID Application' || true)"
  [ -n "$ids" ] || return 1
  [ "$(echo "$ids" | wc -l | tr -d ' ')" = "1" ] || { echo "release-local: several Developer ID identities; set APPLE_SIGNING_IDENTITY" >&2; return 2; }
  echo "$ids" | sed -E 's/^[^"]*"([^"]+)".*$/\1/'
}

# The sidecars must be staged (scripts/prepare-sidecars.sh) and mapped by bundle.macOS.files.
check_sidecars() { bash scripts/prepare-sidecars.sh --check; }

make_dmg() { # <app> <out.dmg>
  local app="$1" out="$2" stage
  mkdir -p "$(dirname "$out")"
  rm -f "$out"
  stage="$(mktemp -d "${TMPDIR:-/tmp}/amara-dmg.XXXXXX")"
  ditto "$app" "$stage/$APP_NAME.app"
  ln -s /Applications "$stage/Applications"
  # create (not "tauri bundle dmg"): no mount, so no stale volume.
  hdiutil create -quiet -volname "$APP_NAME" -srcfolder "$stage" -ov -format UDZO "$out"
  rm -rf "$stage"
}

if [ "$mode" = "check" ]; then
  id="$(find_identity)" && log "Developer ID Application identity: $id" || log "MISSING: no Developer ID Application identity (security find-identity -v -p codesigning)"
  if xcrun notarytool history --keychain-profile "$PROFILE" >/dev/null 2>&1; then log "notary profile '$PROFILE': present"; else log "MISSING: notary keychain profile '$PROFILE'"; fi
  exit 0
fi

if [ "$mode" = "unsigned" ]; then
  log "unsigned path: ad-hoc signed app + unsigned DMG, no notarization"
  check_sidecars
  APPLE_SIGNING_IDENTITY=- npx --no-install tauri build --bundles app
  [ -d "$APP" ] || fail "build did not produce $APP"
  make_dmg "$APP" "$DMG"
  log "built: $APP"
  log "built: $DMG   (UNSIGNED: Gatekeeper will block it on another Mac)"
  exit 0
fi

# ---- signed path ----
id="$(find_identity)" || rc=$?
if [ -z "${id:-}" ]; then
  fail "no 'Developer ID Application' identity in the keychain (0 found). Install the certificate first (Taisun's step), or use --unsigned. Nothing was built." 2
fi
xcrun notarytool history --keychain-profile "$PROFILE" >/dev/null 2>&1 \
  || fail "notary keychain profile '$PROFILE' is missing. Create it with: xcrun notarytool store-credentials $PROFILE (Taisun's step). Nothing was built." 2

check_sidecars
log "building with identity '$id'"
APPLE_SIGNING_IDENTITY="$id" npx --no-install tauri build --bundles app
[ -d "$APP" ] || fail "build did not produce $APP"

# Sign nested code deepest-first, then the app. Tauri signs the main binary and the app, but
# Contents/Resources/ollama-runtime/* and Contents/MacOS/amara-router come from bundle.macOS.files and
# must be signed here. The hardened runtime (--options runtime) is applied ONLY here, never in
# tauri.conf.json (hardenedRuntime=false there): the ad-hoc build we install must not get library
# validation, which could break the bundled ollama/llama-server, Muse BLE or the router. Live-test
# the first signed build with Muse, Ollama and the router before trusting it.
sign() { codesign --force --timestamp --options runtime --sign "$id" "$@"; }
main_exe="$(/usr/libexec/PlistBuddy -c 'Print CFBundleExecutable' "$APP/Contents/Info.plist")"
nested_list="$(mktemp "${TMPDIR:-/tmp}/amara-nested.XXXXXX")"
# every Mach-O in the bundle except the main executable, deepest path first
while IFS= read -r -d '' f; do
  [ "$f" = "$APP/Contents/MacOS/$main_exe" ] && continue
  if file "$f" | grep -q "Mach-O"; then printf '%s\n' "$f"; fi
done < <(find "$APP/Contents" -type f -print0) \
  | awk -F/ '{ print NF "\t" $0 }' | sort -rn -s -k1,1 | cut -f2- > "$nested_list"
grep -qx "$APP/Contents/MacOS/amara-router" "$nested_list" || fail "amara-router not found in the bundle"
nested=0
while IFS= read -r f; do sign "$f"; nested=$((nested+1)); done < "$nested_list"
log "signed $nested nested binaries deepest-first; signing the app"
sign --entitlements "$ENTITLEMENTS" "$APP"
# every nested binary must carry the runtime flag
while IFS= read -r f; do
  codesign -dv --verbose=4 "$f" 2>&1 | grep -q "flags=.*runtime" || fail "nested binary lacks the hardened runtime flag: ${f#"$APP"/}"
done < "$nested_list"
codesign -dv --verbose=4 "$APP" 2>&1 | grep -q "flags=.*runtime" || fail "app lacks the hardened runtime flag"
rm -f "$nested_list"

codesign --verify --deep --strict --verbose=2 "$APP"
! codesign -d --entitlements - "$APP" 2>/dev/null | grep -q "get-task-allow" || fail "get-task-allow present in a release build"

submit() { # <file>
  log "notarizing $(basename "$1") (profile $PROFILE), waiting"
  xcrun notarytool submit "$1" --keychain-profile "$PROFILE" --wait
}

work="$(mktemp -d "${TMPDIR:-/tmp}/amara-notarize.XXXXXX")"
trap 'rm -rf "$work"' EXIT
ditto -c -k --keepParent "$APP" "$work/app.zip"
submit "$work/app.zip"
xcrun stapler staple "$APP"

make_dmg "$APP" "$DMG"
codesign --force --timestamp --sign "$id" "$DMG"
submit "$DMG"
xcrun stapler staple "$DMG"

log "verifying"
codesign --verify --deep --strict --verbose=2 "$APP"
spctl -a -vvv -t exec "$APP" 2>&1 | tee "$work/spctl.txt"
grep -q "Notarized Developer ID" "$work/spctl.txt" || fail "spctl did not report 'Notarized Developer ID'"
xcrun stapler validate "$APP"
xcrun stapler validate "$DMG"
spctl -a -vvv -t open --context context:primary-signature "$DMG"
log "done: $APP and $DMG are signed, notarized and stapled."
log "Gatekeeper test on a quarantined copy and the install are Taisun's steps (see tasks/HANDOFF-FOR-REVIEW.md section 28)."
