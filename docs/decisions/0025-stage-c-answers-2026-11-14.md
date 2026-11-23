# 0025: Stage C answers (Taisun, 2026-11-14, in session)

Questions: `tasks/HANDOFF-FOR-REVIEW.md` §21. His words: "1. from from after noon 2. up to you 3. up to you 4 up to you".

1. **Check-in reminder: "from afternoon".** The Needs-you check-in pointer shows only from 12:00 JST, and only while there's no check-in that day. Before noon it's hidden.
2. **WHOOP "legacy" heuristic (NOOP store written in the last 10 min): "up to you".** Assistant's call: keep it. It is read-only and file-based, and the takeover stage will replace it with a real lock.
3. **Letta, Verba and crew restartable: "up to you".** Assistant's call: yes, but only for container names verified in the repo's `stack/docker-compose.yml` (never guessed). They go through the same approval-every-call path, the 3-try limit and the 60 s timeout. A name that can't be verified stays `unsupported`.
4. **A second speak request while one waits: "up to you".** Assistant's call: keep refusing it, so speech never piles up.
