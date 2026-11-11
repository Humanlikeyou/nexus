/** Sample lines only. None of them name the look. Tagged Sample data in the UI. */

export const SAMPLE_ON_LINES = [
  "Lights out.",
  "Going dark. Stay close.",
  "I'm with you.",
  "Nothing leaves this Mac.",
  "It's just us.",
  "The room just got smaller.",
  "All quiet. Your move.",
  "I heard you. We're off the grid.",
] as const;

export const SAMPLE_OFF_LINES = [
  "Back to the usual light.",
  "Normal. I'm still here.",
] as const;

let onI = 0;
let offI = 0;

export function nextOnLine(): string {
  const text = SAMPLE_ON_LINES[onI % SAMPLE_ON_LINES.length];
  onI += 1;
  return text;
}

export function nextOffLine(): string {
  const text = SAMPLE_OFF_LINES[offI % SAMPLE_OFF_LINES.length];
  offI += 1;
  return text;
}

export function resetLines(): void { onI = 0; offI = 0; }
