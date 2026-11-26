/**
 * Decision 0059 (step 8e): biofeedback, kept apart from neurofeedback.
 *
 * Neurofeedback follows EEG features (training.ts). Biofeedback follows body signals. Nothing here is a
 * sensor that is assumed: each one is reported only from what is actually arriving now.
 *
 * - EEG: from the Muse, when fresh.
 * - IMU (gyroscope): when the Muse sends it. Used for movement holds, nothing else.
 * - PPG heart rate: only when the PPG stream is fresh, at least 20 s is buffered, and one clear pulse
 *   peak stands out (`estimateHeartRate`). It stays an estimate. The acquisition mode is the p50 preset
 *   that `muse.rs` sends when the stream starts (EEG with PPG); the headset does not report its preset back,
 *   so that is stated rather than claimed as verified.
 * - RR intervals and HRV (decision 0061): from the WHOOP strap's beat-to-beat intervals, which NOOP stores.
 *   They arrive when the strap syncs, not live, so HRV is a per-session figure worked out in Rust
 *   (`noop_rr.rs`) after the sync, never in-session feedback. HRV is never computed from EEG or from a
 *   heart-rate estimate.
 */
import { estimateHeartRate, PPG_WINDOW } from "./heartRate";
import type { MuseTelemetry } from "./telemetryStore";

const FRESH_MS = 1500;

export type SensorState = { state: "available" | "not_available"; detail: string };
export type Biofeedback = {
  eeg: SensorState;
  imu: SensorState;
  ppg: SensorState;
  heartRate: { bpm: number; estimate: true; source: "muse_ppg" } | null;
  rrIntervals: SensorState;
  hrv: SensorState;
};

export const NO_RR = "NOOP holds no beat-to-beat (RR) intervals from the WHOOP strap.";
export const NO_HRV = "HRV needs beat-to-beat intervals from the strap, and there are none in NOOP. It is never worked out from EEG or from a heart-rate estimate.";
/** NOOP's RR status, as `noop_rr_status` returns it. */
export type RrStatus = { available: boolean; newestRrAt: string | null; beatsLast24h: number; source: string };
export const PPG_MODE = "The headset is started in the p50 preset (EEG and PPG). The Muse does not report its preset back, so the mode itself is not verified.";

export function biofeedback(t: MuseTelemetry, now: number, rr: RrStatus | null = null): Biofeedback {
  const eegFresh = t.lastPacketAt != null && now - t.lastPacketAt < FRESH_MS;
  const eeg: SensorState = eegFresh
    ? { state: "available", detail: t.source === "live" ? "Live from the Muse." : `Arriving, but from ${t.source ?? "an unknown source"}, not the live headset.` }
    : { state: "not_available", detail: "No EEG is arriving." };
  const imu: SensorState = t.gyro && now - t.gyro.lastPacketAt < FRESH_MS
    ? { state: "available", detail: "Gyroscope from the Muse, used to hold feedback during movement." }
    : { state: "not_available", detail: "The Muse is not sending motion data." };
  let ppg: SensorState;
  let heartRate: Biofeedback["heartRate"] = null;
  if (!t.ppg || now - t.ppg.lastPacketAt >= FRESH_MS) {
    ppg = { state: "not_available", detail: "The Muse is not sending PPG." };
  } else if (t.source !== "live") {
    ppg = { state: "not_available", detail: "PPG is only read from the live headset." };
  } else if (t.ppg.samples.length < PPG_WINDOW) {
    ppg = { state: "not_available", detail: "Collecting 20 seconds of PPG before it can be checked." };
  } else {
    const bpm = estimateHeartRate(t.ppg.samples);
    ppg = bpm == null
      ? { state: "not_available", detail: "PPG is arriving, but no clear pulse stands out (often movement or a loose fit)." }
      : { state: "available", detail: `A clear pulse in the last 20 seconds. ${PPG_MODE}` };
    heartRate = bpm == null ? null : { bpm, estimate: true, source: "muse_ppg" };
  }
  const synced = rr?.newestRrAt ? new Date(rr.newestRrAt).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) : null;
  const haveRr = !!rr?.available && synced !== null;
  return { eeg, imu, ppg, heartRate,
    rrIntervals: haveRr ? { state: "available", detail: `${rr!.source}, synced up to ${synced} (${rr!.beatsLast24h.toLocaleString()} beats in the 24 hours before that). They arrive when the strap syncs, not live.` } : { state: "not_available", detail: NO_RR },
    hrv: haveRr ? { state: "available", detail: "Per session, from the strap's beat-to-beat intervals once they have synced (RMSSD). Not live feedback, and never from EEG or a heart-rate estimate." } : { state: "not_available", detail: NO_HRV } };
}
