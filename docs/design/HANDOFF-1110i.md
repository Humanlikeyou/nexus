# HANDOFF-1110i: per-crew Files and terminal switch — data and API needs (for Claude, after Taisun approves the images)

Design builds the UI. Claude builds enforcement in crew-service and the Tauri commands. Nothing here may relax the existing rules: loopback only, outbound off by default, allowlists fail closed, no secrets in step records, Taisun is the only approver.

## Backend (crew-service, `stack/crew-service/main.py`)
1. **Per-crew grant record** (Postgres), e.g. `crew_tool_grants`:
   `crew_id, enabled bool, tools text[] (fs_read, fs_write, shell), scope_path text, granted_by text ('taisun'), granted_at, expires_at nullable, revoked_at nullable, revoked_by, reason text, request_id nullable`.
   Default: no row = off = read-only (`kb_query`, `memory_query`).
2. **Enforcement:** the effective allowlist for a step = role allowlist ∩ (`CREW_ALLOWED_TOOLS` ∪ active grant tools). A grant is active only when `enabled && now < expires_at && revoked_at is null`. Unknown or expired → dropped, fail closed.
3. **Scope:** `fs_*` and `shell` are jailed to `scope_path` (realpath check, no symlink escape). Shell: cwd = scope, 60 s timeout, output cap, secret redaction, env scrubbed. Always blocked: network/outbound, `git push`, sudo, spending. Delete/overwrite outside scope → crew approval row (existing Phase D path).
4. **Git checkpoint** before each `fs_write` batch (design doc rule 5); store the short hash on the step.
5. **Expiry and stop:** a timer revokes at `expires_at`; revoke kills the running shell child (SIGTERM, then SIGKILL after grace — never leave it running) and records it.
6. **Requests:** a role may emit `request_tool_grant {crew_id, tools, scope_path, duration, reason}` → an action-ledger row with `source: "crew"`, `kind: "tool_grant"`, `risk: "high"`, `expires_at = now + 1h`. Approve creates the grant; deny/expire leaves it off.
7. **Audit:** `crew_tool_audit` (or reuse `crew_steps` with `kind` = `grant` | `revoke` | `fs_write` | `shell` | `blocked`): `at, actor ('taisun' | 'crew-service' | role), crew_id, run_id, action, target (path or command, redacted), exit_code, duration_ms, checkpoint`. No command output, no prompt text, no secrets.

## Tauri commands / API (names are proposals)
| Command | Returns / does |
|---|---|
| `crew_tool_grant_get(crew_id)` | `{ enabled, tools, scope_path, granted_at, expires_at, granted_by, enforced_allowlist: string[], pending_request?: {...} }` — `enforced_allowlist` comes from crew-service, not from the app |
| `crew_tool_grant_set(crew_id, { tools, scope_path, duration: "run" \| "1h" \| "until_off" })` | Creates the grant; resolves only after crew-service confirms the enforced allowlist (UI shows "Applying…"; 15 s timeout → error, stays off) |
| `crew_tool_grant_revoke(crew_id)` | Revokes now, stops a running command |
| `crew_tool_audit(crew_id, { limit, before })` | Audit rows above, newest first |
| Existing `usePendingApprovals({ source: "crew" })` | Must include `kind: "tool_grant"` rows with `crew, role, tools, scope_path, duration, reason, risk, requested_at, expires_at` |

Hook proposal: `useCrewToolGrant(crewId): Wired<CrewToolGrant>` + `useCrewToolAudit(crewId): Wired<AuditRow[]>`, sample adapters labelled "◇ Sample data" in mock mode, `unwired` → "Needs wiring" (no switch shown, read-only stated).

## UI states the data must support
off (default) · pending (request exists) · applying (set called, not confirmed) · on (confirmed; `expires_at` drives "47 min left") · error (confirm failed/timeout → off + reason) · expired/revoked (back to off, audit line).

## Guardrails to test (Claude)
- No grant → `fs_read`/`shell` rejected even if a role names them.
- Expired grant rejected without a restart.
- Path escape (`../`, symlink) rejected and audited as `blocked`.
- `git push` / network rejected with the grant on.
- Only an approval with `actor = taisun` can create a grant; a crew calling `crew_tool_grant_set` is refused.
- Revoke stops a running child.
- Audit rows contain no secrets (reuse the redaction test).

## Open questions (Taisun) — see README "Decisions"
Default duration, folder policy, whether "Until I turn it off" exists, request expiry, CSV export.

## Built (design-build-1111, approved by Taisun 2026-11-11 12:52 JST)
UI: `src-console/crews/toolGrant.ts` (adapter + types), `src-console/screens/CrewToolsPanels.tsx`, `CrewTools.css`; Crews list gained a Manage/Review column and a per-crew page. The live adapter is `unwiredCrewTools` until the commands above exist.

Defaults chosen (README "Decisions"):
- Off by default for every crew. Only Taisun can turn it on; a crew can only *request* it (request card: Approve / Decline).
- Default duration **1 hour** (Taisun). Options: This run only · 1 hour · 24 hours. "Until I turn it off" is **not** offered; 24 hours is the cap.
- Folder: **this run's folder** only (shown as the scope path; sample `~/crew-sandbox/<crew>/latest-run`).
- Requests expire after **60 min**.
- "On" shows only after the backend confirms; confirm timeout **15 s**, then fail closed (off + reason).
- Audit: in-app trail only, **no CSV export**.
- Sends, spending, git push and sudo are always shown as **Blocked**, grant or not.
