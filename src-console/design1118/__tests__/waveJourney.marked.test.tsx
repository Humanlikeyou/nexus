import { describe, expect, it } from "vitest";
import { journeyPosition, trackDoneIds } from "../WaveJourney";
import type { HemisyncWave } from "../../neuro/hemisync/adapters";

const t = (id: string, wave: number, markedDone = false) => ({ id, wave, waveName: `W${wave}`, order: 1, title: id, durationMs: null, src: "", markedDone });
const waves: HemisyncWave[] = [
  { wave: 1, waveName: "Discovery", tracks: [t("a", 1, true), t("b", 1, true)] },
  { wave: 2, waveName: "Threshold", tracks: [t("c", 2), t("d", 2)] },
];

describe("0045 marked done", () => {
  it("counts a mark as done without a session", () => {
    const ids = trackDoneIds(waves, []);
    expect([...ids].sort()).toEqual(["a", "b"]);
    expect(journeyPosition(waves, ids).next?.id).toBe("c");
    expect(journeyPosition(waves, ids).done).toBe(2);
  });
  it("sessions and marks union; no marks means sessions only", () => {
    const ids = trackDoneIds(waves, [{ trackId: "c", sessions: 2 }, { trackId: "d", sessions: 0 }]);
    expect([...ids].sort()).toEqual(["a", "b", "c"]);
    const plain: HemisyncWave[] = [{ wave: 1, waveName: "D", tracks: [t("x", 1)] }];
    expect(trackDoneIds(plain, []).size).toBe(0);
  });
});
