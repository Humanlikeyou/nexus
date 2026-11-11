# Design questions for the Nexus Design Lead

Claude Code does not act on these. Each says where it came from.

- **Follow-ups (Lists) approval copy** (from B08, LANE-T): every write to `~/.hermes/amara/*.json` now shows a native Allow/Deny dialog. A Deny returns an error starting `declined:` and a concurrent dialog `busy:`. `ListsScreen` only maps the existing `ERR_*` kinds, so these show raw. What copy (if any) should Lists show for "you declined" and "another approval is open"? Claude Code has not changed ListsScreen.
- **Money tab as-of date** (from W12b ledger): `ledgerAsOf(ledger)` in `src-console/business/liveLedger.ts` returns the rate's as-of date (or null). Where/how should the Money tab show it?

## W12c Chunk C: recordings loading and gaps (frontend-executor, 2026-11-11)
- Detail view: a 30 s block loads asynchronously, so the first paint of a window (and after a scrub into a new block) reads as "Needs wiring" for a moment before the trace appears. Wanted: a loading state for the lanes, or keep as is?
- Gaps in a recording (dropouts, > 0.5 s) arrive as NaN in lane values and in the focus/calm/band series. `linePath`/stack polygons in RecordingsPanels.tsx and the lane drawing do not break lines at NaN, and "avg focus" over a series with a gap shows NaN. Wanted: how gaps should draw (break the line) and be averaged (ignore). No code changed.
