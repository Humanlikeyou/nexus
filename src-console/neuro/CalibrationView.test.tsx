import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CalibrationView } from "./CalibrationView";

const base = {
  version: "baseline-v1-median-mad", alphaReactivity: null,
  note: "Relative EEG band power and alpha peak frequency are signal measurements. They are not measures of emotion, attention, stress or relaxation.",
  conditions: [
    { condition: "eyes_open", recordings: 1, relBands: {}, alphaPeakHz: null, confidence: "none", uncertainty: "1 high-quality recording so far; at least 3 are needed before there is a baseline." },
    { condition: "eyes_closed", recordings: 4, relBands: { alpha: { median: 31, robustSd: 2, n: 4 } }, alphaPeakHz: { median: 10.1, robustSd: 0.3, n: 4 }, confidence: "low", uncertainty: "From 4 high-quality recordings." },
  ],
};

describe("Neuro › Calibration", () => {
  it("is optional: Skip goes straight on, and the baseline says how sure it is", async () => {
    const call = vi.fn(async (cmd: string) => (cmd === "neuro_baseline_get" ? base : null));
    const onDone = vi.fn();
    render(<CalibrationView onDone={onDone} call={call as never} />);
    expect(screen.getByText(/You can start a session without it/)).toBeTruthy();
    expect(await screen.findByText(/alpha 31.0% \(usual 29.0–33.0%\)/)).toBeTruthy();
    expect(screen.getByText(/at least 3 are needed/)).toBeTruthy();
    expect(screen.getByText(/not measures of emotion, attention, stress or relaxation/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Skip calibration" }));
    expect(onDone).toHaveBeenCalled();
  });

  it("the check waits for the live headset and offers to start its stream", async () => {
    const call = vi.fn(async (cmd: string) => (cmd === "neuro_baseline_get" ? base : null));
    render(<CalibrationView call={call as never} />);
    fireEvent.click(screen.getByRole("button", { name: "Check the headset" }));
    expect(screen.getByRole("button", { name: "Start eyes-open recording" })).toHaveProperty("disabled", true);
    fireEvent.click(screen.getByRole("button", { name: "Start the headset stream" }));
    await waitFor(() => expect(call).toHaveBeenCalledWith("start_eeg_stream"));
  });

  it("never claims (or later stops) a stream that was already running", async () => {
    const call = vi.fn(async (cmd: string) => (cmd === "neuro_baseline_get" ? base : cmd === "get_muse_state" ? { streaming: true } : null));
    const { unmount } = render(<CalibrationView call={call as never} />);
    fireEvent.click(screen.getByRole("button", { name: "Check the headset" }));
    fireEvent.click(screen.getByRole("button", { name: "Start the headset stream" }));
    await waitFor(() => expect(call).toHaveBeenCalledWith("get_muse_state"));
    unmount();
    expect(call).not.toHaveBeenCalledWith("start_eeg_stream");
    expect(call).not.toHaveBeenCalledWith("stop_eeg_stream");
  });
});
