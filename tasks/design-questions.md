# Design questions for the Nexus Design Lead

Claude Code does not act on these. Each says where it came from.

- **Follow-ups (Lists) approval copy** (from B08, LANE-T): every write to `~/.hermes/amara/*.json` now shows a native Allow/Deny dialog. A Deny returns an error starting `declined:` and a concurrent dialog `busy:`. `ListsScreen` only maps the existing `ERR_*` kinds, so these show raw. What copy (if any) should Lists show for "you declined" and "another approval is open"? Claude Code has not changed ListsScreen.
- **Money tab as-of date** (from W12b ledger): `ledgerAsOf(ledger)` in `src-console/business/liveLedger.ts` returns the rate's as-of date (or null). Where/how should the Money tab show it?
