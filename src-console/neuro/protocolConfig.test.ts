import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { PROTOCOLS, getProtocol } from "../legacy-core/lib/neurofeedback/protocols";
import { COMPUTED_BANDS_HZ, defaultSettings, effectiveSettings, targetFeatures } from "./protocolConfig";
import { adaptScale, TARGET_REWARD_RATE, TrainingSession } from "./training";
import { ProtocolSettingsCard, type TrainingApi } from "./TrainingView";

describe("decision 0059: target-feature definitions", () => {
  it("every protocol band maps to a band the training really computes", () => {
    for (const p of PROTOCOLS) {
      for (const f of targetFeatures(p)) {
        if (f.role === "coupling") continue;
        expect(f.computedHz, `${p.id} ${f.band}`).not.toBeNull();
        expect(f.method).toMatch(/^training-v/);
      }
    }
  });

  it("states the computed edges and flags a declared range that differs", () => {
    const delta = targetFeatures(getProtocol("hypnagogic")!).find((f) => f.id === "inhibit:delta")!;
    expect(delta.computedHz).toEqual(COMPUTED_BANDS_HZ.delta);
    expect(delta.declaredHz).toEqual([0.5, 4]);
    const smr = targetFeatures(getProtocol("focus")!).find((f) => f.id === "reward:smr")!;
    expect(smr.declaredHz).toBeUndefined();
    expect(smr.direction).toBe("above_threshold");
  });

  it("names coupling, stability, drift and monitored bands explicitly", () => {
    const creative = targetFeatures(getProtocol("creative-insight")!).map((f) => f.id);
    expect(creative).toEqual(expect.arrayContaining(["coupling:alpha/theta", "inhibit:highbeta", "monitor:gamma"]));
    expect(targetFeatures(getProtocol("deep-meditation")!).find((f) => f.role === "reward")!.direction).toBe("stable_ratio");
    expect(targetFeatures(getProtocol("relaxed-flow")!).some((f) => f.id === "drift:theta")).toBe(true);
  });

  it("protocol descriptions make no claim about mental state or learning", () => {
    for (const p of PROTOCOLS) {
      expect(p.description, p.id).not.toMatch(/concentrat|attention|relaxed|stress|emotion|mental|cognitive|brain|state of|sharpen/i);
      for (const cue of p.coachingCues ?? []) expect(cue, p.id).not.toMatch(/brain is learning|in the zone|building sustained attention/i);
    }
  });
});

describe("decision 0059: protocol settings", () => {
  const calm = getProtocol("calm")!;

  it("defaults reproduce the training before 0059", () => {
    expect(defaultSettings(calm)).toEqual({ durationMin: 10, targetRewardRate: TARGET_REWARD_RATE, sound: "tone", visualFeedback: true, volume: 0.7 });
  });

  it("an invalid saved value never reaches training", () => {
    const s = effectiveSettings(calm, { durationMin: 2, targetRewardRate: 0.95, sound: "loud" as never, volume: Number.NaN, visualFeedback: "yes" as never });
    expect(s).toEqual(defaultSettings(calm));
    expect(effectiveSettings(calm, { targetRewardRate: 0.5, visualFeedback: true }).targetRewardRate).toBe(0.5);
  });

  it("the reinforcement schedule follows the configured target rate", () => {
    // 50% rewarded sits inside the ±12% band around both 60% and 40%: no change. 75% against 40% raises it.
    expect(adaptScale(1, 0.5)).toBe(1);
    expect(adaptScale(1, 0.5, 0.4)).toBe(1);
    expect(adaptScale(1, 0.75, 0.4)).toBeGreaterThan(1);
    const session = new TrainingSession(calm, { guided: true, settings: { ...defaultSettings(calm), targetRewardRate: 0.45 } });
    expect(session.summary().guided?.reward_schedule.target_rate).toBe(0.45);
    // An out-of-range rate is ignored (defence in depth behind the store's validation).
    const bad = new TrainingSession(calm, { guided: true, settings: { ...defaultSettings(calm), targetRewardRate: 0.99 } });
    expect(bad.summary().guided?.reward_schedule.target_rate).toBe(TARGET_REWARD_RATE);
  });

  it("the settings card lists the features and saves a changed setting", async () => {
    const saved: Array<[string, unknown]> = [];
    const training: TrainingApi = { protocols: PROTOCOLS, protocolId: "focus", settingsFor: (id) => effectiveSettings(getProtocol(id)!, null), saveSettings: async (id, s) => { saved.push([id, s]); } };
    render(createElement(ProtocolSettingsCard, { training }));
    expect(screen.getByRole("list", { name: "Target features" })).toHaveTextContent("SMR (12–15 Hz): rewarded above its threshold");
    expect(screen.getByText(/not measures of emotion, attention, stress or relaxation/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save settings" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Target reward rate"), { target: { value: "0.5" } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await screen.findByRole("status");
    expect(saved).toEqual([["focus", expect.objectContaining({ targetRewardRate: 0.5, durationMin: 15 })]]);
  });

  it("settings are locked while a session runs", () => {
    const training: TrainingApi = { protocols: PROTOCOLS, protocolId: "calm", settingsFor: (id) => effectiveSettings(getProtocol(id)!, null), saveSettings: async () => {} };
    render(createElement(ProtocolSettingsCard, { training, disabled: true }));
    expect(screen.getByLabelText("Planned length")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Use defaults" })).toBeDisabled();
  });
});
