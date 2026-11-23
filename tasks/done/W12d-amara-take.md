# W12d: "Amara's take" backend + useAmaraTake(page)
- **Owner:** frontend-executor (+ a tiny Rust store only if kv commands don't fit). UI card is Design Lead's 1110f: do NOT build any card.
## Objective
One daily comment per page (fitness, neuro, mindBody, business, home), written by Amara from that page's actual data, cached per page per day.
## Acceptance criteria
- [ ] `useAmaraTake(page)` returns `Wired<AmaraTake>`; `unwired` until that day's data exists or while generating/failed. Never a templated or pre-written string.
- [ ] Facts are extracted deterministically from real loaded data (with recent-baseline comparisons computed in code); the prompt carries only these facts.
- [ ] Generation goes through the local router via the existing client (`chat_with_router`, light/fast tier; SOUL.md + IDENTITY_GUARD preface already applied there). No cloud unless the existing Claude gate allows it (don't add a new path).
- [ ] Output validator: rejects text that cites none of the fact values (number match with tolerant formatting); a rejected take stays unwired.
- [ ] Stored locally with the cited data points, keyed page+local date (Asia/Tokyo). Refresh at most once more that day when a new session/data lands (fact fingerprint changes).
- [ ] Tests: deterministic fixtures (mocked router reply), validator accepts a take citing fixture numbers and rejects a vague one, cache hit/miss/refresh-once, no generation when data for the day is missing.

## Evidence (frontend-executor, 2026-11-11)
- Code: `src-console/amaraTake/{types,facts,validate,store,generate,live,useAmaraTake}.ts`, tests `amaraTake.test.ts` (16 tests). No screen edited, no card, hook not imported anywhere.
- Gates: `npx tsc --noEmit` clean; `npm run lint` clean; `npm run test:unit` 1195 passed, 9 skipped (126 files; other agents' tests included). Own file: 16 passed.
- Router: `chat_with_router` with `forceProvider: "light"` (fast route; SOUL.md + IDENTITY_GUARD + Claude gate unchanged in llm_router.rs). A reply whose tier is "claude" is discarded.
- Storage: kv `amara_take_<page>` = {day (Asia/Tokyo), attempts, fingerprint, take{text, cited[], fingerprint, generatedAt}}. Max 2 router calls per page per day.
- Not run: real router/NOOP/Tauri end-to-end (mocked only); no live generation was tried.
