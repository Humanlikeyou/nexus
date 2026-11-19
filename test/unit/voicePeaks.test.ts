import { describe, expect, it } from "vitest";
import { amaraVoiceLevel, ingestLevel, parseVoicePeaks } from "../../src-console/hooks/useAmaraVoice";
import { buildModes, fallbackPeaks, voicePeaks } from "../../src-console/sandLens/bessel";

// decision 0040: amara-voice sends her real spectral peaks in speaking level frames.
describe("parseVoicePeaks (same rules as the Rust bridge)", () => {
  it("keeps 1-3 in-band pairs as {f, amp}", () => {
    expect(parseVoicePeaks([[440.5, 1], [880, 0.25]])).toEqual([{ f: 440.5, amp: 1 }, { f: 880, amp: 0.25 }]);
  });
  it.each([
    [[]], [[[440, 1], [500, 1], [600, 1], [700, 1]]], [[[79.9, 1]]], [[[4000.1, 1]]], [[[440, 0]]], [[[440, 1.01]]],
    [[[440]]], [[[440, 1, 2]]], [[["440", 1]]], [[[440, 1], "x"]], [[[Number.NaN, 1]]], [{ f: 440 }], ["440"], [null], [undefined],
  ])("refuses %j", (v) => {
    expect(parseVoicePeaks(v)).toBeUndefined();
  });
});

describe("ingestLevel keeps peaks only while speaking", () => {
  it("stores the peaks of a speaking frame and drops them on a mic frame", () => {
    ingestLevel({ ts: Date.now(), rms: 0.3, peak: 0.5, state: "speaking", peaks: [[300, 1]] });
    expect(amaraVoiceLevel.current.peaks).toEqual([{ f: 300, amp: 1 }]);
    ingestLevel({ ts: Date.now(), rms: 0.2, peak: 0.4, state: "listening", peaks: [[300, 1]] });
    expect(amaraVoiceLevel.current.peaks).toBeUndefined();
  });
  it("a speaking frame with bad peaks keeps the level and has no peaks", () => {
    ingestLevel({ ts: Date.now(), rms: 0.3, peak: 0.5, state: "speaking", peaks: [[20, 1]] });
    expect(amaraVoiceLevel.current).toEqual({ rms: 0.3, peak: 0.5 });
  });
});

describe("voicePeaks", () => {
  const modes = buildModes().modes;
  it("uses her real peaks when there are some", () => {
    expect(voicePeaks([{ f: 523, amp: 1 }], 4, modes)).toEqual([{ f: 523, amp: 1 }]);
  });
  it("falls back to the onset walk when there are none", () => {
    expect(voicePeaks(undefined, 4, modes)).toEqual(fallbackPeaks(4, modes));
    expect(voicePeaks([], 9, modes)).toEqual(fallbackPeaks(9, modes));
  });
});
