import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const voice = vi.hoisted(() => ({
  getVoiceStatus: vi.fn(), getVoiceTurns: vi.fn(), listVoices: vi.fn(), setVoice: vi.fn(), startVoice: vi.fn(), stopVoice: vi.fn(),
}));
vi.mock("../bridge", async (orig) => ({ ...(await orig<typeof import("../bridge")>()), ...voice }));

import type { VoiceStatus, VoiceTurn } from "../types";
import { STATE_VIEW, VOICE_STATUS_POLL_MS, VoiceField, turnLatency } from "./VoiceScreen";

const status = (over: Partial<VoiceStatus> = {}): VoiceStatus => ({
  installed: true, running: true, pid: 4242, state: "idle", state_age_s: 3, voice: "neosoul_g", rss_mb: 3123.4,
  launchd: false, dir: "/Users/x/projects/amara-voice", ...over,
});
const turn = (over: Partial<VoiceTurn> = {}): VoiceTurn => ({
  time: "2026-10-31T18:01:00+0900", heard: "Amara, what time is it?", text: "what time is it?", route: "hermes",
  reply: "It's six p.m.", asr_s: 0.42, token_s: 2.1, first_audio_s: 5.4, total_s: 9.0, barged: null, ...over,
});

async function renderField() {
  render(<VoiceField />);
  await act(async () => { await Promise.resolve(); });
}

describe("Chat & Agents › Voice", () => {
  beforeEach(() => {
    Object.values(voice).forEach((f) => f.mockReset());
    voice.getVoiceStatus.mockResolvedValue(status());
    voice.getVoiceTurns.mockResolvedValue([]);
    voice.listVoices.mockResolvedValue({ voices: ["neosoul", "neosoul_g"], current: "neosoul_g" });
  });
  afterEach(() => vi.useRealTimers());

  it("every state word has a label; latency text", () => {
    for (const s of ["loading", "idle", "listening", "heard", "thinking", "speaking", "error", "stopped", "not_installed"] as const) {
      expect(STATE_VIEW[s].label).toBeTruthy();
    }
    expect(turnLatency(turn())).toBe("ASR 0.4 s · first token 2.1 s · first audio 5.4 s");
    expect(turnLatency(turn({ route: "local", token_s: null, first_audio_s: null, total_s: 1.2 }))).toBe("ASR 0.4 s · done 1.2 s");
  });

  it("not installed: clear inactive state, no Start", async () => {
    voice.getVoiceStatus.mockResolvedValue(status({ installed: false, running: false, state: "not_installed", pid: null }));
    await renderField();
    expect(screen.getByText("Amara Voice isn’t set up on this Mac")).toBeInTheDocument();
    expect(screen.getByText("Not installed")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Start" })).toBeNull();
  });

  it("running: live state, facts, Stop; polls status", async () => {
    vi.useFakeTimers();
    voice.getVoiceStatus.mockResolvedValueOnce(status({ state: "speaking" })).mockResolvedValue(status({ state: "listening" }));
    await renderField();
    const card = screen.getByRole("region", { name: "Amara Voice" });
    expect(within(card).getByText("Speaking")).toBeInTheDocument();
    expect(within(card).getByText("3,123 MB")).toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Stop" })).toBeInTheDocument();
    await act(async () => { vi.advanceTimersByTime(VOICE_STATUS_POLL_MS); await Promise.resolve(); });
    expect(within(card).getByText("Listening")).toBeInTheDocument();
  });

  it("stopped: Start calls voice_start", async () => {
    voice.getVoiceStatus.mockResolvedValue(status({ running: false, state: "stopped", pid: null, rss_mb: null }));
    voice.startVoice.mockResolvedValue(status({ state: "loading" }));
    await renderField();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Start" })); await Promise.resolve(); });
    expect(voice.startVoice).toHaveBeenCalled();
    expect(screen.getByText("Loading")).toBeInTheDocument();
  });

  it("start failure is shown", async () => {
    voice.getVoiceStatus.mockResolvedValue(status({ running: false, state: "stopped" }));
    voice.startVoice.mockRejectedValue(new Error("Python not found at /nope"));
    await renderField();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Start" })); await Promise.resolve(); });
    expect(screen.getByRole("alert")).toHaveTextContent("Python not found");
  });

  it("recent turns newest first, with route badge and latencies", async () => {
    voice.getVoiceTurns.mockResolvedValue([
      turn({ text: "what time is it?" }),
      turn({ text: "", heard: "the akiya in Utsunomiya", route: "ignored", ignored: "hint-echo", reply: "" }),
      turn({ text: "Zugeloid", route: "repeat", reply: "Sorry, say that again?", token_s: null, first_audio_s: null }),
    ]);
    await renderField();
    const rows = within(screen.getByRole("list", { name: "Recent turns" })).getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("Zugeloid");
    expect(rows[0]).toHaveTextContent("Asked to repeat");
    expect(rows[1]).toHaveTextContent("Not acted on: hint-echo");
    expect(rows[2]).toHaveTextContent("first audio 5.4 s");
  });

  it("voice picker saves and offers a restart while running", async () => {
    voice.setVoice.mockResolvedValue({ voices: ["neosoul", "neosoul_g"], current: "neosoul" });
    await renderField();
    await act(async () => { fireEvent.change(screen.getByLabelText("Voice"), { target: { value: "neosoul" } }); await Promise.resolve(); });
    expect(voice.setVoice).toHaveBeenCalledWith("neosoul");
    expect(screen.getByText(/Restart to switch to neosoul/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Restart" })).toBeInTheDocument();
  });
});
