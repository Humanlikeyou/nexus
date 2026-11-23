# Handoff 0017: NOOP pages in Mind-Body > Fitness (round 1)

Built 2026-11-13 from `docs/noop/page-inventory.md` "top 10", items 1 to 3. Nothing was run in the live app and Playwright was not run (the parent runs e2e). Axe, contrast and the two themes are unverified visually.

## What shipped

Fitness now has a section bar under its hero: **Summary · Workouts · Sleep detail · Trends**. It is a `SegmentedControl` (radiogroup named "Fitness section"), the same control the Mind-Body and Settings bars use, so there is no new nav pattern. Summary is the screen exactly as it was. Nav labels and shortcuts are untouched. The bar only shows when the health store has data; the empty and error states of Fitness are unchanged.

| Tab | Source | What it shows |
|---|---|---|
| Workouts | `noop_recent_workouts` (days 7/14/30/90, backend max 90) | Sessions, active time, time in zones 4-5, one stacked bar of minutes per zone, then a list: sport, day and time (JST), duration, distance, kcal, avg/max HR, Effort, steps, and the zone bar for each session. A session with no zones says "No zone data for this session" and is left out of the total bar (the card says how many). |
| Sleep detail | `noop_hypnogram` + the health snapshot's recent days | Night picker (last 14 nights, default latest), hypnogram (rows: Awake, REM, Light, Deep; time axis bed to wake), stage breakdown bar with minutes and percent, tiles (time asleep, efficiency, resting HR, HRV), "Stage timeline as text" disclosure, then 14-night charts for time asleep (bars) and efficiency. |
| Trends | `noop_query metric_series`, all 9 keys, read once for 365 days | Metric picker (9), range Week/Month/3 months/Year, one chart with latest, delta vs average, usual band and hover readout, a caption (average, lowest, highest, days with a reading), plus an "All metrics" grid of 9 tiles with sparklines. Tiles are buttons (`aria-pressed`) that select the metric. Range changes slice the one read, so they cost nothing. |

## Layout and design-system decisions

- Solid `Card`s only, no blur. Hairlines are inset `box-shadow`. Spacing, type, radius and colour are tokens. New CSS is `src-console/fitness/fitness.css` (scoped `.fx*`); nothing in `styles/` changed.
- Zone colours: an emerald ramp (zones 1-3) into copper (zone 4) and `--data-recovery-low` (zone 5). Colour is never the only cue: each bar has minutes in text and an `img` label listing every zone.
- Hypnogram: stage is carried by the row, not the colour. Awake is copper so it stands out from the emerald stages. Every block has a 1px stroke. Text alternative: the SVG `aria-label` summary plus a "Stage timeline as text" list.
- Charts reuse `TrendChart`/`ChartHeader`, so axes, usual band, hover and text labels match the rest of Fitness. Every chart has an accessible name with min, max and latest.
- Focus: the tile buttons use the emerald 3px ring with 2px offset. Selected tile has a 2px inset emerald ring plus `aria-pressed`.
- Zero is a gap (not a reading) for HRV, resting HR, respiratory rate, SpO2 and sleep, matching "Last 14 days". Steps and active energy keep a real zero. Sleep minutes show as hours.
- Never invented numbers: no store reads as "Needs wiring" (DataTag), a failed read is a plain sentence with Retry (the backend's text is never shown), no rows is an honest empty state. Sample values only behind `nexus.sampleAdapters=1`, every panel tagged "Sample data", and sample mode makes no backend calls.

## Files

`src-console/fitness/`: `noopData.ts` (adapters and `useLoad`), `sampleData.ts`, `workoutsModel.ts`, `sleepDetailModel.ts`, `trendsModel.ts`, `PanelState.tsx`, `WorkoutsPanel.tsx`, `SleepDetailPanel.tsx`, `TrendsExplorer.tsx`, `fitness.css`, and tests `models.test.ts`, `noopData.test.ts`, `panels.test.tsx`. Edited: `src-console/screens/HealthScreen.tsx` (tab bar, wrapper around the Summary content, `fitnessDays`). E2E: `test/e2e/fitness-0017.spec.ts`, fixtures added to `test/e2e/fixtures/snapshot.ts` (`NOOP_WORKOUTS`, `NOOP_HYPNOGRAM`, `NOOP_SERIES_BY_KEY`; `responses()` itself is unchanged so other specs behave as before).

## Backend asks (not built)

1. **`sleep_summary` payload shape is not documented anywhere in the repo** (it is in the allowlist, `noop_query.rs:17`, but no type, fixture or test shows its fields). I did not consume it; the Sleep detail 14-night charts come from the health snapshot's daily rows (shape known). If `sleep_summary` carries per-night stage minutes, awake time or "sleep needed", someone should document it so the sleep debt, consistency and hours-vs-needed cards (inventory #8) can use it.
2. No wrapper is missing for these three tabs. A per-night stage read for nights older than the hypnogram's picker is covered by `noop_hypnogram(day)`.

## Next tabs (inventory top 10)

4. **Today tiles**: Key Metrics (RHR, HRV, resp, SpO2, skin temp, steps) from the snapshot. Easiest next, no new data. Could sit at the top of Summary.
5. **Live heart rate**: a card from `live_heart_rate` (50-sample trace) with the same unwired/live states.
6. **Week in Review**: client-side digest from `metric_series` (the Trends data hook can be reused; lift `loadTrends` into a shared cache).
7. **Day view**: one day's metrics, sleep and workouts together. Reuses the Workouts and Sleep detail models.
8. **Sleep consistency / debt / hours vs needed**: blocked on confirming the "needed" source (ask 1).
9. **Data freshness / sources panel** from `data_freshness`.
10. **Settings > Devices**: NOOP source, store path and battery history (extends `NoopSettingsGroup`).

Workout detail (HR trace per session) is the natural follow-up to the Workouts list: the range query exists at `noop.rs:1234`, but it needs a read command exposed to the bridge.

## Known and unchecked

- The `.m-warn` contrast bug is not touched.
- Not verified: axe in both themes, the real-glass look on the Mac, narrow-width layout. The year range draws 365 slots; it uses a 7-day smoothing line past 90 days, so check that it reads well.
- `Tab` order inside the tab bar follows `SegmentedControl` (roving tabindex, arrows move and select).
