#!/usr/bin/env bash
# Phase 16 migration probe (automated part). SYNTHETIC data only, in temp dirs.
# Runs the Rust test `migration_probe_backup_restore_parity_on_synthetic_data`, which:
#   1. builds a fake app-data dir in a temp folder (fixtures, invented),
#   2. backs it up with the app's own backup code (backups::run_backup -> packager::pack_to),
#   3. restores that archive into a SECOND temp dir with the app's own restore steps,
#   4. checks file counts, checksums, SQLite integrity_check, the keys-to-re-enter list (names only),
#      secret files absent, no absolute source path in restored files, source untouched.
# It never reads the real app data, the Keychain, ~/Library/Application Support/com.taisun.nexus,
# ~/Amara Backups or any real archive, and makes no network call.
# Usage: scripts/migration-probe.sh        Exit 0 only if the test passes and every PROBE line is ok.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
export NEXUS_OUTBOUND_DISABLED=1
log="$(mktemp -t migration-probe.XXXXXX)"
trap 'rm -f "$log"' EXIT
if ! cargo test --manifest-path src-tauri/Cargo.toml --lib \
    commands::packager::migration_probe_tests -- --nocapture >"$log" 2>&1; then
  grep -E 'PROBE|panicked|assertion|error' "$log" || tail -20 "$log"
  echo "FAIL migration probe"; exit 1
fi
grep -E '^PROBE ' "$log"
n=$(grep -c '^PROBE ' "$log")
bad=$(grep -E '^PROBE ' "$log" | { grep -vE ' ok$' || true; } | wc -l | tr -d ' ')
if [ "$n" -lt 9 ] || [ "$bad" != "0" ]; then echo "FAIL migration probe ($n probe lines, $bad not ok)"; exit 1; fi
echo "PASS migration probe ($n probe lines, synthetic data, temp dirs)"
