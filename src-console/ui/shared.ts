import "./shared.css";

/** Joins class names, skipping empty values. */
export function cx(...names: Array<string | false | null | undefined>): string {
  return names.filter(Boolean).join(" ");
}

/** Joins space-separated id lists (aria-describedby), skipping empty values. */
export function joinIds(...ids: Array<string | false | null | undefined>): string | undefined {
  const joined = ids.filter(Boolean).join(" ");
  return joined || undefined;
}

export type Tone = "neutral" | "accent" | "info" | "success" | "warning" | "danger";
