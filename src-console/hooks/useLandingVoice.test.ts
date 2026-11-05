import { describe, expect, it } from "vitest";
import type { VoiceState, VoiceStatus } from "../types";
import type { AmaraVoiceReason, AmaraVoiceState } from "./useAmaraVoice";
import { LANDING_COPY, resolveLandingVoice } from "./useLandingVoice";

const status = (state: VoiceState, over: Partial<VoiceStatus> = {}): VoiceStatus => ({ installed: true, running: true, pid: 1, state, state_age_s: 1, voice: "v", rss_mb: 1, launchd: false, dir: "/x", ...over });
const feed = (state: AmaraVoiceState = "idle", reason: AmaraVoiceReason | null = null) => ({ state, reason });
const off = status("stopped", { running: false });

describe("resolveLandingVoice", () => {
  it("shows a running loop waiting for the wake word as listening, though the feed says idle", () => {
    expect(resolveLandingVoice(feed(), status("idle"), false)).toEqual({ state: "listening", line: "Amara is listening" });
    expect(resolveLandingVoice(feed(), status("heard"), false).state).toBe("listening");
  });

  it("never glows before the first answer", () => {
    expect(resolveLandingVoice(feed(), null, false)).toEqual({ state: "idle", line: LANDING_COPY.checking });
  });

  it("lets the live feed lead while the loop runs", () => {
    for (const state of ["listening", "thinking", "speaking"] as const) {
      expect(resolveLandingVoice(feed(state), status("idle"), false).state).toBe(state);
      expect(resolveLandingVoice(feed(state), null, false).state).toBe(state);
    }
  });

  it("uses status for thinking and speaking when the bridge is silent", () => {
    expect(resolveLandingVoice(feed(), status("thinking"), false).state).toBe("thinking");
    expect(resolveLandingVoice(feed(), status("speaking"), false).state).toBe("speaking");
  });

  it("stopped beats a stale feed", () => {
    for (const state of ["idle", "listening", "speaking", "error"] as const) {
      expect(resolveLandingVoice(feed(state, state === "error" ? "unavailable" : null), off, false)).toEqual({ state: "idle", line: LANDING_COPY.off });
    }
  });

  it("says when voice isn't set up", () => {
    expect(resolveLandingVoice(feed("error", "unavailable"), status("not_installed", { installed: false, running: false }), false)).toEqual({ state: "idle", line: LANDING_COPY.notInstalled });
  });

  it("loading is dim, from either source", () => {
    const loading = { state: "idle", line: LANDING_COPY.loading };
    expect(resolveLandingVoice(feed(), status("loading"), false)).toEqual(loading);
    expect(resolveLandingVoice(feed("idle", "model_loading"), status("idle"), false)).toEqual(loading);
  });

  it("a failed turn is an error, and the line says she still listens", () => {
    const failed = { state: "error", line: LANDING_COPY.lastTurnFailed };
    expect(resolveLandingVoice(feed("error"), status("idle"), false)).toEqual(failed);
    expect(resolveLandingVoice(feed(), status("error"), false)).toEqual(failed);
  });

  it("ignores the feed's unavailable when status shows the loop came back", () => {
    expect(resolveLandingVoice(feed("error", "unavailable"), status("idle"), false).state).toBe("listening");
  });

  it("unreachable is an error, unless the feed is proving she's active", () => {
    expect(resolveLandingVoice(feed(), status("idle"), true)).toEqual({ state: "error", line: LANDING_COPY.unreachable });
    expect(resolveLandingVoice(feed("speaking"), null, true).state).toBe("speaking");
  });
});
