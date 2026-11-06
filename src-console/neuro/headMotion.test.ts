import { describe, expect, it } from "vitest";
import { GYRO_BUFFER, headMoved } from "./headMotion";

const REST = [-6, 5, -3]; // the sensor's resting offset, about 8 deg/s
const still = (n = GYRO_BUFFER) => Array.from({ length: n }, (_, i) => [REST[0] + (i % 3) * .3, REST[1] - (i % 2) * .3, REST[2]]);
const turned = (samples: number) => [...still(GYRO_BUFFER - samples), ...Array.from({ length: samples }, () => [REST[0] + 20, REST[1], REST[2]])];

describe("headMoved", () => {
  it("reads a still head as still, despite the resting offset", () => expect(headMoved(still())).toBe(false));
  it("sees a real turn", () => expect(headMoved(turned(12))).toBe(true));
  it("ignores one bad packet, even at 100 deg/s", () => {
    const glitch = still();
    for (const at of [200, 201, 202]) glitch[at] = [100, 0, 0];
    expect(headMoved(glitch)).toBe(false);
  });
  it("forgets a movement once it has left the EEG window", () => expect(headMoved([...turned(12), ...still(60)].slice(-GYRO_BUFFER))).toBe(false));
  it("waits for enough samples", () => expect(headMoved(turned(12).slice(-20))).toBe(false));
});
