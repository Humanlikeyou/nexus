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

## Not done yet

- **Step 3:** the one-way `context_items` goal refresh, and freezing the `events.db` `goal/current` writer.
- **Step 4:** domain owners calling `activity::record`, and takes writing recommendation and outcome rows. Until then the store API has no production caller (`#[allow(dead_code)]` on the module, with a comment).
- **Step 5:** the backfill, which waits for Taisun's answer.
