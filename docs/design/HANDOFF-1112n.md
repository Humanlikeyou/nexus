# HANDOFF 1112n: Neuro redesign and Hemi-Sync (Design Lead, 2026-11-12)

Taisun's ask: Neuro looks underwhelming next to the other pages and doesn't share their charts; add the Hemi-Sync collection (listen while the Muse records, see which tracks were done and their data). Decision: `docs/decisions/0010-hemisync-neuro-2026-11-12.md`. Contract used unchanged: `src-console/neuro/hemisync/adapters.ts`, `hemisyncAdapter()` from `live.ts`.

## Why Neuro looked underwhelming (before)
Checked against Mind–Body Overview and Fitness, with before shots from the mock.
1. No charts at all by default. Neuro's only trend was a 60 s sparkline inside a collapsed "Live readings and signal" group, and "Recent sessions" was also collapsed. The first screen after the hero and the take was a goal picker. The other pages open on lede sentences and `ChartHeader` + `TrendChart` rows.
2. Different page grammar. Neuro was boxes (Card, StatTile, InsetGroup) with its own padding and max-width; Mind–Body is sections (`mb-section`: hairline, title, one-sentence lede, header-over-chart rows) inside `page-inset`.
3. Real session data was never plotted. `data.sessions` and the recordings adapter held focus, calm, time in target and per-session band series (`SessionGraphs` existed but wasn't mounted on Neuro), yet only a text list showed.
4. The two pages' honesty conventions (usual range, delta words, tags) were missing.

## What changed
- `screens/NeuroScreen.tsx`: root is now `neuro page-inset` (padding and max-width come from the shared page rule). Adds `sessionId` prop. New order: hero, Amara's take, session flow (unchanged), live readings (now open while streaming), **Hemi-Sync**, **Done with the Muse**, **Focus and calm over time**, **Band powers · latest session**, Recent sessions (no longer collapsed). All existing connect/scan/start/stop/training behaviour and tests untouched. AUX lock (`AUX_PROVEN`) and every recording delete guard untouched.
- `screens/NeuroTrends.tsx` (new): Focus, Calm and Time in target rows (`ChartHeader` + `TrendChart`, your usual band, delta words) from `recordings.list()`; latest session's band powers via the existing `SessionGraphs` from `recordings.series()`. Needs two scored sessions; otherwise says so.
- `screens/HemisyncSection.tsx` (new): wave-grouped library (collapsible, track titles, minutes, "Done n×"), Import button with `importProgress()` polling, track pick, one Start, rating sheet, and the "Done with the Muse" section (focus/calm charts across Hemi-Sync sessions + per-track table: sessions, last date, avg rating, focus, calm).
- `screens/useHemisyncRun.ts` (new): the one Start flow. Audio is an HTML `Audio(track.src)`.
- `screens/NeuroSections.css` (new, tokens only, class prefix `hs-`); `NeuroScreen.css` drops its own padding/max-width.
- `app/useMuseSession.ts` + `App.tsx`: the Muse session id is now allocated when the recording starts (was generated at save) and exposed as `sessionId`; the saved session reuses it. Needed so `linkSession` can be called while recording. Same id format, same value written to the saved session.

## The Start flow
1. Pick a track, press Start. Not connected: explains ("Connect your Muse first…"), starts nothing (the button is `aria-disabled`, so it stays clickable and can explain).
2. Audio `play()` runs from the click (so the browser allows sound). If it fails, the Muse is not started. Then the existing `onStart` starts the Muse. If the protocol isn't "Recording only" it is switched to it first (Hemi-Sync records only; no feedback sounds on top of the track).
3. When the recording has a session id: `linkSession` once. If a Muse session is already running, the track joins it.
4. Track end: stops the Muse (existing `onStop`) and `finishSession(completed: true)`. Stop button or "Stop session": audio stops, `finishSession(completed: false, listenedMs)`. Muse fails to start: audio stops, nothing linked, message. Leaving the page mid-track: audio stops and `finishSession` is called; the Muse recording carries on.
5. Then a sheet (1–5, optional note) calls `rate`. Skip leaves it unrated.
If any write returns unwired, the page says the link isn't saved yet (Needs wiring tag) instead of pretending.

## States covered
Unwired ("Needs wiring", no numbers), sample (tagged only where the value state is "sample"), live (no tags), empty library (import prompt), importing (progress bar and "n of m · phase"), starting/playing (position bar), no Muse, rating sheet, track end, Muse failed, unsaved link.

## Needs from backend (not done by Design)
- `live.ts` is still the unwired/sample picker; the Hemi-Sync section is wired to it and will light up when the live branch lands. `HemisyncTrack.src` must be playable by an `<audio>` element (asset protocol, `<app data>/hemisync/**`). `durationMs` may stay null.
- `finishSession` is also called on page leave and Muse-end, so it must accept being called for a session whose rating comes later, and tolerate a second call.
- No contract field was missing. One request: `stats()` and `sessions()` should reflect a `finishSession`/`rate` write immediately (the page re-reads after each write).
- `src-console/app/useMuseSession.ts` change touches the recording start path: please have the security reviewer glance at it with the muse-recordings rule in mind (no file is moved, rewritten or deleted).

## Not covered
- axe was run on the four resting states (unwired/sample × Muse/no Muse, both themes). The playing state and the rating sheet are covered by vitest logic tests only (no real audio in the mock), not by axe.
- Real audio and the real Muse: not run. Live app: not driven.

## Open questions for Taisun
1. Hemi-Sync switches the training protocol to "Recording only" when you press Start. OK, or should it keep your last goal?
2. Waves with 7 groups collapse by default (wave 1 open). Prefer everything open?
3. Should "Done with the Muse" also list tracks you haven't done yet, greyed, as a checklist?

## Answered (Taisun, 2026-11-12)
1. Start switches to "Recording only": keep.
2. Waves: the wave of the most recent Hemi-Sync session opens (Wave 1 until any session); others collapsed.
3. "Done with the Muse": a checklist of every track by wave with done ticks and "N of M done" (built).
