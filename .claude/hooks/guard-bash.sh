#!/usr/bin/env bash
# PreToolUse(Bash) guard for the Amara OS worktree.
# Blocks (exit 2, reason on stderr) the actions Taisun has frozen or must approve:
#   - git push (any form), git merge while on / into nexus-v2, gh pr create/merge
#   - force / destructive git: --force, -f on push, reset --hard, clean -f, branch -D,
#     stash drop/clear, checkout -- . / restore . (discard work)
#   - git add -A / --all / . (the orchestrator commits by explicit path only)
#   - rm -rf, and any write/move/delete aimed at the Muse replays/sessions/muse folders
#   - launchctl load/bootstrap/unload, and edits under ~/.hermes
# Runs `npm run check:secrets` before any `git commit` and blocks the commit if it fails.
# Approved exceptions are not encoded here: Taisun's OK is logged in docs/decisions/ and the
# human runs the command himself (`! <command>`).
set -u
input="$(cat)"
cmd="$(printf '%s' "$input" | /usr/bin/python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("tool_input",{}).get("command",""))
except Exception: print("")')"
[ -z "$cmd" ] && exit 0

block() { echo "BLOCKED by .claude/hooks/guard-bash.sh: $1" >&2; echo "See .claude/rules/hard-rules.md. Ask Taisun; log his OK in docs/decisions/." >&2; exit 2; }

# Normalise for matching: strip quotes, backslashes, backticks and $( ) so wrappers such as
# bash -c '...', "$(...)", /usr/bin/git or g\it can't hide a command; split on ; & | ( ) too.
c="$(printf '%s' "$cmd" | tr '\n\t' '  ' | tr -d "\"'\\\\\`" | sed -E 's/\$\(/ ( /g; s/[;&|()]/ ; /g; s#(^| )[^ ]*/(git|rm|gh|launchctl)( |$)#\1\2\3#g' | tr -s ' ')"
# Fail-safe token checks (over-blocking is acceptable; ask Taisun to run it himself).
has() { printf ' %s ' "$c" | grep -Eqi "$1"; }
if has ' git ' && has ' push( |$)'; then block "git push is frozen until Taisun approves that specific push."; fi
if has ' git ' && has ' merge( |$)' && has 'nexus-v2|projects/nexus( |/|$)'; then block "merging into nexus-v2 is frozen."; fi
if has ' gh ' && has ' pr ' && has ' (create|merge)( |$)'; then block "PRs need Taisun's explicit OK."; fi
if has ' rm ' && has ' -[a-z]*r[a-z]* | --recursive '; then block "recursive rm needs Taisun's OK."; fi
if has 'com\.taisun\.nexus/(replays|sessions|muse)' && ! has '^ *(ls|stat|wc|head|tail|cat|file|du|find [^;]*-(name|type|newer|mtime)[^;]*$|python3 -I -c)'; then
  has ' (rm|mv|truncate|shred|unlink|cp|rsync|tee|ln|touch|chmod|chflags|sed -i|perl -i|-delete|-exec)( |$)|>' && block "Muse recordings are sacred: no writes/moves/deletes under replays/, sessions/ or muse/."
fi

# --- git push / PRs (frozen) ---
printf '%s' "$c" | grep -Eq '(^|[;&|( ]|\$\()git( +-[^ ]+( +[^- ][^ ]*)?)* +push( |$)' && block "git push is frozen until Taisun approves that specific push."
printf '%s' "$c" | grep -Eq '(^|[;&|( ])gh +pr +(create|merge)' && block "PRs need Taisun's explicit OK."

# --- merges into nexus-v2 (frozen) ---
if printf '%s' "$c" | grep -Eq '(^|[;&|( ])git( +-[^ ]+( +[^- ][^ ]*)?)* +merge( |$)'; then
  printf '%s' "$c" | grep -Eq 'nexus-v2|projects/nexus( |/|$|;)' && block "merging into nexus-v2 is frozen."
  br="$(git -C "${CLAUDE_PROJECT_DIR:-.}" branch --show-current 2>/dev/null)"
  [ "$br" = "nexus-v2" ] && block "merge while on nexus-v2 is frozen."
fi
printf '%s' "$c" | grep -Eq 'git +-C +[^ ]*projects/nexus/? +(merge|push|rebase|reset|commit|checkout)' && block "the main repo ~/projects/nexus is read-only for agents."

# --- force / destructive git ---
printf '%s' "$c" | grep -Eq 'git .*(--force|--force-with-lease)( |$|=)' && block "force flags need Taisun's OK."
printf '%s' "$c" | grep -Eq 'git +reset +.*--hard' && block "reset --hard needs Taisun's OK."
printf '%s' "$c" | grep -Eq 'git +clean +.*-[a-zA-Z]*f' && block "git clean -f needs Taisun's OK."
printf '%s' "$c" | grep -Eq 'git +branch +.*-D( |$)' && block "branch -D needs Taisun's OK."
printf '%s' "$c" | grep -Eq 'git +stash +(drop|clear|pop)( |$)' && block "stash drop/clear/pop is not allowed (shared stash stack)."
printf '%s' "$c" | grep -Eq 'git +(checkout|restore) +(.* )?(-- +)?\.( |$)' && block "discarding the working tree needs Taisun's OK (7 uncommitted doc edits are his)."
printf '%s' "$c" | grep -Eq 'git +(checkout|restore) +([^;&|]* )?(amara-router/)?(BUILD|CLAUDE|NEXT_SESSION_PROMPT|README|STAGES|TAISUN-STEPS)\.md' && block "those uncommitted doc edits are Taisun's to keep or discard."
printf '%s' "$c" | grep -Eq 'git +add +(.* )?(-A|--all|\.)( |$)' && block "commit by explicit file path only (no git add -A / .)."

# --- filesystem ---
printf '%s' "$c" | grep -Eq '(^|[;&|( ])rm +(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r|-r +-f|-f +-r)' && block "rm -rf needs Taisun's OK."
if printf '%s' "$c" | grep -Eq 'com\.taisun\.nexus/(replays|sessions|muse)'; then
  printf '%s' "$c" | grep -Eq '(^|[;&|( ])(rm|mv|truncate|shred|unlink|find .*-delete|sed +-i|perl +-i|tee|cp|rsync)( |$)|>[^&]' && block "Muse recordings are sacred: no writes/moves/deletes under replays/, sessions/ or muse/."
fi
printf '%s' "$c" | grep -Eq 'launchctl +(load|unload|bootstrap|bootout|enable|kickstart)' && block "don't load/unload LaunchAgents; write the step into TAISUN-STEPS.md."
printf '%s' "$c" | grep -Eq '(\.hermes/)[^ ]*' && printf '%s' "$c" | grep -Eq '(^|[;&|( ])(rm|mv|cp|tee|sed +-i|perl +-i|chmod|git +(apply|checkout|commit|stash))( |$)|>[^&] *[^ ]*\.hermes' && block "~/.hermes is read-only; write patches into TAISUN-STEPS.md / ~/projects/hermes-patches."

# --- secrets gate before commit ---
if printf '%s' "$c" | grep -Eq '(^|[;&|( ])git( +-[^ ]+( +[^- ][^ ]*)?)* +commit( |$)'; then
  root="${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null)}"
  if ! (cd "$root" && npm run --silent check:secrets >/dev/null 2>&1); then
    block "npm run check:secrets failed; fix findings before committing (run it to see file:line)."
  fi
fi
exit 0
