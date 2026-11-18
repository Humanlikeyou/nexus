/* eslint-disable @typescript-eslint/no-explicit-any -- `three` ships no types and @types/three would be a second new dependency (three-shim.d.ts) */
import * as THREE from "three";
import { GPUComputationRenderer } from "three/examples/jsm/misc/GPUComputationRenderer.js";
import { buildModes, LUT_W, ModeScheduler, type Peak, type PlateMode } from "./bessel";
import { BLITFRAG, CANOPYFRAG, DFRAG, DVERT, PFRAG, POS, PVERT, QVERT, SHADOWFRAG, VEL } from "./shaders";
import type { Ambience } from "./ambience";
import type { SandMaterial } from "./settings";

/** Lens states: Voice Off / Wake / Listening / Speaking. */
export type SandState = "off" | "wake" | "listen" | "speak";
export const STATE_NUM: Record<SandState, number> = { off: 0, wake: 1, listen: 2, speak: 3 };
export type SandMoment = "arrive" | "complete" | "notify";

/** 150k, 100k, 60k, 30k grains; auto-scaled down when frames run slow. */
export const TIERS = [388, 316, 245, 174];

export { webgl2Available } from "./gl";

type Layout = { cx: number; cy: number; R: number; card: DOMRect | null };

/**
 * design-1118b: one shared full-viewport canvas (pointer-events: none) and one loop. The lens itself
 * draws nothing: no plate, ring or bezel. Grains sit directly on the green home background.
 */
export class SandEngine {
  private renderer: any; private scene: any; private camera: any; private gpu: any; private posVar: any; private velVar: any;
  private points: any; private pointsMat: any; private densScene: any; private densMat: any; private densRT: any; private shadow: any;
  private cScene: any; private cRT: any; private canopy: any; private blit: any;
  private U: Record<string, { value: any }>;
  private sched = new ModeScheduler();
  private modes: PlateMode[];
  private state: SandState = "off"; private wakeT = 0; private simTime = 0; private last = 0; private frame = 0;
  private raf = 0; private running = false; private disposed = false;
  private tier = 1; private frames: number[] = []; private windowStart = 0; private steppedDown = false; private startedAt = 0;
  private material: SandMaterial = "sand"; private calm = false; private snapPending = true;
  private loud = 0; private peaks: Peak[] = []; private kick = 0; private wave = -1;
  private amb: Ambience | null = null; private canopyOn = true;
  private warp = 0;
  onModeChange: ((m: PlateMode) => void) | null = null;
  onWakeDone: (() => void) | null = null;

  constructor(private canvas: HTMLCanvasElement, private layout: () => Layout, seed: number, opts: { tier?: number } = {}) {
    const { modes, lut } = buildModes();
    this.modes = modes;
    if (opts.tier != null) this.tier = Math.max(0, Math.min(TIERS.length - 1, opts.tier));
    const gl = canvas.getContext("webgl2", { antialias: false, alpha: true, premultipliedAlpha: true, powerPreference: "high-performance" });
    if (!gl) throw new Error("webgl2 unavailable");
    const DPR = Math.min(2, window.devicePixelRatio || 1);
    this.renderer = new THREE.WebGLRenderer({ canvas, context: gl, antialias: false, alpha: true, premultipliedAlpha: true });
    this.renderer.setPixelRatio(DPR); this.renderer.setSize(window.innerWidth, window.innerHeight, false); this.renderer.setClearColor(0x000000, 0);
    this.scene = new THREE.Scene();
    this.camera = new THREE.OrthographicCamera(0, window.innerWidth, window.innerHeight, 0, -10, 10);
    const lt = new THREE.DataTexture(lut, LUT_W, modes.length, THREE.RedFormat, THREE.FloatType);
    lt.minFilter = lt.magFilter = THREE.NearestFilter; lt.needsUpdate = true;
    this.U = {
      uDt: { value: 0 }, uTime: { value: 0 }, uFrame: { value: 0 }, uState: { value: 0 }, uWakeT: { value: 0 }, uLoud: { value: 0 }, uMic: { value: 0 },
      uBreathP: { value: 5 }, uCalm: { value: 0 }, uSnap: { value: 0 }, uKick: { value: 0 }, uWave: { value: -1 },
      uCard: { value: new THREE.Vector4(-2, 2, -1.7, 0.16) }, uBottom: { value: -4.5 },
      uLut: { value: lt }, uRow: { value: new THREE.Vector4() }, uN: { value: new THREE.Vector4() }, uA: { value: new THREE.Vector4() },
    };
    const shared = { tPos: { value: null }, uC: { value: new THREE.Vector2() }, uR: { value: 130 }, uTilt: { value: 0 }, uPx: { value: DPR }, uViewH: { value: window.innerHeight }, uSeed: { value: seed }, uWarp: { value: 0 }, uScale: { value: 1 }, uMat: { value: 0 } };
    const premulNormal = { transparent: true, depthTest: false, depthWrite: false, blending: THREE.CustomBlending, blendSrc: THREE.OneFactor, blendDst: THREE.OneMinusSrcAlphaFactor, blendSrcAlpha: THREE.OneFactor, blendDstAlpha: THREE.OneMinusSrcAlphaFactor };
    const additive = { transparent: true, depthTest: false, depthWrite: false, blending: THREE.CustomBlending, blendSrc: THREE.OneFactor, blendDst: THREE.OneFactor };
    this.pointsMat = new THREE.ShaderMaterial({ ...premulNormal, uniforms: { ...shared, uTime: { value: 0 }, uAlpha: { value: 1 }, uKeep: { value: 0.03 }, uCount: { value: 1 }, uLight: { value: new THREE.Vector3(0.3, 0.5, 0.8) }, uLightK: { value: 1 }, tDens: { value: null }, uViewW: { value: window.innerWidth } }, vertexShader: PVERT, fragmentShader: PFRAG });
    // density → contact shadow
    this.densRT = new THREE.WebGLRenderTarget(Math.ceil(window.innerWidth / 4), Math.ceil(window.innerHeight / 4), { type: THREE.HalfFloatType, minFilter: THREE.LinearFilter, magFilter: THREE.LinearFilter });
    this.pointsMat.uniforms.tDens.value = this.densRT.texture; // heap relief: thick heaps read lighter, thin scatter greyer
    this.densMat = new THREE.ShaderMaterial({ ...additive, uniforms: { ...shared, uGain: { value: 0.12 } }, vertexShader: DVERT, fragmentShader: DFRAG });
    this.densScene = new THREE.Scene();
    this.shadow = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), new THREE.ShaderMaterial({ ...premulNormal, uniforms: { tD: { value: this.densRT.texture }, uTexel: { value: new THREE.Vector2(1 / this.densRT.width, 1 / this.densRT.height) }, uOff: { value: new THREE.Vector2() }, uK: { value: 0.35 }, uShadow: { value: new THREE.Color(0.03, 0.09, 0.06) } }, vertexShader: QVERT, fragmentShader: SHADOWFRAG }));
    this.shadow.frustumCulled = false; this.shadow.renderOrder = 1; this.scene.add(this.shadow);
    // canopy light + mist at 1/4 resolution
    this.canopy = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), new THREE.ShaderMaterial({ ...additive, uniforms: { uRes: { value: new THREE.Vector2(window.innerWidth, window.innerHeight) }, uDpr: { value: 1 }, uTime: { value: 0 }, uDrift: { value: 0.004 }, uInt: { value: 0.1 }, uMist: { value: 0.1 }, uLight: { value: new THREE.Vector2(0.3, 0.2) }, uPar: { value: new THREE.Vector2() }, uLCol: { value: new THREE.Color(1, 0.95, 0.85) }, uMistCol: { value: new THREE.Color(0.55, 0.68, 0.6) } }, vertexShader: QVERT, fragmentShader: CANOPYFRAG }));
    this.canopy.frustumCulled = false; this.cScene = new THREE.Scene(); this.cScene.add(this.canopy);
    this.cRT = new THREE.WebGLRenderTarget(Math.ceil(window.innerWidth / 4), Math.ceil(window.innerHeight / 4), { type: THREE.HalfFloatType, minFilter: THREE.LinearFilter, magFilter: THREE.LinearFilter });
    this.blit = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), new THREE.ShaderMaterial({ ...additive, uniforms: { tC: { value: this.cRT.texture } }, vertexShader: QVERT, fragmentShader: BLITFRAG }));
    this.blit.frustumCulled = false; this.blit.renderOrder = 0; this.scene.add(this.blit);
    this.buildParticles(TIERS[this.tier]);
    window.addEventListener("resize", this.onResize);
    document.addEventListener("visibilitychange", this.onVisibility);
  }

  private onResize = () => {
    const w = window.innerWidth, h = window.innerHeight;
    this.renderer.setSize(w, h, false); this.camera.right = w; this.camera.top = h; this.camera.updateProjectionMatrix();
    this.pointsMat.uniforms.uViewH.value = h; this.pointsMat.uniforms.uViewW.value = w; this.densMat.uniforms.uViewH.value = h;
    this.densRT.setSize(Math.ceil(w / 4), Math.ceil(h / 4)); this.shadow.material.uniforms.uTexel.value.set(1 / this.densRT.width, 1 / this.densRT.height);
    this.cRT.setSize(Math.ceil(w / 4), Math.ceil(h / 4)); this.canopy.material.uniforms.uRes.value.set(w, h);
  };
  /** Pause when the window is hidden. */
  private onVisibility = () => { if (document.hidden) this.stop(); else this.start(); };

  get particleCount() { const s = TIERS[this.tier]; return s * s; }

  private updateLayout() {
    const L = this.layout();
    const R = L.R || 130;
    this.pointsMat.uniforms.uC.value.set(L.cx, L.cy); this.pointsMat.uniforms.uR.value = R;
    this.densMat.uniforms.uC.value.set(L.cx, L.cy); this.densMat.uniforms.uR.value = R;
    if (L.card) { const c = L.card, s = R / 160; this.U.uCard.value.set((c.left + 20 * s - L.cx) / R, (c.right - 20 * s - L.cx) / R, (L.cy - c.top) / R + 0.004, (18 * s) / R); }
    this.U.uBottom.value = (L.cy - window.innerHeight) / R;
  }

  private buildParticles(size: number, pour = false) {
    if (this.gpu) { this.gpu.dispose(); this.scene.remove(this.points); this.densScene.clear(); this.points.geometry.dispose(); }
    const count = size * size;
    this.gpu = new GPUComputationRenderer(size, size, this.renderer);
    const p0 = this.gpu.createTexture(), v0 = this.gpu.createTexture();
    this.updateLayout();
    this.initialPositions(p0, size, pour);
    this.velVar = this.gpu.addVariable("textureVelocity", VEL, v0);
    this.posVar = this.gpu.addVariable("texturePosition", POS, p0);
    this.gpu.setVariableDependencies(this.velVar, [this.posVar, this.velVar]);
    this.gpu.setVariableDependencies(this.posVar, [this.posVar, this.velVar]);
    for (const v of [this.velVar, this.posVar]) { Object.assign(v.material.uniforms, this.U); v.wrapS = v.wrapT = THREE.ClampToEdgeWrapping; v.minFilter = v.magFilter = THREE.NearestFilter; }
    const err = this.gpu.init(); if (err) throw new Error(String(err));
    const g = new THREE.BufferGeometry();
    const ref = new Float32Array(count * 2), pos = new Float32Array(count * 3);
    for (let i = 0; i < count; i++) { ref[i * 2] = ((i % size) + 0.5) / size; ref[i * 2 + 1] = (Math.floor(i / size) + 0.5) / size; }
    g.setAttribute("position", new THREE.BufferAttribute(pos, 3)); g.setAttribute("ref", new THREE.BufferAttribute(ref, 2));
    this.points = new THREE.Points(g, this.pointsMat); this.points.frustumCulled = false; this.points.renderOrder = 2; this.scene.add(this.points);
    const dp = new THREE.Points(g, this.densMat); dp.frustumCulled = false; this.densScene.add(dp);
    this.pointsMat.uniforms.uCount.value = Math.min(1, Math.max(0.55, 60000 / count));
    this.pointsMat.uniforms.uKeep.value = Math.min(1, 1800 / count);
    this.densMat.uniforms.uGain.value = Math.min(0.5, 0.12 * (150000 / count));
  }

  private initialPositions(tex: any, n: number, pour: boolean) {
    const d = tex.image.data as Float32Array; let a = 1234;
    const rng = () => { a |= 0; a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
    const c = this.U.uCard.value;
    for (let i = 0; i < n * n; i++) {
      let x: number, y: number;
      if (pour) { x = c.x + (c.y - c.x) * rng(); y = 1.4 + rng() * 2.2; }
      else if (this.state === "off") {
        const tri = Math.min(1, Math.max(0, ((rng() + rng() + rng() + rng()) / 4 - 0.5) * 1.7 + 0.5));
        const ph = (0.35 + 0.65 * Math.pow(1 - Math.abs(2 * tri - 1), 1.2)) * (0.6 + 0.4 * Math.sin(tri * 29 + 1.3) * Math.sin(tri * 11 + 0.4));
        x = c.x + 0.06 + (c.y - c.x - 0.12) * tri; y = c.z + c.w * ph * Math.pow(rng(), 0.9);
      } else { const r = 0.965 * Math.sqrt(rng()), an = rng() * Math.PI * 2; x = r * Math.cos(an); y = r * Math.sin(an); }
      d[i * 4] = x; d[i * 4 + 1] = y; d[i * 4 + 2] = 0; d[i * 4 + 3] = 1;
    }
  }

  setState(s: SandState) { if (s === this.state) return; this.state = s; if (s === "wake") this.wakeT = 0; if (s !== "speak") this.sched.reset(); if (this.calm) this.snapPending = true; }
  setMaterial(m: SandMaterial) { this.material = m; }
  setCalm(on: boolean) { if (on !== this.calm) { this.calm = on; this.snapPending = true; } }
  setAmbience(a: Ambience | null, canopy: boolean) { this.amb = a; this.canopyOn = canopy; }
  /** Voice level (0–1) and, when a spectrum exists, its peaks. Without peaks the caller passes fallbackPeaks(). */
  setVoice(loud: number, peaks: Peak[]) { this.loud = loud; this.peaks = peaks; }
  moment(kind: SandMoment) {
    if (this.calm) return;
    if (kind === "complete") this.kick = 1;
    if (kind === "notify") this.wave = 0;
    if (kind === "arrive") this.buildParticles(TIERS[this.tier], true);
  }

  /** Captures/tests: hold the real-time loop off so advance() alone drives the simulation. */
  freeze(on = true) { this.frozen = on; if (on) this.stop(); else this.start(); }
  private frozen = false;
  start() { if (this.running || this.disposed || this.frozen) return; this.running = true; this.last = performance.now(); this.startedAt ||= this.last; this.raf = requestAnimationFrame(this.loop); }
  stop() { this.running = false; cancelAnimationFrame(this.raf); }

  /** Step the simulation without real time (tests / captures): n frames of dt seconds. */
  advance(n: number, dt = 1 / 60, renderLast = true) { for (let i = 0; i < n; i++) { if (i === n - 1) this.forceCanopy = true; this.step(dt, 16, renderLast && i === n - 1); } }
  private forceCanopy = false;

  private loop = (now: number) => {
    if (!this.running) return;
    const dtMs = Math.min(100, now - this.last); this.last = now;
    this.step(Math.min(1 / 30, dtMs / 1000), dtMs);
    this.autoscale(now, dtMs);
    this.raf = requestAnimationFrame(this.loop);
  };

  private autoscale(now: number, dtMs: number) {
    this.frames.push(dtMs); if (this.frames.length > 240) this.frames.shift();
    if (now - this.startedAt < 1500) return;
    if (!this.windowStart) this.windowStart = now;
    if (now - this.windowStart < 1500) return;
    const w = this.frames.slice(-60); const avg = w.reduce((a, b) => a + b, 0) / w.length; this.windowStart = now;
    if (avg > 22 && this.tier < TIERS.length - 1) { this.tier++; this.steppedDown = true; this.buildParticles(TIERS[this.tier]); this.frames = []; }
    else if (avg < 11 && this.tier > 0 && !this.steppedDown) { this.tier--; this.buildParticles(TIERS[this.tier]); this.frames = []; }
  }

  private step(dt: number, _dtMs: number, render = true) {
    this.frame++;
    this.updateLayout();
    const a = this.amb; const energy = a && !this.calm ? a.energy : 1;
    const breathP = 6 - (2 * (energy - 0.7)) / 0.5;
    this.simTime += dt * energy;
    if (this.state === "wake") { this.wakeT += dt / 2.0; if (this.wakeT >= 1.1) { this.state = "listen"; this.onWakeDone?.(); } }
    const { active, changed } = this.sched.update(this.state === "speak" ? this.peaks : [], this.state === "speak" ? this.loud : 0, dt, performance.now());
    if (changed) this.onModeChange?.(changed);
    const row = [0, 0, 0, 0], nn = [0, 0, 0, 0], aa = [0, 0, 0, 0];
    active.forEach((m, i) => { row[i] = m.row; nn[i] = m.n; aa[i] = m.a; });
    const U = this.U;
    U.uRow.value.set(...row); U.uN.value.set(...nn); U.uA.value.set(...aa);
    U.uDt.value = dt * energy; U.uTime.value = this.simTime; U.uFrame.value = this.frame; U.uState.value = STATE_NUM[this.state]; U.uWakeT.value = this.wakeT;
    U.uLoud.value = this.loud; U.uMic.value = this.state === "listen" ? 0.12 + 0.08 * Math.sin(this.simTime * 0.9) * Math.sin(this.simTime * 0.37) : 0;
    U.uBreathP.value = breathP; U.uCalm.value = this.calm ? 1 : 0;
    U.uKick.value = this.kick; this.kick = Math.max(0, this.kick - dt * 3);
    U.uWave.value = this.wave; if (this.wave >= 0) { this.wave += dt * 1.1; if (this.wave > 1.6) this.wave = -1; }
    U.uSnap.value = this.calm && this.snapPending ? 1 : 0;
    this.gpu.compute(); this.snapPending = false; U.uSnap.value = 0;
    // organic rim only once the grains form the lens (not while heaped on the card)
    const warpT = this.state === "off" ? 0 : this.state === "wake" ? Math.min(1, Math.max(0, (this.wakeT - 0.5) / 0.45)) : 1;
    this.warp += (warpT - this.warp) * (1 - Math.exp(-dt / 0.2));
    const PU = this.pointsMat.uniforms, DU = this.densMat.uniforms;
    const tex = this.gpu.getCurrentRenderTarget(this.posVar).texture;
    const tilt = this.state === "wake" && !this.calm ? 0.55 * Math.sin(Math.PI * Math.min(1, this.wakeT / 0.95)) : 0;
    PU.uTilt.value += (tilt - PU.uTilt.value) * (1 - Math.exp(-dt / 0.12)); DU.uTilt.value = PU.uTilt.value;
    PU.tPos.value = tex; DU.tPos.value = tex; PU.uWarp.value = this.warp; DU.uWarp.value = this.warp;
    const mat = this.material === "fireflies" ? 1 : 0; PU.uMat.value = mat; DU.uMat.value = mat; PU.uTime.value = this.simTime;
    PU.uAlpha.value = (1 - (a?.dim ?? 0) * 0.25) * (this.state === "speak" ? 1 : 0.92);
    // canopy light direction → grain lighting + shadow offset (shadow falls away from the light)
    const lx = a ? a.lx : 0.3, ly = a ? a.ly : 0.2;
    const L = this.layout();
    const dx = lx * window.innerWidth - L.cx, dy = -(ly * window.innerHeight - L.cy);
    const len = Math.hypot(dx, dy) || 1;
    PU.uLight.value.set((dx / len) * 0.62, (dy / len) * 0.62, 0.78).normalize();
    PU.uLightK.value = a ? Math.min(1, 0.6 + a.li * 3) : 0.85;
    this.shadow.material.uniforms.uOff.value.set((-dx / len) * 2.2 / this.densRT.width, (-dy / len) * 2.2 / this.densRT.height);
    this.shadow.visible = mat === 0;
    // canopy
    const CU = this.canopy.material.uniforms;
    CU.uTime.value = performance.now() / 1000; CU.uDrift.value = this.calm ? 0 : 0.004;
    if (a) { CU.uInt.value = a.li; CU.uLight.value.set(a.lx, a.ly); CU.uLCol.value.setRGB(...a.lc); CU.uMist.value = this.calm ? 0 : 0.1 * (1 - a.dim * 0.3); }
    this.blit.visible = this.canopyOn && !!a;
    const r = this.renderer;
    if (!render) return;
    if (this.blit.visible && (this.frame % 6 === 1 || this.frame < 3 || this.forceCanopy)) { r.setRenderTarget(this.cRT); r.clear(); r.render(this.cScene, this.camera); }
    if (mat === 0) { r.setRenderTarget(this.densRT); r.setClearColor(0x000000, 0); r.clear(); r.render(this.densScene, this.camera); }
    r.setRenderTarget(null); r.render(this.scene, this.camera); this.forceCanopy = false;
  }

  /** Grain positions in plate units (tests / capture checks). */
  readPositions(): Float32Array {
    const rt = this.gpu.getCurrentRenderTarget(this.posVar); const s = TIERS[this.tier];
    const buf = new Float32Array(s * s * 4); this.renderer.readRenderTargetPixels(rt, 0, 0, s, s, buf); return buf;
  }
  stats() { return { state: this.state, tier: this.tier, count: this.particleCount, wakeT: this.wakeT, warp: this.warp }; }

  dispose() {
    this.disposed = true; this.stop();
    window.removeEventListener("resize", this.onResize); document.removeEventListener("visibilitychange", this.onVisibility);
    try { this.gpu?.dispose(); this.densRT.dispose(); this.cRT.dispose(); this.points?.geometry.dispose(); this.pointsMat.dispose(); this.densMat.dispose(); this.renderer.dispose(); } catch { /* context already lost */ }
  }
}
