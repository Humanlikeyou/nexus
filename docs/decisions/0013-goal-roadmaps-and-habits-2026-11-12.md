# 0013: Goal roadmaps by Amara, and a real habit tracker (Taisun, 2026-11-12, in session)

His ask: the Goals page (habits and milestones) looks empty: research the best habit-tracker patterns and design that. When he enters a goal, Amara builds an interactive, high-reasoning roadmap to reach it; now and then one of her takes, anywhere in the app, asks whether he has done a step; Yes / Not yet; Yes raises the goal's percentage.

His answers:
1. **Model and timing:** the deep tier, local (no cloud), run **overnight while he sleeps** (a nightly job, e.g. ~03:00 JST, when no chat turn or crew run is active), so new or updated roadmaps are waiting in the morning as suggestions he can accept or edit. Never blocks the day.
2. **Check-ins:** at most **one step question per day across the whole app**, in whichever take he sees first; never repeats a step answered that day.
3. **Progress:** **weighted steps** (Amara sizes each step small / medium / big; he can edit sizes); % = done weight / total weight.
4. **"Not yet":** ask again in a few days; after two "not yet"s, offer to split the step into a smaller first move he can do today.

Constraints: local only (the deep tier through the Amara Router; Claude only if he has Allow Claude on, otherwise never); roadmaps and answers stored in app data (versioned, like other stores); nothing to Letta; the take question uses the existing 1111t answer/undo machinery; no invented progress (only his Yes moves it). The overnight job only runs while the app is running (renderer scheduler with background throttling off); if the Mac sleeps, it runs at the next opportunity and says so.

Build plan (next session): research note → Design Lead habit tracker (streaks, heatmap, today ticks, "never miss twice", habit sources) → backend `goal_roadmap` store + nightly deep job + step answers → takes integration (one step question per day) → tests and e2e.
