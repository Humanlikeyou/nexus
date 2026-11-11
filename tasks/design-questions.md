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
