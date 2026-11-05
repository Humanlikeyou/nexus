import { useEffect, useState } from "react";
import { getVoiceStatus } from "../bridge";
import { LENS_LABEL } from "../components/AmaraLens/AmaraLens";
import type { VoiceStatus } from "../types";
import { type AmaraVoiceReason, type AmaraVoiceState, useAmaraVoice } from "./useAmaraVoice";

/** Same process check as Chat & Agents › Voice; slower because this page stays open for long periods. */
export const LANDING_STATUS_POLL_MS = 2000;
export const LANDING_STATUS_TIMEOUT_MS = 4000;
/** One missed poll keeps the last answer; the second one means the service can't be reached. */
export const LANDING_STATUS_MISSES = 2;

export const LANDING_COPY = {
  checking: "Checking Amara Voice…",
  loading: "Amara's voice is loading",
  off: "Amara's voice is off",
  notInstalled: "Amara Voice isn't set up",
  unreachable: "Can't reach Amara Voice",
  lastTurnFailed: "Her last reply failed. Amara is still listening.",
} as const;

export interface LandingVoice { state: AmaraVoiceState; line: string }

const live = (state: AmaraVoiceState) => state === "speaking" || state === "thinking" || state === "listening";
const show = (state: AmaraVoiceState): LandingVoice => ({ state, line: LENS_LABEL[state] });

/**
 * What the big lens shows. The live feed and the voice_status poll answer different questions:
 * - The feed (amara-voice://state) is instant, so it leads for listening, thinking and speaking.
 *   Its "idle" alone proves nothing: it is also the value before any event and after missed heartbeats.
 * - voice_status says whether the loop really runs. Its word "idle" (logs/state) means the loop is
 *   running and waiting for the wake word, which Taisun sees as listening.
 * A loop that isn't running always wins, so a stale feed can't keep the lens glowing.
 */
export function resolveLandingVoice(feed: { state: AmaraVoiceState; reason: AmaraVoiceReason | null }, status: VoiceStatus | null, unreachable: boolean): LandingVoice {
  if (unreachable) return live(feed.state) ? show(feed.state) : { state: "error", line: LANDING_COPY.unreachable };
  if (!status) return live(feed.state) ? show(feed.state) : { state: "idle", line: LANDING_COPY.checking };
  if (!status.installed) return { state: "idle", line: LANDING_COPY.notInstalled };
  if (!status.running) return { state: "idle", line: LANDING_COPY.off };
  if (live(feed.state)) return show(feed.state);
  // amara-voice shows "error" until the next utterance, but it goes on waiting for the wake word.
  if (feed.state === "error" && feed.reason !== "unavailable") return { state: "error", line: LANDING_COPY.lastTurnFailed };
  if (feed.reason === "model_loading") return { state: "idle", line: LANDING_COPY.loading };
  switch (status.state) {
    case "idle": case "listening": case "heard": return show("listening");
    case "thinking": case "speaking": return show(status.state);
    case "error": return { state: "error", line: LANDING_COPY.lastTurnFailed };
    case "loading": return { state: "idle", line: LANDING_COPY.loading };
    default: return { state: "idle", line: LANDING_COPY.off };
  }
}

function statusWithTimeout(): Promise<VoiceStatus> {
  return new Promise((resolve, reject) => {
    const timer = window.setTimeout(() => reject(new Error("voice_status timed out")), LANDING_STATUS_TIMEOUT_MS);
    getVoiceStatus().then(resolve, reject).finally(() => window.clearTimeout(timer));
  });
}

/** Live feed plus voice_status, resolved to one state and line. A feed change re-checks at once. */
export function useLandingVoice(): LandingVoice {
  const { state, reason } = useAmaraVoice();
  const [poll, setPoll] = useState<{ status: VoiceStatus | null; misses: number }>({ status: null, misses: 0 });
  useEffect(() => {
    let current = true;
    let pending = false;
    const check = async () => {
      if (pending) return;
      pending = true;
      try {
        const status = await statusWithTimeout();
        // Same answer as last time: keep the object, so a quiet poll doesn't re-render the page.
        if (current) setPoll((prev) => (prev.misses === 0 && prev.status?.state === status.state && prev.status.running === status.running && prev.status.installed === status.installed ? prev : { status, misses: 0 }));
      } catch {
        if (current) setPoll((prev) => ({ ...prev, misses: prev.misses + 1 }));
      } finally {
        pending = false;
      }
    };
    void check();
    const timer = window.setInterval(() => { void check(); }, LANDING_STATUS_POLL_MS);
    return () => { current = false; window.clearInterval(timer); };
  }, [state]);
  return resolveLandingVoice({ state, reason }, poll.status, poll.misses >= LANDING_STATUS_MISSES);
}
