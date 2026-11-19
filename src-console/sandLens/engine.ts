/* eslint-disable @typescript-eslint/no-explicit-any -- `three` ships no types and @types/three would be a second new dependency (three-shim.d.ts) */
import * as THREE from "three";
import { GPUComputationRenderer } from "three/examples/jsm/misc/GPUComputationRenderer.js";
import { buildModes, LUT_W, ModeScheduler, type Peak, type PlateMode } from "./bessel";
import { DFRAG, DVERT, PFRAG, POS, PVERT, QVERT, SHADOWFRAG, VEL } from "./shaders";
import type { SandMaterial } from "./settings";
import { calmPulse, heartPulse } from "./pulse";

/** Lens states: Voice Off / Wake / Listening / Speaking / Thinking. */
export type SandState = "off" | "wake" | "listen" | "speak" | "think";
export const STATE_NUM: Record<SandState, number> = { off: 0, wake: 1, listen: 2, speak: 3, think: 4 };

/** 150k, 100k, 60k, 30k grains; auto-scaled down when frames run slow. */
export const TIERS = [388, 316, 245, 174];

export { webgl2Available } from "./gl";

type Layout = { cx: number; cy: number; R: number };

/**
 * design-1118b: one shared full-viewport canvas (pointer-events: none) and one loop. The lens itself
 * draws nothing: no plate, ring or bezel. Grains sit directly on the green home background.
 */
export class SandEngine {
  private renderer: any; private scene: any; private camera: any; private gpu: any; private posVar: any; private velVar: any;
  private points: any; private pointsMat: any; private densScene: any; private densMat: any; private densRT: any; private shadow: any;
  private U: Record<string, { value: any }>;
  private sched = new ModeScheduler();
  private pk: number[] = [];
  private modes: PlateMode[];
  private state: SandState = "off"; private wakeT = 0; private simTime = 0; private last = 0; private frame = 0;
  private raf = 0; private running = false; private disposed = false;
  private tier = 1; private frames: number[] = []; private windowStart = 0; private steppedDown = false; private startedAt = 0;
  private material: SandMaterial = "sand"; private calm = false; private snapPending = true;
  private loud = 0; private peaks: Peak[] = [];
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
    // last antinode radius per mode: grains outside it see a free edge (the clamped rim would otherwise hoard them)
    this.pk = modes.map((md) => { let i = LUT_W - 1; const a = (j: number) => Math.abs(lut[md.row * LUT_W + j]); while (i > 1 && a(i - 1) >= a(i)) i--; return i / (LUT_W - 1); });
    const lt = new THREE.DataTexture(lut, LUT_W, modes.length, THREE.RedFormat, THREE.FloatType);
    lt.minFilter = lt.magFilter = THREE.NearestFilter; lt.needsUpdate = true;
    this.U = {
      uDt: { value: 0 }, uTime: { value: 0 }, uFrame: { value: 0 }, uState: { value: 0 }, uWakeT: { value: 0 }, uLoud: { value: 0 }, uMic: { value: 0 },
      uBreathP: { value: 5 }, uCalm: { value: 0 }, uSnap: { value: 0 }, uStir: { value: 0 }, uRipAge: { value: 9 }, uRipAmp: { value: 0 }, uInt: { value: 0 },
      uLut: { value: lt }, uRow: { value: new THREE.Vector4() }, uPk: { value: new THREE.Vector4(1, 1, 1, 1) }, uN: { value: new THREE.Vector4() }, uA: { value: new THREE.Vector4() },
    };
    const shared = { tPos: { value: null }, uC: { value: new THREE.Vector2() }, uR: { value: 130 }, uTilt: { value: 0 }, uPx: { value: DPR }, uViewH: { value: window.innerHeight }, uSeed: { value: seed }, uWarp: { value: 0 }, uScale: { value: 1 }, uMat: { value: 0 }, uBeat: { value: 0 }, uGlow: { value: 0 } };
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
    this.buildParticles(TIERS[this.tier]);
    window.addEventListener("resize", this.onResize);
    document.addEventListener("visibilitychange", this.onVisibility);
  }

  private onResize = () => {
    const w = window.innerWidth, h = window.innerHeight;
    this.renderer.setSize(w, h, false); this.camera.right = w; this.camera.top = h; this.camera.updateProjectionMatrix();
    this.pointsMat.uniforms.uViewH.value = h; this.pointsMat.uniforms.uViewW.value = w; this.densMat.uniforms.uViewH.value = h;
    this.densRT.setSize(Math.ceil(w / 4), Math.ceil(h / 4)); this.shadow.material.uniforms.uTexel.value.set(1 / this.densRT.width, 1 / this.densRT.height);
  };
  /** Pause when the window is hidden. */
  private onVisibility = () => { if (document.hidden) this.stop(); else this.start(); };

  get particleCount() { const s = TIERS[this.tier]; return s * s; }

  private updateLayout() {
    const L = this.layout();
    const R = L.R || 130;
    this.pointsMat.uniforms.uC.value.set(L.cx, L.cy); this.pointsMat.uniforms.uR.value = R;
    this.densMat.uniforms.uC.value.set(L.cx, L.cy); this.densMat.uniforms.uR.value = R;
  }

  private buildParticles(size: number) {
    if (this.gpu) { this.gpu.dispose(); this.scene.remove(this.points); this.densScene.clear(); this.points.geometry.dispose(); }
    const count = size * size;
    this.gpu = new GPUComputationRenderer(size, size, this.renderer);
    const p0 = this.gpu.createTexture(), v0 = this.gpu.createTexture();
    this.updateLayout();
    this.initialPositions(p0, size);
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

  /** Start as the even floating layer (a sunflower spread; the shaders take over from the first frame). */
  private initialPositions(tex: any, n: number) {
    const d = tex.image.data as Float32Array; const N = n * n, GA = 2.3999632297;
    for (let i = 0; i < N; i++) { const r = 0.965 * Math.sqrt((i + 0.5) / N), an = i * GA; d[i * 4] = r * Math.cos(an); d[i * 4 + 1] = r * Math.sin(an); d[i * 4 + 2] = 0; d[i * 4 + 3] = 1; }
  }

  setState(s: SandState) { if (s === this.state) return; this.state = s; if (s === "wake") this.wakeT = 0; if (s !== "speak") this.sched.reset(); if (this.calm) this.snapPending = true; }
  /** A syllable onset (0..1 strength) in her speech: launches one ripple across the plate. Ignored when calm or not speaking. */
  onset(strength: number) { if (this.state !== "speak" || this.calm) return; this.ripAge = 0; this.ripAmp = Math.min(1, Math.max(0.25, strength)); }
  private ripAge = 9; private ripAmp = 0; private intens = 0;
  setMaterial(m: SandMaterial) { this.material = m; }
  setCalm(on: boolean) { if (on !== this.calm) { this.calm = on; this.snapPending = true; } }
  /** Voice level (0–1) and, when a spectrum exists, its peaks. Without peaks the caller passes fallbackPeaks(). */
  setVoice(loud: number, peaks: Peak[]) { this.loud = loud; this.peaks = peaks; }
  /** Live heart rate (seconds per beat), or null for no pulse. Never guessed: the caller passes null without fresh data. */
  setPulse(period: number | null) { this.pulseBpm = period ? 60 / period : null; }
  private pulseBpm: number | null = null; private bpmCur = 0; private beatPhase = 0; private pulseAmp = 0;

  /** Captures/tests: hold the real-time loop off so advance() alone drives the simulation. */
  freeze(on = true) { this.frozen = on; if (on) this.stop(); else this.start(); }
  private frozen = false;
  start() { if (this.running || this.disposed || this.frozen) return; this.running = true; this.last = performance.now(); this.startedAt ||= this.last; this.raf = requestAnimationFrame(this.loop); }
  stop() { this.running = false; cancelAnimationFrame(this.raf); }

  /** Step the simulation without real time (tests / captures): n frames of dt seconds. */
  advance(n: number, dt = 1 / 60, renderLast = true) { for (let i = 0; i < n; i++) this.step(dt, 16, renderLast && i === n - 1); }
  private clockMs = 0;
  private ring = 0;
  private stir = 0;

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
    // voice off breathes a little slower than listening
    const breathP = this.state === "off" ? 7.2 : 5.6;
    this.simTime += dt;
    if (this.state === "wake") { this.wakeT += dt / 2.0; if (this.wakeT >= 1.1) { this.state = "listen"; this.onWakeDone?.(); } }
    this.clockMs += dt * 1000; // scheduler runs on simulation time, so stepped captures and tests behave like real time
    const { active, changed } = this.sched.update(this.state === "speak" ? this.peaks : [], this.state === "speak" ? this.loud : 0, dt, this.clockMs);
    if (changed) { this.onModeChange?.(changed); this.stir = 1; } // a new pitch moves the old lines too
    this.U.uStir.value = this.state === "speak" ? this.stir : 0; this.stir = Math.max(0, this.stir - dt / 0.5);
    const row = [0, 0, 0, 0], nn = [0, 0, 0, 0], aa = [0, 0, 0, 0], pk = [1, 1, 1, 1];
    active.forEach((m, i) => { row[i] = m.row; nn[i] = m.n; aa[i] = m.a; pk[i] = this.pk[m.row]; });
    const U = this.U;
    U.uRow.value.set(...row); U.uPk.value.set(...pk); U.uN.value.set(...nn); U.uA.value.set(...aa);
    U.uDt.value = dt; U.uTime.value = this.simTime; U.uFrame.value = this.frame; U.uState.value = STATE_NUM[this.state]; U.uWakeT.value = this.wakeT;
    // the plate rings down over ~0.3 s, so the short gaps between syllables don't stop the figure forming
    this.ring = Math.max(this.loud, this.ring * Math.exp(-dt / 0.3));
    U.uLoud.value = this.state === "speak" ? this.ring : this.loud; U.uMic.value = this.state === "listen" && !this.calm ? 0.12 + 0.08 * Math.sin(this.simTime * 0.9) * Math.sin(this.simTime * 0.37) : 0;
    this.ripAge += dt / 0.7; this.intens += ((this.state === "speak" ? this.loud : 0) - this.intens) * (1 - Math.exp(-dt / 0.6));
    U.uRipAge.value = this.ripAge; U.uRipAmp.value = this.ripAmp; U.uInt.value = Math.min(1, this.intens);
    U.uBreathP.value = breathP; U.uCalm.value = this.calm ? 1 : 0;
    // heartbeat: rate eased toward the live bpm, fading in/out; calm = slower brightness-only swell
    const bt = this.pulseBpm;
    if (bt) { this.bpmCur = this.bpmCur ? this.bpmCur + (bt - this.bpmCur) * (1 - Math.exp(-dt / 3)) : bt; }
    this.pulseAmp += ((bt ? 1 : 0) - this.pulseAmp) * (1 - Math.exp(-dt / 1.5));
    if (this.bpmCur) this.beatPhase += dt * this.bpmCur / 60;
    const beatV = this.calm ? calmPulse(this.beatPhase / 2) : heartPulse(this.beatPhase);
    U.uSnap.value = this.calm && this.snapPending ? 1 : 0;
    this.gpu.compute(); this.snapPending = false; U.uSnap.value = 0;
    // the organic rim is always there: the sand floats in every state
    this.warp += (1 - this.warp) * (1 - Math.exp(-dt / 0.2));
    const PU = this.pointsMat.uniforms, DU = this.densMat.uniforms;
    const tex = this.gpu.getCurrentRenderTarget(this.posVar).texture;
    const tilt = this.state === "wake" && !this.calm ? 0.18 * Math.sin(Math.PI * Math.min(1, this.wakeT / 0.95)) : 0;
    PU.uTilt.value += (tilt - PU.uTilt.value) * (1 - Math.exp(-dt / 0.12)); DU.uTilt.value = PU.uTilt.value;
    PU.tPos.value = tex; DU.tPos.value = tex; PU.uWarp.value = this.warp; DU.uWarp.value = this.warp;
    const beatS = this.calm ? 0 : 0.016 * beatV * this.pulseAmp, glow = (this.calm ? 0.035 : 0.07) * beatV * this.pulseAmp;
    PU.uBeat.value = beatS; DU.uBeat.value = beatS; PU.uGlow.value = glow; DU.uGlow.value = glow;
    const mat = this.material === "fireflies" ? 1 : 0; PU.uMat.value = mat; DU.uMat.value = mat; PU.uTime.value = this.simTime;
    PU.uAlpha.value = this.state === "speak" ? 1 : 0.92;
    // a fixed soft light from the upper left → grain lighting + shadow offset (shadow falls away from it)
    const lx = 0.3, ly = 0.2;
    const L = this.layout();
    const dx = lx * window.innerWidth - L.cx, dy = -(ly * window.innerHeight - L.cy);
    const len = Math.hypot(dx, dy) || 1;
    PU.uLight.value.set((dx / len) * 0.62, (dy / len) * 0.62, 0.78).normalize();
    PU.uLightK.value = 0.85;
    this.shadow.material.uniforms.uOff.value.set((-dx / len) * 2.2 / this.densRT.width, (-dy / len) * 2.2 / this.densRT.height);
    this.shadow.visible = mat === 0;
    const r = this.renderer;
    if (!render) return;
    if (mat === 0) { r.setRenderTarget(this.densRT); r.setClearColor(0x000000, 0); r.clear(); r.render(this.densScene, this.camera); }
    r.setRenderTarget(null); r.render(this.scene, this.camera);
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
    try { this.gpu?.dispose(); this.densRT.dispose(); this.points?.geometry.dispose(); this.pointsMat.dispose(); this.densMat.dispose(); this.renderer.dispose(); } catch { /* context already lost */ }
  }
}
