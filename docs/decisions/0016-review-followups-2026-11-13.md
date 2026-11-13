# 0016: Follow-ups from the 2026-11-13 review (Taisun, 2026-11-13, in session)

Context: `tasks/REVIEW-2026-11-13.md` §6–7. Taisun picked from four questions in session:

1. **Off-disk copy of the code:** "Google Drive". A `git bundle` of the repo plus a copy of his uncommitted docs and untracked files goes to his Google Drive. A bundle is a file copy, not a push, so decision 0001 still holds. Private code goes to Google's servers, and he chose that.
2. **Chat path:** "Router direct + Hermes for tools". Ordinary typed turns go straight to the local Amara Router, with streaming text and a real Stop. Hermes is used only when a turn needs its tools (vault, follow-ups), chosen by an explicit rule first (no classifier).
3. **Letta in chat:** "Yes, read-only in the prompt". The human and persona blocks are added to the system prompt, read-only and cached per conversation, through the existing memory_read path. No new Letta writes.
4. **CLAUDE.md:** "Draft a trimmed one". Claude writes `tasks/proposed/CLAUDE.md` and Taisun swaps it in himself. His file stays untouched.

Also kept from the review §7: Letta stays a supervised service; Phases 9–17 (signed DMG, first run, migration) stay. Start the Apple Developer ID.

Still frozen (0001): merge into `nexus-v2`, push, PR.
