# Restore drill (2026-11-13, fourth run)

A backup of the real app data was made into a scratch folder, unpacked into a second scratch folder
and checked. The live app data was only read. `~/Amara Backups` was not used. Both scratch folders
were deleted afterwards.

Run by the ignored test `restore_drill_on_a_copy` (`src-tauri/src/commands/backup_schedule.rs`):
`run_backup` (same path as Back Up Now, secrets excluded) then `unpack_archive_checked`.

| Item | Result |
|---|---|
| Free disk before | 46.7 GB (rule: skip under 15 GiB) |
| Archive | `amara-backup-20261113-143501.tar.gz`, 1,383,035,842 bytes (1.29 GiB), 0600 |
| Archive entries (files + folders) | 3,330 |
| Files restored (manifest excluded) | 3,294 |
| Files listed in the manifest `sha256` | 3,294 |
| Hashes matching the manifest | 3,294 of 3,294 (0 differ, 0 missing, 0 extra) |
| Time: backup / unpack / total | 109.7 s / 31.8 s / 211.8 s (total includes the compare) |
| Secret-looking file names inside the restored tree | 0 (no `crew_service_token`, no `amara-n8n.key`, no n8n key file, no `.env`) |
| Keychain items inside | None. A backup never reads the Keychain; the restore preview lists the 7 names to type in again. |

## Muse folders against the originals (read-only compare)

| Folder | Original files | Byte-identical |
|---|---|---|
| `replays` | 18 | 18 |
| `sessions` | 11 | 11 |
| `muse` | 5 | 5 |

All 34 are byte-identical. **Muse files left out of the backup because a JSON held a secret-looking
field: 0.**

Rules now in force for the Muse folders (`replays/`, `sessions/`, `muse/`), enforced in the packer:
- `.ndjson` replays, `.bands.json` sidecars and every other non-JSON file: always copied byte-for-byte.
- A session/muse `.json` is copied byte-for-byte only if a parse-and-scan with the redaction rules finds
  no secret-looking field anywhere. If it finds one, the file is left out of the backup and listed in the
  manifest `skipped` ("left out of the backup; the original is untouched"). It is never redacted and
  never copied raw. The originals are never touched.
- JSON in those folders that doesn't parse is copied as is (EEG data, not config) and noted in `skipped`.
- A secret-looking file NAME still keeps a file out.

Earlier runs found 16 `.json` files that the redaction rewrite changed (11 session files compacted, 5
`.bands.json` caches with a last-digit float change); that is what this replaces.
Tests: `muse_files_are_byte_identical_unless_a_json_has_a_secret_field_then_left_out_and_listed`.

## Redaction false positives (also fixed)

The first run showed `llm_usage.json` losing `prompt_tokens` / `completion_tokens` and
`amara_take_*.json` losing a `key` field. Now a number under a `*_tokens` name is kept. A bare `key`
is removed by default whatever its value looks like, and kept only in a `take.cited[]` entry with the
full `CitedPoint` shape (`key`, `label`, `display`, `baseline`; `src-console/amaraTake/types.ts`).
Strings under `access_tokens`, `password`, `*_token` and camelCase `*Key` names are still removed.
Tests: `bare_key_is_removed_except_in_a_cited_point_and_camel_keys_are_secrets`,
`redaction_keeps_token_counts_and_identifier_keys_but_not_real_secrets`.

## Open question for Taisun

12 Hemi-Sync audio files of 50 MB or more are left out of every backup by the per-file cap
(`MAX_PACK_FILE_BYTES`, 50 MB). They are listed in the manifest `skipped`. The cap is unchanged. Do
you want them backed up (about 600 MB+ more per archive), or kept out and re-imported from your
originals after a restore?

## How the drill ran, and other notes

- The drill read the real app data folder strictly read-only (nothing in it was written). It only runs
  by hand: the test is `#[ignore]`d and needs `DRILL_DIR` (a new empty folder) and `DRILL_SRC` (the
  data folder to read). It does not run in `cargo test`.
- Nightly timing: if the Mac sleeps through 03:00 JST with the app open, the missed slot runs on wake
  (the 60-second tick sees it). After a failed run it waits 1 hour and gives up on that slot after 3
  failures.
- Optional git bundle (setting `gitBundle`, default off, no upload): a bundle of `~/projects/nexus-amara`
  made with `/usr/bin/git bundle create - --branches --tags`. It holds COMMITTED history of branches
  and tags only: not uncommitted or untracked files, not stashes. A history scan on 2026-11-13 found no
  secret-shaped strings, but that is not a guarantee. Moving a bundle to Google Drive stays a manual
  step (decision 0016).
- `-pre-update` archives are named `amara-backup-YYYYMMDD-HHMMSS-pre-update.tar.gz` (a name this lane
  chose; nothing writes them yet because the updater is not built). `restore_preview` accepts them;
  `backup_list` does not show them yet.
- Restore (the existing Unpack) never writes `notify_relay.json` or `notify_relay_sent.json`, and always
  rewrites `config.json` so a backup cannot turn on Allow Claude.
