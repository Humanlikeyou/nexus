import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { makeSampleHemisync, unwiredHemisync, type HemisyncAdapter, type HemisyncLibrary, type HemisyncTrack } from "../neuro/hemisync/adapters";
import { HemisyncSection, type HemisyncSectionProps } from "./HemisyncSection";

const track = (wave: number, waveName: string, order: number, title: string, min: number): HemisyncTrack => ({ id: `t-${wave}-${order}`, wave, waveName, order, title, durationMs: min * 60_000, src: `asset://hs/${wave}-${order}.m4a` });
const live = <T,>(value: T): Wired<T> => ({ state: "live", value });
const LIB: HemisyncLibrary = { waves: [{ wave: 1, waveName: "Discovery", tracks: [track(1, "Discovery", 1, "Orientation", 38)] }, { wave: 5, waveName: "Exploring", tracks: [track(5, "Exploring", 1, "Advanced Focus 12", 36), track(5, "Exploring", 4, "Intro to Focus 15", 40)] }], importedAt: "2026-11-12T10:00:00+09:00", sourceFolder: "~/Desktop/Hemisync Collection" };

function liveAdapter(over: Partial<HemisyncAdapter> = {}, lib: Wired<HemisyncLibrary> = live(LIB)): HemisyncAdapter & { linkSession: ReturnType<typeof vi.fn>; finishSession: ReturnType<typeof vi.fn>; rate: ReturnType<typeof vi.fn> } {
  const ok = live({ ok: true as const });
  return { library: () => lib, sessions: () => live([]), stats: () => live([]), importFrom: async () => live({ added: 3, skipped: 1 }), importProgress: () => null,
    linkSession: vi.fn(async () => ok), finishSession: vi.fn(async () => ok), rate: vi.fn(async () => ok), ...over } as never;
}

class FakeAudio {
  static last: FakeAudio | null = null;
  static count = 0;
  currentTime = 0; duration = 600; preload = ""; listeners: Record<string, Array<() => void>> = {};
  play = vi.fn(async () => undefined); pause = vi.fn(); load = vi.fn(); removeAttribute = vi.fn();
  constructor(public src: string) { FakeAudio.last = this; FakeAudio.count++; }
  addEventListener(name: string, fn: () => void) { (this.listeners[name] ??= []).push(fn); }
  emit(name: string) { this.listeners[name]?.forEach((fn) => fn()); }
}

beforeEach(() => { FakeAudio.last = null; FakeAudio.count = 0; vi.stubGlobal("Audio", FakeAudio); });
afterEach(() => { vi.unstubAllGlobals(); });

function setup(adapter: HemisyncAdapter, props: Partial<HemisyncSectionProps> = {}) {
  const handlers = { setProtocolId: vi.fn(), onStartMuse: vi.fn(), onStopMuse: vi.fn() };
  const base: HemisyncSectionProps = { adapter, connected: true, recording: false, sessionId: null, operation: null, protocolId: "off", ...handlers };
  const view = render(<HemisyncSection {...base} {...props} />);
  const again = (next: Partial<HemisyncSectionProps>) => view.rerender(<HemisyncSection {...base} {...props} {...next} />);
  return { ...handlers, again };
}
const pick = (title: string) => fireEvent.click(screen.getByRole("button", { name: new RegExp(`^${title}`) }));
const start = () => fireEvent.click(screen.getByRole("button", { name: "Start" }));

describe("Hemi-Sync section", () => {
  it("unwired: says it isn't connected, tags Needs wiring, never shows sample or numbers", async () => {
    setup(unwiredHemisync);
    expect(screen.getByText("Your Hemi-Sync library isn't connected yet.")).toBeInTheDocument();
    expect(screen.getAllByText("Needs wiring").length).toBeGreaterThan(0);
    expect(screen.queryByText("Sample data")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Import your collection" }));
    expect(await screen.findByText(/Importing isn't connected yet/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Start" })).toBeNull();
  });

  it("sample: groups tracks by wave, tags Sample data, and lists what was done with the Muse", () => {
    setup(makeSampleHemisync());
    expect(screen.getAllByText("Sample data").length).toBeGreaterThan(0);
    expect(screen.getByText("Wave 1 · Discovery · 2 tracks")).toBeInTheDocument();
    expect(screen.getByText("Wave 5 · Exploring · 2 tracks")).toBeInTheDocument();
    expect(screen.getByText("Wave 7 · Odyssey · 1 track")).toBeInTheDocument();
    expect(screen.getByText("5 tracks across 3 waves. Pick one, press Start, and the Muse records while it plays.")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: "What the Muse found" })).toBeInTheDocument();
  });

  it("What the Muse found: per-track focus and calm against his usual, best tracks, next track; no duplicated checklist or progress bar", () => {
    setup(makeSampleHemisync());
    const table = screen.getByRole("table", { name: "Focus and calm by track" });
    const row = within(table).getByText("Advanced Focus 12").closest("tr") as HTMLElement;
    expect(row).toHaveTextContent("62 · +3 vs usual");   // usual focus = mean(62, 55) = 58.5 -> 59
    expect(row).toHaveTextContent("71 · +2 vs usual");   // usual calm = mean(71, 66) = 68.5 -> 69
    expect(screen.getByText(/Your usual: focus 59, calm 69/)).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Best tracks so far" })).toHaveTextContent("Best for focus: Advanced Focus 12 · 62 / 100");
    expect(screen.getByRole("list", { name: "Best tracks so far" })).toHaveTextContent("Best for calm: Advanced Focus 12 · 71 / 100");
    expect(screen.getByText("Next track to try: Orientation (Wave 1 · Discovery).")).toBeInTheDocument();
    // the Wave journey already counts tracks: no second checklist, "n of m done" or progress bar here
    expect(screen.queryByText("2 of 5 done")).toBeNull();
    expect(screen.queryByRole("region", { name: /checklist$/ })).toBeNull();
    expect(screen.queryByRole("progressbar", { name: "Tracks done with the Muse" })).toBeNull();
  });

  it("opens the wave of the most recent session; wave 1 until any session exists", () => {
    const open = (label: string) => (screen.getByText(label).closest("details") as HTMLDetailsElement).open;
    const { unmount } = render(<HemisyncSection adapter={makeSampleHemisync()} connected recording={false} sessionId={null} operation={null} protocolId="off" setProtocolId={vi.fn()} onStartMuse={vi.fn()} onStopMuse={vi.fn()} />);
    expect(open("Wave 5 · Exploring · 2 tracks")).toBe(true);
    expect(open("Wave 1 · Discovery · 2 tracks")).toBe(false);
    expect(open("Wave 7 · Odyssey · 1 track")).toBe(false);
    unmount();
    setup(liveAdapter());
    expect(open("Wave 1 · Discovery · 1 track")).toBe(true);
    expect(open("Wave 5 · Exploring · 2 tracks")).toBe(false);
  });

  it("live: no sample tag and no tags at all when every value is live", () => {
    setup(liveAdapter());
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.queryByText("Needs wiring")).toBeNull();
    expect(screen.getByText(/No Hemi-Sync sessions with the Muse yet\. Pick .+ above and press Start with your Muse on\./)).toBeInTheDocument();
  });

  it("empty library: asks to import, and importing shows progress then the result", async () => {
    let resolve!: (v: Wired<{ added: number; skipped: number }>) => void;
    const progress = { copied: 12, total: 42, phase: "Copying Wave 2" };
    const adapter = liveAdapter({ importFrom: () => new Promise((r) => { resolve = r; }), importProgress: () => progress }, live({ waves: [], importedAt: null, sourceFolder: null }));
    setup(adapter);
    expect(screen.getByText("No tracks yet. Import your collection and it stays on this Mac.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Import your collection" }));
    const bar = await screen.findByRole("progressbar", { name: "Import progress" });
    expect(bar).toHaveAttribute("aria-valuenow", "29");
    expect(screen.getByText("12 of 42 · Copying Wave 2")).toBeInTheDocument();
    await act(async () => { resolve(live({ added: 42, skipped: 0 })); });
    expect(await screen.findByText("42 added, 0 already there.")).toBeInTheDocument();
  });

  it("no Muse: Start explains what is needed and starts neither audio nor the Muse", () => {
    const h = setup(liveAdapter(), { connected: false });
    pick("Advanced Focus 12");
    expect(screen.getByRole("button", { name: "Start" })).toHaveAttribute("aria-disabled", "true");
    start();
    expect(screen.getAllByText(/Connect your Muse first/).length).toBeGreaterThan(0);
    expect(FakeAudio.count).toBe(0);
    expect(h.onStartMuse).not.toHaveBeenCalled();
  });

  it("Start with no track picked asks for one", () => {
    setup(liveAdapter());
    start();
    expect(screen.getByText("Pick a track first.")).toBeInTheDocument();
    expect(FakeAudio.count).toBe(0);
  });

  it("one Start: plays the track, starts the Muse, links once the session id exists, Stop ends both and opens the rating sheet", async () => {
    const adapter = liveAdapter();
    const h = setup(adapter);
    pick("Intro to Focus 15");
    start();
    await waitFor(() => expect(h.onStartMuse).toHaveBeenCalledTimes(1));
    expect(FakeAudio.last?.src).toBe("asset://hs/5-4.m4a");
    expect(FakeAudio.last?.play).toHaveBeenCalled();
    expect(adapter.linkSession).not.toHaveBeenCalled(); // the Muse has no session id yet
    h.again({ operation: "starting" });
    h.again({ operation: null, recording: true, sessionId: "ses-9" });
    await waitFor(() => expect(adapter.linkSession).toHaveBeenCalledTimes(1));
    expect(adapter.linkSession).toHaveBeenCalledWith(expect.objectContaining({ sessionId: "ses-9", trackId: "t-5-4" }));
    h.again({ operation: null, recording: true, sessionId: "ses-9" });
    expect(adapter.linkSession).toHaveBeenCalledTimes(1);
    expect(screen.getByText(/Playing and recording/)).toBeInTheDocument();

    FakeAudio.last!.currentTime = 90;
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(h.onStopMuse).toHaveBeenCalledTimes(1);
    expect(FakeAudio.last!.pause).toHaveBeenCalled();
    await waitFor(() => expect(adapter.finishSession).toHaveBeenCalledWith({ sessionId: "ses-9", listenedMs: 90_000, completed: false }));

    const sheet = await screen.findByRole("dialog", { name: "How was that session?", hidden: true });
    fireEvent.click(within(sheet).getByRole("radio", { name: "4", hidden: true }));
    fireEvent.change(within(sheet).getByLabelText("Note (optional)"), { target: { value: " Settled fast " } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Save", hidden: true }));
    await waitFor(() => expect(adapter.rate).toHaveBeenCalledWith({ sessionId: "ses-9", rating: 4, note: "Settled fast" }));
  });

  it("decision 0059: pause keeps the Muse recording; seek stays inside the track; volume applies", async () => {
    const adapter = liveAdapter();
    const h = setup(adapter);
    pick("Intro to Focus 15");
    start();
    await waitFor(() => expect(h.onStartMuse).toHaveBeenCalledTimes(1));
    h.again({ operation: "starting" });
    h.again({ operation: null, recording: true, sessionId: "ses-9" });
    const el = FakeAudio.last! as FakeAudio & { paused?: boolean; volume?: number };
    el.paused = false;
    el.pause.mockImplementation(() => { el.paused = true; });
    el.play.mockImplementation(async () => { el.paused = false; });
    fireEvent.click(await screen.findByRole("button", { name: "Pause" }));
    expect(el.pause).toHaveBeenCalled();
    expect(screen.getByText("Paused. The Muse is still recording.")).toBeInTheDocument();
    expect(h.onStopMuse).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    await waitFor(() => expect(screen.queryByText("Paused. The Muse is still recording.")).not.toBeInTheDocument());
    el.currentTime = 20;
    fireEvent.click(screen.getByRole("button", { name: "Back 30 seconds" }));
    expect(el.currentTime).toBe(0);
    el.currentTime = 590;
    fireEvent.click(screen.getByRole("button", { name: "Forward 30 seconds" }));
    expect(el.currentTime).toBe(599.5);
    fireEvent.change(screen.getByLabelText("Volume"), { target: { value: "0.4" } });
    expect(el.volume).toBe(0.4);
  });

  it("track end: stops the Muse too and finishes as completed", async () => {
    const adapter = liveAdapter();
    const h = setup(adapter);
    pick("Orientation");
    start();
    await waitFor(() => expect(h.onStartMuse).toHaveBeenCalled());
    h.again({ operation: null, recording: true, sessionId: "ses-1" });
    await waitFor(() => expect(adapter.linkSession).toHaveBeenCalled());
    h.again({ operation: null, recording: true, sessionId: "ses-1" });
    act(() => FakeAudio.last!.emit("ended"));
    expect(h.onStopMuse).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(adapter.finishSession).toHaveBeenCalledWith({ sessionId: "ses-1", listenedMs: 600_000, completed: true }));
  });

  it("the Muse failing to start stops the audio, links nothing and says so", async () => {
    const adapter = liveAdapter();
    const h = setup(adapter);
    pick("Orientation");
    start();
    await waitFor(() => expect(h.onStartMuse).toHaveBeenCalled());
    h.again({ operation: "starting" });
    h.again({ operation: null, recording: false });
    await waitFor(() => expect(FakeAudio.last!.pause).toHaveBeenCalled());
    expect(adapter.linkSession).not.toHaveBeenCalled();
    expect(adapter.finishSession).not.toHaveBeenCalled();
    expect(screen.getByText("The Muse didn't start, so the track was stopped.")).toBeInTheDocument();
  });

  it("a track that can't play never starts the Muse", async () => {
    const h = setup(liveAdapter());
    pick("Orientation");
    vi.stubGlobal("Audio", class extends FakeAudio { play = vi.fn(async () => { throw new Error("blocked"); }); });
    start();
    expect(await screen.findByText(/couldn't be played, so the Muse wasn't started either/)).toBeInTheDocument();
    expect(h.onStartMuse).not.toHaveBeenCalled();
  });

  it("feedback protocols are switched to record-only first; the Muse starts once the switch lands", async () => {
    const h = setup(liveAdapter(), { protocolId: "calm" });
    pick("Orientation");
    start();
    await waitFor(() => expect(h.setProtocolId).toHaveBeenCalledWith("off"));
    expect(h.onStartMuse).not.toHaveBeenCalled();
    h.again({ protocolId: "off" });
    await waitFor(() => expect(h.onStartMuse).toHaveBeenCalledTimes(1));
  });

  it("a recording that is already running: the track joins it and links to its session", async () => {
    const adapter = liveAdapter();
    const h = setup(adapter, { recording: true, sessionId: "ses-live" });
    pick("Orientation");
    start();
    await waitFor(() => expect(adapter.linkSession).toHaveBeenCalledWith(expect.objectContaining({ sessionId: "ses-live", trackId: "t-1-1" })));
    expect(h.onStartMuse).not.toHaveBeenCalled();
  });

  it("an unsaved link is reported, not hidden", async () => {
    const adapter = liveAdapter({ linkSession: vi.fn(async () => UNWIRED) as never });
    setup(adapter, { recording: true, sessionId: "s" });
    pick("Orientation");
    start();
    expect(await screen.findByText(/link isn't saved yet/)).toBeInTheDocument();
  });

  it("won't join a running training session that plays feedback sounds", async () => {
    const { onStartMuse } = setup(liveAdapter(), { recording: true, sessionId: "s1", protocolId: "calm" });
    pick("Orientation"); start();
    expect(await screen.findByText(/Hemi-Sync records only/)).toBeInTheDocument();
    expect(FakeAudio.count).toBe(0);
    expect(onStartMuse).not.toHaveBeenCalled();
  });
});

describe("Hemi-Sync workflow gaps", () => {
  it("no Muse: the note offers Find Muse", () => {
    const onFindMuse = vi.fn();
    setup(liveAdapter(), { connected: false, onFindMuse });
    fireEvent.click(screen.getByRole("button", { name: "Find Muse" }));
    expect(onFindMuse).toHaveBeenCalledTimes(1);
  });

  it("the Next-track card has its own Start: Pick and start picks the track and starts it, outside the collapsed list", async () => {
    const h = setup(liveAdapter());
    const card = screen.getByRole("region", { name: "Next track" });
    fireEvent.click(within(card).getByRole("button", { name: "Pick and start" }));
    await waitFor(() => expect(h.onStartMuse).toHaveBeenCalledTimes(1));
    expect(FakeAudio.last?.src).toBe("asset://hs/1-1.m4a");
  });

  it("journey rows pick a track, and the card then says Start this track", () => {
    setup(liveAdapter());
    fireEvent.click(screen.getByRole("button", { name: "Choose Orientation" }));
    const card = screen.getByRole("region", { name: "Next track" });
    expect(within(card).getByRole("button", { name: "Start this track" })).toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Picked" })).toBeDisabled();
  });

  it("saving a rating with no number says why and saves nothing; a saved rating confirms with a notice", async () => {
    const notices: string[] = [];
    const on = (e: Event) => notices.push((e as CustomEvent).detail.message);
    window.addEventListener("nexus:notice", on);
    const adapter = liveAdapter();
    const h = setup(adapter);
    pick("Orientation"); start();
    await waitFor(() => expect(h.onStartMuse).toHaveBeenCalled());
    h.again({ operation: null, recording: true, sessionId: "ses-3" });
    await waitFor(() => expect(adapter.linkSession).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    const sheet = await screen.findByRole("dialog", { name: "How was that session?", hidden: true });
    fireEvent.click(within(sheet).getByRole("button", { name: "Save", hidden: true }));
    expect(within(sheet).getByText(/Nothing was saved\. Pick a number/)).toBeInTheDocument();
    expect(adapter.rate).not.toHaveBeenCalled();
    fireEvent.click(within(sheet).getByRole("radio", { name: "5", hidden: true }));
    fireEvent.click(within(sheet).getByRole("button", { name: "Save", hidden: true }));
    await waitFor(() => expect(adapter.rate).toHaveBeenCalled());
    await waitFor(() => expect(notices).toContain("Rated Orientation 5 of 5"));
    window.removeEventListener("nexus:notice", on);
  });

  const withSessions = (rating: 1 | 2 | 3 | 4 | 5 | null) => liveAdapter({
    sessions: () => live([{ id: "ses-7", trackId: "t-5-1", startedAt: "2026-11-10T21:30:00+09:00", listenedMs: 60_000, completed: true, rating, note: null, metrics: { focusAvg: 60, calmAvg: 70, bands: null } }]),
    stats: () => live([{ trackId: "t-5-1", sessions: 1, lastAt: "2026-11-10T21:30:00+09:00", avgRating: rating, focusAvg: 60, calmAvg: 70 }]),
  });

  it("What the Muse found: one scored session draws no trend and no 'best'", () => {
    setup(withSessions(null));
    expect(screen.getByText(/Your usual appears after two scored sessions\./)).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Best tracks so far" })).toBeNull();
    expect(screen.queryByText(/A trend appears/)).toBeNull();
    expect(screen.queryByLabelText(/across your Hemi-Sync sessions/)).toBeNull();
  });

  it("What the Muse found: an unrated session gets Rate, and a row opens its recording", async () => {
    const adapter = withSessions(null);
    const onOpenRecording = vi.fn();
    setup(adapter, { onOpenRecording });
    const table = screen.getByRole("table", { name: "Focus and calm by track" });
    fireEvent.click(within(table).getByRole("button", { name: "Open the recording of Advanced Focus 12" }));
    expect(onOpenRecording).toHaveBeenCalledWith("ses-7");
    fireEvent.click(within(screen.getByRole("list", { name: "Sessions not rated yet" })).getByRole("button", { name: "Rate Advanced Focus 12" }));
    const sheet = await screen.findByRole("dialog", { name: "How was that session?", hidden: true });
    fireEvent.click(within(sheet).getByRole("radio", { name: "4", hidden: true }));
    fireEvent.click(within(sheet).getByRole("button", { name: "Save", hidden: true }));
    await waitFor(() => expect(adapter.rate).toHaveBeenCalledWith({ sessionId: "ses-7", rating: 4 }));
  });

  it("a rated row has no Rate button", () => {
    setup(withSessions(4));
    expect(screen.queryByRole("button", { name: /^Rate / })).toBeNull();
  });

  it("Best tracks for you shows the backend ranking, with the plain caveat", async () => {
    const ranking = async () => ({ minSessions: 3, focusField: "focusAvg", calmField: "calmAvg",
      focus: { ranked: [{ rank: 1, trackId: "t-5-1", n: 4, mean: 71.2, sessionIds: [] }], notEnoughSessions: [] },
      calm: { ranked: [], notEnoughSessions: [] }, rating: { ranked: [{ rank: 1, trackId: "t-1-1", n: 3, mean: 4.5, sessionIds: [] }], notEnoughSessions: [] } });
    setup(liveAdapter(), { ranking });
    expect(await screen.findByRole("heading", { name: "Best tracks for you" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Best tracks by focus" })).toHaveTextContent("Advanced Focus 12 · 71 / 100");
    expect(screen.getByRole("list", { name: "Best tracks by rating" })).toHaveTextContent("Orientation · 4.5 / 5");
    expect(screen.getByText(/Plain averages over your own sessions/)).toBeInTheDocument();
  });
});

describe("Hemi-Sync marked done (0045)", () => {
  it("Mark wave done and per-track Mark done call the adapter; a marked track shows Marked done and Undo", async () => {
    const markDone = vi.fn(async () => live({ ok: true as const }));
    const markWaveDone = vi.fn(async () => live({ ok: true as const }));
    setup(liveAdapter({ markDone, markWaveDone } as never));
    fireEvent.click(screen.getAllByRole("button", { name: "Mark wave done" })[0]);
    await waitFor(() => expect(markWaveDone).toHaveBeenCalledWith(1, true));
    fireEvent.click(screen.getByRole("button", { name: "Mark Orientation done" }));
    await waitFor(() => expect(markDone).toHaveBeenCalledWith(["t-1-1"], true));
  });
  it("a marked wave counts as done everywhere: Marked done label, Undo, and the next track moves on", () => {
    const marked: HemisyncLibrary = { ...LIB, waves: LIB.waves.map((w) => (w.wave === 1 ? { ...w, tracks: w.tracks.map((t) => ({ ...t, markedDone: true })) } : w)) };
    setup(liveAdapter({}, live(marked)));
    fireEvent.click(screen.getByText("All tracks by wave"));
    fireEvent.click(screen.getByText(/^Wave 1 · Discovery/));
    expect(screen.getAllByText(/Marked done/).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Undo marked done for Orientation" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Undo wave done" })).toBeTruthy();
    expect(screen.getByText(/Pick Advanced Focus 12 above/)).toBeTruthy();
  });
});
