# Design questions for the Nexus Design Lead

Claude Code does not act on these. Each says where it came from.

- **Follow-ups (Lists) approval copy** (from B08, LANE-T): every write to `~/.hermes/amara/*.json` now shows a native Allow/Deny dialog. A Deny returns an error starting `declined:` and a concurrent dialog `busy:`. `ListsScreen` only maps the existing `ERR_*` kinds, so these show raw. What copy (if any) should Lists show for "you declined" and "another approval is open"? Claude Code has not changed ListsScreen.
- **Money tab as-of date** (from W12b ledger): `ledgerAsOf(ledger)` in `src-console/business/liveLedger.ts` returns the rate's as-of date (or null). Where/how should the Money tab show it?

## W12c Chunk C: recordings loading and gaps (frontend-executor, 2026-11-11)
- Detail view: a 30 s block loads asynchronously, so the first paint of a window (and after a scrub into a new block) reads as "Needs wiring" for a moment before the trace appears. Wanted: a loading state for the lanes, or keep as is?
- Gaps in a recording (dropouts, > 0.5 s) arrive as NaN in lane values and in the focus/calm/band series. `linePath`/stack polygons in RecordingsPanels.tsx and the lane drawing do not break lines at NaN, and "avg focus" over a series with a gap shows NaN. Wanted: how gaps should draw (break the line) and be averaged (ignore). No code changed.

## R13-D crew approvals (frontend-executor, 2026-11-11)
- A run waiting for an approval has status `awaiting_approval` ("Awaiting approval", wait tone). The run page reuses the running copy ("Running… checking every 5 seconds.") and keeps the Stop button for it; a denied run ends `cancelled` (off tone). Wanted: dedicated copy for a run that is waiting for you, and a link from the run page to its approval?
- Approve now opens the native "Approve Action" dialog; a Deny or an error shows as the card's existing status note. Wanted: how an error or a declined dialog should look.
- **Take chips → source** (item E): chips now jump to an existing element id. Neuro (sessions), Enterprise (revenue / ledger rows) and Fitness recovery/effort have no stable element ids yet, so those chips stay inert. Add ids where you want a target (e.g. `neuro-last-session`, `ent-revenue`, `fitness-recovery`) and Claude Code will map them.

## design-1111v live wiring (backend, 2026-11-12)
- Enterprise Tasks and Content show a just-saved item with the subtitle "Just added · sample data" even when it was saved for real (live). Wanted: copy for a live "Just added" row (and removing the sample label there). Not changed (copy).
- Tasks and Content still say "venture link needs wiring" although tasks and ideas now carry a venture id (`venture` on `read_tasks` / `read_content_pipeline` rows). Wanted: how the venture should show on a task or card.
- Dates & deadlines "Edit" stays disabled ("Needs wiring · life_date_upsert"): `life_date_upsert` accepts an id (edit) now, but the Edit flow is a Design call.

## 1111a panels label any value "◇ Sample data" (blocks live wiring) — 2026-11-12
`TodaySections.tsx` (Goals hero + Milestones card :29 :51; Weekly review :164 :179 :190 :193 :198), `PageViews.tsx` (Patterns :33 :41 :46 :56; Personal money :84 :85 :103 :107 :112; Memory :163) render `<DataTag kind="sample" />` whenever the adapter returns a value, so a live value would be labelled sample. `life/DatesView.tsx` and `standalone/BackupView.tsx` already do it right (`{x.state === "sample" && <DataTag kind="sample" />}`). Ask: make these tags state-conditional the same way (no visual change in sample mode). Until then Claude keeps goals / patterns / weekly review / money / memory unwired; Milestones already show the real goals.json through the unwired fallback card.
Also for Goals: habits added through the one add path have no `source`, so their days can't be counted from Fitness/Neuro/check-ins. Either the add preview asks for a source, or the panel needs a manual tick (handoff §1 allows it only for `source = manual`).

## 1111s Backup live (backend 2026-11-12)
- `backup_settings_*`, `backup_now`, `backup_list` are live. No nightly timer and no keep-14 prune exist (prune needs Taisun's OK, hard rule 7), so the "Every night" toggle and "keeps the last 14" are stored preferences only; the hero keeps its Needs wiring tag because `last` is null. Wanted: copy that says nightly/keep aren't running yet, or keep as is until the scheduler lands.
- `backups[].version` is "" (the archive name has no app version; the dialog shows "—"); `last` needs a stored checksum verify (not built); `BackupRun` done/total/left are "" (progress events carry phase + pct only).
- Files ≥ 50 MB (large Muse replays) are skipped and listed in the result, so a backup doesn't hold big raw EEG captures. Taisun's call whether backups should include them.
