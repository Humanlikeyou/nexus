import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => { while (a.length && a[a.length - 1] === undefined) a.pop(); return (invoke as (...x: unknown[]) => unknown)(...a); } }));

import { makeLivePersonal } from "../personal/adapters";
import { Composer } from "./Composer";
import { DICTATE_MAX_CHARS, dictateOnce, dictationReady } from "./dictation";
import type { AddAdapter } from "./flow";
// @ts-expect-error vite ?raw import (no ambient type in this project)
import composerSource from "./Composer.tsx?raw";

type Fake = { ready: boolean; text: string | null | "throw" };
let fake: Fake;
const calls = (cmd: string) => invoke.mock.calls.filter(([c]) => c === cmd);

beforeEach(() => {
  invoke.mockReset();
  fake = { ready: true, text: "run three times a week" };
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === "voice_dictate_ready") return fake.ready;
    if (cmd === "dictate_once") { if (fake.text === "throw") throw new Error("down"); return fake.text; }
    throw new Error(`unexpected command ${cmd}`);
  });
});

const parse = vi.fn(async (t: string) => ({ state: "live" as const, value: { kind: "preview" as const, preview: { title: t, fields: [] as [string, string][], payload: {} } } }));
const save = vi.fn(async () => ({ state: "live" as const, value: { id: "1", title: "x" } }));
const adapter = (): AddAdapter => ({ command: "x", wired: true, parse, save, undo: async () => ({ state: "live", value: { ok: true as const } }), history: () => ({ state: "live", value: [] }) });
const mount = () => render(<Composer title="New goal" kind="Habit" placeholder="p" hint="h" working="w" adapter={adapter()} savedText={() => "ok"} />);
const mic = () => document.querySelector<HTMLButtonElement>(".v-mic")!;

describe("dictation (W4)", () => {
  beforeEach(() => { parse.mockClear(); save.mockClear(); });

  it("ready only when the backend says so; any error is not ready", async () => {
    expect(await dictationReady()).toBe(true);
    fake.ready = false;
    expect(await dictationReady()).toBe(false);
    invoke.mockImplementation(async () => { throw new Error("no ipc"); });
    expect(await dictationReady()).toBe(false);
  });

  it("returns trimmed, capped text; null on nothing, error or not ready", async () => {
    fake.text = `  ${"y ".repeat(600)}  `;
    const t = (await dictateOnce("add"))!;
    expect(t.length).toBeLessThanOrEqual(DICTATE_MAX_CHARS);
    expect(t.startsWith("y y")).toBe(true);
    fake.text = null; expect(await dictateOnce("add")).toBeNull();
    fake.text = "throw"; expect(await dictateOnce("checkin")).toBeNull();
    expect((calls("dictate_once")[0][1] as { purpose: string }).purpose).toBe("add");
  });

  it("the mic stays disabled without a ready loop, and a typed add never dictates", async () => {
    fake.ready = false;
    mount();
    await waitFor(() => expect(calls("voice_dictate_ready")).toHaveLength(1));
    expect(mic().disabled).toBe(true);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "run daily" } });
    fireEvent.submit(document.querySelector("form")!);
    await waitFor(() => expect(parse).toHaveBeenCalledWith("run daily"));
    expect(calls("dictate_once")).toHaveLength(0);
    expect(save).not.toHaveBeenCalled();
  });

  it("when ready the mic dictates once, goes to the preview, and never saves", async () => {
    mount();
    await waitFor(() => expect(mic().disabled).toBe(false));
    await act(async () => { fireEvent.click(mic()); });
    await waitFor(() => expect(parse).toHaveBeenCalledWith("run three times a week"));
    expect(calls("dictate_once")).toHaveLength(1);
    expect(calls("dictate_once")[0][1]).toMatchObject({ purpose: "add" });
    await waitFor(() => expect(screen.getByLabelText(/^Preview:/)).toBeTruthy());
    expect(save).not.toHaveBeenCalled();
  });

  it("nothing heard: no text, no parse, no save", async () => {
    fake.text = null;
    mount();
    await waitFor(() => expect(mic().disabled).toBe(false));
    await act(async () => { fireEvent.click(mic()); });
    expect(parse).not.toHaveBeenCalled();
    expect(save).not.toHaveBeenCalled();
  });

  it("check-in transcribe asks for 'checkin' and is unwired on nothing", async () => {
    const p = makeLivePersonal(async (c, a) => invoke(c, a));
    expect(await p.transcribe()).toEqual({ state: "live", value: "run three times a week" });
    expect(calls("dictate_once")[0][1]).toMatchObject({ purpose: "checkin" });
    fake.text = null;
    expect((await p.transcribe()).state).toBe("unwired");
  });

  it("logic only: the Composer mic keeps its markup, class and copy", () => {
    expect(composerSource).toContain('className="v-mic" disabled={');
    expect(composerSource).toContain('aria-label="Dictate on this Mac (needs wiring: on-device transcription)"');
    expect(composerSource).toContain('title="Needs wiring · voice is transcribed on this Mac only"');
  });
});
