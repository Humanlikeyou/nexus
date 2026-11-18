# 0035: Fitness Recovery card says why today has no score (Taisun, 2026-11-18, in session)

Question: Overview (Mind–Body) falls back to the latest day that has a score (up to 2 days old). Fitness › Recovery shows today only and says "Arrives after the sleep record closes". On 2026-11-18 there was no score at all, because the night came through as heart rate only. The options were (1) Fitness also falls back to "latest reading", or (2) Fitness says why it's empty and links to the Needs-you note.

His words: **"2"**

Scope: the Design Lead makes a small change to the Fitness Recovery card's empty state, logic and copy only. It uses the existing `strap-hr-only` Needs-you pointer (`needs_you_list`). No fallback to an older day.
