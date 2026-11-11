import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AmaraTakeCard } from "./AmaraTakeCard";
import { ASK_MORE_EVENT, liveNote, sampleTakeView, takeView, type AskMoreDetail } from "./takeView";
import { UNWIRED } from "../lib/sampleAdapters";
import type { AmaraTake } from "./types";

const live: AmaraTake = { page: "fitness", day: "2026-11-11", text: "Recovery is 74 %, above your usual 61 %.", cited: [{ key: "recovery", label: "Recovery", display: "74 %", baseline: "61 %" }], fingerprint: "x", generatedAt: Date.parse("2026-11-10T22:40:00Z") };

describe("design-1110f Amara's take", () => {
  it("fresh (live): the generated text, one chip per cited number, JST time, no sample tag", () => {
    render(<AmaraTakeCard page="fitness" view={takeView("fitness", { state: "live", value: live }, false)} />);
    expect(screen.getByText(live.text)).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Numbers Amara used" })).toHaveTextContent("Recovery74 %");
    expect(screen.getByText("Today, 7:40 AM")).toBeInTheDocument();
    expect(screen.queryByText("Sample data")).toBeNull();
  });
  it("sample (preview): mockup note tagged Sample data", () => {
    render(<AmaraTakeCard page="business" view={sampleTakeView("business", null)} />);
    expect(screen.getByText("Sample data")).toBeInTheDocument();
    expect(screen.getAllByText("$1,580").length).toBe(2); // bold in the text + its chip
  });
  it("generating: skeleton plus a status line, while the first check runs", () => {
    render(<AmaraTakeCard page="neuro" view={takeView("neuro", UNWIRED, true)} />);
    expect(screen.getByRole("status")).toHaveTextContent("Amara is reading today's data");
  });
  it("no data yet and unwired: nothing claims to be written by Amara", () => {
    const { unmount } = render(<AmaraTakeCard page="neuro" view={sampleTakeView("neuro", "nodata")} />);
    expect(screen.getByText(/once a Muse session ends/)).toBeInTheDocument();
    unmount();
    render(<AmaraTakeCard page="neuro" view={takeView("neuro", UNWIRED, false)} />);
    expect(screen.getByText("Needs wiring")).toBeInTheDocument();
    expect(screen.getByText(/Nothing here is written by Amara today/)).toBeInTheDocument();
  });
  it("failed: one manual Try again", () => {
    const retry = vi.fn();
    render(<AmaraTakeCard page="fitness" view={{ state: "failed", reason: "Couldn't write today's note" }} onRetry={retry} />);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(retry).toHaveBeenCalledTimes(1);
  });
  it("Ask more opens Home chat with the take's chips and question (nothing is sent)", () => {
    const seen: AskMoreDetail[] = [];
    const on = (e: Event) => seen.push((e as CustomEvent<AskMoreDetail>).detail);
    window.addEventListener(ASK_MORE_EVENT, on);
    render(<AmaraTakeCard page="fitness" view={{ state: "fresh", note: liveNote(live), sample: false }} />);
    fireEvent.click(screen.getByRole("button", { name: "Ask more ›" }));
    window.removeEventListener(ASK_MORE_EVENT, on);
    expect(seen).toEqual([{ from: "Fitness", chips: [{ label: "Recovery", value: "74 %" }], question: "What's behind today's Fitness note?", sample: false }]);
  });
  it("compact (Home) drops the source line", () => {
    render(<AmaraTakeCard page="home" compact view={sampleTakeView("home", null)} />);
    expect(screen.queryByText(/refreshes once/)).toBeNull();
  });
});
