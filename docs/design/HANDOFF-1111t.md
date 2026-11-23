# HANDOFF-1111t: take memory, daily suggestion, follow-through and outcomes (for Claude Code)

**Status:** this is a proposal. **Build only after Taisun approves the images** (`README.md` §3, §5). The UI is Design's job once it's approved. This file covers the backend and data contracts. The base is `amara-integration` 3ca4ea2+ (`src-console/amaraTake/*`, design-1110f d1f2168; 1111a `src-console/personal/*`).

**Hard rules** (same as every Nexus task):
- Local commits only, staging explicit paths. Never stage the 7 docs.
- Everything runs on the Mac, with the router set `localOnly: true` exactly as `live.ts` does today. No cloud path for this feature.
- Never invent numbers. A failed read means *no data*, never a guess.
- Never touch or move existing replay or session files.
- Sample data only behind `nexus.sampleAdapters`, tagged ◇ Sample data.

## 1. What exists today (keep it)
- `generate.ts runTake`: one take per page per day. At most 2 router calls, the `validateTake` number guard, `facts.ts` FactSets with 7+ day baselines.
- `store.ts`: **a single** `amara_take_<page>` kv record, overwritten daily. **This is why Amara has no memory.**
- `AmaraTakeCard`: states fresh / generating / nodata / failed / unwired. Ask more.

## 2. Data model (new, all local)
Use the same local store the app already uses (`loadKv`/`saveKv` via `bridge`). If an append-only log in kv gets heavy, put it in a small SQLite table behind new Tauri commands; that's Claude's call, with no new service. All timestamps are ms, and days are `tokyoDay` strings.

```ts
// Append-only. Never overwritten; "forget" sets forgottenAt and blanks text/cited.
type TakeLogEntry = {
  id: string; page: TakePage; day: string; generatedAt: number;
  kind: "daily" | "followup";            // followup = the take that carries a verdict
  text: string; cited: CitedPoint[]; fingerprint: string; tier: string;
  trialId: string | null;                 // the trial this day's suggestion belongs to
  memoryLine: string | null;              // the "↺ Oct 7 I suggested…" sentence, template-built (see §5)
  forgottenAt: number | null;
};

// Static, versioned JSON in the repo (src-console/amaraTake/catalog.json). Human-reviewed; the model never adds entries.
type RecCatalogEntry = {
  id: string; page: TakePage; version: number;
  title: string; how: string;                       // fixed copy
  trigger: { factKey: string; op: "below_baseline" | "above_baseline" | "below" | "above"; by?: number; minBaselineN: 7 };
  outcome: { metric: string; lag: "same_session" | "same_day" | "next_morning" | "that_night" | "event"; better: "up" | "down"; wobbleFloor: number };
  detector: DetectorId | null;                      // null = tap only
  trialDays: 21 | 14; cooldownDays: 60;            // cooldown after "none" or "worse"
  evidence: { level: "strong" | "moderate" | "early" | "weak" | "own"; summary: string; sources: { cite: string; url: string; reviewedAt: string }[] };
  contraindications?: string[];                    // e.g. "recovery < 34%" for any effortful item
};

type Trial = {
  id: string; page: TakePage; recId: string; recVersion: number;
  startDay: string; plannedEndDay: string;          // extended by 1 per "not today"
  status: "running" | "ended" | "kept" | "stopped" | "extended";
  primaryMetric: string;                            // fixed at start (pre-registered); no metric shopping later
  baselineWindow: { from: string; to: string };     // 14 days before startDay
};

type Adherence = {
  trialId: string; day: string;
  state: "pending" | "done" | "skipped" | "not_today" | "unknown";
  source: "tap" | "detected" | "midnight";          // midnight = no tap + no detection → unknown
  evidenceRef: string | null;                       // session id / WHOOP activity id / HA event id / ledger id
  reason: "no_time" | "didnt_feel_like_it" | "forgot" | "not_useful" | null;
  markedAt: number; undoneAt: number | null;
};

type Outcome = {
  trialId: string; computedAt: number; methodVersion: 1;
  kind: "interim" | "final" | "habit_recheck";
  groups: { with: Stat; without: Stat; before: Stat };   // Stat = { n, mean, median, sd }
  diff: number; ci90: [number, number]; wobble: number;
  verdict: "too_early" | "none" | "better" | "worse" | "unclear";
  confounders: { metric: string; with: number; without: number; flagged: boolean }[];
  excluded: { unknown: number; notToday: number; noMeasurement: number };
};
```

Migration: on first run, copy today's `amara_take_<page>` record into `TakeLog`. Keep `store.ts` as the daily cache. The log is the memory.

## 3. Recommendation engine (deterministic selection; the model only writes the "why")
1. **Gate.** Amara suggests nothing until the page has its 7+ value baseline (`makeFact` already enforces n ≥ 7). That's state `t8` #1.
2. **Running trial?** If one is running, today's suggestion is that trial's entry. There is one trial per page at a time.
3. **Otherwise, pick.** Candidates are catalog entries for the page whose `trigger` fires on today's FactSet, excluding cooldowns and contraindications. Rank by evidence level (strong > moderate > early > own; **weak is never picked**, only shown as "also considered"), then by how far the trigger metric is from baseline. Pick nothing if no trigger fires: "No suggestion today, on purpose" (`t8` #2).
4. **Why sentence.** Add the trigger facts and the trial or outcome numbers to the FactSet, and extend `buildPrompt` with one more sentence: "Explain in one sentence why this suggestion fits the facts". Validate it with `validateTake` plus a new **causal-word guard** that rejects `caus|prove|because of|fixed|cured|guarantee`. If validation fails, use a template `why` built from the trigger facts. Never patch model text.
5. The evidence card text and sources come **only** from the catalog. Before shipping, each source in the mock must be checked by a person; the mocks cite these illustratively:
   - slow-breathing review, Zaccaro 2018 (15 studies)
   - HRV-biofeedback meta-analysis, Lehrer 2020 (58 studies)
   - 2018 US Physical Activity Guidelines Advisory Committee report
   - HRV-guided training, Kiviniemi 2007
   - room light and melatonin, Gooley 2011
   - reading, 2009 unpublished, rated weak

   Business entries are `level: "own"` with no outside claim.

Starter catalog, one entry or more per page:
- **Neuro:** breathe ~6/min for 5 min before a session (calm, same session). A 10-min walk outside before a session.
- **Fitness:** a 25-min brisk walk before 17:00 (sleep latency, that night). 15-min mobility.
- **Mind–Body:** easy day when HRV is below baseline (next-morning HRV). Bed by goal time after a short night.
- **Business:** the top decision before noon (days to decide). A 90-min focus block on the launch task.
- **Home:** lights warm and dim after 21:00 (sleep onset, that night). Bedroom ≤ 20 °C (only if HA has a sensor).

## 4. Adherence detection
Detection runs when the relevant data arrives (a session saved, a WHOOP sync, an HA event, a ledger write) and once at **00:05 Tokyo** for the previous day. A detected Done needs a concrete `evidenceRef`; there is never a "probably". A tap always wins, and Undo resets the day to `pending`.

| Detector | Rule | Source |
|---|---|---|
| `muse_breathing_before_session` | A breathing-type session ≥ 4 min ending ≤ 30 min before a neurofeedback session that day | `list_sessions` (needs a session type field; if it doesn't exist, ship tap-only and show `t8` #4) |
| `whoop_walk` | Activity type walk ≥ 20 min, or a step delta ≥ 2,500 within 30 min, before the deadline | NOOP `health_snapshot` / activity rows |
| `whoop_easy_day` | Day strain < 10 by 23:59 | NOOP day row |
| `ha_evening_lights` | The scene "evening warm" is applied at 21:00 ± 15 min and stays until bed | Home Assistant event log (read only; "Let Home do it" needs Taisun's yes, see README Q5) |
| `ledger_decision_before_noon` | Decision recorded with a timestamp before 12:00 | Business ledger |
| `checkin_saved` | A check-in saved that day | 1111a check-ins |

At midnight, a day with no tap and no detection becomes `unknown`. **Unknown is never treated as skipped.**

## 5. Outcome evaluation (method v1)
- **Unit.** One trial day with a measured outcome at the entry's lag:
  - same-session calm or focus
  - next-morning HRV or recovery
  - that night's sleep latency or efficiency
  - days-to-decide per decision
- **Groups.** With = done days. Without = skipped days in the same trial. Before = the 14 days before `startDay`. Excluded and counted: unknown, not_today, and days with no measurement.
- **Effect.** `diff = mean(with) − mean(without)`. The **90% interval** comes from a seeded bootstrap (2,000 resamples, deterministic seed = trialId) so the same data always gives the same words. Show `before` as context: if with ≈ without but both differ from before, say so; don't credit the suggestion.
- **Wobble (minimal meaningful change).** `max(entry.wobbleFloor, 0.5 × SD of the metric over the 28 days before startDay)`. The mocks use ±3 for calm.
- **Verdict rules** (fixed, in this order):
  1. `too_early` if n(with) < 7 or n(without) < 7. Show "need 7 + 7" and the next look date.
  2. `unclear` if a confounder is flagged: sleep duration or recovery differs between groups by > 0.5 SD, or the weekend or launch-week share differs by > 30 points.
  3. `better` / `worse` if the whole CI90 is on one side of 0 and |diff| > wobble, with direction from `outcome.better`.
  4. `none` otherwise.
- **Windows.** Interim at day 7 (almost always too early, and it says so). Final at `plannedEndDay` (21 days, or 14 for Business). On "Run it 2 more weeks", +14 days, then re-evaluate. After "Keep it as a habit", a monthly `habit_recheck` over 28 days. If a kept habit drifts to `none`, the take says so.
- **After a verdict.**
  - `worse` → stop and cool down for 60 days, and say so in the take.
  - `none` → cool down for 60 days for that page and metric.
  - `better` → offer Keep / Run 2 more / Stop.
  - `unclear` → offer to extend.
- **Words.** The verdict label and the "linked, not proven" tag come from code, not the model. A follow-up take's FactSet includes the `Outcome` numbers (n, means, diff, CI) so `validateTake` lets exactly those numbers through and nothing else.
- **Memory line.** A template over Trial, Adherence and Outcome rows ("On {date} I suggested {title}. You did it on {done} of {days} days; {metric} was {diff} ({ci}) — {verdict words}."). It is **not** model recall, so it can't misremember. The model may reference it in the take text only with numbers that are in the facts.
- **Tests.** Pure TS module `amaraTake/outcome.ts` with unit tests for each verdict, the exclusions, the confounder flag, determinism (same seed gives the same CI) and the causal-word guard. Reuse the mock numbers in `proposal/shots.cjs` as fixtures. The breathing trial is 67 / 60 / 58 with n 11 / 6 / 12, and the walk trial is 60 / 59 with n 12 / 7.

## 6. UI contracts (for Design's build after approval)
- `useAmaraTake(page)` grows a `recommendation: Wired<{ entry, trial, today: Adherence, whyText }>` and `followup: Wired<Outcome> | null`.
- New commands or hooks:
  - `markAdherence(trialId, day, state, reason?)`
  - `undoAdherence`
  - `takeHistory(page, { filter, q, before })` (paged, 20 per page)
  - `forgetTake(id)`
  - `trialAction(trialId, "keep" | "extend" | "stop")`
  - `listTrials()` for Patterns › Trials
  - `weekSuggestions(weekStart)` for Weekly review
- Each surface has its own Needs wiring state (`t8`). The take itself keeps working when the suggestion layer is unwired.
- Forgetting a take blanks its text and keeps the adherence rows, unless Taisun chooses "forget answers too". Then the trial result is recomputed and the next take says it changed.

## 7. Wiring order
1. **Take log (memory).** Append-only TakeLog, migration from `amara_take_<page>`, `takeHistory` and `forgetTake`, plus Settings › Memory › Takes retention (default 1 year, README Q4). UI: the History sheet (`t4`).
2. **Catalog and selection.** `catalog.json` with the starter entries and **verified** sources, the deterministic selection, the why-sentence prompt, the causal-word guard and the template fallback. UI: the suggestion block (`t1`, `t9`) and the empty states (`t8`).
3. **Trials and manual adherence.** The Trial and Adherence stores, `markAdherence`/`undo`, midnight unknown, and the Not-today extension. UI: `t2`.
4. **Detectors.** In order: WHOOP walk / easy day (NOOP rows already exist), check-in, Business ledger, Home Assistant (read only), then Muse breathing (needs a session type field).
5. **Outcome evaluator.** `outcome.ts` (pure, seeded bootstrap), interim and final scheduling, follow-up takes with Outcome facts, and the memory-line template. UI: `t3a`, `t3b`.
6. **Surfaces.** Patterns › Trials (`t7`), the Weekly review tile, table and trial-backed "One change" (`t5`), Goals "Ready to keep" / habit from a trial and the monthly re-check (`t6`).
7. **Gates.**
   - check:secrets
   - tsc
   - vitest (new outcome, selection, guard and detector tests)
   - cargo (if any Rust is added)
   - lint
   - e2e and axe serious + critical = 0 in both themes, contrast ≥ 4.5

   Then install once, gracefully, with the backup untouched.

## 8. Open decisions (README §5)
Trial length vs daily variety · auto-detect Done · Business suggestions on or off · 1-year take retention · "Let Home do it". Until Taisun answers, the defaults are: trials, auto-detect with Undo, Business on as "Your data only", 1 year, and suggest-only (no HA action).


## 9. Addendum: Business › Personal money (read-only accounts, conservative growth advice)
Mocks `m1`–`m6`. Everything in §2–§7 applies: take memory, one suggestion, trials, adherence and outcomes. This section adds the money sources, security and advice rules.

### 9.1 Sources (READ-ONLY; connection options need research before any build)
| Source | Likely route (to research) | Data | Notes |
|---|---|---|---|
| Chime | A US aggregator such as **Plaid** (Transactions + Balance products only; no Transfer, Auth-for-payments or Payment Initiation) | balances, transactions | Re-auth every ~90 days → `expired` state. Check whether Chime is supported and what aggregator consent costs |
| Fidelity | An aggregator (Plaid Investments, or Fidelity's own data-access program via an approved aggregator) | holdings, balances, fund expense ratios if available | Fidelity has restricted screen-scraping; use only a token-based, approved route |
| Japanese banks | (a) **monthly statement CSV import** (default, works everywhere), (b) a bank's own read API under Japan's open-banking (電子決済等代行業) rules, or (c) a licensed Japanese aggregator (for example, Moneytree LINK) | balances, transactions in JPY | Which banks is an open question (README Q7). CSV parsers are per bank; Shift-JIS encoding |
| Ledger | **Watch-only**: public addresses or xpub/ypub/zpub, read through a public block explorer or the user's own node (Electrum server / Bitcoin Core), plus a price feed | on-chain balances | **Never** a seed phrase, private key or Ledger Live login. No device connection is needed |
| FX rate | One daily JPY/USD reference rate (for example, a central-bank or ECB reference) | rate + timestamp | Shown with its time. Used only for display and the "FX moved it" split |

There is no open-source shortcut that logs in with his bank password. No screen-scraping, and no storing of bank usernames or passwords: OAuth or aggregator tokens only.

### 9.2 Security requirements
- **Scopes.** Request read scopes only. The build must include a test that fails if any aggregator product or scope with transfer, payment or trade ability is configured. There are no Tauri commands that move money, and no UI for it.
- **Secrets.** Aggregator client secrets and access tokens go in the **macOS Keychain only** (the 1111s rule). They are never in kv, SQLite, backups, logs, crash reports, chat, the router prompt or crew tools. The UI shows Set / Not set only. Backups exclude them (HANDOFF-1111s), and a restore asks him to reconnect.
- **Ledger input guard.** The watch-only field accepts only valid public-key or address formats. It detects BIP-39 word lists (12/15/18/21/24 words from the English or Japanese list) **before** anything is stored, then clears the field, never logs it, and shows the "treat it as exposed" warning. Unit test: a seed string never reaches any store, log or network call.
- **Data at rest.** Account nicknames, balances and transactions go in the local store with the same 0600 file rules as backups. Account numbers are never stored, beyond an aggregator's opaque account id. A transaction description goes to the local model only as category + amount, never full descriptions with names. `localOnly: true`, with no cloud path.
- **Crews.** Money data isn't available to crews or n8n unless Taisun turns that on per crew (and the 1110i Files/terminal switch doesn't cover it).
- **Staleness.** Each account has `lastSyncAt` and a status of `ok | stale | expired | error | not_connected`. Expired or stale balances render greyed with "as of {date}" and are **excluded** from today's suggestion (m6 #6).

### 9.3 Data model additions
```ts
type MoneyAccount = { id: string; nickname: string; institution: "chime" | "fidelity" | "jp_bank" | "ledger"; kind: "checking" | "savings" | "brokerage" | "ira" | "ordinary" | "time_deposit" | "btc" | "eth";
  route: "aggregator" | "csv" | "bank_api" | "watch_only"; currency: "USD" | "JPY" | "BTC" | "ETH"; scope: "personal" | "venture";
  status: "ok" | "stale" | "expired" | "error" | "not_connected"; lastSyncAt: number | null; keychainRef: string | null /* never the secret */ };
type Balance = { accountId: string; at: number; native: number; usd: number | null; fx: { rate: number; at: number; source: string } | null };
type Txn = { accountId: string; day: string; amount: number; currency: string; category: string; isTransferBetweenOwn: boolean };
type WatchAddress = { accountId: string; chain: "btc" | "eth"; kind: "address" | "xpub"; value: string /* public only */ };
```
**`scope`**: venture accounts (Taiyo Skin, the trading bankroll, receivables) are excluded from the personal total and from personal suggestions (README Q6). The "FX moved it" split is `ΔUSD − (Δnative × today's rate)`, computed only for JPY accounts.

### 9.4 Money catalog and outcome rules (extends §3 and §5)
- **Catalog order** (conservative; first match wins):
  1. Buffer under 3 months of median essential spend → build buffer.
  2. High-interest debt → pay it down.
  3. Fund fees over 0.2% → lower-cost equivalent.
  4. Savings rate under target → automatic payday transfer.
  5. Cash over the 6-month buffer for 3+ months → automatic transfer into a diversified fund he **already holds** (README Q9).
  6. Concentration: any single holding over 20% of investments → note diversification.
  7. JPY→USD needs → convert a fixed amount monthly; never time the rate.
  8. Unused subscriptions (1111a).
- **Never suggested:**
  - individual stocks
  - leverage or margin
  - options
  - crypto purchases
  - market or FX timing
  - tax or legal specifics, which get "ask a professional"
- **Required copy.** Each money suggestion has an `evidence` entry plus a `risk` sentence (required for anything invested) and ends with the fixed line "Not financial advice. Amara reads your accounts and suggests; she can't move, buy, sell or convert anything, and you decide." The validator rejects money text containing `guarantee|risk-free|sure thing|will go up|moon|get rich`, as well as the causal words.
- **Outcome metrics are behaviour only:**
  - savings rate (monthly, income-matched)
  - buffer months
  - annual fee estimate
  - concentration %
  - adherence (the recurring deposit exists)

  **Market returns are never a verdict metric.** They're shown as context, and judged only over 3+ years against a stated benchmark (m5). FX outcomes are always `unclear` by design.
- **Windows.**
  - Savings rate: 3 paydays (monthly), compared with the 3 months before, income-matched. If income differs by more than 15%, the verdict is `unclear`.
  - Fees: `better` ("locked in") on the first sync showing the switch, because fees are deterministic.
- **Partial data.** No suggestion if any personal account is `expired`, or if a `stale` account holds more than 10% of the personal total.

### 9.5 Wiring order (after §7 steps 1–3)
1. Research spike: confirm the routes for Chime, Fidelity and his Japanese banks (Q7), pricing and scopes, then write up the findings. **No sign-ups or connections without Taisun's OK.**
2. `MoneyAccount` store and Keychain refs, plus the read-only scope test.
3. Japanese bank CSV import (no third party needed). FX reference rate.
4. Ledger watch-only and the seed-phrase guard and tests. Explorer or own node, plus the price feed.
5. Aggregator for Chime and Fidelity (Link flow in the system browser, token to the Keychain), with the expired/re-auth flow.
6. Personal total, the scope split and "FX moved it" (`m3`). Money catalog, validator additions and behaviour-only outcomes (`m4`, `m5`). States (`m6`).
7. Gates as §7. Add a secrets scan of backups, logs and a fixture chat transcript for aggregator tokens and seed words.

## 10. Built (design-build-1111t, UI only)
Taisun approved 1111t on 11 Nov 15:32 JST (`BUILD-APPROVED.md`). His answers change the mock in these ways, and the build follows them:
- **A new suggestion every day, no 3-week trials.** The take shows "Today's one thing" with a *New today* pill and how often that type has come up. Patterns › **Follow-through** replaces Trials.
- **Follow-through groups outcomes by suggestion type over time** (periods table per type), with the same honesty rules: ≥ 5 days each way, 90% range + n shown, "better" only when the range clears the normal wobble, always "linked, not proven".
- **Amara may mark Done automatically, with Undo** (Settings › Memory › Takes toggle, default on). Every auto-Done shows what it saw ("WHOOP: 27-min walk…") and "Not right? Undo".
- **Takes are kept 1 year, then summarized into Memory** (History sheet footer + Settings).
- **No "Let Home do it" button.** Home suggestions are things he does; nothing calls Home Assistant.
- **Money:** may suggest adding to funds he already holds; USD with a JPY view toggle; Japanese banks by statement CSV import; venture funds outside the personal total; accounts live on Business › Personal; business suggestions from his own data only (evidence level "Your data only").
- **Placeholder citations stay marked** ("Placeholder citation · not checked yet") until a person sets `verified: true`.

### Files (all `src-console/`)
| File | What |
|---|---|
| `takes/adapters.ts` | `TakesAdapter` (`Wired<T>`): `today`, `memory`, `historyCount`, `history`, `answer`, `undo`, `forget`, `typeAction`, `outcomes`, `week`, `goals`, `settings`, `setAutoDone`, `detects`. `unwiredTakes` (default), `sampleTakes(preview)` (sample mode only), `CAUSAL` guard. Preview flag `localStorage nexus.takesPreview` (JSON: `today`, `answer`, `unwired`, `historyEmpty`) |
| `takes/TakeExtras.tsx` | `useTakeSlots(page, compact)`: memory line, "History · N takes ›", today's block (suggestion / follow-up / learning / none today / Needs wiring) |
| `takes/SuggestionBlock.tsx` | Suggestion with why + numbers + evidence (level, sources, risk, also considered) and `AnswerBar` (Done / Skipped / Not today, skip reasons, Undo, detected-Done) |
| `takes/FollowUp.tsx` | Follow-up take block and dot chart; "Make it a habit / Keep suggesting / Stop suggesting" only when better |
| `takes/HistorySheet.tsx` | Right-side sheet: All / With a suggestion / Follow-ups, search, by week, Forget a take (confirm), empty + unwired |
| `takes/FollowThrough.tsx` | Patterns › Follow-through: type list with verdict chips, chart, days table, periods-over-time table, Result / Method / Not claimed |
| `takes/WeekTakes.tsx` | Weekly review tile, "Suggestions this week" table, "One change" from the clearest result, takes this week; Goals "Ready to keep" and "Suggested lately" |
| `takes/TakesSettings.tsx` | Settings › Memory › Takes (1 year + summarize; auto-mark Done toggle) |
| `money/adapters.ts` | `MoneyAdapter` (`Wired<T>`): `accounts`, `overview`, `take`, `followUps`, `routes`, `addWatchOnly`, `importStatement`, `answer`, `undo`, `reconnect`, `disconnect`. **No method moves money** (test enforces names). `NFA_LINE`, `MONEY_BANNED`, sample FX ¥151.20 (sample only). Preview flag `nexus.moneyPreview` (`{scenario: "connected" \| "states" \| "none"}`) |
| `money/seedGuard.ts` | `guardWatchInput()`: refuses 11+ word phrases (English or kana), xprv/WIF/64-hex keys; accepts xpub/ypub/zpub, BTC and ETH addresses |
| `money/ConnectSheet.tsx` | m2: Chime / Fidelity (Needs wiring), Japanese bank CSV, Ledger watch-only. A refused phrase never enters React state |
| `money/MoneyView.tsx` | m1 accounts + can/can't + Ledger safety, m3 overview with USD/JPY, m4 money take (+ NFA line), m5 follow-ups, m6 states |
| `amaraTake/AmaraTakeCard.tsx` | New optional `slots` prop (memory, history, block, sheet); `AmaraTake` fills them from `takesAdapter()` |
| `personal/PageViews.tsx`, `personal/TodaySections.tsx` | Patterns chips Findings / Follow-through; PersonalMoneyView = money module + existing bills/subscriptions; Weekly review and Goals additions; Takes card under Settings › Memory |

Tests: `takes/takes.test.tsx`, `money/money.test.tsx` (seed never reaches adapter, log, storage or DOM; no money-moving method; banned + causal words; placeholder marks; unwired = Needs wiring with no numbers), `test/e2e/takes-1111t.spec.ts` (axe serious/critical = 0 incl. contrast, both themes, sample and unwired).

## 11. Wiring order for Claude (replaces §7 / §9.5 where they differ)
Every adapter is unwired in the live app today, so the take works exactly as before plus a "Today's one thing · Needs wiring" box. Replace `unwiredTakes` / `unwiredMoney` with live adapters; keep the `Wired<T>` shapes.
1. **Take log** (`TakeLog`, migration from `amara_take_<page>`, retention 365 days then a summary into Letta Memory). Wire `memory`, `historyCount`, `history`, `forget`, `settings`.
   - **Built (D-takes-hr):** Rust JSONL log (`commands/take_log.rs`), migration, retention computed in `takes/takeLog.ts`, live adapter wired for those five methods.
   - **TODO (not built):** the 365-day summary into Memory. Taisun: do not write into live Letta; keep it local/planned. Until then entries older than 365 days are only hidden, never deleted.
2. **Daily selection** (catalog with verified sources, one per page per day, why-sentence via router with the `CAUSAL` guard + template fallback, "own data only" for Business). Wire `today` (kinds `suggestion`, `learning`, `none_today`, `followup`).
3. **Answers** (`answer`, `undo`; midnight → `unknown`, never skipped; Not today excluded). Wire `setAutoDone`.
4. **Detectors** (WHOOP walk / easy day, check-in, business ledger, Home Assistant read-only, Muse breathing) → `answer` with `source: "detected"` and a `detail` string; `detects(page)` true once that page's detector exists.
5. **Outcomes by type over time** (`outcome.ts`, seeded bootstrap; `periods` per type). Wire `outcomes`, `week`, `goals`, `typeAction` (keep → habit in Goals with before/after numbers; monthly re-check).
6. **Money** (§9.5 steps 1–7): research spike first, no sign-ups without Taisun's OK. Then `accounts` + Keychain refs (UI shows Set / Not set only), JP CSV import (`importStatement`), FX reference rate (replace `SAMPLE_FX` with the live rate + its time), Ledger watch-only (`addWatchOnly`, only ever called with a guard-accepted public value; add the Rust-side guard + test too), aggregator for Chime/Fidelity (`reconnect` / `disconnect`), `overview` (venture scope excluded), money `take` (catalog §9.4, `MONEY_BANNED`, `NFA_LINE`, `partial` when any account is expired or a stale one holds > 10%), behaviour-only `followUps`.
7. Gates as §7. Note: no HA action and no money-moving command may be added; `money.test.ts` fails on adapter method names that look like transfer / pay / trade / convert / withdraw.
