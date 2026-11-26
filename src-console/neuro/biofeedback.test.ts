import { render, screen } from "@testing-library/react";
import { createElement } from "react";
import { describe, expect, it } from "vitest";
import { biofeedback, NO_HRV } from "./biofeedback";
import { PPG_RATE, PPG_WINDOW } from "./heartRate";
import type { MuseTelemetry } from "./telemetryStore";
import { BiofeedbackCard } from "./TrainingView";
import { nextInWave } from "../screens/useHemisyncRun";
import type { HemisyncTrack } from "./hemisync/adapters";

const NOW = 1_000_000;
const base = { channels: {}, bands: { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 }, packetCount: 10, sampleCount: 120, lastPacketAt: NOW - 100, channelLastPacketAt: {}, aux: null, source: "live" } as unknown as MuseTelemetry;
/** A clean 66 bpm pulse at the PPG rate. */
const pulse = (n = PPG_WINDOW + PPG_RATE) => Array.from({ length: n }, (_, i) => 50_000 + 2_000 * Math.sin(2 * Math.PI * 1.1 * (i / PPG_RATE)));

describe("decision 0059: biofeedback never assumes a sensor", () => {
  it("with nothing arriving, every sensor is not available and there is no heart rate", () => {
    const b = biofeedback({ ...base, lastPacketAt: null }, NOW);
    for (const k of ["eeg", "imu", "ppg", "rrIntervals", "hrv"] as const) expect(b[k].state).toBe("not_available");
    expect(b.heartRate).toBeNull();
  });

  it("PPG heart rate only from fresh, live PPG with a clear pulse, and always an estimate", () => {
    const ok = biofeedback({ ...base, ppg: { samples: pulse(), lastPacketAt: NOW - 50 } }, NOW);
    expect(ok.ppg.state).toBe("available");
    expect(ok.heartRate).toMatchObject({ estimate: true, source: "muse_ppg" });
    expect(ok.heartRate!.bpm).toBeGreaterThan(60);
    expect(ok.ppg.detail).toMatch(/not verified/);
    expect(biofeedback({ ...base, ppg: { samples: pulse(), lastPacketAt: NOW - 5000 } }, NOW).heartRate).toBeNull();
    expect(biofeedback({ ...base, source: "replay", ppg: { samples: pulse(), lastPacketAt: NOW - 50 } } as MuseTelemetry, NOW).heartRate).toBeNull();
    expect(biofeedback({ ...base, ppg: { samples: pulse(PPG_RATE * 5), lastPacketAt: NOW - 50 } }, NOW).ppg.detail).toMatch(/Collecting 20 seconds/);
    const flat = Array.from({ length: PPG_WINDOW + 64 }, (_, i) => 50_000 + ((i * 7919) % 13));
    expect(biofeedback({ ...base, ppg: { samples: flat, lastPacketAt: NOW - 50 } }, NOW).heartRate).toBeNull();
  });

  it("decision 0061: RR and HRV come from NOOP's strap data when it is there, and say it is not live", () => {
    const rr = { available: true, newestRrAt: "2026-11-26T01:33:49Z", beatsLast24h: 67382, source: "WHOOP strap RR intervals, read from NOOP" };
    const b = biofeedback(base, NOW, rr);
    expect(b.rrIntervals.state).toBe("available");
    expect(b.rrIntervals.detail).toMatch(/WHOOP strap.*synced up to.*not live/);
    expect(b.hrv.state).toBe("available");
    expect(b.hrv.detail).toMatch(/Per session.*never from EEG/);
    expect(biofeedback(base, NOW, { ...rr, available: false, newestRrAt: null }).hrv.state).toBe("not_available");
  });

  it("without NOOP's RR data, RR intervals and HRV are not available, even with a good pulse", () => {
    const b = biofeedback({ ...base, ppg: { samples: pulse(), lastPacketAt: NOW - 50 }, gyro: { samples: [[0, 0, 0]], lastPacketAt: NOW - 50 } } as MuseTelemetry, NOW);
    expect(b.imu.state).toBe("available");
    expect(b.rrIntervals.state).toBe("not_available");
    expect(b.hrv).toEqual({ state: "not_available", detail: NO_HRV });
  });

  it("the card shows each sensor's state in words", () => {
    render(createElement(BiofeedbackCard, { read: biofeedback(base, NOW) }));
    const list = screen.getByRole("list", { name: "Biofeedback sensors" });
    expect(list).toHaveTextContent("EEG (Muse): available");
    expect(list).toHaveTextContent("Heart rate variability: not available");
    expect(screen.getByRole("status")).toHaveTextContent("No heart-rate reading right now.");
  });
});

describe("decision 0059: Hemi-Sync plays in order, when asked", () => {
  const t = (id: string, wave: number, order: number, src = "x"): HemisyncTrack => ({ id, wave, waveName: "W", order, title: id, src } as HemisyncTrack);
  const tracks = [t("b", 1, 2), t("a", 1, 1), t("c", 1, 3, ""), t("d", 1, 4), t("z", 2, 1)];

  it("offers the next track in the same wave, skipping tracks with no audio here", () => {
    expect(nextInWave(tracks, tracks[1])?.id).toBe("b");
    expect(nextInWave(tracks, tracks[0])?.id).toBe("d");
    expect(nextInWave(tracks, tracks[3])).toBeNull();
    expect(nextInWave(tracks, t("u", 0, 1))).toBeNull();
    expect(nextInWave(tracks, null)).toBeNull();
  });
});
