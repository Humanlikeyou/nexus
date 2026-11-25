import type { RGBA } from "./prims";

/** The colours a chart needs, read from the page's CSS variables so light and dark follow the theme. */
export type Theme = { accent: RGBA; copper: RGBA; label: RGBA; label2: RGBA; label3: RGBA; sep: RGBA; bg: RGBA; deep: RGBA; rem: RGBA; light: RGBA; dark: boolean };

/** rgb()/rgba()/#hex/color(srgb …) as 0..1 channels, or null if the text isn't one of those. */
export function parseCssColor(text: string): RGBA | null {
  const s = text.trim().toLowerCase();
  const hex = /^#([0-9a-f]{3,8})$/.exec(s);
  if (hex) {
    let h = hex[1];
    if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join("");
    if (h.length !== 6 && h.length !== 8) return null;
    const n = (i: number) => parseInt(h.slice(i, i + 2), 16) / 255;
    return [n(0), n(2), n(4), h.length === 8 ? n(6) : 1];
  }
  const fn = /^(rgba?|color)\((.+)\)$/.exec(s);
  if (!fn) return null;
  const body = fn[2].replace("/", " ").replace(/,/g, " ").split(/\s+/).filter(Boolean);
  const nums = fn[1] === "color" ? (body[0] === "srgb" ? body.slice(1) : null) : body;
  if (!nums || nums.length < 3) return null;
  const scale = fn[1] === "color" ? 1 : 255;
  const ch = (t: string, k: number) => (t.endsWith("%") ? parseFloat(t) / 100 : parseFloat(t) / k);
  const out: RGBA = [ch(nums[0], scale), ch(nums[1], scale), ch(nums[2], scale), nums[3] === undefined ? 1 : ch(nums[3], 1)];
  return out.every(Number.isFinite) ? out : null;
}

const FALLBACK: Theme = { accent: [0.05, 0.48, 0.37, 1], copper: [0.55, 0.38, 0.22, 1], label: [0.11, 0.11, 0.12, 1], label2: [0.37, 0.37, 0.39, 1], label3: [0.6, 0.6, 0.62, 1], sep: [0.24, 0.24, 0.26, 0.18], bg: [1, 1, 1, 1], deep: [0.05, 0.48, 0.37, 1], rem: [0.05, 0.48, 0.37, 0.58], light: [0.05, 0.48, 0.37, 0.26], dark: false };

const VARS: Array<[keyof Omit<Theme, "dark">, string]> = [["accent", "--accent"], ["copper", "--copper"], ["label", "--label"], ["label2", "--label-secondary"], ["label3", "--label-tertiary"], ["sep", "--separator"], ["bg", "--bg-content"], ["deep", "--data-sleep-deep"], ["rem", "--data-sleep-rem"], ["light", "--data-sleep-light"]];

/** Resolves each token through a probe element, so var() chains and color-mix() come back as plain colours. */
export function readTheme(host: Element): Theme {
  const out: Theme = { ...FALLBACK };
  try {
    const probe = document.createElement("span");
    probe.style.display = "none";
    host.appendChild(probe);
    for (const [key, name] of VARS) {
      probe.style.color = "";
      probe.style.color = `var(${name})`;
      const parsed = parseCssColor(getComputedStyle(probe).color);
      if (parsed) (out[key] as RGBA) = parsed;
    }
    probe.remove();
  } catch { /* keep the fallbacks */ }
  out.dark = out.bg[0] * 0.3 + out.bg[1] * 0.59 + out.bg[2] * 0.11 < 0.5;
  return out;
}

export const themeSignature = (t: Theme) => [t.accent, t.copper, t.label, t.bg, t.sep].map((c) => c.map((n) => n.toFixed(3)).join(",")).join("|");
export const css = (c: RGBA) => `rgba(${Math.round(c[0] * 255)},${Math.round(c[1] * 255)},${Math.round(c[2] * 255)},${c[3]})`;
