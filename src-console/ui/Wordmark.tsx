import { cx } from "./shared";

/**
 * Amara wordmark (screen-specs › Shell › Wordmark, Decision 17): "AMARA" drawn as monoline stroked
 * caps after the Taiyo Skin logo, not set in a font. Source: nexus-audit/wm/amara-wordmark.svg.
 * Paths sit on a 100-unit cap grid: crossbarless A, round caps/joins, stroke 0.11 × cap, gap 0.85 × cap.
 * Colour comes from --wordmark (currentColor), so it follows light/dark automatically.
 */
export const WORDMARK_PATH =
  "M0 100L45 0L90 100 M175 100V0L221 62L267 0V100 M352 100L397 0L442 100 M527 100V0H567A26 26 0 0 1 567 52H527 M561 52L605 100 M690 100L735 0L780 100";

export type WordmarkSize = "sidebar" | "about" | "splash";

/** Cap height in px per placement; the SVG is 7.94 × cap wide and 1.14 × cap tall (stroke overhang included). */
export const WORDMARK_CAP: Record<WordmarkSize, number> = { sidebar: 12, about: 32, splash: 48 };
/** Stroke in grid units: 11 (0.11 × cap) everywhere except the sidebar, which holds the 1.5px floor (12.5 × 0.12). */
const STROKE: Record<WordmarkSize, number> = { sidebar: 12.5, about: 11, splash: 11 };

export type WordmarkProps = {
  size: WordmarkSize;
  /** Accessible name; ignored when decorative. */
  label?: string;
  /** Hide from assistive tech when a parent already carries the name (sidebar lockup, splash status). */
  decorative?: boolean;
  className?: string;
};

export function Wordmark({ size, label = "Amara OS", decorative = false, className }: WordmarkProps) {
  const cap = WORDMARK_CAP[size];
  const width = Math.round(7.94 * cap * 100) / 100;
  const height = Math.round(1.14 * cap * 100) / 100;
  return (
    <svg
      className={cx("wordmark", `wordmark--${size}`, className)}
      viewBox="-7 -7 794 114"
      width={width}
      height={height}
      fill="none"
      stroke="currentColor"
      strokeWidth={STROKE[size]}
      strokeLinecap="round"
      strokeLinejoin="round"
      focusable="false"
      {...(decorative ? { "aria-hidden": true } : { role: "img", "aria-label": label })}
    >
      {!decorative && <title>{label}</title>}
      <path d={WORDMARK_PATH} />
    </svg>
  );
}
