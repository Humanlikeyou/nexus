# 0044: Merge amara-integration into nexus-v2, then rebuild the crew there (Taisun, 2026-11-19, in session)

His word (verbatim): **"Approve"**. He was answering decision 0043's two options. Option (a) was "approve merging amara-integration into nexus-v2, then rebuild the crew in ~/projects/nexus/stack". Claude read "Approve" as (a).

- This is the hard rule 1 OK for this one action only: a local `git merge --no-ff amara-integration` into nexus-v2 in ~/projects/nexus. **No push.**
- State at approval (read-only check):
  - nexus-v2 at d200630, clean
  - amara-integration 283 commits ahead
  - nexus-v2 18 commits not on amara-integration, so conflicts are possible
- The guard hook blocks agents from running this merge, and ~/projects/nexus is read-only for agents. Taisun runs the merge himself (command given in session). If it conflicts, he aborts with `git merge --abort` and Claude resolves on amara-integration first.
- After the merge, Claude rebuilds only the crew service from ~/projects/nexus/stack (`docker compose up -d --build crew`). That uses the existing compose file and env with no edits. Then Claude verifies /health and /crews/approvals.
