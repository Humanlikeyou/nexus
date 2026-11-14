import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { live } from "../lib/localStores";
import { UNWIRED, sample, type Wired } from "../lib/sampleAdapters";
import type { WeeklyReview } from "../personal/adapters";
import { weeklyReviewFrom, type ReviewWire } from "../personal/liveInsights";
import { WeeklyReadyCard } from "./NeedsYouToday";

const mock = vi.hoisted(() => ({ wr: null as unknown }));
vi.mock("../personal/adapters", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../personal/adapters")>()),
  personalAdapter: () => ({ weeklyReview: () => mock.wr }),
}));

/** A stored review as weekly_review_read returns it. `suggestion` null = no change to try this week. */
function reviewWire(suggestion: ReviewWire["suggestion"]): ReviewWire {
  return {
    state: "ready", weekStart: "2026-11-08", weekEnd: "2026-11-14", dueAt: "2026-11-15T10:00:00Z", computedAt: "2026-11-14T10:00:00Z",
    health: { state: "data", nights: 6, avgSleepH: 7.3, sleepDeltaH: 0.2, avgHrv: 58, avgRecovery: 70, note: "" },
    work: { state: "data", focusSessions: 4, avgFocus: 60, note: "" },
    checkins: { state: "none", count: 0, avgMood: null, avgEnergy: null, notes: [], note: "" },
    goals: { state: "none", milestones: [], habits: 0, note: "" },
    money: { state: "none", personalChangeUsd: null, accounts: [], note: "" },
    suggestion,
    suggestionNote: "Nothing repeated enough this week to suggest a change.",
    summary: null,
    experiments: [],
  } as unknown as ReviewWire;
}

const SUGGESTION = {
  title: "Move hard workouts to Friday.",
  reason: "Focus dropped the day after each hard workout.",
  evidenceFindingId: "sleep-hrv:1",
  confidence: 3,
};

/** The sample review (sample mode): it carries one change to try, labelled as sample by its state. */
function sampleReview(): WeeklyReview {
  return weeklyReviewFrom(reviewWire(SUGGESTION));
}

function renderCard(wr: Wired<WeeklyReview>) {
  mock.wr = wr;
  return render(<WeeklyReadyCard />);
}

const sampleTag = () => document.querySelector('.data-tag[data-kind="sample"]');

describe("Sunday weekly-review card (Home > Needs you): tag and count follow the data", () => {
  beforeEach(() => { mock.wr = UNWIRED; });

  it("a live review with a change to try shows that count and no sample tag", () => {
    renderCard(live(weeklyReviewFrom(reviewWire(SUGGESTION))));
    expect(screen.getByText(/1 change to try/)).toBeInTheDocument();
    expect(sampleTag()).toBeNull();
  });

  it("a live review with no change this week shows no count and no sample tag", () => {
    renderCard(live(weeklyReviewFrom(reviewWire(null))));
    expect(screen.queryByText(/change to try/)).toBeNull();
    expect(sampleTag()).toBeNull();
  });

  it("sample mode still shows the sample tag and its change count", () => {
    renderCard(sample(sampleReview()));
    expect(screen.getByText(/1 change to try/)).toBeInTheDocument();
    expect(sampleTag()).not.toBeNull();
    expect(sampleTag()).toHaveTextContent("Sample data");
  });

  it("with no review loaded it shows the needs-wiring tag, not sample", () => {
    renderCard(UNWIRED);
    expect(screen.getByText("Health, work and money from your week")).toBeInTheDocument();
    expect(sampleTag()).toBeNull();
    expect(screen.queryByText(/change to try/)).toBeNull();
  });
});
