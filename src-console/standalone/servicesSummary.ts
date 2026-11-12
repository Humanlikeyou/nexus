import { valueOf } from "../lib/sampleAdapters";
import type { Service, ServicesSummary, StandaloneAdapter } from "./adapters";

/**
 * Live sidebar summary. `attention` counts services that are failed, degraded or restarting.
 * `waiting` (not checked yet) and `stopped` (all off) are not alarms. `updateReady` has no source
 * (the updater is not built), so it is always false rather than guessed.
 */
export function summaryFromServices(services: Service[]): ServicesSummary {
  return { attention: services.filter((s) => s.state === "failed" || s.state === "degraded" || s.state === "restarting").length, updateReady: false };
}

/** Sidebar footer line (design-1111s): "1 service needs attention ›" in preview, otherwise null (keep "Amara ready"). */
export function servicesFooter(adapter: StandaloneAdapter): { text: string; attention: number } | null {
  const s = valueOf(adapter.summary());
  if (!s) return null;
  if (s.attention > 0) return { text: `${s.attention} service${s.attention === 1 ? "" : "s"} need${s.attention === 1 ? "s" : ""} attention`, attention: s.attention };
  if (s.updateReady) return { text: "Update ready", attention: 0 };
  return null;
}
