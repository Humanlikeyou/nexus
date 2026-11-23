import { describe, expect, it, vi } from "vitest";
import { citeTarget, goToCite, liveNote } from "./takeView";
import type { AmaraTake } from "./types";

const take = (page: AmaraTake["page"], keys: string[]): AmaraTake => ({
  page, day: "2026-11-11", text: "x", fingerprint: "f", generatedAt: Date.parse("2026-11-11T00:00:00Z"),
  cited: keys.map((key) => ({ key, label: key, display: "1", baseline: null })),
});

describe("take chips jump to their source (1110f onCite)", () => {
  it("maps cited facts only to element ids that exist on that page", () => {
    expect(citeTarget("mindBody", "hrv")).toBe("mb-readiness");
    expect(citeTarget("mindBody", "sleep")).toBe("mb-sleep");
    expect(citeTarget("neuro", "focus")).toBeUndefined();
    expect(citeTarget("business", "todayRevenue")).toBeUndefined();
    const note = liveNote(take("mindBody", ["hrv", "unknown"]));
    expect(note.chips.map((c) => c.sourceId)).toEqual(["mb-readiness", undefined]);
  });

  it("scrolls when the source is on the page, and does nothing when it isn't", () => {
    const el = document.createElement("section");
    el.id = "mb-readiness";
    el.scrollIntoView = vi.fn();
    document.body.append(el);
    expect(goToCite("mb-readiness")).toBe(true);
    expect(el.scrollIntoView).toHaveBeenCalledWith(expect.objectContaining({ block: "start" }));
    el.remove();
    expect(goToCite("mb-readiness")).toBe(false);
    expect(goToCite(undefined)).toBe(false);
  });
});
