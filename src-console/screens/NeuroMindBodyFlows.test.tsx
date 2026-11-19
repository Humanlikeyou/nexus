import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import { createSampleRecordings } from "../neuro/recordings";
import type { MuseTelemetry } from "../neuro/telemetryStore";
import { INACTIVE_TRAINING } from "../neuro/training";
import type { useTraining } from "../neuro/useTraining";
import type { RecordedSession, SystemSnapshot } from "../types";
import { ASK_MORE_EVENT } from "../amaraTake/takeView";
import { NeuroField } from "./NeuroScreen";
import { SessionsField } from "./SessionsScreen";
import { MindBodyField, type MindBodyState } from "./MindBodyScreen";
import { liveRecordings } from "../neuro/recordings";
import { makeSampleHemisync, type HemisyncAdapter } from "../neuro/hemisync/adapters";

const kv = vi.hoisted(() => ({ store: new Map<string, string>(), saved: [] as Array<[string, string]> }));
vi.mock("../bridge", async (orig) => ({ ...(await orig<typeof import("../bridge")>()),
  loadKv: async (k: string) => kv.store.get(k) ?? null,
  saveKv: async (k: string, v: string) => { kv.saved.push([k, v]); kv.store.set(k, v); } }));
vi.mock("../neuro/useMuseTelemetry", () => ({ useMuseTelemetry: () => ({ channels: { TP9: [], AF7: [], AF8: [], TP10: [] }, bands: { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 }, packetCount: 0, sampleCount: 0, lastPacketAt: null, channelLastPacketAt: {} } as MuseTelemetry) }));

const session = (id: string, o: Partial<RecordedSession> = {}): RecordedSession => ({
  id, device_type: "muse", start: "2026-10-27T21:00:00+09:00", end: "2026-10-27T21:12:00+09:00", duration_secs: 720, data_points: 46080, file_path: `/x/eeg_${id}.ndjson`,
  summary: { avg_focus: 61, avg_relaxation: 48 },
  body_context: { today: { day: "2026-10-27", recovery: 72, hrv: 64, rhr: 52 }, latest_night: { day: "2026-10-27", total_sleep_min: 450 } } as never, ...o,
});
const snap = (extra: Partial<SystemSnapshot> = {}): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra });
const NOW = Date.parse("2026-11-10T18:00:00+09:00");
const training = () => ({ protocolId: "off", setProtocolId: vi.fn(), protocols: [], state: { ...INACTIVE_TRAINING }, begin: vi.fn(), summary: vi.fn(), finish: vi.fn(), advance: vi.fn(), finished: null, dismissFinished: vi.fn() }) as unknown as ReturnType<typeof useTraining>;
const neuro = (props: Partial<React.ComponentProps<typeof NeuroField>> = {}) => {
  const h = { onScan: vi.fn(), onConnect: vi.fn(), onDisconnect: vi.fn(), onStart: vi.fn(), onStop: vi.fn() };
  render(<NeuroField data={snap()} devices={[]} operation={null} error={null} sessionStartedAt={null} sessionEndedAt={null} training={training()} {...h} {...props} />);
  return h;
};
beforeEach(() => { kv.store.clear(); kv.saved.length = 0; });

describe("Mind-Body: Settings › Devices buttons (GAP 4)", () => {
  it("inactive setup opens Settings › Devices", async () => {
    const onOpen = vi.fn();
    render(<MindBodyField sessions={[]} onOpen={onOpen} load={async () => ({ status: "inactive", steps: null }) as MindBodyState} />);
    fireEvent.click(await screen.findByRole("button", { name: "Open Settings › Devices" }));
    expect(onOpen).toHaveBeenCalledWith({ id: "settings", view: "devices" });
  });
});

describe("Neuro: search, saved session, last session (GAP 5, 2, 14, 3)", () => {
  it("a search that found nothing keeps saying so, with Search again", () => {
    const h = neuro({ scanEmpty: true });
    expect(screen.getByText(/No Muse found\. Put it in pairing mode/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Search again" }));
    expect(h.onScan).toHaveBeenCalledTimes(1);
  });

  it("after Stop: a Session saved card with minutes, focus/calm, body context, Open recording and Done", () => {
    const onOpenRecording = vi.fn(), onDismissSaved = vi.fn();
    neuro({ data: snap({ sessions: [session("s1")], muse: { ...INITIAL_SNAPSHOT.muse, connected: true, device_name: "Muse S", streaming: false } }), lastSavedId: "s1", onOpenRecording, onDismissSaved });
    const card = screen.getByRole("status", { name: "Session saved" });
    expect(card).toHaveTextContent("12 min · focus 61 · calm 48");
    expect(card).toHaveTextContent("recovery that day 72%");
    expect(card).toHaveTextContent("Neuro take is updating");
    fireEvent.click(within(card).getByRole("button", { name: "Open recording ›" }));
    expect(onOpenRecording).toHaveBeenCalledWith("s1");
    fireEvent.click(within(card).getByRole("button", { name: "Done" }));
    expect(onDismissSaved).toHaveBeenCalled();
  });

  it("connected and idle still shows the last-session hero, and its chips open that recording", () => {
    const onOpenRecording = vi.fn();
    neuro({ data: snap({ sessions: [session("s1")], muse: { ...INITIAL_SNAPSHOT.muse, connected: true, device_name: "Muse S", streaming: false } }), onOpenRecording });
    expect(screen.getByLabelText("Last session")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Next step" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Open the Oct 27 recording/ }));
    expect(onOpenRecording).toHaveBeenCalledWith("s1");
  });
});

describe("Neuro history: open by id, context, ask, filter, retention (GAP 3, 9, 10, 12, 15, 7)", () => {
  const sessions = [session("a"), session("b", { start: "2026-11-07T11:57:00+09:00", end: "2026-11-07T12:08:00+09:00", duration_secs: 682 })];
  const hemisync = (): HemisyncAdapter => {
    const base = makeSampleHemisync();
    return { ...base, sessions: () => ({ state: "live", value: [{ id: "b", trackId: "s-5-1", startedAt: "2026-11-07T11:57:00+09:00", listenedMs: 1, completed: true, rating: 4, note: "Settled fast", metrics: { focusAvg: 1, calmAvg: 1, bands: null } }] }), library: () => base.library() };
  };

  it("initialOpenId opens that recording; the detail shows Hemi-Sync track and rating, body context and the session note", async () => {
    kv.store.set("neurofeedback-note:b", "Counted breaths");
    render(<SessionsField data={snap({ sessions })} adapter={liveRecordings(sessions, NOW)} initialOpenId="b" hemisync={hemisync()} />);
    expect(screen.getByRole("heading", { level: 1, name: "Recording" })).toBeInTheDocument();
    expect(screen.getByTestId("rec-hemisync")).toHaveTextContent("Advanced Focus 12");
    expect(screen.getByTestId("rec-hemisync")).toHaveTextContent("rated 4 of 5");
    expect(screen.getByLabelText("Body context at the start of this session")).toHaveTextContent("72%");
    expect(await screen.findByTestId("rec-note")).toHaveTextContent("Counted breaths");
  });

  it("Ask Amara opens the chat flow with the session facts and a question", () => {
    const seen: Array<{ question: string; chips: Array<{ label: string }> }> = [];
    const on = (e: Event) => seen.push((e as CustomEvent).detail);
    window.addEventListener(ASK_MORE_EVENT, on);
    render(<SessionsField data={snap({ sessions })} adapter={liveRecordings(sessions, NOW)} initialOpenId="b" hemisync={hemisync()} />);
    fireEvent.click(screen.getByRole("button", { name: "Ask Amara ›" }));
    window.removeEventListener(ASK_MORE_EVENT, on);
    expect(seen).toHaveLength(1);
    expect(seen[0].question).toMatch(/Muse session on Nov 7.*Advanced Focus 12/);
    expect(seen[0].chips.map((c) => c.label)).toEqual(expect.arrayContaining(["Focus", "Hemi-Sync", "Rating"]));
  });

  it("Back keeps the channel filter and reports the close", () => {
    const onClose = vi.fn();
    render(<SessionsField data={snap({ sessions })} adapter={liveRecordings(sessions, NOW)} onClose={onClose} />);
    fireEvent.click(screen.getByRole("radio", { name: "5 channels" }));
    const table = screen.getByRole("table", { name: "Recordings" });
    expect(within(table).queryAllByRole("button")).toHaveLength(0);
    fireEvent.click(screen.getByRole("radio", { name: "4 channels" }));
    fireEvent.click(within(screen.getByRole("table", { name: "Recordings" })).getAllByRole("button")[0]);
    fireEvent.click(screen.getByRole("button", { name: "‹ Neuro history" }));
    expect(onClose).toHaveBeenCalled();
    expect(screen.getByRole("radio", { name: "4 channels" })).toBeChecked();
  });

  it("Keep them is remembered: saved to kv, and the banner stays gone on the next visit", async () => {
    const old = [session("old", { start: "2026-02-18T22:51:00+09:00" }), session("new", { start: "2026-11-07T11:57:00+09:00" })];
    const first = render(<SessionsField data={snap({ sessions: old })} adapter={liveRecordings(old, NOW)} />);
    fireEvent.click(screen.getByRole("button", { name: "Keep them" }));
    expect(kv.saved).toEqual([["recordings-retention-kept", JSON.stringify(["old"])]]);
    first.unmount();
    render(<SessionsField data={snap({ sessions: old })} adapter={liveRecordings(old, NOW)} />);
    await screen.findByRole("table", { name: "Recordings" });
    await vi.waitFor(() => expect(screen.queryByText(/is older than 90 days/)).toBeNull());
  });

  it("the empty history's Start session calls the starter", () => {
    const onStartSession = vi.fn();
    render(<SessionsField data={snap({ sessions: [] })} adapter={liveRecordings([], NOW)} onStartSession={onStartSession} />);
    fireEvent.click(screen.getByRole("button", { name: "Start session" }));
    expect(onStartSession).toHaveBeenCalled();
    void createSampleRecordings;
  });
});
