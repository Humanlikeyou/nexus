# 0053: Shared data architecture (Taisun, 2026-11-25, in session)

The design is in `docs/specs/data-architecture-2026-11-25.md` (`495eb14`). His reply to §8: **"approved 2.yes 4 confirmed"**.

| §8 question | Answer |
|---|---|
| 1. Two files and four tables | **Approved** |
| 2. Start with the `goals.json` safety fix | **Yes** |
| 3. Optional backfill of activity from existing records | **Not answered, so not done.** Ask before running it |
| 4. Permission model (neural never on the cloud tier; business never mixed with personal) | **Confirmed** |

## Implemented in this commit

**Step 2: `goals.json` is safe** (`goals.rs`)
- `save_goals_at` takes the History store lock (`adds::store_lock`), the same one every History/Undo goal write already holds, so a console save can't interleave with them.
- An existing file that can't be read or parsed is **never overwritten**. The save is refused, and a copy is kept as `goals.json.unreadable-<hash>`, once per distinct content.
- The write is atomic (temp + rename, 0600) and read back before it counts as saved.
- The format is unchanged.
- **Tests:** a corrupt file is never replaced (this test fails on the previous code and passes now); a save is atomic, private and read back; concurrent saves never tear the file; a missing file starts empty.
- `goal_venture`'s test pinned the old behaviour (an unreadable file was overwritten with link-free goals). It now asserts the save is refused and the file left as it was. Its purpose, that a caller can never set a venture link, is unchanged.

**Step 1: the stores** (`amara_store.rs`)
- `<data>/amara.db` (scopes personal, neural, ops) and `<data>/business/amara_business.db` (scope business). Both are created at startup in a background thread; creation is idempotent and touches no existing file.
- The first creation is logged in `<data>/migrations/log.jsonl`.
- Tables: `activity`, `recommendation`, `recommendation_evidence`, `recommendation_outcome` and `schema_meta` (v1). A newer schema is refused.
- **Enforced in the database:**
  - a scope CHECK per file;
  - append-only triggers on `activity` and outcomes;
  - confidence needs a basis;
  - evidence role and quality vocabularies;
  - cascade from a recommendation to its evidence and outcomes.
- **Enforced in Rust:**
  - the record-reference parser, with its store-to-scope map;
  - the evidence scope rules: business only with business, and personal with neural;
  - an observation is required, except for takes;
  - telemetry evidence needs a time window;
  - the outcome status machine (compare-and-swap);
  - forget, which expires recommendations left without an observation.
- **Backups:** both files are already inside the backup's full data-folder copy (`nexus-config`), so no backup change was needed.

## Steps 3 and 4 (Taisun: "Go on", same day)

**Step 3: goal copies are one-way**
- `migrate_legacy_terrence_context`, which runs on every snapshot load, used to insert missing goals only, so the copies went stale. `refresh_goal_copies` now inserts missing goals and refreshes changed ones (title, progress, deadline, metadata).
- A copy whose goal was deleted from `goals.json` is archived. A copy the user deleted stays deleted. Nothing writes `goals.json`, and a test pins that.
- `events.db`: `migrate_existing_data` no longer copies goals. `insert_events` refuses domain sources (`goal`, `habit`, `project`, `task`, `checkin`, `money`, `date`) before opening the database. The existing goal rows stay as frozen history.

**Step 4: owners record activity after their own successful write** (best effort: a failure is logged and never fails the user's change)

| Owner | Activity |
|---|---|
| `goals.rs` `save_goals_at` | `goal.created`, `goal.progressed`, `goal.completed` (habits are skipped; they have day marks) |
| `habits.rs` `mark_in` | `habit.completed` / `habit.missed`, linked to its History entry |
| `checkin.rs` `save_in` | `checkin.logged` (no text stored) |
| `life_dates.rs` `upsert_in` | `date.done` when a date becomes done |
| `money_entries.rs` `entry_add_in` | `money.logged` (no amounts stored) |
| `sessions.rs` `save_session_in` | `neuro.session_finished`, neural scope |

- **Takes:**
  - a new take becomes a `take` recommendation (`take:<id>`), and `take_log.jsonl` stays as the takes UI log;
  - `take_shown` becomes a `shown` outcome;
  - `take_rating` becomes `evaluated` (helpful = `helped`, not helpful = `no_effect`);
  - business-page takes go to the business file, neuro-page takes to the neural scope;
  - forgetting a take removes its recommendation and its outcomes.
- **Deleting a goal** removes the activity and evidence that point at it.
- **Hooks only record when the folder has its production name** (`takes`, `checkins`, `life`, `sessions`), so scratch folders in tests record nothing.

**Tests:** goal refresh and archive, the `events.db` refusal and its source pin, each owner's hook against its real write function in a temp folder, business takes isolated, take forget, and "a failing store never fails the owner's write". The full Rust suite passed twice in a row.

## Not done

- **Step 5:** the backfill, which waits for Taisun's answer.
- **Not covered yet:** workout activity from NOOP, and business actions (Enterprise, Phase 11).
- Any UI that shows activity or recommendations (Design work).
- Nothing has run on Taisun's Mac until a build with these commits is installed.

## Foundations phase (Taisun's prompt "Implement the approved shared data foundations", same day)

**What already existed and was reused, not rebuilt:**
- Activity, recommendation, evidence and outcome tables (steps 1–4).
- Consequential-action approvals: `execution_approvals` in `execution.db` (0052), bound to the parameter fingerprint, expiring and single-use.
- Audit history: the append-only `execution_transitions` (0052), History/Undo (`adds`), and the append-only outcomes.

**Schema change v1 → v2, and why.** The prompt requires a recommendation to store what was observed, why it matters, the suggested action, the uncertainty, and impact and effort estimates. It also requires activity to say whether it is confirmed or estimated. The approved v1 tables had no place for these, so v2 adds nullable columns:
- `recommendation`: `observation`, `rationale`, `suggested_action`, `uncertainty`, `impact`, `effort` (`impact` and `effort` are CHECKed to low, medium or high).
- `activity`: `certainty` (`confirmed` or `estimated`, default `confirmed`).

The change is additive only: no new table, and nothing renamed or removed.

**Provenance needs no column.** It is derived from the existing `source`: user = manual; scheduler or noop = automated; model = inferred; import or backfill = imported.

**Migration** (`migrate_to_v2`):
- A file that already holds rows is first copied with `VACUUM INTO <file>.v1-<timestamp>.bak` (0600).
- The ALTERs and the version bump then run in one `BEGIN IMMEDIATE` transaction that re-checks the version under the lock, so parallel first opens migrate once.
- On any error it rolls back and the file stays v1.
- **Rollback:** `restore_backup(db, backup)`, run with the app closed, puts the backup in place and keeps the current file as `<file>.replaced-<timestamp>`. Nothing is deleted.

**Evidence can't be invented** (`amara_store/resolve.rs`):
- Every evidence reference is looked up in its owning store: goals, habit days, check-ins, dates, money, takes, Muse sessions, recordings (file metadata only, never samples), ledger executions and business ledger entries.
- A missing record is refused.
- A model-written recommendation (every one created over IPC; `produced_by = model:<purpose>`) may cite only records that were found. A store that can't be checked from here (NOOP, derived rows) is refused for models and allowed only for rules or the user.
- The stored `snapshot_sha256` is the resolver's hash of the record as read; callers can't supply it.
- Recommendations hold references and hashes only, never copies of goals, workouts, telemetry or business records.

**Repositories and commands** (`amara_store/commands.rs`, registered and bound):
- `activity_list` (with provenance and certainty);
- `recommendation_list`, `recommendation_get` (with its evidence and outcomes), `recommendation_create`;
- `recommendation_respond` (shown, accepted, modified with what changed, dismissed);
- `recommendation_record_outcome` (completed, evaluated, expired);
- `approvals_list` (read-only from the ledger);
- `audit_trail(record_ref)`: activity, recommendations that target or cite the record, and the executions they produced.

Each command takes a scope (personal, neural or business). Business commands open only the business file.

**Fixes found by the new tests:**
- Parallel first opens of a new store could each try the v2 migration ("duplicate column"). The fix is the locked re-check above.
- The shared SQLite opener set WAL mode before its busy timeout, and the WAL switch can return "locked" without waiting. It now sets the timeout first and retries that one step briefly, for lock errors only. This affects every store's first open, and only by waiting instead of failing.

**Tests:**
- duplicate activity inserts, including 8 threads at once;
- invented, missing or unverifiable evidence refused, with the hash from the resolver;
- record linking through the audit trail;
- a failed migration changes nothing, then the backup restores v1 with its data;
- business and personal never crossing through the commands;
- the full lifecycle with every field.

The store suite passed 10 runs in a row. All gates pass.

**Not built:** dashboards (per the prompt), and recommendation producers (no rules or model producer writes recommendations yet besides takes).
