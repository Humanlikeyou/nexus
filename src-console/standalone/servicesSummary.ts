import { valueOf } from "../lib/sampleAdapters";
import type { StandaloneAdapter } from "./adapters";

/** Sidebar footer line (design-1111s): "1 service needs attention ›" in preview, otherwise null (keep "Amara ready"). */
export function servicesFooter(adapter: StandaloneAdapter): { text: string; attention: number } | null {
  const s = valueOf(adapter.summary());
  if (!s) return null;
  if (s.attention > 0) return { text: `${s.attention} service${s.attention === 1 ? "" : "s"} need${s.attention === 1 ? "s" : ""} attention`, attention: s.attention };
  if (s.updateReady) return { text: "Update ready", attention: 0 };
  return null;
}
