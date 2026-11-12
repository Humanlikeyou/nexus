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
const pick = (title: string) => fireEvent.click(screen.getByRole("button", { name: new RegExp(title) }));
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
    const table = screen.getByRole("table", { name: "Tracks done with the Muse" });
    expect(within(table).getByText("Advanced Focus 12")).toBeInTheDocument();
    expect(within(table).getByText("4.0 / 5")).toBeInTheDocument();
    expect(screen.getByText("2 of 5 done")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Tracks done with the Muse" })).toHaveAttribute("aria-valuenow", "2");
  });

  it("checklist: every track by wave with a done tick (text, not colour alone) and per-wave counts", () => {
    setup(makeSampleHemisync());
    const w5 = screen.getByRole("region", { name: "Wave 5 Exploring checklist" });
    expect(within(w5).getByText("2 of 2")).toBeInTheDocument();
    expect(within(w5).getByText(/Advanced Focus 12/)).toHaveTextContent("done");
    const w1 = screen.getByRole("region", { name: "Wave 1 Discovery checklist" });
    expect(within(w1).getByText("0 of 2")).toBeInTheDocument();
    expect(within(w1).getByText(/Orientation/)).toHaveTextContent("not done yet");
    expect(screen.getByRole("region", { name: "Wave 7 Odyssey checklist" })).toBeInTheDocument();
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
    expect(screen.getByText("0 of 3 done")).toBeInTheDocument();
  });

  it("live: no sample tag and no tags at all when every value is live", () => {
    setup(liveAdapter());
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.queryByText("Needs wiring")).toBeNull();
    expect(screen.getByText("No Hemi-Sync sessions yet. Pick a track above and press Start with your Muse on.")).toBeInTheDocument();
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
});
