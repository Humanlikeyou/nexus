# Migration test (Phase 16): move Amara OS to a faster Mac

Goal: prove that one backup, one DMG and a passphrase are enough to rebuild Amara OS on a different Mac
or user account. Two parts: an automated part (done, synthetic data) and a fresh-account part that only
Taisun can run (`[GATE: Taisun]`, needs his login, passphrase and keys).

## Part A: automated (done, no live data)

Run: `scripts/migration-probe.sh` (runs the Rust test `commands::packager::migration_probe_tests`).

It builds a fake app-data folder in a temp dir (invented fixtures), backs it up with the app's own backup
code (`backups::run_backup` -> `packager::pack_to`), restores that archive into a second temp dir with
the app's own restore steps (`unpack_archive_checked`, `plan_restore`, `apply_restore`), then checks:

| Probe | What it proves |
|---|---|
| archive modes | archive 0600 in a 0700 folder |
| keys to re-enter | `restore_preview_in` lists the Keychain names to type again (names and labels only, 10 today) |
| manifest hashes | every file in the unpacked pack matches its manifest sha256, and the manifest lists every file |
| file parity | per-folder file counts equal; every file byte-identical (JSON outside the Muse folders compared as parsed value, because the secret-field pass may re-serialise it); no extra files |
| secrets absent | `crew_service_token`, `amara-n8n.key`, `.env` and live-only approval state (`notify_relay.json`) are not restored |
| config.json | secret fields removed, `allow_claude` never turned on, other settings kept |
| SQLite | `PRAGMA integrity_check` is `ok` on the restored `events.db`, row count equal |
| no absolute paths | no restored file contains the source folder or home path |
| source untouched | the source tree is byte-identical after the whole run |

It never reads the real app data, the Keychain, `~/Amara Backups` or a real archive.

Also automated: `test/scripts/noAbsolutePaths.test.ts` fails if `/Users/thevisualdude` (or `/Users/taisun`,
`/home/thevisualdude`) appears in shipped source (`src-tauri/src`, `src-tauri/tauri.conf.json`,
`src-console` non-test files, `amara-router/src`, `scripts`, `stack`).

## Part B: fresh-account run [GATE: Taisun]

Needs a fresh macOS user account (or a spare Mac), the DMG, one real backup archive, and the n8n key-file
passphrase. Nothing here is run by Claude. Tick each box and note date, build and result.

Install and restore
- [ ] Create the fresh macOS user (or use the spare Mac). Copy the DMG and one backup `.tar.gz` over (not through chat).
- [ ] Install the DMG, open Amara OS, choose Restore in the welcome step.
- [ ] Restore preview shows the file groups and the "type these in again" list (names only). Restore saves today's data first.
- [ ] Restore finishes; app data is 0700 and files 0600.
- [ ] Unlock the n8n key file with its passphrase. n8n workflows decrypt.
- [ ] Re-enter each key listed by restore (Telegram token and chat, n8n encryption key, Home Assistant token, Freqtrade login, Oura/Hevy/coach keys, Letta password, Weaviate key, n8n API key, crew token). Each shows "set", never a value.
- [ ] Re-pair the WHOOP 4 (single-writer lock, one central).
- [ ] Pick the model tier by RAM; models download into Application Support (no Ollama.app needed).

Check on the new account
- [ ] Every health light is green, no terminal needed.
- [ ] Chat works on the light tier and on the deep tier.
- [ ] Crews: roster, a read-only run, approvals dialog.
- [ ] n8n: workflow count and names match the old Mac.
- [ ] Letta: agent list matches.
- [ ] `kb.query` returns hits.
- [ ] Muse replay of a known file opens and plays (read-only; the file is unchanged).
- [ ] Health store snapshot matches the old Mac (sleep, HRV, recovery, strain for a known day).
- [ ] Takes history is present.
- [ ] Life and Enterprise data are present.
- [ ] Settings match (Cloud AI is still OFF; outbound default off; the Startup toggle state is not carried over).
- [ ] No path points at the old Mac (Settings > Logs show nothing under the old home folder).

Record the result in `tasks/HANDOFF-FOR-REVIEW.md` and tick the Migration box in
`docs/release/READINESS-0.1.0.md`.

## What is not carried over (by design)

Keychain items, the n8n key file, outbound and supervisor opt-ins, Home windows profiles, the Telegram
relay approvals and its sent ledger, and files of 50 MB or more (Hemi-Sync audio; open question in
`restore-drill.md`).
