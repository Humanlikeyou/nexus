/**
 * 2D primitives in pixel space (y down), flattened into one triangle buffer with per-vertex RGBA.
 * Pure: no three here, so the geometry is testable and the renderer just uploads it.
 */
export type RGBA = [number, number, number, number];
export type Prim =
  | { k: "rect"; x: number; y: number; w: number; h: number; c: RGBA; c1?: RGBA }
  | { k: "poly"; pts: number[]; c: RGBA }
  | { k: "tris"; pts: number[]; c: RGBA }
  | { k: "line"; pts: ReadonlyArray<readonly [number, number]>; w: number; c: RGBA; dash?: [number, number] }
  | { k: "dot"; x: number; y: number; r: number; c: RGBA };

export const alpha = (c: RGBA, a: number): RGBA => [c[0], c[1], c[2], c[3] * a];
export const mix = (a: RGBA, b: RGBA, t: number): RGBA => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t];

/** Split a polyline into the "on" pieces of a dash pattern (lengths in px). */
export function dashSegments(pts: ReadonlyArray<readonly [number, number]>, on: number, off: number): Array<Array<[number, number]>> {
  const out: Array<Array<[number, number]>> = [];
  let cur: Array<[number, number]> | null = null, drawing = true, left = on;
  for (let i = 0; i + 1 < pts.length; i++) {
    let [x, y] = pts[i];
    const [x2, y2] = pts[i + 1];
    let len = Math.hypot(x2 - x, y2 - y);
    if (len === 0) continue;
    const ux = (x2 - x) / len, uy = (y2 - y) / len;
    if (drawing && !cur) cur = [[x, y]];
    while (len > 1e-9) {
      const step = Math.min(left, len);
      x += ux * step; y += uy * step; len -= step; left -= step;
      if (drawing) cur!.push([x, y]);
      if (left <= 1e-9) {
        if (drawing) { out.push(cur!); cur = null; } else cur = [[x, y]];
        drawing = !drawing; left = drawing ? on : off;
        if (drawing && !cur) cur = [[x, y]];
      }
    }
  }
  if (cur && cur.length > 1) out.push(cur);
  return out;
}

/** Triangles for a ring segment (a dial arc), angles in radians, y down. */
export function arcStrip(cx: number, cy: number, r0: number, r1: number, a0: number, a1: number, steps = 32): number[] {
  const out: number[] = [];
  for (let i = 0; i < steps; i++) {
    const s = a0 + ((a1 - a0) * i) / steps, e = a0 + ((a1 - a0) * (i + 1)) / steps;
    const p = (r: number, a: number) => [cx + Math.cos(a) * r, cy - Math.sin(a) * r];
    const [ax, ay] = p(r0, s), [bx, by] = p(r1, s), [cx2, cy2] = p(r0, e), [dx, dy] = p(r1, e);
    out.push(ax, ay, bx, by, cx2, cy2, bx, by, dx, dy, cx2, cy2);
  }
  return out;
}

/** A bar with a little real depth: front face, lit top, shaded side (oblique extrusion). */
export function extrudedBar(x: number, y: number, w: number, base: number, d: number, front: RGBA, top: RGBA, side: RGBA): Prim[] {
  return [
    { k: "poly", pts: [x + w, y, x + w + d, y - d, x + w + d, base - d, x + w, base], c: side },
    { k: "poly", pts: [x, y, x + d, y - d, x + w + d, y - d, x + w, y], c: top },
    { k: "rect", x, y, w, h: base - y, c: front },
  ];
}

class Buf {
  pos: number[] = []; col: number[] = [];
  v(x: number, y: number, c: RGBA) { this.pos.push(x, y, 0); this.col.push(c[0], c[1], c[2], c[3]); }
  tri(ax: number, ay: number, bx: number, by: number, cx: number, cy: number, c: RGBA) { this.v(ax, ay, c); this.v(bx, by, c); this.v(cx, cy, c); }
}

const DOT_SEGMENTS = 16;
function dot(b: Buf, x: number, y: number, r: number, c: RGBA) {
  for (let i = 0; i < DOT_SEGMENTS; i++) {
    const a0 = (i / DOT_SEGMENTS) * Math.PI * 2, a1 = ((i + 1) / DOT_SEGMENTS) * Math.PI * 2;
    b.tri(x, y, x + Math.cos(a0) * r, y + Math.sin(a0) * r, x + Math.cos(a1) * r, y + Math.sin(a1) * r, c);
  }
}
function stroke(b: Buf, pts: ReadonlyArray<readonly [number, number]>, w: number, c: RGBA) {
  const hw = w / 2;
  for (let i = 0; i + 1 < pts.length; i++) {
    const [x0, y0] = pts[i], [x1, y1] = pts[i + 1];
    const len = Math.hypot(x1 - x0, y1 - y0);
    if (len === 0) continue;
    const nx = (-(y1 - y0) / len) * hw, ny = ((x1 - x0) / len) * hw;
    b.tri(x0 + nx, y0 + ny, x0 - nx, y0 - ny, x1 + nx, y1 + ny, c);
    b.tri(x1 + nx, y1 + ny, x0 - nx, y0 - ny, x1 - nx, y1 - ny, c);
  }
  // Round joins only for opaque strokes: translucent ones would show the overlap.
  if (c[3] >= 0.99 && w >= 2) for (let i = 1; i + 1 < pts.length; i++) dot(b, pts[i][0], pts[i][1], hw, c);
  if (c[3] >= 0.99 && w >= 2 && pts.length > 1) { dot(b, pts[0][0], pts[0][1], hw, c); dot(b, pts[pts.length - 1][0], pts[pts.length - 1][1], hw, c); }
}

/** Flatten primitives (painter's order) into position (xyz) and colour (rgba) buffers. */
export function buildBuffers(prims: readonly Prim[]): { pos: Float32Array; col: Float32Array; vertices: number } {
  const b = new Buf();
  for (const p of prims) {
    if (p.k === "rect") {
      if (p.w <= 0 || p.h <= 0) continue;
      const c1 = p.c1 ?? p.c;
      b.v(p.x, p.y, p.c); b.v(p.x + p.w, p.y, c1); b.v(p.x, p.y + p.h, p.c);
      b.v(p.x + p.w, p.y, c1); b.v(p.x + p.w, p.y + p.h, c1); b.v(p.x, p.y + p.h, p.c);
    } else if (p.k === "poly") {
      for (let i = 2; i + 1 < p.pts.length / 2; i++) b.tri(p.pts[0], p.pts[1], p.pts[i * 2], p.pts[i * 2 + 1], p.pts[i * 2 + 2], p.pts[i * 2 + 3], p.c);
    } else if (p.k === "tris") {
      for (let i = 0; i + 5 < p.pts.length; i += 6) b.tri(p.pts[i], p.pts[i + 1], p.pts[i + 2], p.pts[i + 3], p.pts[i + 4], p.pts[i + 5], p.c);
    } else if (p.k === "line") {
      if (p.dash) for (const seg of dashSegments(p.pts, p.dash[0], p.dash[1])) stroke(b, seg, p.w, p.c);
      else stroke(b, p.pts, p.w, p.c);
    } else dot(b, p.x, p.y, p.r, p.c);
  }
  return { pos: Float32Array.from(b.pos), col: Float32Array.from(b.col), vertices: b.pos.length / 3 };
}
