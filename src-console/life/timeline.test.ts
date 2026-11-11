import { describe, expect, it } from "vitest";
import { overlaps, placeLabels } from "./timeline";

describe("design-1111v lane labels never crowd", () => {
  it("two close dates go on different rows", () => {
    const l = placeLabels([{ id: "a", x: 140, label: "Launch", date: "Nov 21" }, { id: "b", x: 170, label: "Akiya offer", date: "Dec 18" }], { width: 610 });
    expect(l.map((x) => x.row)).toEqual([0, 1]);
    expect(overlaps(l)).toBe(false);
  });

  it("a label near the right edge ends at its dot", () => {
    const [l] = placeLabels([{ id: "a", x: 590, label: "Residence card", date: "Sep 14" }], { width: 610 });
    expect(l.anchor).toBe("end");
    expect(l.right).toBeLessThanOrEqual(610);
  });

  it("three dates on top of each other merge instead of overlapping", () => {
    const l = placeLabels([{ id: "a", x: 140, label: "Launch", date: "Nov 21" }, { id: "b", x: 150, label: "Tax", date: "Nov 23" }, { id: "c", x: 160, label: "Batch", date: "Nov 25" }], { width: 610 });
    expect(l).toHaveLength(2);
    expect(l.some((x) => x.label.includes(" + "))).toBe(true);
    expect(overlaps(l)).toBe(false);
  });

  it("the sample Enterprise lane (mock gap) has no overlaps", () => {
    const x = (m: number) => 110 + (m / 12) * (610 - 110 - 13);
    const l = placeLabels([{ id: "l1", x: x(0.67), label: "Launch", date: "Nov 21" }, { id: "l3", x: x(3.47), label: "Tax filing", date: "Feb 15" }], { width: 610 });
    expect(overlaps(l)).toBe(false);
  });
});
