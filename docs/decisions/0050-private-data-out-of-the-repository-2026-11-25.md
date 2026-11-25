# 0050: Private data out of the repository (Taisun, 2026-11-25, in session)

His words: **"Implement the approved privacy and local-storage remediation for Nexus."** The prompt also says: no automatic history rewrite or force-push, report history separately, never print secret values, and no new memory features.

## What changed

- **Private documents move to the app-data folder.** `config/terrance.profile.md` and `config/terrance.identity.md` are private (his name, profile, personal references). They now live in `<app data>/config/`, which is `~/Library/Application Support/com.taisun.nexus/config/`. They are untracked with `git rm --cached` and ignored. The files stay on disk in the checkout, and a copy is kept in `~/Backups/nexus-private-config-20261125/` (0600).
- **Generic defaults ship instead.** The repository has `config/defaults/identity.default.md` and `profile.default.md`, with no personal information. They are embedded at build time, so packaged and development builds load the same way: the private copy in app data, else the default. Nothing is read from beside the binary or from `~/projects/nexus` any more.
- **Session logs** (`store_session_log`, `list_session_logs`, insights, archive) always use `<app data>/session_logs` and `<app data>/archive`. Development builds used to write them into `<repo>/memory/session_logs`.
- **One-time migration** (`private_store.rs`, run at startup after the existing data-dir migration). It COPIES the private documents and any `memory/session_logs` and `memory/archive` logs from the old locations into app data:
  - It never overwrites, never follows a symlink, and never deletes or changes a source.
  - It writes 0600 files in 0700 folders and caps file sizes.
  - It is idempotent: a marker file holds counts only, and even without the marker nothing is overwritten.
  - Old locations: the checkout above a development binary, `~/projects/nexus` and `~/projects/nexus-amara`.
- **`.gitignore`** now covers private documents, `memory/`, session logs, runtime context and chat stores, SQLite side files, and local secrets and signing files.

## Order that matters

- **Launch the new build once before merging this into nexus-v2.** Merging removes the tracked files from the `~/projects/nexus` checkout. The first launch copies them into app data first.
- The `~/Backups/nexus-private-config-20261125/` copy covers this in any case.

## Not changed

- Continuity behaviour, command names and the context store (`terrance_context.json` was already in app data).
- No new memory features.
- No history rewrite and no push. See `docs/security/history-cleanup.md`.
