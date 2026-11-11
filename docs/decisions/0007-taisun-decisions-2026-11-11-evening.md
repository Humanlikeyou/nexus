# 0007: Taisun's decisions, 2026-11-11 ~18:10 JST ("do what you think is best")

Applied as written; not to be reopened.

1. **4–5 Nov pushes/merges:** not confirmed by him. No action, no revert; nothing is pushed without his OK.
2. **7 uncommitted docs:** stay uncommitted, for him.
3. **Disk / infra:** `/tmp/rev1110*` already deleted; don't recreate large /tmp build copies. Letta CVE-2026-77937: plan the upgrade, don't apply it, don't touch Letta data. Duplicate n8n workflows and stale containers: list only; never delete, stop or modify.
4. **No recording delete path at all.** `removeConfirmed` stays a no-op that cannot delete. (1111t "Forget a take" is unrelated and may stay.)
5. **`save_session` refuses a duplicate id** with an error; never overwrites. Existing files untouched.
6. **Follow-up approvals:** once per session, with a list of the edits; not one dialog per edit. Taisun is the only approver. Deny is final for that batch.
7. **Router:** the 60 s grace stays. The bearer is required when cloud is on, loopback only, read from the Keychain; never logged or returned. `/health` stays exempt.
8. **Crews:** tools read-only by default, writes need approval; the 1110i Files/terminal switch defaults off, fails closed, is enforced in crew-service, and writes still need approval when on. Always blocked: network/outbound, git push, sudo, spending. Taisun approves. At most 2 automatic retries, then "Needs you". Concurrency 1. **MiniMax removed.** Other cloud paths opt-in and off when outbound is disabled. Cost $0.00 locally, real cost only for the Claude tier. SOUL.md: a copy in app data, never read from ~/.hermes, refreshed only with his OK. Crew runs wait while a deep chat turn is in progress. Skill proposals live in app-data `skills/` (versioned, audit log), never auto-applied, always through the Crews "Needs you" queue. 1110i defaults: this run only; sandbox `~/crew-sandbox/<crew>`; cap 24 h; grant requests expire in 1 h; no audit CSV export.
9. **Local `pre-push` hook:** refuses every push unless `NEXUS_PUSH_OK=1`. Installed 2026-11-11 at `~/projects/nexus/.git/hooks/pre-push` (shared by both worktrees; not versioned, never pushed). `.claude/hooks/guard-bash.sh` stays as defence in depth.

Out of scope tonight (his words): the NOOP merge, a live Telegram relay send path, standalone phases (bundling, signing, notarization, DMG, updater, WHOOP takeover).
