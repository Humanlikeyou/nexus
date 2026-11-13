import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AmaraTakeCard } from "../amaraTake/AmaraTakeCard";
import { sampleTakeView } from "../amaraTake/takeView";
import type { TakePage } from "../amaraTake/types";
import { CAUSAL, sampleTakes, unwiredTakes, type TakesAdapter, type TakesPreview } from "./adapters";
import { FollowThroughView } from "./FollowThrough";
import { HistorySheet } from "./HistorySheet";
import { SuggestionBlock } from "./SuggestionBlock";
import { useTakeSlots } from "./TakeExtras";
import { TakesSettingsCard } from "./TakesSettings";
import { ReadyToKeepCard, WeekSuggestionsCard } from "./WeekTakes";

const PAGES: TakePage[] = ["neuro", "fitness", "mindBody", "business", "home"];

function Card({ page, adapter, compact = false }: { page: TakePage; adapter: TakesAdapter; compact?: boolean }) {
  const slots = useTakeSlots(page, compact, adapter);
  return <AmaraTakeCard page={page} view={sampleTakeView(page, null)} compact={compact} slots={slots} />;
}
const textOf = (t: TakesAdapter) => {
  const out: string[] = [];
  for (const p of PAGES) {
    const today = t.today(p);
    if (today.state !== "unwired" && today.value.kind === "suggestion") { const s = today.value.suggestion; out.push(s.title, s.how, s.why, s.evidence.summary, s.evidence.risk ?? "", s.evidence.alsoConsidered ?? ""); }
    const m = t.memory(p); if (m.state !== "unwired" && m.value) out.push(m.value.text);
  }
  const o = t.outcomes(); if (o.state !== "unwired") for (const x of o.value) out.push(x.headline, x.sentence);
  const w = t.week(); if (w.state !== "unwired") { out.push(w.value.change?.body ?? ""); w.value.rows.forEach((r) => out.push(r.note)); }
  return out;
};

describe("design-1111t takes", () => {
  it("one new suggestion per page, each with evidence; placeholder citations stay marked", () => {
    const t = sampleTakes();
    for (const p of PAGES) {
      const v = t.today(p);
      expect(v.state).toBe("sample");
      if (v.state === "unwired" || v.value.kind !== "suggestion") throw new Error(p);
      for (const src of v.value.suggestion.evidence.sources) expect(src.verified).toBe(false);
    }
    render(<Card page="neuro" adapter={t} />);
    expect(screen.getAllByText("Placeholder citation · not checked yet").length).toBeGreaterThan(0);
    expect(screen.getByText("New today")).toBeInTheDocument();
  });
  it("sample copy never makes a causal claim", () => {
    for (const s of textOf(sampleTakes())) expect(s).not.toMatch(CAUSAL);
  });
  it("no 3-week trials and no Let Home do it", () => {
    const { container } = render(<>{PAGES.map((p) => <Card key={p} page={p} adapter={sampleTakes()} />)}<FollowThroughView adapter={sampleTakes()} /><WeekSuggestionsCard adapter={sampleTakes()} /></>);
    expect(container.textContent).not.toMatch(/Let Home do it|3-week|21-day|Trial day/i);
  });
  it("Done / Skipped / Not today, skip reasons, and Undo", async () => {
    const t = sampleTakes();
    render(<Card page="neuro" adapter={t} />);
    const group = screen.getByRole("group", { name: /Your answer/ });
    await act(async () => { fireEvent.click(within(group).getByRole("button", { name: "Skipped" })); });
    expect(within(group).getByRole("button", { name: "Skipped" })).toHaveAttribute("aria-pressed", "true");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "No time" })); });
    expect(screen.getByRole("button", { name: "No time" })).toHaveAttribute("aria-pressed", "true");
    await act(async () => { fireEvent.click(within(group).getByRole("button", { name: "Done" })); });
    expect(screen.queryByRole("group", { name: /Your answer/ })).toBeNull(); // Done folds the block (Taisun 2026-11-14)
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Undo" })); });
    expect(within(group).getByRole("button", { name: "Done" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getAllByText(/Sample data: undone\./).length).toBe(1);
  });
  it("auto-detected Done shows what it saw and can be undone", async () => {
    render(<Card page="fitness" adapter={sampleTakes({ answer: { fitness: "detected" } })} />);
    expect(screen.getByText(/WHOOP: 27-min walk/)).toBeInTheDocument();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Not right? Undo" })); });
    expect(screen.getByRole("group", { name: /Your answer/ })).toBeInTheDocument();
  });
  it("Done folds the block to one slim line for the day; Undo opens it again", async () => {
    const t = sampleTakes();
    const { container } = render(<Card page="home" adapter={t} compact />);
    const fold = () => container.querySelector(".t-block") as HTMLElement;
    expect(fold().dataset.folded).toBeUndefined();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Done" })); });
    expect(fold().dataset.folded).toBe("true");
    expect(screen.queryByRole("group", { name: /Your answer/ })).toBeNull();
    expect(screen.queryByRole("heading", { level: 3 })).toBeNull();
    const line = screen.getByRole("group", { name: /^Done today: / });
    expect(line).toHaveTextContent("Done today");
    expect(container.querySelector(".t-fold__in[inert]")).not.toBeNull();
    await act(async () => { fireEvent.click(within(line).getByRole("button", { name: "Undo" })); });
    expect(fold().dataset.folded).toBeUndefined();
    expect(screen.getByRole("group", { name: /Your answer/ })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: /^Done today: / })).toBeNull();
  });
  it("a Done answer from the store is folded on first render and stays folded across re-renders (reload, same day)", () => {
    const t = sampleTakes({ answer: { neuro: "done" } });
    const { container, rerender } = render(<Card page="neuro" adapter={t} />);
    expect((container.querySelector(".t-block") as HTMLElement).dataset.folded).toBe("true");
    expect(screen.getByRole("group", { name: /^Done today: / })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: /Your answer/ })).toBeNull();
    rerender(<Card page="neuro" adapter={t} />);
    expect((container.querySelector(".t-block") as HTMLElement).dataset.folded).toBe("true");
  });
  it("follows the stored answer: a later Undo in the store opens the block, and Skipped never folds", () => {
    const t = sampleTakes();
    const v = t.today("neuro");
    if (v.state === "unwired" || v.value.kind !== "suggestion") throw new Error("x");
    const base = v.value.suggestion;
    const noop = async () => ({ state: "sample" as const, value: { ok: true as const } });
    const ui = (answer: typeof base.answer) => <SuggestionBlock s={{ ...base, answer }} detects onAnswer={noop} onUndo={noop} />;
    const { container, rerender } = render(ui({ state: "done", source: "tap", at: "8:12 PM" }));
    const folded = () => (container.querySelector(".t-block") as HTMLElement).dataset.folded;
    expect(folded()).toBe("true");
    rerender(ui({ state: "waiting" }));
    expect(folded()).toBeUndefined();
    rerender(ui({ state: "skipped", source: "tap", at: "8:40 PM", reason: null }));
    expect(folded()).toBeUndefined();
  });
  it("reduced motion: folding is a state, not an animation; the line is there at once and nothing waits on a transition", async () => {
    document.documentElement.dataset.reduceMotion = "true";
    try {
      const { container } = render(<Card page="home" adapter={sampleTakes()} compact />);
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Done" })); });
      expect(screen.getByRole("group", { name: /^Done today: / })).toBeVisible();
      expect((container.querySelector(".t-block") as HTMLElement).dataset.folded).toBe("true");
    } finally { delete document.documentElement.dataset.reduceMotion; }
  });
  it("unwired: the take still shows, today's one thing says Needs wiring, no sample data", () => {
    render(<Card page="neuro" adapter={unwiredTakes} />);
    expect(screen.getByText(/Today's one thing/)).toBeInTheDocument();
    expect(screen.getAllByText("Needs wiring").length).toBeGreaterThan(0);
    expect(screen.queryByRole("group", { name: /Your answer/ })).toBeNull();
    expect(screen.queryByText(/History ·/)).toBeNull();
    render(<><FollowThroughView adapter={unwiredTakes} /><WeekSuggestionsCard adapter={unwiredTakes} /><ReadyToKeepCard adapter={unwiredTakes} /><TakesSettingsCard adapter={unwiredTakes} /></>);
    expect(screen.queryByText(/Looks better/)).toBeNull();
  });
  it("detection unwired: Done / Skipped by tap, auto-detect says Needs wiring", () => {
    render(<Card page="neuro" adapter={sampleTakes({ unwired: ["detection"] })} />);
    expect(screen.getByText(/Tap Done or Skipped for now/)).toBeInTheDocument();
  });
  it("learning and none-today states never invent a suggestion", () => {
    const opts: TakesPreview = { today: { neuro: "learning", fitness: "none_today" } };
    render(<><Card page="neuro" adapter={sampleTakes(opts)} /><Card page="fitness" adapter={sampleTakes(opts)} /></>);
    expect(screen.getByText("No suggestion yet.")).toBeInTheDocument();
    expect(screen.getByText(/3 of 7 so far/)).toBeInTheDocument();
    expect(screen.getByText("No suggestion today, on purpose.")).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: /Your answer/ })).toBeNull();
  });
  it("compact Home shows only the title, level and Done / Skip", () => {
    render(<Card page="home" adapter={sampleTakes()} compact />);
    const g = screen.getByRole("group", { name: /Your answer/ });
    expect(within(g).getAllByRole("button").map((b) => b.textContent)).toEqual(["Done", "Skip"]);
    expect(screen.queryByText(/Why this, from your data/)).toBeNull();
  });
  it("follow-up shows n and range; 'better' is linked, not proven", () => {
    render(<Card page="neuro" adapter={sampleTakes({ today: { neuro: "followup" } })} />);
    expect(screen.getAllByText(/linked, not proven/).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Make it a habit" })).toBeInTheDocument();
  });
  it("follow-through groups results by type over time with the same rules", () => {
    render(<FollowThroughView adapter={sampleTakes()} />);
    expect(screen.getAllByRole("button", { current: false }).length + 1).toBeGreaterThanOrEqual(7);
    expect(screen.getByRole("table", { name: /over time/ })).toBeInTheDocument();
    expect(screen.getByText(/Same rules for every type/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Harder session on green-recovery days/ }));
    expect(screen.getByText(/Not claimed:/).parentElement).toHaveTextContent("Looks worse");
  });
  it("history: filters, search, kept 1 year then summarized, forget with confirm, empty state", async () => {
    const t = sampleTakes();
    render(<HistorySheet page="neuro" open onClose={() => {}} adapter={t} />);
    expect(screen.getByText(/Kept 1 year, then summarized into Memory/)).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search takes" }), { target: { value: "zzzz" } });
    expect(screen.getByText("No takes match.")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search takes" }), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Forget a take…" }));
    const before = screen.getAllByRole("article").length;
    fireEvent.click(screen.getAllByRole("button", { name: /^Forget the / })[0]);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Forget" })); });
    expect(screen.getAllByRole("article").length).toBe(before - 1);
  });
  it("history empty and unwired", () => {
    const { unmount } = render(<HistorySheet page="neuro" open onClose={() => {}} adapter={sampleTakes({ historyEmpty: true })} />);
    expect(screen.getByText(/No takes yet/)).toBeInTheDocument();
    unmount();
    render(<HistorySheet page="neuro" open onClose={() => {}} adapter={unwiredTakes} />);
    expect(screen.getByText(/Take history/)).toBeInTheDocument();
    expect(screen.getAllByText("Needs wiring").length).toBeGreaterThan(0);
  });
  it("settings: auto-mark Done toggle with Undo note", async () => {
    render(<TakesSettingsCard adapter={sampleTakes()} />);
    const sw = screen.getByRole("switch", { name: /Mark Done automatically/ });
    expect(sw).toHaveAttribute("aria-checked", "true");
    await act(async () => { fireEvent.click(sw); });
    expect(screen.getByRole("status")).toHaveTextContent("You mark Done yourself.");
  });
  it("suggestion block without detection rule falls back to tap", () => {
    const t = sampleTakes();
    const v = t.today("business");
    if (v.state === "unwired" || v.value.kind !== "suggestion") throw new Error("x");
    render(<SuggestionBlock s={{ ...v.value.suggestion, detectRule: null }} detects onAnswer={t.answer.bind(null, "business", "x")} onUndo={() => t.undo("business", "x")} />);
    expect(screen.getByText(/Tap Done or Skipped for now/)).toBeInTheDocument();
  });
});
