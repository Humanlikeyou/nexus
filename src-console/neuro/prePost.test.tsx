import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CalibrationRun } from "./calibration";
import { PrePostCard } from "./CalibrationView";

describe("decision 0059: before-and-after comparison", () => {
  it("a post-session report says so, and a plain calibration stays pre-meditation", () => {
    const run = new CalibrationRun(20);
    expect(run.report().kind).toBe("pre_meditation");
    expect(run.report("post_session").kind).toBe("post_session");
    expect(run.report("post_session").source).toBe("replay"); // never assumed live
  });

  it("shows each change with whether it is beyond usual variation, and the caveat", () => {
    render(<PrePostCard prePost={{ preReportId: "cal-1", minutesBetween: 40, note: "A measured change in the EEG signal between two resting recordings. It does not show a lasting change.",
      changes: [
        { condition: "eyes_closed", feature: "alpha", pre: 31, post: 40, change: 9, usualRobustSd: 2.9, changeInUsualSpreads: 3.1, beyondUsualVariation: true },
        { condition: "eyes_closed", feature: "alpha_peak_hz", pre: 10, post: 10.2, change: 0.2, usualRobustSd: null, changeInUsualSpreads: null, beyondUsualVariation: false },
        { condition: "eyes_open", feature: "beta", pre: 20, post: 20, change: 0, usualRobustSd: 1, changeInUsualSpreads: 0, beyondUsualVariation: false },
      ] }} />);
    const closed = screen.getByRole("table", { name: "Eyes closed before and after" });
    expect(closed).toHaveTextContent("+9.0% · larger than your usual variation");
    expect(closed).toHaveTextContent("no baseline to judge by");
    expect(screen.getByRole("table", { name: "Eyes open before and after" })).toHaveTextContent("±0.0% · within your usual variation");
    expect(screen.getByText(/does not show a lasting change/)).toBeInTheDocument();
    expect(screen.getByText(/40 minutes earlier/)).toBeInTheDocument();
  });

  it("with nothing to pair with, it says so instead of inventing a comparison", () => {
    render(<PrePostCard prePost={null} />);
    expect(screen.getByText(/nothing to compare with/)).toBeInTheDocument();
  });
});
