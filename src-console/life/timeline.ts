/**
 * design-1111v Dates & deadlines: a 12-month timeline in three lanes. Labels are placed so they never overlap
 * (the mock's crowded Enterprise lane): each label takes the first row where it fits after the label before it;
 * labels near the right edge end at their dot; when no row is free the label joins its neighbour ("A + B").
 */
export type LanePoint = { id: string; x: number; label: string; date: string };
export type PlacedLabel = { ids: string[]; x: number; anchor: "start" | "end"; row: number; label: string; date: string; left: number; right: number };

/** About 6.2 px per character at 11 px in the app font, plus the 8 px gap from the dot. */
export const textWidth = (s: string) => Math.ceil(s.length * 6.2);

export function placeLabels(points: LanePoint[], { width, rows = 2, gap = 13 }: { width: number; rows?: number; gap?: number }): PlacedLabel[] {
  const sorted = [...points].sort((a, b) => a.x - b.x);
  const placed: PlacedLabel[] = [];
  const ends: number[] = Array.from({ length: rows }, () => -Infinity);
  for (const p of sorted) {
    const w = Math.max(textWidth(p.label), textWidth(p.date));
    const end = p.x + 8 + w > width;
    const left = end ? p.x - 8 - w : p.x + 8;
    const right = end ? p.x - 8 : p.x + 8 + w;
    // The dot itself must stay clear too, so a row is free when this label starts after the last one ended.
    const row = ends.findIndex((e) => Math.min(left, p.x) >= e + gap);
    if (row >= 0) {
      ends[row] = Math.max(right, p.x);
      placed.push({ ids: [p.id], x: p.x, anchor: end ? "end" : "start", row, label: p.label, date: p.date, left, right });
      continue;
    }
    // No free row: merge into the nearest earlier label rather than draw over it.
    const near = placed.reduce((best, l) => (Math.abs(l.x - p.x) < Math.abs(best.x - p.x) ? l : best), placed[placed.length - 1]);
    near.ids.push(p.id);
    near.label = `${near.label} + ${p.label}`;
    near.date = `${near.date} · ${p.date}`;
    const nw = Math.max(textWidth(near.label), textWidth(near.date));
    if (near.anchor === "start") near.right = near.left + nw; else near.left = near.right - nw;
    ends[near.row] = Math.max(ends[near.row], near.right);
  }
  return placed;
}

/** Labels in one row must not overlap. */
export function overlaps(labels: PlacedLabel[]): boolean {
  for (const a of labels) for (const b of labels) if (a !== b && a.row === b.row && a.left < b.right && b.left < a.right) return true;
  return false;
}
