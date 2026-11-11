import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { DatesView } from "./DatesView";
import { LifeField } from "./LifeScreen";
import { GoalsSection, WeeklyReviewSection } from "../personal/TodaySections";
import { PersonalMoneyView } from "../personal/PageViews";
import { personalAdapter } from "../personal/adapters";
import { INITIAL_SNAPSHOT } from "../bridge";
import { AmaraTakeCard } from "../amaraTake/AmaraTakeCard";
import { takeView } from "../amaraTake/takeView";
import { UNWIRED } from "../lib/sampleAdapters";

const take = () => screen.getByRole("region", { name: /^Amara's take$/ });

describe("Life pages show Amara's take (design-1111w)", () => {
  beforeEach(() => { localStorage.setItem("nexus.sampleAdapters", "1"); });
  afterEach(() => { cleanup(); localStorage.removeItem("nexus.sampleAdapters"); });

  it("Goals, Weekly review, Dates and Personal money each render the Life take card", () => {
    const personal = personalAdapter();
    const { rerender } = render(<GoalsSection adapter={personal} goals={[]} />);
    expect(take()).toBeInTheDocument();
    expect(take()).toHaveTextContent(/4 of 5/);
    expect(take()).toHaveTextContent("Sample data");

    rerender(<WeeklyReviewSection adapter={personal} />);
    expect(take()).toBeInTheDocument();
    expect(take()).toHaveTextContent(/lease notice/i);

    rerender(<DatesView adapter={personal} />);
    expect(take()).toBeInTheDocument();

    rerender(<PersonalMoneyView adapter={personal} />);
    const moneyTakes = screen.getAllByRole("region", { name: /^Amara's take$/ });
    expect(moneyTakes).toHaveLength(1);
    expect(moneyTakes[0]).toHaveTextContent(/4 of 5/);
    expect(moneyTakes[0]).toHaveTextContent(/5\.5 months/);
    expect(moneyTakes[0]).toHaveTextContent(/Today's one thing · Money/);
    expect(moneyTakes[0]).not.toHaveTextContent(/Today's one thing · Life/);
    expect(moneyTakes[0]).toHaveTextContent(/Not financial advice/);
    expect(document.querySelector(".life__take")).toBeTruthy();
  });

  it("LifeField in sample mode shows the take on the Goals landing", () => {
    render(<LifeField data={INITIAL_SNAPSHOT} view="goals" />);
    expect(take()).toBeInTheDocument();
    expect(take()).toHaveAttribute("data-state", "fresh");
  });

  it("live mode with no Life facts is a Needs wiring card, with no invented numbers", () => {
    localStorage.removeItem("nexus.sampleAdapters");
    render(<AmaraTakeCard page="life" view={takeView("life", UNWIRED, false)} />);
    expect(take()).toHaveAttribute("data-state", "unwired");
    expect(take()).toHaveTextContent(/isn't written yet/);
    expect(take()).not.toHaveTextContent("4 of 5");
    expect(take()).not.toHaveTextContent("Mar 1");
  });

  it("Personal money in live mode is still one Needs wiring take, with no money numbers", async () => {
    localStorage.removeItem("nexus.sampleAdapters");
    render(<PersonalMoneyView adapter={personalAdapter()} />);
    const region = await screen.findByRole("region", { name: /^Amara's take$/ });
    await waitFor(() => expect(region).toHaveAttribute("data-state", "unwired"));
    expect(screen.getAllByRole("region", { name: /^Amara's take$/ })).toHaveLength(1);
    expect(region).toHaveTextContent(/Needs wiring|isn't written yet/);
    expect(region).not.toHaveTextContent("4 of 5");
    expect(region).not.toHaveTextContent("5.5");
    expect(region).not.toHaveTextContent("$250");
  });
});
