import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { makeSamplePersonal, MOOD_STEPS, unwiredPersonal, isTokyoSunday } from "./adapters";
import { CheckInSection, GoalsSection, WeeklyReviewSection } from "./TodaySections";
import { DatesView } from "../life/DatesView";
import { MemorySection, PatternsView, PersonalMoneyView } from "./PageViews";

describe("design-1111a personal OS", () => {
  it("unwired: every surface says Needs wiring and shows no numbers or dates", () => {
    render(<>
      <GoalsSection adapter={unwiredPersonal} goals={[]} /><CheckInSection adapter={unwiredPersonal} /><DatesView adapter={unwiredPersonal} />
      <WeeklyReviewSection adapter={unwiredPersonal} /><PatternsView adapter={unwiredPersonal} /><PersonalMoneyView adapter={unwiredPersonal} /><MemorySection adapter={unwiredPersonal} />
    </>);
    expect(screen.getAllByText("Needs wiring").length).toBeGreaterThanOrEqual(7);
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.queryByText(/\$\d/)).toBeNull();
    expect(screen.queryByText(/Sep 14|Residence card/)).toBeNull();
  });
  it("check-in: 5-step mood, energy, Save needs both; voice without a transcription path says so and keeps no audio", async () => {
    render(<CheckInSection adapter={unwiredPersonal} />);
    const mood = screen.getByRole("radiogroup", { name: "Mood" });
    expect(mood.querySelectorAll("[role=radio]")).toHaveLength(5);
    expect(MOOD_STEPS).toHaveLength(5);
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.click(screen.getByRole("radio", { name: /4\s*Pleasant/ }));
    fireEvent.click(screen.getByRole("radio", { name: /3\s*OK/ }));
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /Say it/ })); });
    expect(screen.getByRole("status")).toHaveTextContent("no audio is recorded");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Save" })); });
    expect(screen.getByRole("status")).toHaveTextContent("nothing was saved");
  });
  it("memory: delete asks first and goes through the adapter; unwired never claims a delete", async () => {
    const a = makeSamplePersonal();
    render(<MemorySection adapter={a} />);
    fireEvent.click(screen.getByRole("button", { name: "Delete: Usual HRV is about 60 ms" }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete for good" })); });
    expect(screen.queryByText("Usual HRV is about 60 ms")).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("Sample data");
    await act(async () => { fireEvent.click(screen.getAllByRole("button", { name: "Keep" })[0]); });
    expect(screen.getByText("Late coffee seems to cut your afternoon energy.")).toBeInTheDocument();
  });
  it("weekly review: Try it sets a reminder and never changes a goal", async () => {
    render(<WeeklyReviewSection adapter={makeSamplePersonal()} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Try it this week" })); });
    expect(screen.getByRole("status")).toHaveTextContent("Your goals didn't change");
  });
  it("personal money (Life › Personal money) is personal only, links to Finances, and is in USD", () => {
    const onOpenFinances = vi.fn();
    render(<PersonalMoneyView adapter={makeSamplePersonal()} onOpenFinances={onOpenFinances} />);
    // design-1111v: the "Kept apart" banner and the "Business › Personal" eyebrow are gone; the venture wall is the link.
    expect(screen.queryByText(/Kept apart from Business/)).toBeNull();
    expect(screen.queryByText(/Business › Personal/)).toBeNull();
    expect(document.body.textContent).not.toMatch(/Taiyo Skin operating|Trading bankroll/);
    fireEvent.click(screen.getByRole("button", { name: "Enterprise › Finances ›" }));
    expect(onOpenFinances).toHaveBeenCalled();
    expect(screen.getByText("Left after bills").parentElement).toHaveTextContent("$1,412");
    // 1111t: accounts are unwired here, so no balance or yen figure is shown and the read-only rule is stated.
    expect(screen.queryByText(/¥/)).toBeNull();
    expect(screen.getByText(/Read-only by design/)).toBeInTheDocument();
  });
  it("Sunday is judged in Tokyo", () => {
    expect(isTokyoSunday(new Date("2026-11-14T16:00:00Z"))).toBe(true); // Sun 01:00 JST
    expect(isTokyoSunday(new Date("2026-11-14T14:00:00Z"))).toBe(false); // Sat 23:00 JST
  });
});
