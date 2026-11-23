import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => { while (a.length && a[a.length - 1] === undefined) a.pop(); return (invoke as (...x: unknown[]) => unknown)(...a); } }));

import { confirmNavySpeak, liveNavy, NAVY_SPEAK_MAX_CHARS, sanitizeSpeakText, setNavyLocalOnly } from "./adapters";
// @ts-expect-error vite ?raw import (no ambient type in this project)
import adaptersSource from "./adapters.ts?raw";
import { enterNavy, exitNavy, finishEnter, finishExit, navyStore } from "./store";

const flush = () => new Promise((r) => setTimeout(r, 0));
type Fake = { ready: boolean; speakResult: boolean | "throw" };
let fake: Fake;

function install() {
  invoke.mockImplementation(async (cmd: string, args?: { on?: boolean; text?: string }) => {
    if (cmd === "set_local_only") return args?.on;
    if (cmd === "voice_speak_ready") return fake.ready;
    if (cmd === "voice_speak_local") { if (fake.speakResult === "throw") throw new Error("down"); return fake.speakResult; }
    throw new Error(`unexpected command ${cmd}`);
  });
}

async function reset() {
  invoke.mockReset();
  invoke.mockImplementation(async () => false);
  await setNavyLocalOnly(false);
  await confirmNavySpeak(); // ready -> false
  localStorage.removeItem("nexus.sampleAdapters");
  navyStore.reset();
  fake = { ready: true, speakResult: true };
  install();
  invoke.mockClear();
}

const calls = (cmd: string) => invoke.mock.calls.filter(([c]) => c === cmd);
async function enter(by: "voice" | "chat") { enterNavy(by); await flush(); finishEnter(); }

describe("navy speak(): local, voice-opened only, fail closed", () => {
  beforeEach(reset);

  it("speaks once, through the local command only, when opened by voice", async () => {
    await enter("voice");
    expect(navyStore.get().caption?.spoken).toBe(true);
    expect(calls("voice_speak_local")).toHaveLength(1);
    expect(typeof (calls("voice_speak_local")[0][1] as { text: string }).text).toBe("string");
    expect(invoke.mock.calls.every(([c]) => ["set_local_only", "voice_speak_ready", "voice_speak_local"].includes(c as string))).toBe(true);
  });

  it("never speaks, or even probes the speak path, for a typed (chat) look", async () => {
    await enter("chat");
    expect(navyStore.get().caption?.spoken).toBe(false);
    expect(calls("voice_speak_local")).toHaveLength(0);
    expect(calls("voice_speak_ready")).toHaveLength(0);
    // Even with the entry confirmed by an earlier voice look, adapter-level typed calls stay unwired.
    await confirmNavySpeak();
    expect(liveNavy.speak("Hi", { openedBy: "chat" }).state).toBe("unwired");
    expect(liveNavy.speak("Hi", { openedBy: "hold" }).state).toBe("unwired");
    expect(liveNavy.speak("Hi").state).toBe("unwired");
    expect(calls("voice_speak_local")).toHaveLength(0);
  });

  it("is unwired when Amara Voice is not running or not patched (ready=false)", async () => {
    fake.ready = false;
    await enter("voice");
    expect(navyStore.get().caption?.spoken).toBe(false);
    expect(calls("voice_speak_local")).toHaveLength(0);
  });

  it("fails closed when the readiness check throws", async () => {
    invoke.mockImplementation(async (cmd: string, args?: { on?: boolean }) => { if (cmd === "set_local_only") return args?.on; throw new Error("boom"); });
    await enter("voice");
    expect(navyStore.get().caption?.spoken).toBe(false);
    expect(liveNavy.speak("x", { openedBy: "voice" }).state).toBe("unwired");
  });

  it("stops claiming the line was spoken, and goes unwired, when the hand-over throws or is refused", async () => {
    for (const r of ["throw", false] as const) {
      await reset();
      fake.speakResult = r;
      await enter("voice");
      await flush();
      expect(navyStore.get().caption?.spoken).toBe(false);
      expect(liveNavy.speak("again", { openedBy: "voice" }).state).toBe("unwired");
    }
  });

  it("does not speak without the local-only confirmation", async () => {
    fake.ready = true;
    await confirmNavySpeak();
    await setNavyLocalOnly(false);
    expect(liveNavy.speak("x", { openedBy: "voice" }).state).toBe("unwired");
    expect(calls("voice_speak_local")).toHaveLength(0);
  });

  it("caps and sanitises the text before it leaves the renderer", async () => {
    expect(sanitizeSpeakText("a\u0007b\n\tc   d")).toBe("a b c d");
    expect(Array.from(sanitizeSpeakText("é".repeat(1000))).length).toBe(NAVY_SPEAK_MAX_CHARS);
    expect(sanitizeSpeakText(" \n\u0000 ")).toBe("");
    await setNavyLocalOnly(true);
    await confirmNavySpeak();
    const w = liveNavy.speak(`hello\u0007 ${"x".repeat(1000)}`, { openedBy: "voice" });
    expect(w.state).toBe("live");
    const sent = (calls("voice_speak_local")[0][1] as { text: string }).text;
    expect(Array.from(sent).length).toBeLessThanOrEqual(NAVY_SPEAK_MAX_CHARS);
    expect(Array.from(sent).every((c) => c.charCodeAt(0) >= 32)).toBe(true);
    expect(liveNavy.speak("   ", { openedBy: "voice" }).state).toBe("unwired");
  });

  it("has no outbound host or cloud TTS in the adapter source, and localOnly still holds", async () => {
    const src = adaptersSource.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, "");
    for (const banned of ["http://", "https://", "fetch(", "WebSocket", "XMLHttpRequest", "speechSynthesis", "elevenlabs", "openai"]) {
      expect(src.toLowerCase()).not.toContain(banned.toLowerCase());
    }
    await enter("voice");
    expect(liveNavy.localOnly()).toEqual({ state: "live", value: { ok: true } });
    finishExit(); exitNavy("voice"); finishExit();
  });
});
