import { afterEach, describe, expect, it } from "vitest";
import { SAND_DEFAULTS, SAND_KEY, __resetSandSettings, calmActive, sandSettings, sessionSeed, setSandSettings } from "../settings";

describe("design-1118b sand settings", () => {
  afterEach(() => { localStorage.removeItem(SAND_KEY); __resetSandSettings(); document.documentElement.removeAttribute("data-reduce-motion"); sessionStorage.removeItem("nexus.sandSeed"); });

  it("defaults: sand lens, sand material, sound OFF, every Stage B step on except sound", () => {
    expect(SAND_DEFAULTS).toMatchObject({ lens: "sand", material: "sand", sound: false, tokens: true, ambience: true, moments: true, calm: false });
  });

  it("the Stage A 'spores' skin is gone: a saved spores choice becomes sand", () => {
    localStorage.setItem(SAND_KEY, JSON.stringify({ material: "spores" }));
    __resetSandSettings();
    expect(sandSettings().material).toBe("sand");
  });

  it("the classic lens stays selectable and persists", () => {
    setSandSettings({ lens: "classic" });
    __resetSandSettings();
    expect(sandSettings().lens).toBe("classic");
  });

  it("Calm follows the setting or reduced motion", () => {
    expect(calmActive({ ...SAND_DEFAULTS })).toBe(false);
    expect(calmActive({ ...SAND_DEFAULTS, calm: true })).toBe(true);
    document.documentElement.setAttribute("data-reduce-motion", "true");
    expect(calmActive({ ...SAND_DEFAULTS })).toBe(true);
  });

  it("the edge seed is stable within a session", () => {
    const a = sessionSeed();
    expect(sessionSeed()).toBe(a);
  });
});

describe("design-1118b rename: spores → sand", () => {
  it("no UI source names spores any more (only the migration comment in settings.ts)", () => {
    const files = (import.meta as unknown as { glob: (p: string[], o: object) => Record<string, string> }).glob(["../../**/*.{ts,tsx,css}", "!../../**/__tests__/**", "!../../**/*.test.*"], { query: "?raw", import: "default", eager: true });
    const paths = Object.keys(files);
    expect(paths.length).toBeGreaterThan(100);
    const hits = paths.filter((p) => !/(^|\/)sandLens\/settings\.ts$|^\.\.\/settings\.ts$/.test(p) && /spore/i.test(files[p]));
    expect(hits).toEqual([]);
  });
});
