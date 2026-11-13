import { useEffect } from "react";
import { recordScreenOpen } from "../bridge";
import type { ModuleId } from "../types";
import { SYSTEMS } from "./routes";

const ALLOWED: ReadonlySet<string> = new Set(SYSTEMS.map((s) => s.id));

/** Records one local screen-open (id + time only) per change of top-level screen, and once on mount.
 *  Keyed on the top-level id, so sub-view changes and re-renders do not count. Fire-and-forget. */
export function useScreenOpens(active: ModuleId): void {
  useEffect(() => {
    if (!ALLOWED.has(active)) return;
    try {
      void Promise.resolve(recordScreenOpen(active)).catch(() => undefined);
    } catch {
      /* never surface to React */
    }
  }, [active]);
}
