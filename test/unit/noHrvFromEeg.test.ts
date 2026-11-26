// @vitest-environment node
// Decision 0059: HRV needs beat-to-beat (RR) intervals from a heart sensor. Nexus has none connected, so nothing in the
// NEUROSENSE code may compute HRV, least of all from EEG or from a PPG heart-rate estimate. This scans every non-test
// source under src-console/neuro for an HRV statistic or an hrv value assigned from a calculation.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const NEURO = join(__dirname, "../../src-console/neuro");
export const HRV_CALCULATION = /rmssd|sdnn|pnn50|successive differences|\bhrv\w*\s*[:=]\s*(?:-?\d|Math\.|[A-Za-z_]\w*\()/i;

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return sources(p);
    return /\.(ts|tsx)$/.test(f) && !/\.test\./.test(f) ? [p] : [];
  });
}

describe("decision 0059: no HRV from EEG or heart-rate estimates", () => {
  it("no neuro module computes HRV", () => {
    const files = sources(NEURO);
    expect(files.length).toBeGreaterThan(20);
    for (const f of files) expect(readFileSync(f, "utf8"), f).not.toMatch(HRV_CALCULATION);
  });

  it("the check catches a real calculation (fail-before proof)", () => {
    expect("export const hrv = Math.sqrt(mean(diffs));").toMatch(HRV_CALCULATION);
    expect("const rmssd = 42;").toMatch(HRV_CALCULATION);
    expect('hrv: { state: "not_available", detail: NO_HRV }').not.toMatch(HRV_CALCULATION);
  });
});
