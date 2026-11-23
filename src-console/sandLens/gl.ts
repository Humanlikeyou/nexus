/** WebGL2 probe, kept apart from the engine so Home can decide without loading `three`. */
export function webgl2Available(): boolean {
  try { const c = document.createElement("canvas"); return !!c.getContext("webgl2"); } catch { return false; }
}
