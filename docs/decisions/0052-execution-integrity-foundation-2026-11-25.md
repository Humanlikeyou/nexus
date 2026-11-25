# 0052: Backend execution integrity foundation (Taisun, 2026-11-25, in session)

His words: **"Implement the approved backend reliability architecture by extending existing Nexus infrastructure. This is a non-destructive enhancement, not a rebuild."** He also said: stop after this phase and wait for review.

**Order and database note.** This is Phase 3 of the handoff table. Phase 2 (the shared data-architecture proposal) has not been written or approved. The prompt says "using the approved database architecture", and none exists yet. So this phase takes the smallest change that touches no existing data:
- **One new SQLite file**, `<data>/execution.db`, opened with the existing `shared::sqlite::open_private_wal` (0600, WAL, busy timeout).
- **Two new folders:** `<data>/workspace/skill_versions/` and `<data>/execution_responses/`.

No existing table, file or store was migrated or changed in shape. Phase 2 may fold the ledger into its plan.

## What existed (traced in code)

- **Model actions:**
  - The model proposes an action with the inline `<nexus_action>` protocol. `terrence_actions.rs` stores it in `terrence_core.db` (`terrence_actions`).
  - A native "Approve Action" dialog shows it, with a parameter fingerprint checked after the dialog.
  - `execute_local_action` runs one of 8 local tools (open a link or app, reveal, volume ×3, a Mac workflow, organize a folder).
  - There is a manual Retry (a new row), and Cancel.
- **What was missing there:**
  - Status was **overwritten in place**, with no history, and the update wasn't atomic (read, check, then write).
  - There was **no verification**, and success meant the command returned 0.
  - An action left `executing` by a quit stayed that way forever; only Hermes conversations were reconciled.
- **Tools outside Nexus:** Hermes owns its own tool execution (MCP tools such as vault, follow-ups and inbox), and Nexus only records Hermes's reported action events. OpenClaw's gateway mode was removed; `openclaw.rs` only reads skills and workspace files and writes skills. Nothing in Nexus runs a model's tool call through OpenClaw.
- **Approvals:** `approval_gate.rs` handles native dialogs, in-flight locks and the `declined:`, `busy:`, `expired:` and `changed:` contract. `memory_write.rs` handles Letta writes with backup, approval and a re-read before patching.
- **Skills:**
  - `save_skill` writes `<data>/workspace/skills/<stem>.md`, and `read_skills` loads project, then learned, then legacy, with the first name winning.
  - `skillsEvolution.ts` counted a skill "created" as soon as `save_skill` returned, and it became **active at once** in the next prompt.
  - A generated skill with a project skill's name was silently shadowed. There were no versions and no read-back.
- **Persistence errors:** `subagents.ts` `saveToDisk` caught and logged, so callers reported success after a failed save.
- **Other stores:**
  - Outbox (`outbox.rs`): telemetry delivery to a gateway, with backoff and acknowledgements. Not reused for execution: delivering telemetry is not executing an action.
  - `events.db`: a telemetry time series, not an execution ledger.
  - ActivityBus: renderer job progress, not a ledger.
  - History/Undo (`adds::logged_write`): user edits to domain stores. Unchanged.

## What changed

- **Ledger** (`commands/execution/ledger.rs`):
  - `executions` (current state, queryable).
  - `execution_transitions`: append-only. SQLite triggers refuse UPDATE and DELETE.
  - `execution_approvals`: bound to one action and the parameter fingerprint, with an expiry, used at most once.
  - `schema_meta`: version 1. A newer file is refused.
  - Every status change is a compare-and-swap plus its transition row, in one transaction. All ten states are supported, with an allowed-transition table.
- **Controller** (`commands/execution/mod.rs`):
  - **Registry:** a closed registry of Rust operations, with no shell, file or network passthrough.
  - **Permission table** (`required_authorization`), by class and risk:
    - read-only and staging: no approval;
    - reversible local: approval unless the user started it;
    - consequential external, destructive and high risk: always approval.
  - **Approvals:** only `grant_user_approval` can create one. It is called from Rust native-dialog flows and has no IPC, so a model can't authorize itself.
  - **Each run:** timeout, response capture (inline up to 16 KiB, else a file reference), and redaction of secrets, personal fields and long text.
  - **Verification:** read-back against the expected state gives VERIFIED, FAILED (mismatch) or UNVERIFIED (no read-back possible).
  - **Retries:** only for failures that certainly didn't reach the target.
  - **Timeouts:** never resubmitted. They are reconciled by read-back, else left UNVERIFIED.
  - **Partial runs:** PARTIAL with the completed steps and the compensation result.
  - **Duplicates:** an idempotency key means a request never runs twice, including under concurrent submits.
  - **Restart recovery** never re-runs anything:
    - never attempted → CANCELLED;
    - attempted → reconciled or UNVERIFIED;
    - executed → verified now.
- **The model-action path now goes through it.**
  - Allow in the native dialog is recorded as a 2-minute approval bound to the exact parameters. The run uses `idempotency_key = terrence_action:<id>`.
  - Volume and mute are verified by a separate `osascript` read of the volume settings. The other six tools have no independent read-back and are recorded UNVERIFIED, never VERIFIED.
  - A failed verification now marks the action **failed**.
  - The action-row status update is now compare-and-swap.
  - At startup, after the ledger recovery, actions left `executing` take their outcome from the ledger, or `needs_attention` if it has none.
- **Skill lifecycle** (`commands/skills_lifecycle.rs`):
  - States: PROPOSED → VALIDATED → STORED → READ_BACK_VERIFIED → ACTIVE, plus REJECTED and RETIRED.
  - **Versions:** every version is kept in `skill_versions/<stem>/v<N>.md` and never overwritten. Each version's state history is in `manifest.json`.
  - **Validation:** title, size, and the existing injection guard.
  - **Storing:** a controller operation. It is VERIFIED only if the file read back has the expected SHA-256.
  - **Activating:** VERIFIED only if `read_skills`' own precedence resolves to the expected content. A project skill with the same name blocks activation instead of a false success.
  - **Existing skills are preserved:** a live skill that predates the manifest is imported as a version before it is replaced.
  - Restoring means activating an older version.
  - **Never auto-activated.** There is no configured permission for it, so activation needs the native dialog (`activate_skill_version`).
- **Skills evolution** now calls `propose_skill_version` and counts only READ_BACK_VERIFIED versions. A failure is counted and shown in Activity as failed. Generated skills wait for review instead of going live.
- **Subagents:**
  - `saveToDisk` returns whether it worked and drops the cache on failure.
  - Create, toggle, stop, add-job and delete report failure.
  - The Telegram bridge (disabled) says so.
- **Backups** include `execution.db` as a consistent `VACUUM INTO` snapshot, and the restore plan allows it.
- **New commands:**
  - read-only: `list_executions`, `execution_history`, `list_skill_versions`;
  - lifecycle: `propose_skill_version`, `activate_skill_version` (native dialog), `reject_skill_version`.

  None of these can grant an approval.

## Not changed

- Direct user workflows (History/Undo writes, settings, goals) keep their own paths.
- Hermes and its tools.
- The outbox.
- ActivityBus semantics.
- `events.db`.
- The UI: no screen shows the ledger or the skill-review state yet. That is Design work.

## Limits

- Six of the eight local tools are UNVERIFIED by design: there is no independent read-back for opening a link or an app, revealing, adjusting volume by a delta, a workflow, or organizing a folder.
- Restart recovery reads back with the stored, redacted parameters. An operation whose read-back needs a redacted value stays UNVERIFIED.
- A timed-out operation's thread may still finish later. Its late result is ignored, and `reconcile` (read-back only) can settle it afterwards. There is no production caller of `reconcile` yet.
- No UI exists for approving or rejecting generated skills, so they stay READ_BACK_VERIFIED until one is built.
- Hermes-side tool execution is outside Nexus and outside this ledger.

## Follow-up: security review of 329ea31 (same day)

An automated review found that the skill-activation dialog named the skill and version but not its content, so the user approved text they could not see. Fixed:
- The dialog now shows the start of the exact stored text (up to 1,400 characters, with a count of the rest) and a checksum prefix.
- Activation is bound to the checksum shown. If the version's content differs, it is refused with `changed:` and nothing is activated.
- The activate operation re-reads the version, re-checks the checksum and re-runs content validation before anything goes live, so a tampered file, or one that a stricter guard would now reject, is refused.

Three tests cover this. The redaction test fixtures were also renamed to the repository's `FAKE_*_NOT_REAL` convention so `check:secrets` passes on tracked files. They were always fake values.
