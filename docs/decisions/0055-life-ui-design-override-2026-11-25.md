# 0055: Design boundary lifted for the Life host (Taisun, 2026-11-25, in session)

His prompt: **"Build the Life user experience on top of the approved Life domain services. Use the existing Trajectory page as the starting point. Do not create a duplicate Life route …"**

That conflicted with hard rule 10 (`.claude/rules/design-boundary.md`), so I asked. **His answer: "Override for Life."**

**Scope of the override:**
- **Covered:** the Life host only. That is `src-console/life/`, the `life` entries in `src-console/app/routes.ts`, and the Life screens' CSS, in the existing dark green visual identity.
- **Not covered:** every other screen, `src-console/styles/` tokens, and `ui/charts/`. Those stay Design-owned.
- **Stays stable:** the internal route id `life` and the `trajectory` redirect.

## Done in this session (backend slice)

Nine Tauri commands over the Life repository (0054), registered, bound in `boundTable.ts` and in the regenerated bindings:

| Command | Purpose |
|---|---|
| `life_update` | Set a goal's or habit's Life fields |
| `life_progress` | Status, the progress fraction (only with a baseline), and the history |
| `life_habit_history` | Each day in a range: done, not done, or no record |
| `life_areas`, `life_area_upsert`, `life_area_archive` | List, create or rename, and archive life areas |
| `life_link_add`, `life_link_remove`, `life_links` | Goal-to-project and goal-to-habit links |

The existing recommendation commands (0053) serve accept, modify, dismiss, explain (`recommendation_get` with its evidence) and recording the outcome.

## Not done: the UI itself

The UI build plan is `tasks/PLAN-0055-life-ui.md`. It is large (five destinations, timelines, a calendar, trends, a weekly planner, recommendation controls, proactive rules, and e2e in light and dark), so it is started in a fresh session from that plan.
