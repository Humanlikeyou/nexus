import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { INACTIVE_TRAINING } from "./training";
import type { MuseTelemetry } from "./telemetryStore";
import type { useTraining } from "./useTraining";

const audio = vi.hoisted(() => ({ init: vi.fn(), setMode: vi.fn(), cleanup: vi.fn(), setMuted: vi.fn(), playBell: vi.fn(), updateState: vi.fn(), applyCue: vi.fn() }));
vi.mock("../legacy-core/hooks/useAudioFeedback", () => ({ useAudioFeedback: () => audio }));
vi.mock("../bridge", () => ({ loadKv: async () => null, saveKv: async () => undefined }));
import { NeuroFlow } from "../screens/NeuroFlow";

const telemetry = { channels: { TP9: [], AF7: [], AF8: [], TP10: [] }, bands: { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 }, packetCount: 0, sampleCount: 0, lastPacketAt: null, channelLastPacketAt: {} } as unknown as MuseTelemetry;
const training = (protocolId: string) => ({ protocolId, setProtocolId: vi.fn(), protocols: [{ id: "calm", name: "Calm", eyesClosed: true }], state: INACTIVE_TRAINING, begin: vi.fn(), summary: vi.fn(), finish: vi.fn(), advance: vi.fn(), finished: null, dismissFinished: vi.fn() }) as unknown as ReturnType<typeof useTraining>;

describe("Recording only makes no app sound (Hemi-Sync)", () => {
  it("never starts the drone or 432 Hz tone when the protocol is off", () => {
    render(<NeuroFlow training={training("off")} telemetry={telemetry} sessions={[]} running action={null} />);
    expect(audio.init).not.toHaveBeenCalled();
    expect(audio.setMode).not.toHaveBeenCalled();
  });
  it("still starts feedback audio for a real protocol", () => {
    render(<NeuroFlow training={training("calm")} telemetry={telemetry} sessions={[]} running action={null} />);
    expect(audio.init).toHaveBeenCalled();
  });
});
