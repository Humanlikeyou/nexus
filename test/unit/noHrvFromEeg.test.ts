// @vitest-environment node
// Decision 0059/0061: HRV needs beat-to-beat (RR) intervals from a heart sensor. The only source is the WHOOP strap's RR
// intervals in NOOP, read in Rust (noop_rr.rs). Nothing in the NEUROSENSE console code may compute HRV, least of all
// from EEG or from a PPG heart-rate estimate. This scans every non-test
// source under src-console/neuro for an HRV statistic or an hrv value assigned from a calculation.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const NEURO = join(__dirname, "../../src-console/neuro");
// Decision 0061: session HRV is computed in Rust (noop_rr.rs) from the WHOOP strap's RR intervals only; the
// console may show it (a field named rmssdMs) but never compute any HRV statistic.
// Blocked: any assignment to an hrv/rmssd/sdnn/pnn50 name (with or without a type annotation, arrow functions
// included), a function of that name, or an object property set from a calculation. Allowed: a typed field
// such as `rmssdMs: number | null`, and words in strings such as "(RMSSD)".
export const HRV_CALCULATION = new RegExp([
  "successive differences",
  "\\b(?:rmssd|sdnn|pnn50|hrv)\\w*\\s*(?::[^=;{,)]*)?=(?![=>])",
  "function\\s+(?:rmssd|sdnn|pnn50|hrv)",
  "\\b(?:rmssd|sdnn|pnn50|hrv)\\w*\\s*:\\s*(?:-?\\d|Math\\.|[A-Za-z_][\\w.]*\\()",
].join("|"), "i");

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
    expect("rmssdMs: number | null").not.toMatch(HRV_CALCULATION);
    expect('detail: "Per session, from the beat-to-beat intervals (RMSSD)."').not.toMatch(HRV_CALCULATION);
    expect("hrv: haveRr ? { state: \"available\" } : null").not.toMatch(HRV_CALCULATION);
    // The cases the 0061 security review showed the first narrowed pattern missed.
    for (const bad of ["const rmssdMs = sqrt(mean(d2));", "const rmssd: number = Math.sqrt(x);", "const rmssdOf = (rr) => Math.sqrt(1);", "function rmssd(rr){ return 1; }", "const hrv = rr.reduce((a, b) => a + b) / n;", "const sdnn = std(x)", "let rmssdMs = Math.sqrt(m)", "out.hrv = compute(rr)"]) {
      expect(bad, bad).toMatch(HRV_CALCULATION);
    }
  });
});
