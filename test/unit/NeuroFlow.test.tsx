import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NeuroFlow } from "../../src-console/screens/NeuroFlow";
import { INACTIVE_TRAINING } from "../../src-console/neuro/training";
import type { MuseTelemetry } from "../../src-console/neuro/telemetryStore";

vi.mock("../../src-console/bridge", () => ({ loadKv: vi.fn().mockResolvedValue(null), saveKv: vi.fn().mockResolvedValue(undefined) }));
vi.mock("../../src-console/components/AmaraLens/AmaraLens", () => ({ AmaraLens: () => <i data-testid="lens" /> }));
vi.mock("../../src/hooks/useAudioFeedback", () => ({
  useAudioFeedback: () => ({ init: vi.fn(), setMode: vi.fn(), cleanup: vi.fn(), setMuted: vi.fn(), playBell: vi.fn(), updateState: vi.fn() }),
}));

const protocols = [
  { id: "calm", name: "Calm", description: "Soft attention.", eyesClosed: false, defaultDurationMin: 10 },
  { id: "relaxed-flow", name: "Relaxed flow", description: "Easy.", eyesClosed: false, defaultDurationMin: 12 },
  { id: "focus", name: "Focus", description: "Steady.", eyesClosed: false, defaultDurationMin: 10 },
];
const telemetry = { channels: {}, bands: {}, packetCount: 0, sampleCount: 0, lastPacketAt: null, channelLastPacketAt: {}, aux: null } as unknown as MuseTelemetry;
const training = (over: Record<string, unknown> = {}) => ({
  protocols, protocolId: "calm", setProtocolId: vi.fn(), advance: vi.fn(), dismissFinished: vi.fn(),
  state: INACTIVE_TRAINING, finished: null, ...over,
}) as never;
const ui = (t: never, running = false) => <NeuroFlow training={t} telemetry={telemetry} sessions={[]} running={running} action={<button>Start</button>} />;

describe("NeuroFlow", () => {
  it("opens on the goal step with the chosen goal's copy and the action, and the medical disclaimer", () => {
    render(ui(training()));
    expect(screen.getByRole("heading", { name: "What would you like from this session?" })).toBeInTheDocument();
    expect(screen.getByText("Calm the mind and let the body soften.")).toBeInTheDocument();
    expect(screen.getByText("10 minutes · Watch the light, or close your eyes")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start" })).toBeInTheDocument();
    expect(screen.getByText("For relaxation and attention training. It isn't a medical treatment.")).toBeInTheDocument();
  });

  it("with no feedback chosen it says it only records", () => {
    render(ui(training({ protocolId: "off" })));
    expect(screen.getByText("Records your session with no feedback.")).toBeInTheDocument();
  });

  it("picking a goal selects that goal's first protocol", () => {
    const t = training();
    render(ui(t));
    fireEvent.click(screen.getByRole("radio", { name: "Sharpen focus" }));
    expect((t as unknown as { setProtocolId: ReturnType<typeof vi.fn> }).setProtocolId).toHaveBeenCalledWith("focus");
    fireEvent.click(screen.getByRole("radio", { name: "Just record" }));
    expect((t as unknown as { setProtocolId: ReturnType<typeof vi.fn> }).setProtocolId).toHaveBeenCalledWith("off");
  });

  it("the fit step lists the four sensors and holds Begin until contact is good", () => {
    const fit = { TP9: "good", AF7: "poor", AF8: "good", TP10: "none" };
    const t = training({ state: { ...INACTIVE_TRAINING, stage: "fit", fit, fitReady: false } });
    const { rerender } = render(ui(t, true));
    expect(screen.getByRole("list", { name: "Sensor contact" }).querySelectorAll("li")).toHaveLength(4);
    expect(screen.getByRole("button", { name: "Waiting for good contact" })).toBeDisabled();
    const ready = training({ state: { ...INACTIVE_TRAINING, stage: "fit", fit, fitReady: true } });
    rerender(ui(ready, true));
    fireEvent.click(screen.getByRole("button", { name: "Begin baseline" }));
    expect((ready as unknown as { advance: ReturnType<typeof vi.fn> }).advance).toHaveBeenCalledOnce();
  });

  it("the baseline step reports progress and which eyes state to hold", () => {
    render(ui(training({ state: { ...INACTIVE_TRAINING, stage: "baseline-closed", calibrationPercent: 42.4 } }), true));
    expect(screen.getByText("Close your eyes")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Baseline" })).toHaveAttribute("aria-valuenow", "42");
  });

  it("the training step shows live readout with dashes when a number is missing, and toggles sound", () => {
    const state = { ...INACTIVE_TRAINING, phase: "training", stage: "training", label: "In the target", targetPercent: null, delayMs: 1500 };
    render(ui(training({ state }), true));
    expect(screen.getByRole("status")).toHaveTextContent("In the target");
    expect(screen.getByText("In target").nextSibling).toHaveTextContent("—");
    expect(screen.getByText("Delay").nextSibling).toHaveTextContent("1.5 s");
    const sound = screen.getByRole("button", { name: "Sound off" });
    fireEvent.click(sound);
    expect(screen.getByRole("button", { name: "Sound on" })).toHaveAttribute("aria-pressed", "true");
  });

  it("a held training step says feedback is held", () => {
    render(ui(training({ state: { ...INACTIVE_TRAINING, phase: "held", stage: "training", label: "Blink", hold: "blink" } }), true));
    expect(screen.getByText("Feedback held")).toBeInTheDocument();
  });

  it("the summary shows the real numbers, never a made-up one, and Done dismisses it", () => {
    const finished = {
      protocol_name: "Calm", time_in_target_pct: 63.4, heart_rate: null,
      guided: { trained_seconds: 600, held_seconds: { blink: 30, jaw: 0 }, reward_schedule: { threshold_scale: 0.9 }, alpha_eyes_closed_ratio: 1.5 },
    };
    const t = training({ finished });
    render(ui(t, false));
    expect(screen.getByRole("heading", { name: "Calm" })).toBeInTheDocument();
    expect(screen.getByText("63%")).toBeInTheDocument();
    expect(screen.getByText("Trained").nextSibling).toHaveTextContent("10 min");
    expect(screen.getByText("Held").nextSibling).toHaveTextContent("30 s");
    expect(screen.getByText(/Most of the held time was blinks/)).toBeInTheDocument();
    expect(screen.getByText(/Alpha rose 1\.5×/)).toBeInTheDocument();
    expect(screen.queryByText(/Heart rate/)).toBeNull();
    expect(screen.getByRole("button", { name: "Save note" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect((t as unknown as { dismissFinished: ReturnType<typeof vi.fn> }).dismissFinished).toHaveBeenCalledOnce();
  });
});
