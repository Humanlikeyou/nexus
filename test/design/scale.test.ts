import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

/** design-1110g: every spacing value is on the Fibonacci scale and no text is under 11px. */
const SCALE = new Set([0, 1, 3, 5, 8, 13, 21, 34, 55, 89]);
const ROOT = path.resolve(__dirname, "../../src-console");
const css = (dir: string): string[] => fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? css(path.join(dir, e.name)) : e.name.endsWith(".css") ? [path.join(dir, e.name)] : []));
const SPACE = /(?<![\w-])((?:margin|padding|gap|row-gap|column-gap|scroll-padding|scroll-margin)(?:-[a-z-]+)?|--[\w-]*(?:space|pad|gap|inset)[\w-]*)\s*:\s*([^;{}]*)/g;
const FONT = /(?<![\w-])font-size\s*:\s*([^;{}]*)/g;
const PX = /(-?\d+(?:\.\d+)?)px/g;
/** Home keeps its existing golden layout (design-1110g brief): its optical top inset is the one named exception. */
const HOME_KEEP = new Set(["--landing-pad-top: 56px"]);
/** Window chrome is aligned to the macOS traffic lights, not the scale: the 78px inset, the wordmark's +6px and its lockup. */
const CHROME_FILES = new Set(["styles/wordmark.css"]);
const isChrome = (prop: string, value: string): boolean => /traffic/.test(prop) || /traffic/.test(value);

export function offScale(text: string): string[] {
  const bad: string[] = [];
  for (const m of text.matchAll(SPACE)) if (!isChrome(m[1], m[2])) for (const p of m[2].matchAll(PX)) if (!SCALE.has(Math.abs(Number(p[1]))) && !HOME_KEEP.has(`${m[1]}: ${p[0]}`)) bad.push(`${m[1]}: ${p[0]}`);
  return bad;
}
export function tinyText(text: string): string[] {
  const bad: string[] = [];
  for (const m of text.matchAll(FONT)) for (const p of m[1].matchAll(PX)) if (Number(p[1]) < 11) bad.push(`font-size: ${p[0]}`);
  for (const m of text.matchAll(/--text-[\w-]+\s*:\s*(\d+(?:\.\d+)?)px/g)) if (Number(m[1]) < 11) bad.push(m[0]);
  return bad;
}

describe("golden-ratio design system", () => {
  const files = css(ROOT);
  it("finds the stylesheets", () => { expect(files.length).toBeGreaterThan(20); });
  it("the check itself catches off-scale spacing and tiny text", () => {
    expect(offScale(".a { padding: 12px 8px; gap: var(--space-3, 16px); --mb-gap: 13px; }")).toEqual(["padding: 12px", "gap: 16px"]);
    expect(offScale(":root { --shell-traffic-inset: 78px; } .m { padding-left: calc(var(--shell-traffic-inset) + 6px); }")).toEqual([]);
    expect(tinyText(".a { font-size: 10px; } :root { --text-badge: 10px; }")).toHaveLength(2);
  });
  for (const f of files) {
    const rel = path.relative(ROOT, f);
    it(`${rel}: spacing on the Fibonacci scale, text ≥ 11px`, () => {
      const text = fs.readFileSync(f, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
      if (!CHROME_FILES.has(rel.split(path.sep).join("/"))) expect(offScale(text)).toEqual([]);
      expect(tinyText(text)).toEqual([]);
    });
  }
});
