import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProposalsCard, WatchlistCard } from "../Trading";
import { ContentItemTools, LiveFinancesCard } from "../EnterpriseWiring";
import { TreasuryBody } from "../TreasuryView";
import { UNWIRED } from "../../lib/sampleAdapters";
import { liveTreasury, liveTreasuryMonitor } from "../../business/treasury";
import type { EnterpriseLive } from "../enterpriseLive";
import type { LiveMarket } from "../marketProposalsLive";

const live = <T,>(value: T) => ({ state: "live" as const, value });

describe("Trading: record toast + Undo, watchlist", () => {
  const proposal = { id: "p1", side: "watch", kind: "stock", symbol: "ABC", title: "Watch ABC", reason: "r", source: "s", confidence: "low", risk: "k" };
  it("shows the backend toast after the second confirm and Undo calls undoWatch", async () => {
    const undoWatch = vi.fn(async () => true);
    const a = {
      proposals: () => live([proposal]), market: () => UNWIRED, newsNote: () => UNWIRED, choices: () => live([]),
      recordChoice: vi.fn(async () => live({ proposalId: "p1", choice: "approved", recordedAt: "t", toast: "Recorded. Watch-only — no order was sent.", historyId: "h1" })),
      undoWatch,
    } as unknown as LiveMarket;
    render(<ProposalsCard adapter={a} />);
    expect(screen.getAllByText("Not financial advice").length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    fireEvent.click(await screen.findByRole("button", { name: "Confirm" }));
    expect(await screen.findByText(/Recorded\. Watch-only/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(undoWatch).toHaveBeenCalledWith("h1"));
  });
  it("watchlist: null price reads No price yet; research states and drop + Undo", async () => {
    const w = { picks: [{ ticker: "ABC", kind: "stock", reason: "why", source: "src", sourceUrl: null, pickedAt: "", nextReviewAt: "", quote: { price: null, currency: null, fetchedAt: null, status: "stale", error: null } }], feed: { state: "connected" }, research: { running: false }, reviewDue: false, disclaimer: "" };
    const a = {
      watchlist: () => live(w),
      researchNow: vi.fn(async () => live({ state: "rate_limited", retryAfterMinutes: 30 })),
      dropPick: vi.fn(async () => live({ historyId: "h2" })),
      undoWatch: vi.fn(async () => true),
    } as unknown as LiveMarket;
    render(<WatchlistCard adapter={a} />);
    expect(screen.getByText(/No price yet/)).toBeTruthy();
    expect(screen.getByText(/Price is stale/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Research now" }));
    expect(await screen.findByText(/Try again in 30 minutes/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Drop ABC" }));
    fireEvent.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(a.undoWatch).toHaveBeenCalledWith("h2"));
  });
});

describe("Enterprise wiring", () => {
  it("finances: fx line shows as-of and the stale note; no yen sign", () => {
    const a = { fx: () => live({ usdPerJpy: 0.0065, asOf: "2026-10-01", ageDays: 40, stale: true }), funds: () => live([{ name: "Taiyo", balanceJpy: 100, balanceUsd: null }]), sources: () => UNWIRED } as unknown as EnterpriseLive;
    const { container } = render(<LiveFinancesCard adapter={a} />);
    expect(screen.getByTestId("fx-line").textContent).toMatch(/as of 2026-10-01.*out of date/);
    expect(screen.getByText("Dollar figure needs a rate")).toBeTruthy();
    expect(container.textContent).not.toContain("¥");
  });
  it("content: Published asks Did you post it? then postedByMe; Undo; draft shows as model draft and saves", async () => {
    const a = {
      setStage: vi.fn(async () => live({ id: "c1", title: "T", from: "review", to: "published", toast: "Moved to Published." })),
      undoStage: vi.fn(async () => true),
      draft: vi.fn(async () => live({ title: "T", text: "Hello draft", tier: "local" })),
      saveDraft: vi.fn(async () => live({ id: "d1", title: "T", file: "f", bytes: 1, toast: "Draft saved." })),
      undoDraft: vi.fn(async () => true),
    } as unknown as EnterpriseLive;
    render(<ContentItemTools title="T" stage="review" adapter={a} />);
    fireEvent.change(screen.getByLabelText(/Move/), { target: { value: "published" } });
    fireEvent.click(screen.getByRole("button", { name: "Yes, I posted it" }));
    await waitFor(() => expect(a.setStage).toHaveBeenCalledWith("T", "review", "published", true));
    fireEvent.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(a.undoStage).toHaveBeenCalledWith("c1"));
    fireEvent.click(screen.getByRole("button", { name: "Draft with Amara" }));
    expect(await screen.findByText(/Model draft/)).toBeTruthy();
    expect(screen.getByText("Hello draft")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Save draft" }));
    expect(await screen.findByText("Draft saved.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(a.undoDraft).toHaveBeenCalledWith("d1"));
  });
});

describe("Treasury", () => {
  it("empty states with no data, and nothing invented", () => {
    render(<TreasuryBody treasury={liveTreasury(null)} monitor={liveTreasuryMonitor(null)} />);
    expect(screen.getByText(/No income streams yet/)).toBeTruthy();
    expect(screen.getByText(/No income recorded this month/)).toBeTruthy();
    expect(screen.queryByTestId("tr-review")).toBeNull();
  });
  it("stale wins over ok; ok carries its as-of date", () => {
    const flag = { id: "f", kind: "stale_data", streamId: null, sequenceId: null, since: "2026-11-01T00:00:00Z", detail: "", acked: false };
    const stale = liveTreasuryMonitor({ flags: [flag], monitor: { status: { state: "ok", asOf: "2026-11-01" } }, review: null });
    render(<TreasuryBody treasury={liveTreasury(null)} monitor={stale} />);
    expect(screen.getByTestId("tr-stale")).toBeTruthy();
    expect(screen.queryByTestId("tr-ok")).toBeNull();
  });
  it("ok shows ok as of", () => {
    const ok = liveTreasuryMonitor({ flags: [], monitor: { status: { state: "ok", asOf: "2026-11-15" } }, review: null });
    render(<TreasuryBody treasury={liveTreasury(null)} monitor={ok} />);
    expect(screen.getByTestId("tr-ok").textContent).toContain("ok as of 2026-11-15");
  });
});

import { LiveHomeDevices } from "../LiveHomeDevices";
describe("Smart home live devices", () => {
  const dev = (o: object) => ({ entityId: "x.1", name: "N", deviceType: "light", room: "Living", state: "off", unit: null, system: false, controllable: true, services: ["turn_on", "turn_off"], brightnessPct: null, percentage: null, temperature: null, currentTemperature: null, minTemperature: null, maxTemperature: null, hvacModes: [], volume: null, levelService: null, level: null, levelMax: null, ...o });
  it("rooms, controls via ha_device_set, undo; scenes/covers say approval + can't be undone; status-only has no control", async () => {
    const calls = {
      list: vi.fn(async () => ({ roomsAvailable: true, devices: [dev({ entityId: "light.a", name: "Lamp" }), dev({ entityId: "scene.e", name: "Evening", deviceType: "scene", state: null, services: ["turn_on"] }), dev({ entityId: "cover.c", name: "Blind", deviceType: "cover", state: "closed", services: ["open_cover", "close_cover"] }), dev({ entityId: "sensor.t", name: "Temp", deviceType: "sensor", controllable: false, services: [], state: "21", unit: "°C" })] })),
      set: vi.fn(async () => ({ id: "h9", undoable: true, message: "Lamp is on." })),
      undo: vi.fn(async () => true),
    };
    render(<LiveHomeDevices calls={calls as never} />);
    expect(await screen.findByText("Living")).toBeTruthy();
    expect(screen.getAllByText(/Asks for your approval on this Mac/).length).toBe(2);
    expect(screen.getByText(/status only/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Temp/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Lamp/ }));
    await waitFor(() => expect(calls.set).toHaveBeenCalledWith("light.a", "turn_on", null, "typed"));
    fireEvent.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(calls.undo).toHaveBeenCalledWith("h9"));
  });
});

describe("Smart home live level sliders", () => {
  const dev = (o: object) => ({ entityId: "light.a", name: "Lamp", deviceType: "light", room: "Living", state: "on", unit: null, system: false, controllable: true, services: ["turn_on", "turn_off", "toggle"], brightnessPct: 50, percentage: null, temperature: null, currentTemperature: null, minTemperature: null, maxTemperature: null, hvacModes: [], volume: null, levelService: "turn_on", level: 50, levelMax: 100, ...o });
  const mk = (devices: object[], set = vi.fn(async () => ({ id: "h1", undoable: true, message: "Done. Undo is available for a few seconds." }))) => {
    const calls = { list: vi.fn(async () => ({ roomsAvailable: true, devices })), set, undo: vi.fn(async () => true) };
    return calls;
  };
  it("has a labelled slider with percent text, commits on release only, then offers Undo", async () => {
    const calls = mk([dev({})]);
    render(<LiveHomeDevices calls={calls as never} />);
    const sl = (await screen.findByRole("slider", { name: "Lamp brightness" })) as HTMLInputElement;
    expect(sl.getAttribute("aria-valuetext")).toBe("50%");
    fireEvent.change(sl, { target: { value: "62" } });
    fireEvent.change(sl, { target: { value: "70" } });
    expect(sl.getAttribute("aria-valuetext")).toBe("70%");
    await new Promise((r) => setTimeout(r, 400));
    expect(calls.set).not.toHaveBeenCalled();
    fireEvent.pointerUp(sl);
    fireEvent.pointerUp(sl);
    await waitFor(() => expect(calls.set).toHaveBeenCalledTimes(1));
    expect(calls.set).toHaveBeenCalledWith("light.a", "turn_on", 70, "typed");
    fireEvent.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(calls.undo).toHaveBeenCalledWith("h1"));
  });
  it("commits on a key release and sends media volume as 0..1", async () => {
    const calls = mk([dev({ entityId: "media_player.tv", name: "TV", deviceType: "media", services: ["media_play", "media_pause", "volume_set"], levelService: "volume_set", level: 0.3, levelMax: 1 })]);
    render(<LiveHomeDevices calls={calls as never} />);
    const sl = await screen.findByRole("slider", { name: "TV volume" });
    expect((sl as HTMLInputElement).value).toBe("30");
    fireEvent.change(sl, { target: { value: "40" } });
    fireEvent.keyUp(sl, { key: "Tab" });
    await new Promise((r) => setTimeout(r, 400));
    expect(calls.set).not.toHaveBeenCalled();
    fireEvent.keyUp(sl, { key: "ArrowRight" });
    await waitFor(() => expect(calls.set).toHaveBeenCalledWith("media_player.tv", "volume_set", 0.4, "typed"));
  });
  it("is disabled while busy and absent for devices without level support", async () => {
    let release: (v: { id: string; undoable: boolean; message: string }) => void = () => {};
    const set = vi.fn(() => new Promise<{ id: string; undoable: boolean; message: string }>((r) => { release = r; }));
    const calls = mk([dev({}), dev({ entityId: "light.p", name: "Plain", levelService: null, level: null })], set as never);
    render(<LiveHomeDevices calls={calls as never} />);
    expect(screen.queryByRole("slider", { name: "Plain brightness" })).toBeNull();
    const sl = (await screen.findByRole("slider", { name: "Lamp brightness" })) as HTMLInputElement;
    fireEvent.change(sl, { target: { value: "20" } });
    fireEvent.pointerUp(sl);
    await waitFor(() => expect(set).toHaveBeenCalled());
    await waitFor(() => expect(sl.disabled).toBe(true));
    release({ id: "h2", undoable: true, message: "Done." });
    await waitFor(() => expect(sl.disabled).toBe(false));
  });
  it("covers get a position slider that says approval is needed; scenes still activate", async () => {
    const calls = mk([dev({ entityId: "cover.b", name: "Blind", deviceType: "cover", state: "open", services: ["open_cover", "close_cover", "stop_cover", "set_cover_position"], levelService: "set_cover_position", level: 60 }), dev({ entityId: "scene.e", name: "Evening", deviceType: "scene", state: null, services: ["turn_on"], levelService: null, level: null })]);
    render(<LiveHomeDevices calls={calls as never} />);
    const sl = await screen.findByRole("slider", { name: "Blind position" });
    expect(screen.getAllByText(/Asks for your approval on this Mac/).length).toBe(2);
    fireEvent.change(sl, { target: { value: "10" } });
    fireEvent.pointerUp(sl);
    await waitFor(() => expect(calls.set).toHaveBeenCalledWith("cover.b", "set_cover_position", 10, "typed"));
    fireEvent.click(screen.getByRole("button", { name: /Activate Evening/ }));
    await waitFor(() => expect(calls.set).toHaveBeenCalledWith("scene.e", "turn_on", null, "typed"));
  });
});

import { SampleHomeControls } from "../SampleHomeControls";
describe("Smart home sample sliders", () => {
  it("are tagged Sample data and never call the backend", () => {
    const invoke = vi.fn();
    (window as unknown as { __TAURI_INTERNALS__: { invoke: unknown } }).__TAURI_INTERNALS__ = { invoke };
    render(<SampleHomeControls />);
    expect(screen.getAllByText(/Sample data/).length).toBeGreaterThan(0);
    const sl = screen.getByRole("slider", { name: "Living light brightness" });
    fireEvent.change(sl, { target: { value: "30" } });
    fireEvent.pointerUp(sl);
    expect(invoke).not.toHaveBeenCalled();
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });
});
