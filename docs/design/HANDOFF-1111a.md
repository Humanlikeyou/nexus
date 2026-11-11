# HANDOFF-1111a: personal-OS features — data needs (for Claude, only after Taisun approves the images)

Design builds every screen. Claude builds stores, commands and hooks, following the `Wired<T>` pattern: each hook returns `live | sample | unwired`; sample adapters are labelled "◇ Sample data" and only in mock mode; `unwired` shows "Needs wiring". Everything stays on the Mac (loopback, outbound off). Never fabricate a value: a missing source shows as missing.

## 1. Goals and habits
- **Store:** `goals` (`id, kind: habit|milestone, title, source: neuro|fitness|journal|business|manual, metric, op, threshold, target_per_week, due_date?, created_at, archived_at?`) and `goal_progress` computed, not stored, from sources.
- **Computation:** per day per habit: `met | nearly | missed | no_data` (nearly = within a tolerance, e.g. 15 min for bedtime). Streaks in days/weeks. Milestones: `current, target, pct` from Business (30-day revenue, launch task counts) or manual.
- **Hooks:** `useGoals(): Wired<Goal[]>`, `useGoalProgress(range = "week"): Wired<HabitWeek[]>`, `useGoalLine(page: "neuro"|"fitness"|"business")` for the one-line progress on each page.
- **Commands:** `goal_list`, `goal_upsert`, `goal_archive`. Manual tick only when `source = manual` or the source has `no_data`.

## 2. Health patterns
- **Inputs (daily rows):** sleep (duration, start), HRV (morning), resting HR, recovery, training effort, Neuro session summary (focus, calm, duration, start), check-in mood/energy/tags.
- **Engine (local, weekly, Sunday 03:00 JST):** for each configured pair (sleep→next session focus, effort≥70→next-day focus, HRV vs calm, tags→mood): with/without groups with `n_with, n_without, mean_with, mean_without, delta`, Pearson `r`, `p`, lag (0–7 days). Confidence 1–5 from n and p (≥3 days each side minimum, Bearable-style). Only publish findings with confidence ≥ 2.
- **Output:** `pattern_findings` (`id, pair, lag_days, window_days, n_with, n_without, mean_with, mean_without, delta, r, p, confidence, sentence, computed_at, source_ids[]`). The sentence is templated from the numbers (no LLM-invented numbers); an LLM may only rephrase with every number checked against the row.
- **Hook:** `usePatterns(window = 28): Wired<Finding[]>`; "See the sessions ›" needs `source_ids`.

## 3. Journal and mood check-ins
- **Store:** `checkins` (`id, at, kind: daily|moment, mood 1–7, energy 1–5, tags[], text?, voice_ms?, transcript_source: typed|voice, created_at, edited_at`). Tags table with user-defined tags.
- **Voice:** reuse the local STT path; Amara proposes mood/energy/tags from the transcript, Taisun confirms before save. Audio retention per decision 4 (default: discard after transcription).
- **Commands/hooks:** `checkin_save`, `checkin_list(range)`, `useCheckins(range)`, `⌘J` global shortcut opening the check-in sheet; reminder (decision 5).
- **Timeline:** `useWeekTimeline(start)` returns per-day `{ sleep_h, hrv_ms, focus?, mood?, energy?, notes[] }` for the Mind–Body Overview lanes.
- **Optional later:** write `mood` to HealthKit State of Mind (iOS 18 API) only if Taisun asks.

## 4. Personal money (USD), separate from Business
- **Separate store:** `personal_accounts`, `personal_transactions` (`date, amount_usd, amount_orig, currency_orig, merchant, category, account_id`), `personal_recurrings` (`merchant, amount, cadence, next_due, last_used_at?, status: scheduled|auto|due|missed`). **No shared tables or totals with Business/venture ledgers**; a test must prove Business numbers don't change when personal rows are added.
- **Derived:** month spend vs usual (median of last 6 months), bills due in 14 days, "left after bills until payday", unused subscriptions (no matching usage signal for ≥55 days — usage signal source TBD; if unknown, don't flag).
- **Source:** CSV import first (no bank connector until Taisun picks one); currency conversion rule needed (decision 9) — until then `amount_usd` is `unwired` for non-USD rows (`DataTag kind="currency"`, as on Business).
- **Hooks:** `usePersonalMoney(month): Wired<PersonalSummary>`, `useBills(days = 14)`, `useSubscriptions()`.

## 5. Memory page
- **Source:** Letta (`127.0.0.1:8283`) archival/core memory, read through a new Tauri command — do **not** edit live Letta data while building; test against a mock Letta.
- **Commands:** `memory_list({ category?, q? })` → `{ id, text, category: about|health|work|preferences|people, source: told|learned|fitness|patterns|business, created_at, updated_at }`; `memory_update(id, text)`; `memory_delete(id)` (hard delete from Letta + embedding); `memory_forget_since(date)` (confirm sheet); `memory_export()`.
- **Proposals:** `memory_pending()` / `memory_decide(id, keep: boolean)` for "Amara wants to remember" (decision 7). Nothing new is written until kept, for the gated categories.
- Deleting a memory doesn't delete chat transcripts — the UI says so; transcripts stay under Agents.

## 6. Life admin
- **Store:** `life_dates` (`id, title, category: residence|housing|akiya|taiyo|other, date, notes, source: taisun|document, reminders: [90, 30, 7], created_at`). No inferred legal deadlines: create only from Taisun's input or a document he attaches.
- **Reminders:** scheduled locally; surface on Home › Today, the lens, and a macOS notification. `reminder_ack`.
- **Natural-language add:** parse "Visa expires Sept 14 next year, remind me 3 months before" into a draft row → confirm sheet.
- **Hooks:** `useLifeDates(months = 12)`, commands `life_date_upsert/delete`.

## 7. Weekly review
- **Job:** Sunday 19:00 JST (decision 6) builds `weekly_reviews` (`week_start, health {avg_sleep, delta, hrv, workouts, recovery_avg}, work {focus_sessions, avg_focus, launch_tasks_done, crew_runs, crew_failed}, money {personal_spend, vs_usual, business_net_30d, decisions_waiting}, highlights[], notes[], suggestion {title, reason, evidence_finding_id}, decision: tried|skipped|null`).
- **Suggestion rule:** exactly one, taken from a published pattern finding (confidence ≥ 3) plus the coming week's calendar/Ventures load; the text quotes the finding's numbers. "Try it this week" creates a reminder (or a goal change, per decision 6); "Not this week" records the skip.
- **Hook:** `useWeeklyReview(weekStart?)`.

## Cross-cutting
- All times JST; all money USD display (decision 9 for conversion).
- Accessibility: every chart has an `aria-label` with the numbers; colour is never the only signal (labels on dots/bars).
- Tests: no fabricated numbers (every sentence number traceable to a row), personal/business separation, memory delete actually removes from the mock Letta, reminders fire once.

## Built (design-build-1111, approved by Taisun 2026-11-11 12:52 JST)
UI in `src-console/personal/` (`adapters.ts`, `TodaySections.tsx`, `PageViews.tsx`, `parts.tsx`, `personal.css`), shared panel styles `src-console/ui/PanelKit.css`. Live adapter is `unwiredPersonal`: every panel shows "Needs wiring" until its store exists; sample values only in mock mode.

Placements: Today › Goals (habits + milestones; falls back to the existing Milestones card when unwired), Today › Check-in, Today › Life admin, Today › Weekly review (moves to the top on Sundays, JST); Mind–Body › Patterns; Business › Personal (USD, walled off from venture funds); Settings › Memory. ⌘K: Check-in, Life admin, Weekly review, Memory.

Defaults chosen:
- Mood scale **5 steps** (Very unpleasant … Very pleasant), energy 5 steps. Check-ins are **text only**: voice is transcribed, no audio kept (transcription shows Needs wiring until built).
- Check-in reminder **22:00 JST**; weekly review **Sunday 19:00 JST**.
- "Try it" sets a reminder only; it never changes a goal.
- Memories about **health and people**, and inferred ones, wait for Keep / Don't keep. Delete asks once ("Delete for good").
- Life admin reminders at **90 / 30 / 7 days**. Real visa / residence dates come from Taisun.
- Patterns published at confidence ≥ 2, recomputed weekly.
- Personal money starts with a **CSV import**; non-USD rows show "Needs wiring · currency conversion" until Taisun gives the JPY→USD rule. No ¥ ever rendered.
- Not built: a1b (goal line on Neuro), a3b (timeline variant).

Wiring order (suggested): 1 check-ins store (+ transcription) → 2 goals/habits from Fitness/Neuro/check-ins → 3 memory (Letta list/edit/delete) → 4 life-admin dates → 5 personal money CSV import → 6 patterns job → 7 weekly review job (needs 1, 2, 5, 6).
