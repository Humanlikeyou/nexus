# Restore drill (2026-11-13, second run)

A backup of the real app data was made into a scratch folder, unpacked into a second scratch folder
and checked. The live app data was only read. `~/Amara Backups` was not used. Both scratch folders
were deleted afterwards.

Run by the ignored test `restore_drill_on_a_copy` (`src-tauri/src/commands/backup_schedule.rs`):
`run_backup` (same path as Back Up Now, secrets excluded) then `unpack_archive_checked`.

| Item | Result |
|---|---|
| Free disk before | 48.9 GB (rule: skip under 15 GiB) |
| Archive | `amara-backup-20261113-124251.tar.gz`, 1,383,030,035 bytes (1.29 GiB), 0600 |
| Archive entries (files + folders) | 3,321 |
| Files restored (manifest excluded) | 3,287 |
| Files listed in the manifest `sha256` | 3,287 |
| Hashes matching the manifest | 3,287 of 3,287 (0 differ, 0 missing, 0 extra) |
| Time: backup / unpack / total | 139.7 s / 35.6 s / 256.2 s (total includes the compare) |
| Secret-looking file names inside the restored tree | 0 (no `crew_service_token`, no `amara-n8n.key`, no n8n key file, no `.env`) |
| Keychain items inside | None. A backup never reads the Keychain; the restore preview lists the 7 names to type in again. |

## Muse folders against the originals (read-only compare)

| Folder | Original files | Byte-identical |
|---|---|---|
| `replays` | 18 | 18 |
| `sessions` | 11 | 11 |
| `muse` | 5 | 5 |

All 34 are byte-identical. The first run of this drill (earlier the same day) found 16 `.json` files
that the backup's secret-field redaction had rewritten (11 session files compacted, 5 `.bands.json`
caches with a last-digit float change). Fixed: `replays/`, `sessions/` and `muse/` are now copied
byte-for-byte with no redaction pass. A file whose NAME looks secret still stays out. Test:
`muse_folder_json_is_packed_byte_identical_and_hashes_match`.

## Redaction false positives (also fixed)

The first run showed `llm_usage.json` losing `prompt_tokens` / `completion_tokens` and
`amara_take_*.json` losing a `key` field. Now a number under a `*_tokens` name is kept, and a bare
`key` is removed only when its parent is itself secret-looking (`worker_api_keys[].key` goes,
`cited[].key` stays). A string under `access_tokens`, `password`, `*_token` etc. is still removed.
Test: `redaction_keeps_token_counts_and_identifier_keys_but_not_real_secrets`.

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
