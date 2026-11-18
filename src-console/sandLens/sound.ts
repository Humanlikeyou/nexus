/**
 * design-1118b step f: sound, OFF by default. Soft wood and water ticks at the plate-mode frequency when
 * the figure changes, and a sand rustle on state changes. WebAudio in the UI only; volume and mute.
 * Nothing plays until the toggle is on and the page has had a user gesture.
 */
let ctx: AudioContext | null = null;
let out: GainNode | null = null;
const clamp = (x: number, a: number, b: number) => Math.min(b, Math.max(a, x));

export function soundOut(volume: number, mute: boolean): GainNode | null {
  if (typeof window === "undefined" || typeof window.AudioContext !== "function") return null;
  if (!ctx) { ctx = new AudioContext({ latencyHint: "interactive" }); out = ctx.createGain(); out.connect(ctx.destination); }
  out!.gain.setTargetAtTime(mute ? 0 : volume * 0.5, ctx.currentTime, 0.03);
  if (ctx.state === "suspended") void ctx.resume().catch(() => undefined);
  return out;
}

export function wood(f: number, g: GainNode) {
  const c = g.context as AudioContext, t = c.currentTime;
  for (const [mul, amp, dec] of [[1, 0.22, 0.35], [2.76, 0.07, 0.12]] as const) {
    const o = c.createOscillator(), e = c.createGain();
    o.frequency.value = clamp(f, 120, 1400) * mul;
    e.gain.setValueAtTime(0.0001, t); e.gain.exponentialRampToValueAtTime(amp, t + 0.005); e.gain.exponentialRampToValueAtTime(0.0001, t + dec);
    o.connect(e); e.connect(g); o.start(t); o.stop(t + dec + 0.05);
  }
}
export function water(f: number, g: GainNode) {
  const c = g.context as AudioContext, t = c.currentTime;
  const o = c.createOscillator(), e = c.createGain(), lp = c.createBiquadFilter();
  lp.type = "lowpass"; lp.frequency.value = 900;
  const f0 = clamp(f / 2, 90, 700);
  o.frequency.setValueAtTime(f0 * 1.04, t); o.frequency.exponentialRampToValueAtTime(f0, t + 0.5);
  e.gain.setValueAtTime(0.0001, t); e.gain.exponentialRampToValueAtTime(0.12, t + 0.03); e.gain.exponentialRampToValueAtTime(0.0001, t + 0.6);
  o.connect(lp); lp.connect(e); e.connect(g); o.start(t); o.stop(t + 0.7);
}
/** Dry sand sliding: band-passed noise. */
export function rustle(g: GainNode) {
  const c = g.context as AudioContext, t = c.currentTime;
  const n = (c.sampleRate * 0.6) | 0, b = c.createBuffer(1, n, c.sampleRate), d = b.getChannelData(0);
  let seed = 7; const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647) * 2 - 1;
  for (let i = 0; i < n; i++) d[i] = rnd() * Math.sin((Math.PI * i) / n);
  const s = c.createBufferSource(), bp = c.createBiquadFilter(), e = c.createGain();
  bp.type = "bandpass"; bp.frequency.value = 4200; bp.Q.value = 0.8; e.gain.value = 0.06;
  s.buffer = b; s.connect(bp); bp.connect(e); e.connect(g); s.start(t);
}
