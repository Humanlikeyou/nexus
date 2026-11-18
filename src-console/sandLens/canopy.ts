/**
 * design-1118b: canopy sunlight only — soft dappled light through leaves, following the time of day.
 * Restored from the pre-4748b6c shader and light math. No mist, no parallax, no glass, no bio tint.
 * Additive-looking patches, very gentle, so Home's colours still read as they are.
 */

export type CanopySun = { lx: number; ly: number; lc: [number, number, number]; li: number };

const clamp = (x: number, a: number, b: number) => Math.min(b, Math.max(a, x));
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
type C3 = [number, number, number];
const mixc = (a: C3, b: C3, t: number): C3 => [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];

/** Minutes since midnight → sun position, colour and strength (from d71cf59 targetAmbience, light only). */
export function canopySun(todMinutes: number): CanopySun {
  const tod = todMinutes / 60;
  const e = Math.sin((Math.PI * (tod - 6)) / 12);
  const day = clamp(e * 1.4 + 0.15, 0, 1), low = day > 0 ? clamp(1 - e * 2.2, 0, 1) : 0;
  const night = 1 - clamp((e + 0.12) * 4, 0, 1);
  let lx = clamp((tod - 5) / 14, 0.05, 0.95), ly = 0.62 - 0.55 * clamp(e, 0, 1);
  if (night > 0.5) { lx = 0.35; ly = 0.05; }
  let lc = mixc([1.0, 0.97, 0.9], [1.0, 0.62, 0.3], low);
  lc = mixc(lc, [0.4, 0.8, 0.62], night);
  const li = lerp(0.13 + 0.06 * clamp(e, 0, 1) + 0.05 * low, 0.05, night);
  return { lx, ly, lc, li };
}

export const minutesNow = (d = new Date()) => d.getHours() * 60 + d.getMinutes() + d.getSeconds() / 60;

/** Same drift rate as the original engine. Frozen when Calm / reduced motion is on. */
export const CANOPY_DRIFT = 0.004;

/** Full-screen quad. vUv matches the original Three.js PlaneGeometry uv. */
export const CANOPYVERT = `attribute vec2 aPos; varying vec2 vUv; void main(){ vUv=aPos*0.5+0.5; gl_Position=vec4(aPos,0.,1.); }`;

/**
 * Canopy dapples from d71cf59. Mist and parallax uniforms are kept at zero so they never draw.
 * Alpha carries the light so a standalone canvas composites over Home.
 */
export const CANOPYFRAG = /* glsl */ `
uniform vec2 uRes; uniform float uTime, uDrift, uInt, uMist, uDpr; uniform vec2 uLight, uPar; uniform vec3 uLCol, uMistCol;
varying vec2 vUv;
float hash(vec2 p){ return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453); }
float noise(vec2 p){ vec2 i=floor(p), f=fract(p); f=f*f*(3.0-2.0*f); return mix(mix(hash(i),hash(i+vec2(1,0)),f.x), mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),f.x), f.y); }
float fbm(vec2 p){ float s=0., a=0.5; for(int i=0;i<5;i++){ s+=a*noise(p); p=p*2.03+vec2(1.7,9.2); a*=0.5; } return s; }
void main(){
  vec2 uv=vec2(vUv.x, 1.0-vUv.y); float asp=uRes.x/uRes.y;
  vec2 q=(uv+uPar)*vec2(asp,1.0)*2.4; float t=uTime*uDrift;
  float n=fbm(q+vec2(t*0.6,t*0.25))*0.65+fbm(q*2.3-vec2(t*0.3,t*0.5))*0.35;
  float spots=smoothstep(0.55,0.74,n);
  float fall=exp(-length((uv-uLight)*vec2(asp,1.0))*1.35);
  vec3 col=uLCol*(spots*0.85+0.22)*fall*uInt;
  float mn=fbm(vec2(uv.x*3.0*asp+uTime*0.018, uv.y*5.0-uTime*0.004));
  col+=uMistCol*smoothstep(0.78,1.0,uv.y)*(0.35+0.65*mn)*uMist;
  float a=max(max(col.r,col.g),col.b);
  gl_FragColor=vec4(col,a);
}`;

export type CanopyHandle = { stop: () => void; redraw: () => void };

function compile(gl: WebGL2RenderingContext, type: number, src: string) {
  const sh = gl.createShader(type); if (!sh) return null;
  gl.shaderSource(sh, src); gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) { gl.deleteShader(sh); return null; }
  return sh;
}

/**
 * Drive the restored canopy shader on a 1/4-resolution canvas. Calm freezes the drift (static patches).
 * Time of day is sampled from `nowMinutes`; the sun does not travel as a visible motion.
 */
export function startCanopy(canvas: HTMLCanvasElement, opts: { calm: () => boolean; nowMinutes?: () => number }): CanopyHandle {
  const gl = canvas.getContext("webgl2", { alpha: true, premultipliedAlpha: true, antialias: false, depth: false, stencil: false });
  if (!gl) return { stop() {}, redraw() {} };
  const vs = compile(gl, gl.VERTEX_SHADER, CANOPYVERT), fs = compile(gl, gl.FRAGMENT_SHADER, CANOPYFRAG);
  const prog = gl.createProgram();
  if (!vs || !fs || !prog) return { stop() {}, redraw() {} };
  gl.attachShader(prog, vs); gl.attachShader(prog, fs); gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return { stop() {}, redraw() {} };
  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(prog, "aPos");
  const u = {
    res: gl.getUniformLocation(prog, "uRes"), time: gl.getUniformLocation(prog, "uTime"), drift: gl.getUniformLocation(prog, "uDrift"),
    int: gl.getUniformLocation(prog, "uInt"), mist: gl.getUniformLocation(prog, "uMist"), dpr: gl.getUniformLocation(prog, "uDpr"),
    light: gl.getUniformLocation(prog, "uLight"), par: gl.getUniformLocation(prog, "uPar"),
    lcol: gl.getUniformLocation(prog, "uLCol"), mistCol: gl.getUniformLocation(prog, "uMistCol"),
  };
  const started = performance.now();
  let raf = 0, running = true, frozenAt: number | null = null, frame = 0;
  const nowMinutes = opts.nowMinutes ?? minutesNow;

  const resize = () => {
    const w = Math.max(1, window.innerWidth), h = Math.max(1, window.innerHeight);
    const cw = Math.max(1, Math.ceil(w / 4)), ch = Math.max(1, Math.ceil(h / 4));
    if (canvas.width !== cw || canvas.height !== ch) { canvas.width = cw; canvas.height = ch; }
    gl.viewport(0, 0, cw, ch);
  };

  const draw = (force = false) => {
    if (!running) return;
    const calm = opts.calm();
    if (calm && frozenAt == null) frozenAt = (performance.now() - started) / 1000;
    if (!calm) frozenAt = null;
    frame++;
    if (!force && !calm && frame % 6 !== 1 && frame > 3) return;
    resize();
    const sun = canopySun(nowMinutes());
    const t = calm ? (frozenAt ?? 0) : (performance.now() - started) / 1000;
    gl.useProgram(prog);
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    gl.disable(gl.DEPTH_TEST);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ZERO);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.uniform2f(u.res, window.innerWidth, window.innerHeight);
    gl.uniform1f(u.time, t);
    gl.uniform1f(u.drift, calm ? 0 : CANOPY_DRIFT);
    gl.uniform1f(u.int, sun.li);
    gl.uniform1f(u.mist, 0);
    gl.uniform1f(u.dpr, 1);
    gl.uniform2f(u.light, sun.lx, sun.ly);
    gl.uniform2f(u.par, 0, 0);
    gl.uniform3f(u.lcol, sun.lc[0], sun.lc[1], sun.lc[2]);
    gl.uniform3f(u.mistCol, 0.55, 0.68, 0.6);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  };

  const loop = () => { if (!running) return; if (!document.hidden) draw(); raf = requestAnimationFrame(loop); };
  const onVis = () => { if (!document.hidden && running) draw(true); };
  window.addEventListener("resize", resize);
  document.addEventListener("visibilitychange", onVis);
  resize();
  draw(true);
  raf = requestAnimationFrame(loop);

  return {
    redraw: () => draw(true),
    stop() {
      running = false;
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
      document.removeEventListener("visibilitychange", onVis);
      try { gl.getExtension("WEBGL_lose_context")?.loseContext(); } catch { /* already gone */ }
    },
  };
}

/** Static CSS fallback when WebGL2 is missing: a soft, still wash at the sun position. No geometry. */
export function canopyFallbackStyle(sun: CanopySun): { background: string; opacity: number } {
  const [r, g, b] = sun.lc.map((v) => Math.round(v * 255));
  const x = `${(sun.lx * 100).toFixed(1)}%`, y = `${(sun.ly * 100).toFixed(1)}%`;
  const c = `rgba(${r},${g},${b},1)`;
  return {
    background: `radial-gradient(ellipse 70% 55% at ${x} ${y}, ${c} 0%, transparent 62%), radial-gradient(ellipse 40% 35% at ${((sun.lx * 100 + 12) % 90 + 5).toFixed(1)}% ${((sun.ly * 100 + 18) % 70 + 8).toFixed(1)}%, ${c} 0%, transparent 55%)`,
    opacity: Math.max(0.04, Math.min(0.16, sun.li * 0.9)),
  };
}
