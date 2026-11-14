import { invokeBound } from "../bridge";
import { buildRouterConfig } from "@/lib/llm-router";
import { getConfig } from "@/lib/api";
import type { RouterResponse } from "@/lib/types";
import { getHomeBrief, loadKv, saveKv } from "../bridge";
import { isSnapshot, noopQuery } from "../lib/noop";
import type { NoopSnapshotPayload, RecordedSession, RevenueSummary } from "../types";
import { personalAdapter } from "../personal/adapters";
import { roadmapAdapter } from "../goals/roadmap";
import { businessFacts, lifeGoalFacts, fitnessFacts, homeFacts, mindBodyFacts, neuroFacts } from "./facts";
import type { TakeDeps } from "./generate";
import { appendTake, entryFromTake, takeLogEntries } from "../takes/takeLog";

const snapshot = async (): Promise<NoopSnapshotPayload | null> => {
  const read = await noopQuery("health_snapshot", { days: 29 });
  return read.ok && isSnapshot(read.value) ? read.value : null;
};

/** Real sources, read only when the page's take is asked for. A failed read is "no data", never a guess. */
export const liveDeps: TakeDeps = {
  now: () => Date.now(),
  kv: { load: loadKv, save: saveKv },
  recent: () => takeLogEntries() ?? [],
  log: (take, tier) => appendTake(entryFromTake(take, tier)),
  // Light tier through the existing chat_with_router client: forceProvider "light" maps to the router's fast
  // route, and SOUL.md + the identity guard are applied in llm_router.rs. No other provider path is added.
  ask: async (prompt) => {
    const routerConfig = buildRouterConfig(await getConfig());
    const resp = await invokeBound<RouterResponse>("chat_with_router", { text: prompt, config: routerConfig, tags: [], forceProvider: "light", anthropicApiKey: undefined, localOnly: true });
    return { content: resp.content, tier: resp.tier };
  },
  facts: async (page, today) => {
    switch (page) {
      case "mindBody": return mindBodyFacts(await snapshot(), today);
      case "fitness": return fitnessFacts(await snapshot(), today);
      case "home": return homeFacts(await snapshot(), await getHomeBrief().catch(() => null), today, Date.now());
      case "neuro": return neuroFacts(await invokeBound<RecordedSession[]>("list_sessions").catch(() => []), today);
      case "business": return businessFacts(await invokeBound<RevenueSummary>("arm_get_revenue_summary", { from: null, to: null }).catch(() => null), today);
      case "life": {
        const g = personalAdapter().goals();
        if (g.state !== "live") return null; // sample or unwired goals are never facts
        const rm = roadmapAdapter();
        return lifeGoalFacts(g.value.milestones.map((m) => {
          const r = rm.roadmap(m.id);
          return { id: m.id, title: m.title, right: m.right, pct: m.pct, roadmapPct: r.state === "live" && r.value && r.value.steps.length ? r.value.pct : null };
        }), today);
      }
    }
  },
};
