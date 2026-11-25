# 0048: Dates & Deadlines, then commit, publish and install (Taisun, 2026-11-25, in session)

His words (verbatim): **"go on"** (continue part 2 of decision 0045), then **"once you are done, commit,git hub it then reinstall"**.

- **Push OK (hard rule 1):** one `git push` of `amara-integration` to origin (github.com/Humanlikeyou/nexus). It adds commits only. `nexus-v2` and `main` are not touched, and later pushes need a new OK.
- **Install:** reinstall Amara OS from the built bundle, with a backup of the current copy first.

## What was built (Dates & Deadlines, from the 0045 brief)

- **Purpose, now stated on the page:** dates with a hard deadline that need lead time (visas, renewals, filings, contracts). Soft targets stay in Goals and small to-dos in Lists.
- **Own lead times per date:** `leadDays` (1 to 730, up to four). Blank means the old 90, 30 and 7 ladder.
  - The smallest lead stays open until the date itself; larger leads open on their day and the two days after.
  - Typing "remind me 3 months before" or "remind me 2 weeks and 3 days before" is now read and shown in the preview. Before, it was ignored.
- **Done and Reopen:** a done date stops reminding, leaves Needs you and Amara's upcoming list, and moves to a Done card. Undo is available.
- **Goal link:** a date can be linked to a goal in the edit form.
- **Amara context:** `nexus_dates` skips done dates and says when a date has its own reminders.
- **Older data is safe:** the new fields default when missing, and an edit that leaves them out keeps what is stored (tests cover both). Nothing is migrated.
