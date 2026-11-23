# Hard rules (Taisun, 2026-11-10). Never break. These override any other doc in this repo.

1. **No `git push`, no merge into `nexus-v2`, no PR** without Taisun's explicit OK for that exact action, logged in `docs/decisions/`. A prior session made 23 merges and 8 pushes with no approval; merges and pushes are **frozen** until he answers. Commit locally on `amara-integration` only. (`README.md`/`CLAUDE.md` lines saying "then `git merge --no-ff` into `nexus-v2`" are superseded.)
2. **No secrets** in chat, commits, logs or docs. `npm run check:secrets` before every commit (the Bash hook enforces it).
3. Run app and router with `NEXUS_OUTBOUND_DISABLED=1`. Add **no new outbound hosts**.
4. Don't change: bundle id `com.taisun.nexus`, productName "Amara OS", crate names (`nexus`, `amara-router`), nav labels or keyboard shortcuts. "Agents" (was "Chat & Agents") is Taisun's rename: keep it.
5. Don't touch `memory/` or Letta files. Don't edit `~/.hermes`; don't install/load LaunchAgents. Write patches + steps into `TAISUN-STEPS.md` (patches under `~/projects/hermes-patches`).
6. Never drive the UI while Taisun may be using it; never type into his composer.
7. Key rotation and backup prune come **last**, only with his OK.
8. No destructive commands (`rm -rf`, `reset --hard`, force-push, stash drops, discarding his uncommitted docs) without his OK.
9. **Muse recordings are sacred** — see `muse-recordings.md`.
10. **Design is out of scope.** See `design-boundary.md`.
11. **Review gate:** when the backlog + in-scope wiring are done, or you're blocked, write `tasks/HANDOFF-FOR-REVIEW.md` and STOP.
12. Never fabricate results. Anything not run is reported as "not run".
