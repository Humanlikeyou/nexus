import { useEffect, useRef } from "react";
import { webgl2Available } from "../sandLens/gl";
import { reducedMotion } from "../shell/Splash";
import { HARMONICS, goalString } from "./tangleString";

/**
 * Milestones arrival, WebGL: one vibrating string per goal, each in its own key (see tangleString.ts for the physics).
 * The string's shape and slope are evaluated in the vertex shader, so the ribbon keeps an even width while it
 * swings; the fragment shader draws a bright core inside a soft glow. Colours are read from the legend swatches,
 * so the strands always match their legend in both themes. No new class or token: the canvas reuses the SVG's class.
 *
 * Falls back to the SVG strands (rendered by the caller) when WebGL2 is missing or the engine fails.
 * Reduced motion: one still frame. Off screen or in a hidden window: the loop stops.
 */
const VIEW_W = 680, VIEW_H = 140;
const SEGMENTS = 240;
const SWING = 0.36; // of the view height, each side of the centre line
const HALF_WIDTH = 7; // px: glow half-width; the core is ~1.6 px of it
/** Afterimages per strand: the fast overtones blur the way a real vibrating string does to the eye. */
const GHOSTS = 4, GHOST_DT = 0.022, GHOST_ALPHA = 0.2;

const VERT = /* glsl */ `
  #define H ${HARMONICS}
  attribute float aU;
  attribute float aSide;
  uniform float uT, uF, uW, uH, uSwing, uHalf, uLift;
  uniform float uA[H];
  uniform float uPh[H];
  varying float vSide;
  const float PI = 3.141592653589793;
  void main() {
    float y = 0.0, dy = 0.0;
    for (int j = 0; j < H; j++) {
      float n = float(j + 1);
      float c = cos(2.0 * PI * n * uF * uT + uPh[j]);
      y += uA[j] * sin(n * PI * aU) * c;
      dy += uA[j] * n * PI * cos(n * PI * aU) * c;
    }
    float amp = uH * uSwing;
    vec2 p = vec2(aU * uW, uH * 0.5 + y * amp);
    vec2 tangent = normalize(vec2(uW, dy * amp));
    vec2 normal = vec2(-tangent.y, tangent.x);
    float w = uHalf * uLift;
    p += normal * aSide * w;
    vSide = aSide;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(p, 0.0, 1.0);
  }
`;
const FRAG = /* glsl */ `
  precision highp float;
  uniform vec3 uColor;
  uniform float uAlpha, uHalf, uLift;
  varying float vSide;
  void main() {
    float d = abs(vSide) * uHalf * uLift;              // px from the string's centre
    float core = 1.0 - smoothstep(0.9, 1.9, d);
    float glow = exp(-(d * d) / (2.0 * 2.6 * 2.6)) * 0.42;
    float a = clamp(core + glow, 0.0, 1.0) * uAlpha;
    gl_FragColor = vec4(uColor * a, a);                  // premultiplied
  }
`;

export function TangleWaves({ count, hot, colourOf, label, onFail }: { count: number; hot: number | null; colourOf: (k: number) => string | null; label: string; onFail: () => void }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const hotRef = useRef(hot);
  const colourRef = useRef(colourOf);
  const failRef = useRef(onFail);
  useEffect(() => { hotRef.current = hot; colourRef.current = colourOf; failRef.current = onFail; });

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || count === 0) return;
    let gone = false, raf = 0, visible = true;
    let cleanup = () => {};
    import("three").then((THREE) => {
      if (gone) return;
      const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, premultipliedAlpha: true });
      renderer.setClearColor(0x000000, 0);
      const scene = new THREE.Scene();
      const camera = new THREE.OrthographicCamera(0, VIEW_W, VIEW_H, 0, -1, 1);
      const STILL_T = 1.3;
      const still = reducedMotion();
      const geo = new THREE.BufferGeometry();
      const u = new Float32Array((SEGMENTS + 1) * 2), side = new Float32Array((SEGMENTS + 1) * 2), pos = new Float32Array((SEGMENTS + 1) * 2 * 3);
      for (let s = 0; s <= SEGMENTS; s++) { u[s * 2] = u[s * 2 + 1] = s / SEGMENTS; side[s * 2] = -1; side[s * 2 + 1] = 1; }
      const idx: number[] = [];
      for (let s = 0; s < SEGMENTS; s++) { const a = s * 2; idx.push(a, a + 1, a + 2, a + 1, a + 3, a + 2); }
      geo.setAttribute("position", new THREE.BufferAttribute(pos, 3));
      geo.setAttribute("aU", new THREE.BufferAttribute(u, 1));
      geo.setAttribute("aSide", new THREE.BufferAttribute(side, 1));
      geo.setIndex(idx);
      const layers = still ? 1 : 1 + GHOSTS;
      const mats = Array.from({ length: count * layers }, (_, j) => {
        const i = j % count, ghost = Math.floor(j / count);
        const str = goalString(i);
        const mat = new THREE.ShaderMaterial({
          vertexShader: VERT, fragmentShader: FRAG, transparent: true, depthTest: false, depthWrite: false, side: THREE.DoubleSide,
          blending: THREE.CustomBlending, blendSrc: THREE.OneFactor, blendDst: THREE.OneMinusSrcAlphaFactor,
          uniforms: {
            uT: { value: 0 }, uF: { value: str.f }, uW: { value: VIEW_W }, uH: { value: VIEW_H }, uSwing: { value: SWING }, uHalf: { value: HALF_WIDTH }, uLift: { value: 1 },
            uA: { value: str.modes.map((m) => m.amp) }, uPh: { value: str.modes.map((m) => m.phase) },
            uColor: { value: new THREE.Color(1, 1, 1) }, uAlpha: { value: 1 },
          },
        });
        const mesh = new THREE.Mesh(geo, mat);
        mesh.frustumCulled = false;
        mesh.renderOrder = j;
        scene.add(mesh);
        return { mat, i, ghost };
      });
      const resize = () => {
        const w = canvas.clientWidth || VIEW_W, dpr = Math.min(2, window.devicePixelRatio || 1);
        renderer.setPixelRatio(dpr);
        renderer.setSize(w, (w * VIEW_H) / VIEW_W, false);
      };
      const paint = () => mats.forEach(({ mat, i }) => {
        const css = colourRef.current(i % 4);
        if (css) mat.uniforms.uColor.value.setStyle(css);
      });
      const draw = (t: number) => {
        const h = hotRef.current;
        mats.forEach(({ mat, i, ghost }) => {
          mat.uniforms.uT.value = t - ghost * GHOST_DT;
          const a = h === null || h === i ? 1 : 0.28;
          mat.uniforms.uAlpha.value = ghost ? a * GHOST_ALPHA * (1 - (ghost - 1) / GHOSTS) : a;
          mat.uniforms.uLift.value = h === i ? 1.35 : 1;
        });
        renderer.render(scene, camera);
      };
      resize(); paint();
      const ro = new ResizeObserver(() => { resize(); if (still) draw(STILL_T); });
      ro.observe(canvas);
      const io = new IntersectionObserver((es) => { visible = es.some((e) => e.isIntersecting); });
      io.observe(canvas);
      const t0 = performance.now();
      let frame = 0;
      const loop = () => {
        raf = requestAnimationFrame(loop);
        if (!visible || document.hidden) return;
        if (++frame % 60 === 0) paint(); // follows a theme switch within a second
        draw((performance.now() - t0) / 1000);
      };
      if (still) draw(STILL_T); else raf = requestAnimationFrame(loop);
      const onHot = () => { if (still) draw(STILL_T); };
      canvas.addEventListener("tangle-hot", onHot);
      cleanup = () => {
        cancelAnimationFrame(raf); ro.disconnect(); io.disconnect(); canvas.removeEventListener("tangle-hot", onHot);
        mats.forEach(({ mat }) => mat.dispose()); geo.dispose(); renderer.dispose();
      };
    }).catch((e) => { console.warn("[tangle] falling back to the SVG strands", e); if (!gone) failRef.current(); });
    return () => { gone = true; cleanup(); };
  }, [count]);

  // Reduced motion draws on demand: repaint the still frame when the hovered strand changes.
  useEffect(() => { canvasRef.current?.dispatchEvent(new Event("tangle-hot")); }, [hot]);

  return <canvas ref={canvasRef} className="d18-tangle__svg" data-testid="tangle-gl" role="img" aria-label={label} style={{ display: "block", aspectRatio: `${VIEW_W} / ${VIEW_H}` }} />;
}

/** Whether the WebGL strands will be tried (the caller hides the SVG strands then). */
export const tangleGlAvailable = () => webgl2Available();
