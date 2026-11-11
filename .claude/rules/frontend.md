---
paths:
  - "src-console/**"
  - "test/**"
---
# Frontend standards

- Prefer adapter/hook wiring over edits to Design-owned screens (`design-boundary.md`).
- `Wired<T>` (`src-console/lib/sampleAdapters.ts`): adapters are synchronous over already-loaded data. Async sources load into `SystemSnapshot` (bridge `loadSystemSnapshot`) or a hook; the adapter reads them. Live pickers may spread `unwired*` for missing methods.
- Sample adapters only when `localStorage["nexus.sampleAdapters"] === "1"`; every sample value has a "◇ Sample data" tag.
- Tests: vitest under `test/unit` (TZ=Asia/Tokyo), Playwright under `test/e2e`. No new colours, tokens or class names.
