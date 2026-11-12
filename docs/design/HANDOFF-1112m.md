# HANDOFF 1112m: System map in Settings › Services (Design Lead)

Decision: `docs/decisions/0011-system-map-2026-11-12.md`. Not committed (orchestrator commits).

## What
- `src-console/standalone/SystemMapView.tsx` + `SystemMapView.css` (named so it doesn't collide with `systemMap.ts` on macOS's case-insensitive disk). Mounted at the top of `ServicesView.tsx`, right under the page hero, in both the wired and unwired branches.
- Reads only `systemMapAdapter()`; contract types untouched.
- Layout: columns by distance from Amara OS (app on the left, then what it talks to, then what those talk to). Rows ordered by neighbour position. Lines carry a status dot at their midpoint. Under ~540 px of card width it becomes a list, one row per path (dot, what it carries, "from to to", status word), so there is no horizontal scroll at 820 px.
- Summary line (`role=status`): "All N paths flowing" / "N broken: A, B" / "13 flowing, 1 off on purpose, 2 not checked". "Checked <time>" stamp beside it. Legend row with words.
- Refresh: `adapter.refresh()` immediately when the map becomes visible, then every 10 s; stopped when scrolled off screen (IntersectionObserver), tab hidden (visibilitychange) or unmounted. If refresh throws, the last map stays and the stamp says "Couldn't re-check just now."
- Honesty: unwired shows "Needs wiring" and no dots; `Sample data` tag only when state is "sample".

## Dot states (existing tokens only)
| Status | Look | Cue besides colour |
|---|---|---|
| ok | steady green (`--tone-success`) | solid dot, solid green line, word "Flowing" |
| broken | red (`--tone-danger`), blinks 1.2 s | ring around the dot, dashed red line, word "Broken" |
| off | steady grey (`--label-secondary`) | solid grey, dotted line, "Off on purpose" |
| unknown | hollow grey | hollow dot, "Not checked" |

Reduced motion (OS setting or the app's `data-reduce-motion=true`): no blink; the ring remains. Forced-colors fallbacks included. Each dot is a `<button>` (24 px target, emerald 3px/2px focus ring), `aria-label="<carries>: <status>"`, `aria-expanded`.
Panel (inline under the map, `role=group`): carries, from/to, status, last worked (relative, or "Not seen working yet"), what's wrong / why it's off / why not checked (from `reason`), the usual fix (from `fix`). Focus moves to it on open; Escape or Close returns focus to the dot.

## Tests and results (run just now)
- `npx tsc --noEmit`: exit 0
- `npm run lint`: exit 0, no output
- `npx vitest run`: 166 files passed, 1 skipped; 1580 passed / 9 skipped (baseline 1563; +10 mine, rest are the backend's `systemMapLive` tests). Earlier runs failed once on the backend's half-written live file; it passes now.
- `npm run check:secrets`: ok (841 tracked files)
- `npx vite build`: built. e2e on `E2E_PORT=4179`: `system-map-1112m.spec.ts` (new, 8 = 4 tests x 2 themes: unwired, sample, keyboard, blink), `standalone-1111s.spec.ts`, `home-windows-1111w.spec.ts`, `ha-settings.spec.ts`: 64 passed, 2 failed (see below). Axe serious/critical = 0 in both themes for the unwired and sample states. Full `npm run test:e2e` and the content-edge e2e: **not run**.
- New files: `SystemMapView.test.tsx` (states, blink vs reduced motion, panel content, Escape, no refresh while hidden/off screen/unmounted), `test/e2e/system-map-1112m.spec.ts`.

## Open questions / needs from others
1. **Contract sample label breaks an existing test (needs backend/Lead).** `makeSampleSystemMap()` in `systemMap.ts` names a node "Health sync (NOOP)". The existing e2e `standalone-1111s.spec.ts:19` "Services with sample adapters" (both themes) fails its no-NOOP check (Taisun's rule from Nov 11). Fix: rename to "Health sync". I did not touch the contract, and I did not hide it in the UI. This is the only red e2e I saw.
2. Sample map includes `crew-router` and `muse-app` as "unknown": fine, but the summary then never says "All paths flowing" in sample mode (by design).
3. The app has a hard minimum width of 820 px, so I tested 1100 and 820 only.
4. Visual polish, not done: in the 16-path sample some lines run behind other nodes (for example the Router to Local models line passes near Hermes), and two dots sit close together near the top. Straight-line layout is simple; a smarter router or per-column ordering by the backend would help. Screenshots are at `test-results/shot-dark-1440.png` and `-820.png` (dark only; light not captured). Taisun should see a live shot before it counts as done.
5. Live checks (blur, real data) not done: no live app driving per rules.
