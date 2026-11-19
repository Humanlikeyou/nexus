/**
 * Syllable onset detection on her speech loudness (0..1). Two envelopes: a fast one (~40 ms) and a slow
 * one (~350 ms). A rising edge is the fast envelope jumping above the slow one; a refractory gap stops
 * one syllable from firing twice. Pure and deterministic, so it is unit-tested without a GPU.
 */
export const ONSET_RISE = 0.07;
export const ONSET_REARM = 0.02;
export const ONSET_FLOOR = 0.1;
export const ONSET_REFRACTORY_S = 0.14;

export class OnsetDetector {
  private fast = 0; private slow = 0; private cool = 0; private armed = true;
  /** Feed one loudness sample; returns the onset strength (0..1) or 0 for none. */
  update(loud: number, dt: number): number {
    const l = Number.isFinite(loud) ? Math.min(1, Math.max(0, loud)) : 0;
    const d = Math.max(0, dt);
    this.fast += (l - this.fast) * (1 - Math.exp(-d / 0.04));
    this.slow += (l - this.slow) * (1 - Math.exp(-d / 0.35));
    this.cool = Math.max(0, this.cool - d);
    const rise = this.fast - this.slow;
    if (rise < ONSET_REARM) this.armed = true; // a held tone re-arms only after the envelopes meet again
    if (this.armed && this.cool <= 0 && rise > ONSET_RISE && this.fast > ONSET_FLOOR) {
      this.cool = ONSET_REFRACTORY_S; this.armed = false;
      return Math.min(1, rise / 0.3);
    }
    return 0;
  }
  reset() { this.fast = this.slow = this.cool = 0; this.armed = true; }
}
