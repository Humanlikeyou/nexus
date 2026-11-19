# 0045: Improvement-cycle brief, before the merge (Taisun, 2026-11-19, in session)

His words (verbatim): **"I want you to work on this first then after all of this is done I'll merge"**. Also: "This prompt was made without full knowledge of the project that I'm building so if somethings seem way off or redundant, let me know and we'll adjust."

- **Merge order.** Decision 0044's merge waits until this cycle is done. He runs the merge himself after that.
- **Design boundary.** The brief explicitly asks for the visual work below, so it counts as his OK under `design-boundary.md` for these items only. Chart redesigns still need a mock-up approved first (his requirement 9).
- **Hard rules still apply:**
  - no push
  - no new outbound hosts without his OK (this affects bank integration)
  - `NEXUS_OUTBOUND_DISABLED=1` for QA
  - Muse recordings are sacred
  - no destructive migrations (his requirement 10)

## The brief (summary, his headings)

1. **Amara's visual presence.** The sand circle gets three voice states:
   - Listening: keep as it is.
   - Thinking: swirling, gathering, reorganizing sand.
   - Speaking: organic, sonic waveform patterns driven by her real voice, rhythm and intensity, not a generic visualizer.
2. **Today.**
   - Redesign the Amara Take graph in Three.js.
   - Keep the check-in. Add: if he hasn't checked in by a configurable time, Amara asks on Telegram and turns his reply into the app check-in.
   - Telegram and the app stay in sync whichever side he starts from.
3. **Life.**
   - Habits become commitments Amara supports. She learns what and why, sets a realistic routine, checks in, reminds, recognizes consistency and helps him recover when he falls behind.
   - Audit Dates & Deadlines: its purpose, its actions, and how it connects to goals, habits, reminders and Amara.
4. **Personal Money: a major expansion.**
   - Dashboard: available money, monthly income, monthly expenses, savings, spending breakdown, savings progress, goals, cash flow.
   - Manual entries feed the dashboard and charts.
   - Bank connection is optional; manual entry must be fully functional on its own.
   - Interactive charts.
   - Amara checks in periodically about money, updates the records from his answers, and gives strategic recommendations. These must be grounded in recorded data, with assumptions labeled.
5. **Mind & Body.**
   - Find and fix the extra ~0.5 s load time without losing function or visual quality.
   - Explore Three.js versions of the Readiness, Sleep, Trends Against Your Usual and Training Load charts. The current Readiness chart is the visual reference.
   - **Show mock-ups first. Don't replace any chart until he approves.**
6. **Fitness summary.** Amara Take becomes a varied recommendation engine:
   - calisthenics, bodyweight strength, mobility, flexibility, recovery, endurance, conditioning, progressive challenges
   - aware of readiness, sleep, recovery, load and history
   - no repeats
7. **Neural.**
   - Redesign the Last Session and Now circles in Three.js. They should be scientific, clean and readable, not decorative.
   - Make Find Muse compact without losing function.
   - Mark Wave One (Discovery) complete everywhere.
   - Make the Done With Muse card useful, or remove it if it's redundant.
8. **Enterprise.**
   - Add manual entry.
   - Audit the page and redesign it around a clear workflow: what it's for, what goes in, what he can do, how the sections connect, and how Amara helps.

## His implementation requirements

1. Audit before changing anything.
2. Preserve working features.
3. Every control has a purpose.
4. Information flows between pages, services and Telegram.
5. No isolated features.
6. Reuse existing infrastructure.
7. Reliability and performance come first.
8. Use Three.js only where he asked for it.
9. Show mock-ups before redesigning charts.
10. Keep data safe; no destructive migrations.

He also asked for: dependencies identified, the approach explained, and work split into completed, needs his approval, and needs more infrastructure.

## His answers (2026-11-19)

- **Neuro rings:** "Brain data (Recommended)".
  - Last session: band power profile plus focus and calm against his usual.
  - Now: live Muse bands while connected; hidden otherwise.
- **His words on the other three:** "For the remaining three questions I'll take your recommendations as well".
  - **Telegram:** replies go through Hermes. Nexus sends the nudge. Hermes gets a tool that saves a reply as a check-in, money update or habit update. The Hermes change is a patch in `~/projects/hermes-patches` plus a step in `TAISUN-STEPS.md`, applied by Taisun (hard rule 5).
  - **Wave One:** a separate per-track "marked done" flag that every view reads. No fake sessions.
  - **Take graph:** the heart-rate trace in the heart-rate take card. Mock-up first.
- **His words:** "After you're done with everything commit and reinstall, I'm going to bed". This is the OK to commit locally and install when the gates pass. Chart replacements still wait for his mock-up approval, so only the mock-ups get built. No push and no merge. The plan is in `tasks/PLAN-0045.md`.
