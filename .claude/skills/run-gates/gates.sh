#!/usr/bin/env bash
# Runs every pre-commit gate and prints one summary line per gate. Exit 1 if any fail.
# Usage: .claude/skills/run-gates/gates.sh [--quick]   (--quick skips the Rust crates not touched in the diff)
set -u
cd "$(git rev-parse --show-toplevel)" || exit 1
fail=0; out="$(mktemp -d)"
run() { local name="$1"; shift; if "$@" >"$out/$name.log" 2>&1; then echo "PASS  $name  $(eval "$SUMMARY")"; else echo "FAIL  $name  (log: $out/$name.log)"; tail -15 "$out/$name.log"; fail=1; fi; }
SUMMARY='true'; run tsc npx tsc --noEmit
run lint npm run --silent lint
SUMMARY='grep -E "^ +Tests " "$out/vitest.log" | tr -s " "'; run vitest npm run --silent test:unit
SUMMARY='grep -E "^ℹ (pass|fail) " "$out/neuro.log" | tr "\n" " "'; run neuro npm run --silent test:neuro
SUMMARY='tail -1 "$out/secrets.log"'; run secrets npm run --silent check:secrets
changed="$(git diff --name-only HEAD; git diff --name-only --cached)"
for c in src-tauri amara-router; do
  if [ "${1:-}" = "--quick" ] && ! printf '%s\n' "$changed" | grep -q "^$c/"; then echo "SKIP  $c (untouched, --quick)"; continue; fi
  SUMMARY='true'; run "$c-fmt" cargo fmt --manifest-path $c/Cargo.toml --check
  run "$c-clippy" cargo clippy --manifest-path $c/Cargo.toml --all-targets -- -D warnings
  SUMMARY='awk "/^test result/{p+=\$4;f+=\$6;i+=\$8} END{print p\" passed, \"f\" failed, \"i\" ignored\"}" "$out/'$c'-test.log"'; run "$c-test" cargo test --manifest-path $c/Cargo.toml
done
echo "logs: $out"; exit $fail
