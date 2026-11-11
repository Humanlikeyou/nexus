/** The only phrases that turn the look on or off. Flexible about commas and a trailing period. */

const strip = (s: string) => s.toLowerCase().replace(/[“”"'`.!?!,]/g, " ").replace(/\s+/g, " ").trim();

const ON = /^(amara\s+)?go dark$/;
const OFF = /^(amara\s+)?back to normal$/;

export function parseNavyPrompt(text: string): "on" | "off" | null {
  const q = strip(text);
  if (!q) return null;
  if (ON.test(q)) return "on";
  if (OFF.test(q)) return "off";
  return null;
}
