// Decision 0066: the briefing views against a fake backend. They show what the backend returned,
// say how the timer really runs, and show every part of a cross-domain suggestion.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { BusinessBriefingTab } from "../business/workspace/BusinessBriefingView";
import type { Invoke } from "../lib/localStores";
import { BriefingsView } from "../life/BriefingsView";
import { lifeWrites, type RecommendationRow } from "../life/lifeApi";
import { RecommendationCard } from "../life/RecommendationCard";
import { parsePlan, planText } from "./briefingApi";

const plan = { metric: "workout.done_sets_7d", params: {}, baseline: 30, target: 9, direction: "at_most", unit: "sets", measure_after: "2026-12-03", window_days: 7, how: "Sets logged in the workout engine." };

const rec = (over: Partial<RecommendationRow> = {}): RecommendationRow => ({
  recommendationId: "r1", scope: "personal", domain: "fitness", kind: "training_volume_rising", title: "Training volume is 3.8× your recent weekly average", body: "",
  observation: "You logged 30 sets in 1 session.", rationale: "Volume is rising much faster than your recent weeks.", suggestedAction: "Keep next week at or below 9 sets.",
  confidence: 0.6, confidenceBasis: "3 earlier weeks", uncertainty: "Only sessions logged in the workout engine are counted.", impact: null, effort: "low",
  status: "proposed", createdAt: "", updatedAt: "", expiresAt: null, producedBy: "rules:cross_domain", modelTier: null, targetRef: null, supersedesId: null,
  evaluationJson: JSON.stringify(plan), periodStart: "2026-10-30T00:00:00+09:00", periodEnd: "2026-11-26T23:59:59+09:00", fingerprint: "training_volume_rising:abc", ...over,
});

const runtime = {
  runsWhileClosed: false, timerStartedAt: "2026-11-26T07:58:00+09:00", modelInference: "None. Briefings and suggestions are fixed rules and arithmetic over your records.",
  note: "Briefings are built by Amara OS while it is open. When it is closed nothing runs; the first minute after the next launch builds anything that was missed.",
  jobs: [
    { id: "personal", label: "Daily personal briefing", schedule: "Every day at 07:00 JST while Amara OS is open", nextDueAt: "2026-11-27T07:00:00+09:00",
      state: { lastDay: "2026-11-26", lastRunAt: "2026-11-26T08:00:20+09:00", lastTrigger: "catch_up", lastResult: "partial", lastError: null, ruleCounts: { created: 1 }, measured: 0 } },
    { id: "enterprise", label: "Enterprise briefing", schedule: "Every day at 07:30 JST while Amara OS is open", nextDueAt: "2026-11-27T07:30:00+09:00",
      state: { lastDay: null, lastRunAt: null, lastTrigger: null, lastResult: null, lastError: null, ruleCounts: {}, measured: 0 } },
  ],
};

const training = { periodStart: "2026-11-20", periodEnd: "2026-11-26", sessions: 1, doneSets: 30, totalReps: 300, activeMin: 40, baselineSetsPerWeek: 8, setsByMuscle: { chest: 30 }, detectedWorkoutMin: null, meanStrain: null, sourceIds: ["workout:w5"], note: "Sessions are the ones you logged." };
const neuro = { periodStart: "2026-11-20", periodEnd: "2026-11-26", sessions: 0, minutes: 0, eligibleTraining: 0, medianTimeInTargetPct: null, medianValidPct: null, trends: [], sourceIds: [], note: "Muse EEG figures describe the recorded signal. They are not a measure of mood, ability or mental health." };

const daily = {
  day: "2026-11-26", builtAt: "2026-11-26T08:00:20+09:00", trigger: "catch_up", method: "Fixed rules and arithmetic over your records. No language model was used.",
  missing: ["WHOOP daily table (NOOP)"], openRecommendations: ["r1"], trajectories: [], training, neuro,
  sections: [
    { id: "today", title: "Today and the week ahead", state: "data", note: "No calendar is connected, so only Life dates are listed.", lines: [{ text: "Visa renewal is due tomorrow.", kind: "self_report", sources: ["date:d1"] }] },
    { id: "body", title: "Sleep and recovery", state: "unavailable", note: "The WHOOP daily table (NOOP) isn't usable right now, so nothing is shown.", lines: [] },
  ],
};

function fakeCall(extra: Record<string, unknown> = {}) {
  const calls: Array<[string, Record<string, unknown> | undefined]> = [];
  const call: Invoke = (cmd, args) => {
    calls.push([cmd, args]);
    const table: Record<string, unknown> = {
      briefing_daily_read: daily, briefing_runtime: runtime,
      briefing_recommendations: [{ recommendation: rec(), evidence: [], outcomes: [] }],
      ...extra,
    };
    return cmd in table ? Promise.resolve(table[cmd] as never) : Promise.reject(new Error(`unexpected ${cmd}`));
  };
  return { call, calls };
}

describe("0066 briefings", () => {
  it("the evaluation plan is read strictly and phrased plainly", () => {
    expect(parsePlan("{")).toBeNull();
    expect(parsePlan(JSON.stringify({ ...plan, direction: "up" }))).toBeNull();
    expect(planText(parsePlan(JSON.stringify(plan))!)).toBe("At most 9 sets (was 30), checked after 2026-12-03.");
  });

  it("the Life briefing shows the sections, what's missing, and that nothing runs while the app is closed", async () => {
    const { call } = fakeCall();
    render(<BriefingsView call={call} writes={lifeWrites(call)} />);
    expect(await screen.findByText("Your briefing for 2026-11-26")).toBeInTheDocument();
    expect(screen.getByText(/Not available right now: WHOOP daily table/)).toBeInTheDocument();
    expect(screen.getByText("Unavailable")).toBeInTheDocument();
    expect(screen.getByText(/No calendar is connected/)).toBeInTheDocument();
    expect(screen.getByText(/When it is closed nothing runs/)).toBeInTheDocument();
    expect(screen.getByText(/catching up after launch/)).toBeInTheDocument();
    expect(screen.getByText(/not a measure of mood/)).toBeInTheDocument();
    expect(screen.getByText("Training volume is 3.8× your recent weekly average")).toBeInTheDocument();
  });

  it("a cross-domain card shows all six parts, and a measurement apart from Taisun's own verdict", async () => {
    const detail = {
      recommendation: rec({ status: "accepted" }),
      evidence: [{ evidenceId: "e1", recordRef: "workout:w5", windowStart: "2026-11-20T00:00:00+09:00", windowEnd: "2026-11-26T23:59:59+09:00", role: "observation", quality: "measured", note: null, capturedAt: "", snapshotSha256: "x" }],
      outcomes: [{ outcomeId: "o1", outcome: "measured", at: "", actor: "system", modificationJson: null, resultRef: null, evaluation: "met", note: "Measured 6 sets for the period ending 2026-12-03 (target at most 9)." }],
    };
    const { call } = fakeCall({ recommendation_get: detail });
    render(<RecommendationCard rec={rec({ status: "accepted" })} writes={lifeWrites(call)} />);
    expect(screen.getByText("You logged 30 sets in 1 session.")).toBeInTheDocument();
    expect(screen.getByText("Keep next week at or below 9 sets.")).toBeInTheDocument();
    expect(screen.getByText(/At most 9 sets \(was 30\), checked after 2026-12-03/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Explain" }));
    expect(await screen.findByText("Interpretation")).toBeInTheDocument();
    expect(screen.getByText("Reporting period")).toBeInTheDocument();
    expect(screen.getByText(/60%, from 3 earlier weeks/)).toBeInTheDocument();
    expect(screen.getByText("Limitations")).toBeInTheDocument();
    expect(screen.getByText("Target met")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Mark done" })).toBeInTheDocument();
  });

  it("the Enterprise briefing reads one business and answers in the business scope", async () => {
    const ent = {
      businessId: "taiyo-skin", name: "Taiyo Skin", day: "2026-11-26", builtAt: "", trigger: "opened", method: "Fixed rules and arithmetic over this business's records only. No language model was used.",
      missing: [], openRecommendations: ["b1"],
      sections: [{ id: "search_needs", title: "Search interest and customer needs", state: "unavailable", lines: [], note: "Search interest is unavailable: Search Console is deferred (decision 0064)." }],
    };
    const { call, calls } = fakeCall({
      briefing_enterprise_read: ent,
      briefing_recommendations: [{ recommendation: rec({ recommendationId: "b1", scope: "business", domain: "enterprise", title: "No Taiyo Skin work recorded this week" }), evidence: [], outcomes: [] }],
      recommendation_respond: "o1",
    });
    render(<BusinessBriefingTab id="taiyo-skin" call={call} />);
    expect(await screen.findByText("Briefing for 2026-11-26")).toBeInTheDocument();
    expect(screen.getByText(/Search Console is deferred/)).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText("No Taiyo Skin work recorded this week")).toBeInTheDocument());
    expect(calls.find(([c]) => c === "briefing_recommendations")?.[1]).toEqual({ scope: "business", businessId: "taiyo-skin" });
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(calls.some(([c, a]) => c === "recommendation_respond" && a?.scope === "business" && a?.response === "dismissed")).toBe(true));
  });

  it("a failed read says so and shows nothing made up", async () => {
    const call: Invoke = () => Promise.reject(new Error("down"));
    render(<BriefingsView call={call} writes={lifeWrites(call)} />);
    expect(await screen.findByText(/couldn't be read\. Nothing is shown rather than guessed/)).toBeInTheDocument();
    expect(screen.queryByText("Today and the week ahead")).not.toBeInTheDocument();
    vi.restoreAllMocks();
  });
});
