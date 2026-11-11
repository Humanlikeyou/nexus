import { invoke } from "@tauri-apps/api/core";
import { buildRouterConfig } from "@/lib/llm-router";
import { getConfig } from "@/lib/api";
import type { RouterResponse } from "@/lib/types";
import { getHomeBrief, loadKv, saveKv } from "../bridge";
import { isSnapshot, noopQuery } from "../lib/noop";
import type { NoopSnapshotPayload, RecordedSession, RevenueSummary } from "../types";
import { businessFacts, fitnessFacts, homeFacts, mindBodyFacts, neuroFacts } from "./facts";
import type { TakeDeps } from "./generate";

const snapshot = async (): Promise<NoopSnapshotPayload | null> => {
  const read = await noopQuery("health_snapshot", { days: 29 });
  return read.ok && isSnapshot(read.value) ? read.value : null;
};

/** Real sources, read only when the page's take is asked for. A failed read is "no data", never a guess. */
export const liveDeps: TakeDeps = {
  now: () => Date.now(),
  kv: { load: loadKv, save: saveKv },
  // Light tier through the existing chat_with_router client: forceProvider "light" maps to the router's fast
  // route, and SOUL.md + the identity guard are applied in llm_router.rs. No other provider path is added.
  ask: async (prompt) => {
    const routerConfig = buildRouterConfig(await getConfig());
    const resp = await invoke<RouterResponse>("chat_with_router", { text: prompt, config: routerConfig, tags: [], forceProvider: "light", anthropicApiKey: undefined, localOnly: true });
    return { content: resp.content, tier: resp.tier };
  },
  facts: async (page, today) => {
    switch (page) {
      case "mindBody": return mindBodyFacts(await snapshot(), today);
      case "fitness": return fitnessFacts(await snapshot(), today);
      case "home": return homeFacts(await snapshot(), await getHomeBrief().catch(() => null), today, Date.now());
      case "neuro": return neuroFacts(await invoke<RecordedSession[]>("list_sessions").catch(() => []), today);
      case "business": return businessFacts(await invoke<RevenueSummary>("arm_get_revenue_summary", { from: null, to: null }).catch(() => null), today);
      case "life": return null;
    }
  },
};
