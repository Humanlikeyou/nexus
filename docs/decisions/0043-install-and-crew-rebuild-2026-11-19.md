# 0043: Install the decision 0042 build and rebuild the crew container (Taisun, 2026-11-19, in session)

His words (verbatim): **"Yes and yes. Then save session"**. He was answering two questions: build and install cb65a3b+ (backup of the current app kept), and rebuild the crew container so crew approvals work.

1. **Crew container:** `docker compose up -d --build crew` in `stack/`, using the existing compose file and `.env.crew`. No compose or env edits. The old container image (built 2026-10-30) predates the approvals endpoint (crew-service main.py, R13-D, 2026-11-14). After the rebuild, GET /crews/approvals answers.
2. **Install:** release build, then the current /Applications copy moves to ~/Backups (new name), ditto, reopen.
3. **Save session.**

**Outcome (same day):**
- **Crew rebuild: not done, stopped for Taisun.**
  - `docker compose up -d --build crew` in this repo's stack refused: LETTA_DB_PASSWORD is not set in this repo's stack/.env.
  - The running crew container comes from ~/projects/nexus/stack (nexus-v2: secrets, but no approvals code). The approvals code is only on amara-integration, and merging is frozen (0001).
  - Options for him: approve the merge, then rebuild in ~/projects/nexus; or build this branch's crew-service with the nexus stack's env (needs checking that this stack has .env.crew etc.).
  - Nothing was changed; the old container stays up and healthy.
- **Installed 7c7c5b0.** Quit, ditto, reopened; the app is running. Rollback: ~/Backups/Amara OS-backup-20261119e.app (222b486).
