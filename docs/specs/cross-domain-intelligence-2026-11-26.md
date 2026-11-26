# Cross-domain intelligence (Phase 15)

Saved 2026-11-26 from Taisun's prompt in session. This is Phase 15 in `AMARA_PROJECT_HANDOFF.md` §5. The full "Personal Intelligence Architecture" specification is not in the repository (§4), so this brief is what Phase 15 follows.

## His prompt (verbatim)

> Connect the completed Nexus domain services through Amara's shared intelligence layer.
>
> Do not rebuild or duplicate the existing domain systems.
>
> Use authorized and available data from:
> - Life.
> - Workout.
> - NEUROSENSE.
> - WHOOP/Fitness.
> - Personal Money, only if a real authorized source exists.
> - Enterprise.
> - TaiyoSkin business analytics.
> - Calendar and commitments, if connected.
> - User-approved reflections and check-ins.
>
> Implement cross-domain analysis.
>
> Examples:
> - Compare workload changes with training patterns.
> - Identify whether upcoming commitments conflict with current priorities.
> - Compare sleep and recovery trends with reported energy.
> - Identify whether a training plan is accumulating excessive volume.
> - Connect business milestones to weekly execution.
> - Compare customer search interest with customer-reported needs.
> - Identify repeated obstacles using actual historical records.
>
> Every recommendation must include:
> 1. Observation.
> 2. Supporting records and reporting period.
> 3. Interpretation.
> 4. Confidence and limitations.
> 5. Suggested action.
> 6. A measurable way to evaluate the action.
>
> Implement:
> - Daily personal briefing.
> - Weekly Life review.
> - Daily or weekly Enterprise briefing.
> - Goal trajectory summaries.
> - Training summaries.
> - Neurotechnology session summaries.
> - Recommendation outcome tracking.
> - Duplicate recommendation suppression.
>
> Do not infer causation from correlation.
>
> Do not treat biometric data as a definitive readiness or psychological assessment.
>
> Use scheduled or event-triggered processing rather than continuous expensive model inference.
>
> Ensure the scheduler's actual runtime behavior is clear. Do not claim briefings will execute while the application is closed unless an appropriate background service is implemented and tested.
>
> Test each domain independently and then test their approved cross-domain data flows.

He added: "Once this prompt is done, and only when this prompt is complete, you can build and reinstall, as well as update the GitHub."

## Decisions (Taisun, 2026-11-26, in session)

| Question | Answer |
|---|---|
| Where the UI goes (design override) | **Life + Enterprise:** a new Briefings view in the Life host, and a Briefing section in Enterprise › Workspaces. No new top-level nav; Home and Mind-Body unchanged. |
| What counts as workload | **Personal records only:** Life dates due, the week plan's actions, and check-ins. The 0053 business/personal isolation stays as it is. |
| Runtime | **An in-app timer plus catch-up:** a Rust timer builds what is due while the app runs, and the first tick after launch builds anything missed. Nothing runs while the app is closed, and the screen says so. |

## Sources found and used (inspection, 2026-11-26)

| Source | Owner (reused, not rebuilt) | Scope |
|---|---|---|
| Goals, roadmaps, habits, habit days, week plan, Life dates | `goals.rs`, `goal_roadmap.rs`, `habits.rs`, `life_plan.rs`, `life_dates.rs` | personal |
| Check-ins (Taisun's own entries) | `checkin.rs` | personal |
| Workout engine sessions | `workout.rs` | personal |
| WHOOP via NOOP: sleep, HRV, recovery, strain, detected workout minutes | `daily_table.rs` (derived from `noop.rs`) | personal |
| NEUROSENSE training records and sessions | `neuro_training.rs`, `sessions.rs` | neural |
| Personal money | `money_entries.rs`: entries Taisun logs himself. The only authorized source; no bank link exists | personal |
| Enterprise workspaces, activity, approvals, connectors, website and customer reports | `business_workspace*`, `amara_business.db` | business |
| Recommendations, evidence, outcomes | `amara_store.rs` (0053) | per scope |
| Calendar | **Not connected.** The app has no calendar connector; Life dates are the commitments source | — |
| Search interest | **Unavailable.** Search Console and GA4 are deferred (0064), so the search-versus-needs comparison reports unavailable and makes no recommendation | — |

## Acceptance criteria (derived from the prompt)

1. Every cross-domain recommendation stores: an observation; evidence records, each with a role, quality and time window, plus the reporting period; an interpretation; a confidence with its basis and limitations; a suggested action; and a measurable evaluation (metric, baseline, target, measured after a stated date). The store refuses a rule recommendation without them.
2. The rules cover: workload against training; commitments against this week's priority; sleep and recovery against reported energy; training-volume accumulation; business milestones against weekly execution; search interest against customer needs (honestly unavailable); repeated obstacles from historical records; and goal trajectory.
3. Wording never claims causation ("happened in the same weeks", never "because"), and biometric figures are never presented as a readiness or psychological assessment.
4. These are built as stored, read-only documents: a daily personal briefing, a weekly Life review (the existing review plus cross-domain items, not a second review), an Enterprise briefing per business, and goal trajectory, training and neurotechnology session summaries.
5. Outcomes are tracked through the existing status machine. A due evaluation is measured from the records ("measured", met or not met), separate from Taisun's own judgement of whether it helped.
6. Duplicates are suppressed: the same finding (a fingerprint) is not raised again while one is open, within a cooldown after the last one, or within 30 days of a dismissal.
7. Rules and arithmetic only, no model inference. Processing is scheduled (an in-app timer) or event-triggered (opening a view builds what is due). The runtime status is shown honestly, including that nothing runs while the app is closed.
8. Personal, neural and business data stay isolated as in 0053. The Enterprise briefing reads business stores only.
9. Tests: each domain's gatherer and rules on their own, then the approved cross-domain flows, all in temp folders.
