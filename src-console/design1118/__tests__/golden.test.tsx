import { describe, expect, it } from "vitest";
import { FIB, GOLDEN_ANGLE, PHI, goldenPhase, phiWave } from "../golden";
import { strandPath, strandTerms } from "../pathModel";
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
import fs from "node:fs";
import path from "node:path";
const cssSrc = fs.readFileSync(path.resolve("src-console/design1118/design1118.css"), "utf8");

const nums = (d: string) => d.match(/-?\d+(\.\d+)?/g)!.map(Number);
const PAGES = [["Goals", todaySrc], ["Neuro", neuroSrc], ["Ventures", venturesSrc], ["Finances", revenueSrc], ["Content", contentSrc], ["Agents", agentsSrc], ["Smart home", homeSrc]] as const;

describe("design-1118a: golden ratio in code, not pictures", () => {
  it("constants: φ, the golden angle, Fibonacci", () => {
    expect(PHI).toBeCloseTo(1.6180339887, 9);
    expect((GOLDEN_ANGLE * 180) / Math.PI).toBeCloseTo(137.5078, 3);
    expect(FIB[10] / FIB[9]).toBeCloseTo(PHI, 3);
    expect(goldenPhase(1)).toBeCloseTo(GOLDEN_ANGLE / (2 * Math.PI), 10);
  });
  it("φ waves: overtone φ faster and φ² smaller", () => {
    expect(phiWave(0.25, 2, 10, 1, 1)).toBeCloseTo(10 * Math.sin(Math.PI / 2 + GOLDEN_ANGLE), 6);
    const two = phiWave(0.25, 2, 10, 1, 2) - phiWave(0.25, 2, 10, 1, 1);
    expect(two).toBeCloseTo((10 / (PHI * PHI)) * Math.sin(0.25 * Math.PI * 2 * PHI + 2 * GOLDEN_ANGLE), 6);
  });
  it("each goal strand has a distinct Fibonacci fundamental, golden-angle phase, and a seamless loop", () => {
    const fundamentals = [2, 3, 5, 8];
    for (let i = 0; i < 4; i++) {
      const t = strandTerms(i);
      expect(t.k1).toBe(fundamentals[i]);
      expect(t.k2).toBe(FIB[i + 3]);
      expect(t.k3).toBe(FIB[i + 4]);
      expect(t.phase).toBeCloseTo(i * GOLDEN_ANGLE, 10);
      const n = nums(strandPath(i, 680, 120, 0.7));
      expect(n[1]).toBeCloseTo(n[n.length - 1], 1);
    }
    expect(strandPath(0)).not.toBe(strandPath(1));
    expect(strandPath(2)).not.toBe(strandPath(3));
  });
  it("no golden spiral, φ grid, or underlay pictures on the new pages or in the art", () => {
    for (const [name, src] of PAGES) {
      expect(src as string, name).not.toMatch(/GoldenUnderlay/);
      expect(src as string, name).not.toMatch(/d18-underlay/);
      expect(src as string, name).not.toMatch(/d18-golden-art/);
    }
    for (const [name, src] of [["Muse", museSrc], ["Hemi-Sync", waveSrc], ["Path", pathSrc], ["css", cssSrc]] as const) {
      expect(src as string, name).not.toMatch(/d18-golden-art/);
      expect(src as string, name).not.toMatch(/GoldenUnderlay/);
      expect(src as string, name).not.toMatch(/d18-underlay/);
    }
    expect(cssSrc).toMatch(/1\.618fr/);
    expect(cssSrc).toMatch(/d18-breathe/);
    expect(pathSrc).toMatch(/d18-strand__tphi2/);
    expect(museSrc).toMatch(/phiWave/);
  });
});
