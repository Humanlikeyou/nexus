import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { INACTIVE_TRAINING } from "../neuro/training";
import type { MuseTelemetry } from "../neuro/telemetryStore";
import type { useTraining } from "../neuro/useTraining";

const audio = vi.hoisted(() => ({ init: vi.fn(), setMode: vi.fn(), cleanup: vi.fn(), setMuted: vi.fn(), playBell: vi.fn(), updateState: vi.fn(), applyCue: vi.fn() }));
vi.mock("../legacy-core/hooks/useAudioFeedback", () => ({ useAudioFeedback: () => audio }));
vi.mock("../bridge", () => ({ loadKv: async () => null, saveKv: async () => undefined }));
import { NeuroFlow } from "./NeuroFlow";

const telemetry = { channels: { TP9: [], AF7: [], AF8: [], TP10: [] }, bands: { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 }, packetCount: 0, sampleCount: 0, lastPacketAt: null, channelLastPacketAt: {} } as unknown as MuseTelemetry;
const training = (protocolId: string) => ({ protocolId, setProtocolId: vi.fn(), protocols: [{ id: "calm", name: "Calm", eyesClosed: true }], state: INACTIVE_TRAINING, begin: vi.fn(), summary: vi.fn(), finish: vi.fn(), advance: vi.fn(), finished: null, dismissFinished: vi.fn() }) as unknown as ReturnType<typeof useTraining>;
const ui = (protocolId: string, running = true) => <NeuroFlow training={training(protocolId)} telemetry={telemetry} sessions={[]} running={running} action={null} />;

beforeEach(() => { Object.values(audio).forEach((fn) => fn.mockClear()); });

describe("Audio follows the protocol mid-run", () => {
  it("initialises once when the protocol changes from off to a real one, and tears down when it goes back", () => {
    const { rerender } = render(ui("off"));
    expect(audio.init).not.toHaveBeenCalled();
    rerender(ui("calm"));
    expect(audio.init).toHaveBeenCalledTimes(1);
    expect(audio.setMode).toHaveBeenCalledWith("tone");
    rerender(ui("calm"));
    expect(audio.init).toHaveBeenCalledTimes(1);
    rerender(ui("off"));
    expect(audio.cleanup).toHaveBeenCalledTimes(1);
  });
  it("cleans up once when the run ends, and not at all if audio never started", () => {
    const a = render(ui("calm"));
    a.rerender(ui("calm", false));
    expect(audio.cleanup).toHaveBeenCalledTimes(1);
    a.unmount();
    audio.cleanup.mockClear();
    const b = render(ui("off"));
    b.rerender(ui("off", false));
    expect(audio.cleanup).not.toHaveBeenCalled();
  });
  it("cleans up on unmount while audio is on", () => {
    const a = render(ui("calm"));
    a.unmount();
    expect(audio.cleanup).toHaveBeenCalledTimes(1);
  });
});

describe("decision 0059: the protocol's settings drive the sound", () => {
  it("plays the chosen sound, and stays silent on the held cue outside training", () => {
    const t = { ...training("calm"), settingsFor: () => ({ durationMin: 10, targetRewardRate: 0.6, sound: "soundscape", visualFeedback: false, volume: 0.4 }) } as unknown as ReturnType<typeof useTraining>;
    render(<NeuroFlow training={t} telemetry={telemetry} sessions={[]} running action={null} />);
    expect(audio.setMode).toHaveBeenCalledWith("soundscape");
    expect(audio.applyCue).toHaveBeenLastCalledWith("off", 0.4);
  });
});
