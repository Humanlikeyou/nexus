// @vitest-environment jsdom
/**
 * Smoke tests for the console shell: lazy screens resolve to real content,
 * the Neuro (Live) session timer behaves, and the isolated clock keeps App still.
 * Tauri is fully mocked; the background scheduler is stubbed out.
 */
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, act, within, cleanup } from "@testing-library/react";

const h = vi.hoisted(() => ({
  muse: { connected: false, device_name: "Muse S", streaming: false },
  appRenders: 0,
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => h.invoke(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}), emit: vi.fn() }));
vi.mock("@/terrance/system/scheduler", () => ({ startScheduler: vi.fn(), stopScheduler: vi.fn() }));
// Count App renders: App calls useSnapshot exactly once per render.
vi.mock("../../src-console/app/useSnapshot", async (importOriginal) => {
  const mod = await importOriginal<typeof import("../../src-console/app/useSnapshot")>();
  return { useSnapshot: () => { h.appRenders++; return mod.useSnapshot(); } };
});

import App from "../../src-console/App";
import { SYSTEMS } from "../../src-console/app/routes";

function installInvoke() {
  h.invoke.mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case "get_muse_state": return { ...h.muse };
      case "migrate_legacy_terrence_context": return null;
      case "start_eeg_recording": return "/tmp/qa-session.csv";
      case "start_eeg_stream": h.muse.streaming = true; return null;
      case "stop_eeg_stream": h.muse.streaming = false; return null;
      case "stop_eeg_recording": return "/tmp/qa-session.csv";
      case "save_session": return "qa-session-id";
      default: throw new Error(`not mocked: ${cmd}`); // bridge.read() falls back + console.warn
    }
  });
}

let errorSpy: ReturnType<typeof vi.spyOn>;
beforeEach(() => {
  h.muse = { connected: false, device_name: "Muse S", streaming: false };
  h.appRenders = 0;
  h.invoke.mockReset();
  installInvoke();
  delete (window as any).__nexusSchedulerStarted;
  vi.spyOn(console, "warn").mockImplementation(() => {});
  errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const nav = () => screen.getByRole("navigation", { name: "Amara OS navigation" });
const go = (label: string) => fireEvent.click(within(nav()).getByTitle(new RegExp(`^${label}( \\(|$)`)));
// Match by attribute value, not a CSS selector: labels like "Chat & Agents" break selector parsing.
const field = (label: string) => (Array.from(document.querySelectorAll<HTMLElement>("section.active-field")).find((el) => el.getAttribute("aria-label") === `${label} system`) ?? null);

describe("lazy screens", () => {
  // Pre-warm every screen chunk so lazy() resolves in microtasks, not on module-transform
  // time; makes resolution independent of machine load (fake timers throughout).
  beforeAll(async () => {
    await Promise.all([
      ...["Health", "Neuro", "Sessions", "Devices", "Settings", "Business", "Revenue", "Activity", "Lists", "Crews", "MindBody", "SmartHome"]
        .map((n) => import(`../../src-console/screens/${n}Screen.tsx`)),
      import("../../src-console/life/LifeScreen.tsx"),
    ]);
  });
  const flush = async () => { await act(async () => { await vi.advanceTimersByTimeAsync(0); }); };
  const loading = (label: string) => within(field(label)!).queryByRole("status", { name: "Loading" });
  async function settle(label: string) {
    for (let i = 0; i < 20 && (!field(label) || loading(label)); i++) await flush();
  }

  it("first visit to every screen resolves to real content without errors", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-21T10:00:00+09:00"));
    render(<App />);
    await flush();
    for (const sys of SYSTEMS) {
      if (sys.id !== "terrance") go(sys.label); // Home is where launch lands, with no button of its own
      await settle(sys.label);
      const f = field(sys.label);
      expect(f, `${sys.id}: active field`).not.toBeNull();
      expect(loading(sys.label), `${sys.id}: still on fallback`).toBeNull();
      expect(f!.textContent!.trim().length, `${sys.id}: blank render`).toBeGreaterThan(20);
      expect(f!.querySelector("h1, h2, [role=heading]"), `${sys.id}: heading`).not.toBeNull();
    }
    expect(errorSpy.mock.calls.map((c) => String(c[0]).slice(0, 200))).toEqual([]);
  });

  it("folded screens stay reachable as host sub-views or sections (ROUTE_ALIASES)", async () => {
    vi.useFakeTimers();
    render(<App />);
    await flush();
    // design-1111v: Business is Enterprise; Money lives under Life as Personal money; Work is Tasks.
    for (const [host, view] of [["Agents", "Crews"], ["Mind–Body", "Fitness"], ["Mind–Body", "Neuro"], ["Mind–Body", "Neuro history"], ["Life", "Goals"], ["Life", "Weekly review"], ["Life", "Dates & deadlines"], ["Life", "Personal money"], ["Enterprise", "Decisions"], ["Enterprise", "Ventures"], ["Enterprise", "Finances"], ["Enterprise", "Tasks"], ["Enterprise", "Content"], ["Settings", "Services"], ["Settings", "Devices"], ["Settings", "Notifications"], ["Settings", "Backup"], ["Settings", "Voice"], ["Settings", "Memory"]] as const) {
      go(host);
      await settle(host);
      fireEvent.click(within(field(host)!).getByRole("radio", { name: view }));
      await settle(host);
      const f = field(host)!;
      await vi.waitFor(() => expect(within(f).getByRole("radio", { name: view }), `${host} › ${view}`).toHaveAttribute("aria-checked", "true"));
      await settle(host);
      expect(f.textContent!.trim().length, `${host} › ${view}: blank`).toBeGreaterThan(20);
    }
    // design-1110b B: Activity is a section of Agents, Lists of Today. design-1111s: Voice is a Settings tab (above).
    for (const [host, section] of [["Agents", "activity"]] as const) {
      go(host);
      await settle(host);
      await vi.waitFor(() => expect(field(host)!.querySelector(`#${section}.page-section`), `${host} › ${section}`).not.toBeNull());
    }
    expect(errorSpy.mock.calls.map((c) => String(c[0]).slice(0, 200))).toEqual([]);
  });

  it("revisiting a screen renders synchronously (no fallback flash)", async () => {
    vi.useFakeTimers();
    render(<App />);
    await flush();
    go("Mind–Body");
    await settle("Mind–Body");
    go("Home");
    go("Mind–Body");
    expect(loading("Mind–Body")).toBeNull();
  });
});

describe("Neuro (Live) session timer (fake timers)", () => {
  const T0 = new Date("2026-10-21T10:00:00+09:00");
  const flush = async (ms = 0) => { await act(async () => { await vi.advanceTimersByTimeAsync(ms); }); };
  const readout = () => document.querySelector(".neuro__session-time .ui-stat__value")?.textContent;
  const click = async (name: RegExp) => { fireEvent.click(screen.getByRole("button", { name })); await flush(); await flush(); };
  const openNeuro = async () => { go("Mind–Body"); await flush(); fireEvent.click(within(field("Mind–Body")!).getByRole("radio", { name: "Neuro" })); await flush(); };

  async function openLive() {
    vi.useFakeTimers();
    vi.setSystemTime(T0);
    h.muse.connected = true;
    render(<App />);
    await flush(); await flush();
    await openNeuro();
    for (let i = 0; i < 5 && !document.querySelector(".neuro__session-time"); i++) await flush();
    expect(readout()).toBe("Ready");
  }

  it("ticks once per second from 00:00 without drift or double-ticks", async () => {
    await openLive();
    await click(/Start session/);
    expect(readout()).toBe("00:00");
    const seen: string[] = [];
    for (let i = 0; i < 5; i++) { await flush(1000); seen.push(readout()!); }
    expect(seen).toEqual(["00:01", "00:02", "00:03", "00:04", "00:05"]);
    await flush(60_000);
    expect(readout()).toBe("01:05");
  });

  it("keeps time across screen switches and re-renders (derived from start, no reset)", async () => {
    await openLive();
    await click(/Start session/);
    await flush(15_000);
    expect(readout()).toBe("00:15");
    go("Enterprise"); await flush(); await flush();
    await flush(10_000);
    await openNeuro(); await flush();
    expect(readout()).toBe("00:25");
    await flush(15_000); // includes a 15 s snapshot refresh re-render
    expect(readout()).toBe("00:40");
  });

  it("stops on Stop session and resets to 00:00 on the next start", async () => {
    await openLive();
    await click(/Start session/);
    await flush(7_000);
    expect(readout()).toBe("00:07");
    await click(/Stop session/);
    expect(readout()).toBe("Ready");
    await flush(5_000);
    expect(readout()).toBe("Ready");
    expect(h.invoke.mock.calls.filter(([c]) => c === "save_session")).toHaveLength(1);
    await click(/Start session/);
    expect(readout()).toBe("00:00");
    await flush(3_000);
    expect(readout()).toBe("00:03");
  });
});

describe("isolated clock", () => {
  it("toolbar clock ticking for 60 s idle does not re-render App beyond snapshot refreshes", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-21T10:00:00+09:00"));
    render(<App />);
    await act(async () => { await vi.advanceTimersByTimeAsync(1_000); });
    go("Today"); // the toolbar clock lives on every screen but the landing page
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    const clockBefore = document.querySelector(".field-clock strong")?.textContent;
    const base = h.appRenders;
    // One act() per 250 ms step: a single act() batches every update into one commit
    // and would hide per-tick re-renders.
    for (let i = 0; i < 240; i++) await act(async () => { await vi.advanceTimersByTimeAsync(250); });
    expect(document.querySelector(".field-clock strong")?.textContent).not.toBe(clockBefore);
    // 4 snapshot refreshes x (busy on, data, busy off) = at most ~12 App renders; nothing per tick.
    expect(h.appRenders - base, "App renders during 60 s idle").toBeLessThanOrEqual(12);
  });
});
