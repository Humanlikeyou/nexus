# HANDOFF-1111u: a Home take from the last 15 minutes of heart rate, and the Recovery & sleep card (for Claude Code)

**Status: APPROVED (Taisun, 11 Nov 17:36 JST) and the UI is BUILT** on `design-build-1111u` (see §10). The backend pieces (§3–§5) are still for Claude. The mockups were injected over a sample-mode snapshot of `amara-integration` at **e7d2ee4**.

## 1. What Taisun asked for (BRIEF.md, 11 Nov 16:04 JST)
- Home's Amara take reads the **last 15 minutes of heart rate** and writes a take from it: the trend, average and range, what it suggests, and the window it used.
- **No button and no countdown.** When he opens Home, or comes back to it, and **30 minutes or more** have passed since the last take, Amara writes a new take automatically. Within 30 minutes the existing take stays, with a quiet "as of HH:MM".
- He called the Recovery & sleep card's empty middle area "trash". `rA`, `rB` and `rC` are the options for it (see §6).

## 2. What exists today (keep it)
- The landing take is `AmaraTake page="home" compact` (`amaraTake/`). It's generated once a day from sleep and recovery facts, with the 1111t slots (memory line, Today's one thing).
- The landing heart widget (`LandingScreen` → `useLiveHeartRate` → `live_heart_rate`) shows the live bpm and the **last 89 seconds** from NOOP (WHOOP).
- The take store is kv `amara_take_<page>`, plus the 1111t TakeLog (memory and History).

## 3. Data: the HR window query (new, read-only)
Add the Tauri command `hr_window({ minutes: 15 }) -> HrWindow | null`, read from NOOP's stored WHOOP heart-rate samples (the same source `live_heart_rate` uses):
```ts
type HrWindow = {
  from: number; to: number;                 // ms epoch, to = newest sample time
  source: "whoop" | "other"; lastSampleAt: number | null;
  perMinute: Array<{ t: number; bpm: number | null }>; // 15 buckets, mean of that minute's samples, null = no samples
  coveredMin: number;                       // buckets with data
  stats: { avg: number; min: number; max: number; first: number; last: number } | null; // over covered buckets
  usual: { lo: number; hi: number; days: number } | null; // his usual for this hour-of-day: 25th–75th pct, last 14 days, waking only
};
```
- **Not enough data:** if `coveredMin < 10`, return the window anyway (so the UI can say "6 of 15 min"), and the take is **not** generated.
- **Source down:** if `now − lastSampleAt > 10 min`, the source counts as unavailable. The UI says "WHOOP hasn't synced since HH:MM", and no take is written from old data.
- **Usual:** `usual` stays `null` until 7 or more days are stored. The text then leaves the comparison out.
- **Privacy:** no raw samples go to the model. Only `stats`, `coveredMin`, the window times and `usual` do.

## 4. The 30-minute rule (enforced in the backend)
Add `home_hr_take_maybe_refresh() -> TakeResult`:
1. The frontend calls it when Home **mounts**, when the window **gains focus**, and when he navigates back to Home. It never runs on a timer.
2. The backend reads the last Home HR take from the TakeLog. If `now − lastTake.generatedAt < 30 min`, it returns that take unchanged with `{ fresh: false }`. **The backend decides, so a reload, two windows or rapid focus changes can never make two takes.** Guard it with a mutex or an in-flight flag per page.
3. Otherwise it calls `hr_window(15)`:
   - If coverage is under 10 minutes or the source is down, it returns the last take plus `{ blocked: "not_enough" | "source_down", detail }`. Nothing is written, and the 30-minute clock does **not** restart, so the next return tries again.
   - Otherwise it builds the facts and generates the take (the existing `generate.ts` path: local model first, number validator, template fallback), appends it to the TakeLog and returns `{ fresh: true }`.
4. The UI shows these states:
   - **regenerating:** the old take dimmed with "Reading your heart rate from HH:MM–HH:MM…"
   - **fresh:** "Just now"
   - **current:** "as of HH:MM · N min ago"
   - **blocked:** a quiet sentence explaining why, with the last take still reachable in History
   - **unwired:** "Needs wiring", with no numbers

## 5. Text rules (extends 1110f and 1111t)
- **Facts:** `avg`, `min`, `max`, `first`, `last`, the direction, the window (`from`–`to`) and `usual.lo`/`usual.hi`. The validator rejects any number not in that set.
- **Shape:** 2 sentences. Sentence 1 says what happened, with numbers. Sentence 2 says how that compares with his usual, and at most one gentle, optional suggestion ("may", never "should").
- No causal words (the 1111t `CAUSAL` guard). No medical claims. Nothing about arrhythmia, stress diagnosis or "your heart is…".
- **Memory line:** the previous HR take, from stored rows (e.g. "At 1:12 PM you were at 84 and climbing").
- The chips are **Avg**, **Range** and **Trend**. The window and source go on one quiet line ("From 1:45–2:00 PM · WHOOP · 15 of 15 min", capped at 15).
- The 1111t "Today's one thing · Home" stays under the take, unchanged. It's daily, not HR-based.

## 6. Landing heart widget
Mock `u1` replaces the 89-second wiggle with the **same 15-minute line the take used**: the newest point, start/end times, and his usual band shaded. That way the chart and the take never disagree, and nothing is shown twice. The bpm number stays "now". If Taisun prefers the live wiggle (Q1), keep `traceShape` and add the 15-minute window only as a caption.

## 7. Recovery & sleep card (Home › Today), options
The current card is a 2-row grid: a 89-px ring, two stat tiles and `Open ›` pushed far right, which leaves about 400 px of nothing. Every option keeps each metric **once**.
- **A · Compact row.** One row: a segmented ring (Horizon `SegRing`, 55 px) with the band and "▲ vs usual", then Sleep, HRV and readiness, each with its "vs usual". `Open ›` moves into the header. The card is about 50% shorter. The readiness reason moves to the Mind–Body page (it's in the aria-label).
- **B · Last night (Horizon + Dawn).** Three columns on a 1 : 1.618 : 1 grid:
  - recovery ring with 7-day bars;
  - the Dawn sleep strip (bed → wake, stage bar, stage totals) with the total and "vs usual" once;
  - HRV on the Horizon scale with the usual marker.

  The readiness row stays underneath. There's no separate Sleep or HRV tile.
- **C · Your body today (merged).** A 1 : 1.618 split: on the left, ring plus a 3-line list (sleep, HRV, readiness); on the right, the **heart-rate take** with the 15-minute line, taking its context from last night ("Sleep ran 40 min short, so keep the hardest work before 4 PM"). If C is picked, the readiness reason folds into the take and the separate readiness row goes. The landing keeps its compact take (the same TakeLog row, no second generation).

**Data:** all of it already exists in the snapshot (`noop.today`, `latestNight`, `sleepModel` stages and week, `fitnessRingsModel` usual), except C's take (§3–4).

## 8. Wiring order
1. `hr_window` command + tests: bucketing, coverage, usual percentile and the stale source.
2. `home_hr_take_maybe_refresh` with the backend 30-minute gate, the in-flight lock and the TakeLog append. Tests: within 30 min returns the same take; 30+ min writes a new one; a blocked result doesn't restart the clock; two concurrent calls make one take.
3. Facts and the prompt for the `home_hr` kind, the validator set, the template fallback and the memory-line template.
4. UI:
   - Home take states, with focus/mount triggers and no timers.
   - The landing 15-minute line, if approved.
   - The Recovery & sleep option Taisun picks.
5. Gates:
   - check:secrets
   - tsc
   - vitest
   - cargo
   - lint
   - e2e and axe (serious and critical = 0, both themes, contrast ≥ 4.5)

   Then install once, gracefully.


## 9. Taisun's picks (17:36 JST)
- **Recovery & sleep card: option C**, merged with the heart-rate take (`rC-recovery-body-now`).
- **The landing keeps the 89-second live trace.** §6's switch to the 15-minute line is **not** done. The 15-minute line appears only inside Today's card.
- **A take needs readings in 10 of the 15 minutes.** WHOOP counts as **down after 10 minutes without a sync**.
- **"Your usual"** is the same time of day over the **last 14 days**.
- **Readiness stays visible on the card as its own small row** (score, label, reason). It isn't folded into the take text.

## 10. Built (design-build-1111u, UI only)
Everything is in `src-console`. Nothing was changed in src-tauri.

**`hrTake/adapters.ts`**
- **Contract:** the types (`HrWindow`, `HrTake`, `HrTakeStatus`, `HrTrigger`) and the constants (15-minute window, 10 minutes covered, 10-minute stale source, 30-minute gap).
- **Helpers:**
  - `windowStats`, `coveredMinutes` (capped at 15), `windowBlock` (the 10-of-15 and 10-minute-stale rule), `trendOf`, `placeOf`.
  - `hrFacts` / `hrTextOk`: the number validator plus the causal and medical word guard.
  - `hrTemplate`: the template take, which is also the fallback when the model fails.
  - `hrMemoryLine`: a template over the previous stored take.
  - `ageLabel`: "Just now", then "as of 2:00 PM · 18 min ago".
- **Live adapter:** returns **unwired** ("Needs wiring") for now. Frontend code may only invoke registered commands (`test/commands.registered.test.ts`). When Claude registers the two commands, wire them in `liveHrTake` and pass each answer through `liveOf`:
  - `home_hr_take_status() -> HrTakeStatus` is a read only.
  - `home_hr_take_maybe_refresh({ trigger: "open" | "focus" | "navigate" }) -> HrTakeStatus` writes at most one take, behind the backend's 30-minute gate.

  `liveOf` shape-checks every answer (`isHrStatus`). An error or a wrong shape means **unwired**, so no numbers are ever invented.
- **`HrTakeStatus`:**
  - `take {take, due}`
  - `none_yet {due}`
  - `not_enough {previous, window, due}`
  - `source_down {previous, lastSampleAt, due}`

  `due` is the backend's own answer to "is the last take 30+ minutes old?". The UI never compares times to decide.
- **Sample adapter:** preview and tests only, and tagged Sample data. It mocks the backend: one stored take, the 30-minute gate, an in-flight lock (concurrent refreshes make one take) and blocked results that don't restart the clock.

  Scenarios are set with `localStorage nexus.hrTakePreview = {"scenario": "first" | "current" | "regenerating" | "new" | "not_enough" | "source_down" | "unwired", "ageMin"?, "delayMs"?}`, and only work when `nexus.sampleAdapters=1`.

**`hrTake/useHrTake.ts`:** asks on mount (`open` on the landing, `navigate` on Today), on window `focus` and on `visibilitychange` back to visible. If `status().due`, it shows **Reading…** and calls `refresh(trigger)`. There's no button, timer or countdown.

**`hrTake/HrTakeCard.tsx`:** every state.
- **fresh:** "Just now".
- **current:** the quiet age.
- **regenerating:** the old take dimmed under "Reading your heart rate from …", with `aria-busy`.
- **not enough HR:** "Only N of the last 15 minutes have readings…".
- **WHOOP unavailable:** "WHOOP hasn't synced since …", with a link to Settings › Devices.
- **none yet.**
- **unwired.**

Each take shows chips (Avg, Range, Trend), the window line ("From 1:45–2:00 PM · WHOOP · 15 of 15 min") and the memory line. The `inline` variant adds the 15-minute line with the usual band (`HrTrace`).

**Landing (`LandingScreen.tsx`):** the compact HR take replaces the daily recovery take under the clock. Today's one thing (1111t) stays under it. The 89-second live trace is unchanged.

**Today (`TerranceScreen.tsx`):** "Recovery & sleep" became **"Your body today"** (C), on a 1 : 1.618 grid.
- **Left:** the recovery ring with band and "vs usual", then Sleep and HRV once each with "vs usual". The usuals are medians of the days before today, from at least 5 days, and are left out otherwise.
- **Right:** the inline HR take. It's the same take as the landing, not a second generation.
- **Below:** the readiness row, unchanged. "Open ›" (Open Fitness) moved to the header.

**Tests**
- `hrTake/hrTake.test.tsx` covers the rules, the template and validator, age, memory, the shape check, the sample gate, the lock and blocked results, every card state in both variants, and that there's no refresh button.
- `hrTake/bodyToday.test.tsx` covers card C.
- The e2e spec is `test/e2e/hr-take-1111u.spec.ts`.

**What Claude still builds (backend):**
1. `hr_window` (§3).
2. The two commands, with the gate and lock in Rust (§4). They must return exactly the `HrTakeStatus` shape above (camelCase).
3. The `home_hr` take kind in the local-model path, validated with the same rules as `hrTextOk` and with `hrTemplate` as the fallback (§5).
4. Writing each take into the 1111t TakeLog, so History and Memory see it.

Once the commands are registered, wiring them into `liveHrTake` is a 2-line change in `hrTake/adapters.ts`, and the UI goes live with no other change.
