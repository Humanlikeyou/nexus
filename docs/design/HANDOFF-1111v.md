# HANDOFF-1111v: Life section, a short Today, and Business pages

**Status:** BUILT (build-1111v, see §9). Approved by Taisun at 19:23 JST, Nov 11 ("approved, you don't need another mockup"), with the sidebar name **Enterprise** (not "Business" or "Work"). The mocks are in `shots/` and `compare/`; built shots and compares are in `/workspace/nexus/build-1111v/`.
- **Base:** amara-integration accf8f6.
- **Scope:** UI only, in `src-console`. Backend items are listed in §7 for the Nexus Lead.
- **Names (Taisun's):**
  - sidebar section **Life**
  - Business money tab **Finances**
  - **Weekly review** stays
  - the Sunday card on Today is **"Your weekly review is ready ›"**

  Everything else follows the rename table in `PROPOSAL.md`.

## 1 · Structure
| Where | Pages / tabs |
|---|---|
| Sidebar | Home ⌘1 · Agents ⌘2 · Mind–Body ⌘3 · **Life ⌘4** · *Work* Business ⌘5 · *System* Settings ⌘6, Smart home ⌘7 |
| Home › Today | Card C *Your body today* (1111u, unchanged, still first) · Sunday only: *Your weekly review is ready ›* · **Needs you today** · Check-in · **To-dos** (was Lists) · the *Agents, schedule and insights* disclosure |
| Life (new host, tab bar like Business) | **Goals** · **Weekly review** · **Dates & deadlines** (was Life admin) · **Personal money** (from Business) |
| Business | Overview · **Decisions** (new) · Ventures (each venture its own page) · **Finances** (was Money) · **Tasks** (was Work) · **Content** (new tab) |

**Implementation notes:**
- **Life host:**
  - `SYSTEM_GROUPS`: add `{ id: "life", label: "Life" }` after Mind–Body in the first group.
  - `HOST_VIEWS.life`: `goals` · `weekly-review` · `dates` · `money`.
  - `ModuleId`: add `"life"`.
  - Landing buttons follow `SYSTEMS`, so Life shows there too.
- **Shortcut change:** inserting Life renumbers ⌘4–⌘6 to ⌘5–⌘7. If he wants Business kept on ⌘4, put Life at ⌘7 instead; this is a one-line change.
- **Tab-bar style:** shell.css `.subview-bar[data-host="business"]` → `:is([data-host="business"],[data-host="life"])`.
- **Removed from Today:**
  - the Goals, Life admin and Weekly review sections
  - the Priorities card (its items become Needs you today rows from To-dos and Tasks; see §3)
  - *Revenue today* (Business owns money)
- **Hero:** title Today, and the detail line reads "<date> · N things need you".
- **Card C** stays at the top of Today and under the Home clock (1111u, untouched).

## 2 · Route renames and redirects (old routes keep working)
| Old route / alias | New route | Redirect |
|---|---|---|
| `terrance:goals` · alias `trajectory` | `life:goals` | `ROUTE_ALIASES.trajectory → life/goals`. `terrance/goals` resolves to `life/goals`. |
| `terrance:weekly-review` | `life:weekly-review` | Anchor removed from `SECTION_ANCHORS.terrance`, plus a redirect. |
| `terrance:life-admin` | `life:dates` | redirect |
| `business:personal` | `life:money` | redirect, and stored `business` view "personal" maps to it |
| `business:revenue` · alias `revenue` | `business:finances` | The view id `revenue` stays as an alias (1110d note). The label changes to Finances. |
| `business:work` | `business:tasks` | redirect |
| *(new)* | `business:decisions`, `business:content` | — |
| `business:ventures` + venture detail | `business:ventures/<id>` | The venture becomes its own route with a back link. The old in-tab detail state maps to it. |
| `lists` · `terrance:lists` | unchanged route, label **To-dos** | The ⌘K label changes to "Today › To-dos". |
| ⌘K entries | Today › Life admin → Life › Dates & deadlines · Today › Weekly review → Life › Weekly review · Business › Money → Business › Finances · Business › Work → Business › Tasks · Business › Personal → Life › Personal money · add Business › Decisions and Business › Content | |

Add a `routes.test.ts` case for every old → new pair.

## 3 · Data rules (from the Nexus Lead, must follow)
- **Personal money moves, its contract doesn't.** The 1111t Money contract is unchanged, and HANDOFF-1111t §10–11 still apply:
  - take
  - daily recommendation
  - fund suggestions only for held funds
  - USD and JPY
  - JP CSV import
  - Ledger watch-only
  - Keychain refs

  Only the mount point changes (`life:money`). Two copy edits: remove the "Business › Personal" eyebrow and the "Kept apart" banner, and add a link to Business › Finances.
- **Finances** reads `read_business_ledger` only and stays read-only. Personal and business money never share a card, a total or a take.
- **One take and one recommendation per page.** They move with their screen and are never duplicated:

  | Page | Take | Recommendation |
  |---|---|---|
  | Home | heart-rate take (1111u) | Home one thing |
  | Today | card C take | — |
  | Personal money | Money take (1111t) | Money one thing |
  | Business Overview | Business take | Business one thing (its copy now points to Business › Decisions) |
  | Weekly review | keeps its "One change to try" | |

  Goals, Dates, Decisions, Tasks, Content and Finances carry **no** take.
- **Needs you today** is the single inbox. It **only lists**: each row is a pointer (`source`, `id`, `title`, `due`, `reason`, `deepLink`) to an item that lives in one source. Sources:
  - To-dos: overdue or due today
  - Dates & deadlines: inside a reminder window
  - Check-in: not done today, after the reminder hour or as a quiet row
  - pending Decisions
  - crew approvals (R13-D)

  No inbox-only items, and no actions except "Open in … ›". When an item is done at its source, the row leaves with a short "Done in To-dos · Undo".

  States to cover (`n02`):
  - loading (skeleton)
  - all clear (shows the next date)
  - partial (a source failed: "didn't load" + Try again; the rest stays current)
  - done at the source
  - an unwired source (disabled, with the reason)
- **Unwired actions are disabled with a reason, never removed.** The exception is features gone with OpenClaw, which stay removed. Current dead buttons:

  | Button | Becomes |
  |---|---|
  | Vote yes/no, tax flag Open | wired to the decisions log (§4) |
  | Draft with crew | disabled: "Needs wiring · crew drafts from a venture" |
  | Open vote | the link "Open in Decisions ›" |
  | Ask the accountant | disabled: "Amara doesn't contact anyone outside" |
  | Publish | disabled: "Amara doesn't post for you" |

## 4 · Decisions log (new, separate from the ledger)
- **What a record holds:**
  - `id`
  - `subject` (vote, or tax flag)
  - `choice` (yes, no or handled)
  - `note?`
  - `decidedAt` (JST)
  - `proposer` (crew or venture)
  - `sourceLink` (deep link)
  - `outcome?` and `outcomeAt?`, filled in later through "Add outcome"
- **Rules:**
  - Append-only.
  - Undo within 8 s deletes the record. After that, a correction is a new record.
  - **No money moves and no outside actions.**
- **Flow (`n21`–`n23`):**
  1. Tap Vote yes.
  2. An inline preview shows choice, time, proposer, source and an optional "why".
  3. Tap Record vote.
  4. The row leaves Waiting, the log row is highlighted with Undo, and an "Also shows on Taiyo Skin › Milestones" link appears.
- **Where decisions show:** the venture page and Needs you today read this log; nothing copies it.
- **Business recommendation:** "Decide before noon" counts itself when a log record lands before 12:00. This replaces "recorded in the Business ledger".

## 5 · Adding things: one pattern on every page
These pages have an add: **Goals** (New goal or habit), **Dates & deadlines** (Add a date), **Tasks** (Add a task) and **Content** (New idea). To-dos keeps its add but should move to the same component. The pattern:
1. **Input:** typing on the page comes first. Use one composer at the top of the list it adds to: a label, a free-text field, a mic and Add.
2. **Working:** the line appears within 100 ms ("Amara is reading your goal…"), and the field and button are disabled.
3. **Preview:** it shows the parsed fields, then Save / Edit / Cancel. For dates, Amara asks when unsure ("Which year?") and never guesses a legal date. A business date typed in Life goes to Business › Ventures.
4. **Confirm:** the saved item is highlighted in its list, with **Undo for 8 s** and an "Also shows in … ›" link.
5. **Error:** the text is kept, with Try again.
6. **History:** a "History · N changes ›" sheet per page. Use the 1111t takes sheet component.

**Chat and voice** call the **same command** and render the **same preview card**, inline in chat. Voice is transcribed **on this Mac only**.

| Add | Command |
|---|---|
| Dates | existing `life_date_upsert` |
| Goals, tasks, content, decisions | need commands; the names are the Lead's call. Use pure `UNWIRED` adapters until they exist, so `commands.registered.test.ts` stays green. |

## 6 · Components and tokens
- **Reused:** `ui-card`, `ui-list-row`, `ui-inset-group`, `ui-badge`, `status-text`, `data-tag`, `pk-table`, `take` / `t-rec`, and the Business tab bar.
- **New and small:** `Composer`, `WorkingLine`, `PreviewCard`, `ResultLine` (with Undo), `InboxRow`, `LaneTimeline` and `Kanban`. Their look is in `proposal/mock.css`.
- **Tokens and spacing:**
  - Fibonacci spacing; 1 : 1.618 two-column grids (`v-g2`).
  - Text ≥ 11 px.
  - App colour tokens only. Lane colours use `--sys-teal` / `--sys-indigo` / `--label-secondary`.
  - No new accents.
- **Sample and wiring tags:** sample data is tagged **Sample data** in mock mode only; unwired parts are tagged **Needs wiring**. Never show invented live numbers.

## 7 · Backend asks (Nexus Lead)
1. A decisions-log store and commands: append, undo within 8 s, add outcome, list.
2. A Needs-you-today aggregator, a read-only pointer list over the 5 sources, or the UI does the merge.
3. Add commands for goals, tasks and content ideas, returning the parsed preview before save.
4. Crew approval items (R13-D) exposed as list pointers.

## 8 · Tests to add when building
- **Routes:** every old → new redirect, plus a ⌘K labels snapshot.
- **Today:** card C is first, Needs you today second, and no Goals, Life admin or Weekly review sections. Sunday shows the ready card.
- **Inbox:** rows only link (no inline mutations); each state renders.
- **Personal money under Life:** the 1111t money tests run unchanged under the new route.
- **Decisions:** Record vote writes one log record and moves no money (no ledger write is invoked); Undo removes it.
- **Composer:** a state machine test for idle → working → preview → saved/undo → error.
- **e2e:** an axe sweep of all new pages in dark and light.

## 9 · Built (build-1111v, branch design-build-1111v)
**Base:** amara-integration accf8f6. UI only, in `src-console` (plus e2e specs and `docs/design/HANDOFF-1111v.md`). No src-tauri, memory/, Letta, ~/.hermes or LaunchAgents changes. The commit hash, install time (JST) and pid are in `/workspace/nexus/build-1111v/README.md`.

**Gates (before install):**
- `check:secrets`: ok
- `tsc --noEmit`: ok
- vitest: 1419 passed, 9 skipped (includes `test/design/scale.test.ts`, 67 passed)
- neuro: 21 passed
- cargo test: 476 passed, 5 ignored
- lint (`eslint src-console`): ok
- e2e on the box (axe, contrast ≥ 4.5, both themes): 695 passed, 12 skipped on the full run; 1 known flake in `route-motion` (light, “last click wins”) failed once and passed on rerun.

**Finishing fixes (not in the first worktree drop):**
- `test/smoke/screens.smoke.test.tsx` now pre-warms Life, uses Enterprise (not Business) and covers the Life / Enterprise tabs.
- Sidebar untitled groups use a unique React key (`group.systems` ids), so Life + Enterprise no longer share `key="main"` with Home / Agents / Mind–Body.

**Taisun's changes from the mockups (19:23 JST):**
- The sidebar section "Business" is now **Enterprise** everywhere: sidebar, landing button, h1 of Overview, ⌘K, the take label, the read-only lane in Dates & deadlines, and copy ("Enterprise › Decisions", "Enterprise › Finances").
- Tabs unchanged: Overview · Decisions · Ventures · Finances · Tasks · Content.
- Defaults: Life ⌘4, Enterprise ⌘5, Settings ⌘6, Smart home ⌘7 (the hints follow sidebar order).
- **Akiya is personal.** It is filtered out of Enterprise › Ventures (sample and live, `PERSONAL_ITEMS` in BusinessPanels) and its offer deadline (Dec 18) is in the **Personal** lane of Life › Dates & deadlines, not the Enterprise lane.
- The evening check-in is a **quiet row all day** in Needs you today. It is not counted in "N things need you".
- Names kept: Weekly review, Finances.

### 9.1 Route renames (all go through `canonicalRoute()` in `navigate`, so every old deep link, stored view and ⌘K path still lands)
| Old | New |
|---|---|
| module `business` (any view) | module `enterprise` (same view, then the rows below) |
| `business:main` | `enterprise:main` (Overview) |
| `business:revenue`, `business:money`, alias `revenue` | `enterprise:finances` |
| `business:work` | `enterprise:tasks` |
| `business:personal`, `enterprise:personal` | `life:money` |
| `business:ventures/<id>` | `enterprise:ventures/<id>` (venture page is its own route; `ventureRoute(id)`) |
| `terrance:goals`, alias `trajectory` | `life:goals` |
| `terrance:weekly-review` | `life:weekly-review` |
| `terrance:life-admin` | `life:dates` |
| `life:main` | `life:goals` |
| `life:personal-money` | `life:money` |
| `terrance:lists` | unchanged; label **To-dos** |
| sessionStorage `amara.business.view` = revenue/money | restores Enterprise › Finances; a stored "personal" is ignored |

⌘K: the old names still find the page (`keywords`: "revenue", "work", "life admin", "trajectory", "lists", "business"); a query of 3+ letters that starts an old name ranks that page first, so "rev" still opens Finances. New entries: Today › Needs you today, Life › *, Enterprise › Decisions / Content. Gone: Home › Goals, Today › Life admin, Today › Weekly review, Business › *.

### 9.2 What was built
- **Today:** hero "Today" · "<date> · N things need you" (the count links to Needs you today); card C first (untouched); Sunday-only "Your weekly review is ready ›" card (opens Life › Weekly review); Needs you today; Check-in; To-dos; the "Agents, schedule and insights" disclosure. Priorities, Revenue today, Goals, Life admin and Weekly review sections are gone from Today.
- **Needs you today** (`inbox/needsYou.ts`, `inbox/NeedsYouToday.tsx`): the UI merges pointers from To-dos (`list_followups`, overdue/due today), Dates & deadlines (reminder window; Enterprise lane excluded), Check-in (quiet), Decisions (decisions log), Amara approvals (`actions` awaiting approval, live) and crew approvals (R13-D, unwired). States: loading, all clear, a failed source with Try again, done at source, unwired source disabled with its reason. Rows only link.
- **Life:** Goals (composer, "This week's habits", "Ready to become a habit"), Weekly review (Suggestions done tile, "Suggested lately · not goals yet", Journal), Dates & deadlines (composer, three-lane timeline Personal / Japan · residence / Enterprise read-only, Upcoming table), Personal money (1111t contract unchanged; Kept-apart banner and Business › Personal eyebrow removed; link to Enterprise › Finances).
- **Enterprise:** Overview (take + Decisions/Tasks/Content link cards), Decisions (Waiting, inline preview "Nothing moves money. Nothing is sent.", Record vote, Undo 8 s, log table, Add outcome), Ventures (no Akiya; venture page shows "Decided:" rows from the log in Milestones), Finances (read-only ledger, "In Decisions" link card replaces the funding/tax cards), Tasks (composer + In progress / Waiting / Backlog / Done), Content (composer + Idea → Published kanban).
- **One add pattern** (`adds/`): Composer → working line → preview (Save / Edit / Cancel, "Which year is it?" for dates, business dates sent to Enterprise) → saved with Undo 8 s and "Also shows in … ›" → error keeps the text. History sheet per page. Mic is disabled ("Needs wiring · on-device transcription").
- **Mock gaps fixed:** (1) Sunday: the e2e fixture takes `openApp(page, { now })`, and the Sunday shot/spec runs at 2026-10-25 14:00 JST, so the date line reads "Sunday, Oct 25". (2) Crowded lane labels: `life/timeline.ts placeLabels()` stacks labels in rows with leader lines, end-anchors near the right edge and merges same-day labels ("A + B"); an e2e test checks no two labels overlap.
- Landing grid: 7 buttons in one row (`repeat(7, auto)`).

### 9.3 Deviations from this handoff (for the Lead / Taisun)
1. **Done at source:** the row leaves with "Done in To-dos ›" (a link), **no Undo**, so the inbox never mutates anything. Undo stays at the source (To-dos).
2. **Sidebar grouping:** Life and Enterprise share an untitled group after Home / Agents / Mind–Body (no "Work" header), then *System* Settings / Smart home.
3. **Extra inbox source:** Amara approvals (`actions` awaiting approval) are live pointers to Home.
4. **Ledger funds still list Akiya** in Enterprise › Finances because that table is ledger data, and the money contract is unchanged. Only the Ventures list hides it.
5. **`life_date_upsert` does not exist** in src-tauri at accf8f6 (the handoff said "existing"). Every add is an UNWIRED adapter in live mode (disabled with "Needs wiring · <command>"); sample previews run only with `nexus.sampleAdapters=1`.
6. Editing a date, marking a task done/editing it, and moving a kanban card are disabled with a reason.

### 9.4 Backend needs (for Claude / the Nexus Lead)
1. **Decisions log** store + commands: list, append, undo (≤ 8 s, deletes), add outcome. Append-only, no money moves, no outside calls. UI contract: `decisions/log.ts` `DecisionsLog`.
2. **Life dates** store + `life_date_upsert` (and list/delete), returning the parsed preview before save; reminders 90/30/7 days.
3. **Add commands** for goals/habits, enterprise tasks and content ideas, each returning a preview before save (UI contract: `adds/flow.ts` `AddAdapter`).
4. **Check-in store** (done today?) so the quiet row can leave when the check-in is saved on another device.
5. **Crew approvals (R13-D)** as list pointers.
6. **Needs you today aggregator** (optional; the UI merges today).
7. **On-device transcription** for the composer mic.
8. A venture id on tasks and content items (the "venture link needs wiring" subtitle).
