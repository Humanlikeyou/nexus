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
