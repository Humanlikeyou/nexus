# 0018: Goal roadmaps catch up when idle (Taisun, 2026-11-13, in session)

Found: no roadmap was drafted overnight. Four goals sit in "drafting" and `lastRunOn` is empty: the job has never completed. The renderer job (`src/terrance/system/roadmapNight.ts`) runs only 02:00–05:00, and only on a scheduler tick with spare heavy budget and an idle activity bus. The Mac slept through almost all of that window on 2026-11-12/13 (activity log: nothing from 00:02 to 03:54, then nothing from about 04:00 to 05:19), and a missed night only waits for the next night.

His answer: **"Catch up when idle"**. If a night was missed and goals are waiting, draft at the next quiet moment, at any time of day:
- on AC power;
- no chat turn, crew run or Muse session for 10 minutes;
- the deep tier, single-flight, with the existing caps.

The 02:00–05:00 slot stays the first choice. Everything else in decision 0013 is unchanged.
