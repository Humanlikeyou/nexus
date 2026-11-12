# 0010: Hemi-Sync sessions in Neuro, and a Neuro redesign (Taisun, 2026-11-12, in session)

His ask: the Neuro tab looks underwhelming next to the other pages; bring it to the same layout, charts and feel. Add his Hemi-Sync collection (`~/Desktop/Hemisync Collection`, Waves 1–7, 42 tracks, ~2.1 GB) so he can listen while recording with the Muse; track which tracks he has done with the Muse and their data; feed it to Amara.

His answers:
1. **Files:** copied into the app (`<app data>/hemisync/`), not played from the Desktop. The source folder is only read, never changed.
2. **Flow:** one Start button: pick a track, Start begins the Muse recording and the audio together; both stop when the track ends or on Stop.
3. **Saved per session:** track, wave, date, how much was listened, the Muse metrics, plus a 1–5 rating and an optional note afterwards.
4. **Amara:** local context only (her context sections and takes on this Mac). Nothing is written to Letta.

Constraints kept: Muse recordings are sacred (no delete/rewrite path; a Hemi-Sync link lives in its own store and never edits a recording). Audio is served only from `<app data>/hemisync/**` through the Tauri asset protocol (capability change: security review). No new outbound host. The Neuro redesign is done by the Design Lead; the backend never restyles UI.
