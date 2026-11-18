import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { FIB, GOLDEN_ANGLE, PHI, goldenSpiralPath, phiGrid, phiWave } from "../golden";
import { GoldenUnderlay } from "../GoldenUnderlay";
import { strandPath } from "../pathModel";
// @ts-expect-error vite ?raw import
import todaySrc from "../../personal/TodaySections.tsx?raw";
// @ts-expect-error vite ?raw import
import neuroSrc from "../../screens/NeuroScreen.tsx?raw";
// @ts-expect-error vite ?raw import
import venturesSrc from "../../screens/BusinessPanels.tsx?raw";
// @ts-expect-error vite ?raw import
import revenueSrc from "../../screens/RevenueScreen.tsx?raw";
// @ts-expect-error vite ?raw import
import contentSrc from "../../screens/EnterprisePages.tsx?raw";
// @ts-expect-error vite ?raw import
import agentsSrc from "../../screens/AgentsScreen.tsx?raw";
// @ts-expect-error vite ?raw import
import homeSrc from "../../screens/SmartHomeScreen.tsx?raw";
// @ts-expect-error vite ?raw import
import museSrc from "../MuseJourney.tsx?raw";
// @ts-expect-error vite ?raw import
import waveSrc from "../WaveJourney.tsx?raw";
// @ts-expect-error vite ?raw import
import pathSrc from "../GoalPath.tsx?raw";

const nums = (d: string) => d.match(/-?\d+(\.\d+)?/g)!.map(Number);

describe("design-1118a round 6: golden ratio underlay and art", () => {
  afterEach(cleanup);
  it("constants: φ, the golden angle, Fibonacci", () => {
    expect(PHI).toBeCloseTo(1.6180339887, 9);
    expect((GOLDEN_ANGLE * 180) / Math.PI).toBeCloseTo(137.5078, 3);
    expect(FIB[10] / FIB[9]).toBeCloseTo(PHI, 3);
  });
  it("the spiral grows by φ every quarter turn", () => {
    const n = nums(goldenSpiralPath(0, 0, 1, 2, 0, 4));
    const r = (k: number) => Math.hypot(n[2 * k], n[2 * k + 1]);
    expect(r(4) / r(0)).toBeCloseTo(PHI, 1);
    expect(r(8) / r(4)).toBeCloseTo(PHI, 1);
  });
  it("the φ grid sits on the golden sections", () => {
    const n = nums(phiGrid(1618, 1000));
    expect(n[0]).toBeCloseTo(1618 / PHI / PHI, 0);
    expect(n[3]).toBeCloseTo(1618 / PHI, 0); // second vertical at 1618/φ = 1000
  });
  it("φ waves: overtone φ faster and φ² smaller", () => {
    expect(phiWave(0.25, 2, 10, 1, 1)).toBeCloseTo(10 * Math.sin(Math.PI / 2 + GOLDEN_ANGLE), 6);
    const two = phiWave(0.25, 2, 10, 1, 2) - phiWave(0.25, 2, 10, 1, 1);
    expect(two).toBeCloseTo((10 / (PHI * PHI)) * Math.sin(0.25 * Math.PI * 2 * PHI + 2 * GOLDEN_ANGLE), 6);
  });
  it("Milestones strands still loop seamlessly with Fibonacci frequencies", () => {
    for (let i = 0; i < 4; i++) {
      const n = nums(strandPath(i, 680, 120, 0.7));
      expect(n[1]).toBeCloseTo(n[n.length - 1], 1);
    }
    expect(strandPath(0)).not.toBe(strandPath(1));
  });
  it("every new page carries the underlay; the waves carry the golden overlay", () => {
    for (const [name, src] of [["Goals", todaySrc], ["Neuro", neuroSrc], ["Ventures", venturesSrc], ["Finances", revenueSrc], ["Content", contentSrc], ["Agents", agentsSrc], ["Smart home", homeSrc]] as const) {
      expect(src as string, name).toMatch(/<GoldenUnderlay \/>/);
      expect(src as string, name).toMatch(/d18-golden/);
    }
    for (const src of [museSrc, waveSrc, pathSrc]) expect(src as string).toMatch(/d18-golden-art/);
    const { getByTestId } = render(<GoldenUnderlay />);
    const svg = getByTestId("golden-underlay");
    expect(svg.getAttribute("aria-hidden")).toBe("true");
    expect(svg.querySelectorAll("path")).toHaveLength(2);
  });
});
