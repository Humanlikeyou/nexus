/**
 * One WebGL renderer for every Mind–Body chart (the page must never hold more than a few contexts).
 * Each chart paints its primitives into the shared offscreen canvas, then copies the pixels into its own 2D canvas.
 * `three` is imported lazily, like sandLens/engine.ts, so it stays out of the main chunk.
 */
import { webgl2Available } from "../../sandLens/gl";
import { buildBuffers, type Prim } from "./prims";

type Shared = { THREE: any; renderer: any; scene: any; camera: any; material: any; mesh: any | null; lost: boolean }; // eslint-disable-line @typescript-eslint/no-explicit-any

let shared: Shared | null = null;
let loading: Promise<Shared | null> | null = null;
let refs = 0;
let supported: boolean | null = null;

/** WebGL2 probe, cached (each probe is a context). jsdom has no canvas: always false there. */
export function gfxSupported(): boolean {
  if (supported === null) supported = typeof navigator !== "undefined" && !/jsdom/i.test(navigator.userAgent) && webgl2Available();
  return supported;
}
export function resetGfxSupportForTests(value: boolean | null = null) { supported = value; }

async function create(): Promise<Shared | null> {
  try {
    const THREE = await import("three");
    const canvas = document.createElement("canvas");
    const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: "low-power" });
    // Colours are authored in sRGB and blended as CSS would: no output conversion.
    renderer.outputColorSpace = THREE.LinearSRGBColorSpace;
    renderer.setClearColor(0x000000, 0);
    renderer.setPixelRatio(1);
    const s: Shared = {
      THREE, renderer, scene: new THREE.Scene(), camera: new THREE.OrthographicCamera(0, 1, 0, 1, -1, 1),
      material: new THREE.MeshBasicMaterial({ vertexColors: true, transparent: true, depthTest: false, depthWrite: false, side: THREE.DoubleSide }), mesh: null, lost: false,
    };
    canvas.addEventListener("webglcontextlost", (e: Event) => { e.preventDefault(); s.lost = true; });
    return s;
  } catch { return null; }
}

/** Takes a reference on the shared renderer (null if three or WebGL failed). Pair with releaseShared(). */
export async function acquireShared(): Promise<Shared | null> {
  if (!loading) loading = create();
  const s = await loading;
  if (!s) { loading = null; return null; }
  shared = s;
  refs++;
  return s;
}

export function releaseShared(): void {
  refs = Math.max(0, refs - 1);
  if (refs > 0 || !shared) return;
  const s = shared;
  shared = null; loading = null;
  try {
    if (s.mesh) { s.scene.remove(s.mesh); s.mesh.geometry.dispose(); }
    s.material.dispose(); s.renderer.dispose(); s.renderer.forceContextLoss?.();
  } catch { /* already gone */ }
}

/** Draws `prims` (pixel space, y down) into `target`. False if the context is gone (caller falls back). */
export function paint(s: Shared, target: HTMLCanvasElement, w: number, h: number, dpr: number, prims: readonly Prim[]): boolean {
  if (s.lost) return false;
  try {
    const { THREE } = s;
    const pw = Math.max(1, Math.round(w * dpr)), ph = Math.max(1, Math.round(h * dpr));
    const { pos, col } = buildBuffers(prims);
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(pos, 3));
    geometry.setAttribute("color", new THREE.BufferAttribute(col, 4));
    if (s.mesh) { s.scene.remove(s.mesh); s.mesh.geometry.dispose(); }
    s.mesh = new THREE.Mesh(geometry, s.material);
    s.mesh.frustumCulled = false;
    s.scene.add(s.mesh);
    Object.assign(s.camera, { left: 0, right: w, top: 0, bottom: h });
    s.camera.updateProjectionMatrix();
    s.renderer.setSize(pw, ph, false);
    s.renderer.render(s.scene, s.camera);
    if (target.width !== pw) target.width = pw;
    if (target.height !== ph) target.height = ph;
    const ctx = target.getContext("2d");
    if (!ctx) return false;
    ctx.clearRect(0, 0, pw, ph);
    ctx.drawImage(s.renderer.domElement, 0, 0, pw, ph);
    return true;
  } catch { return false; }
}
