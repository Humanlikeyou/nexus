import { invokeBound } from "../bridge";
import { buildRouterConfig } from "../legacy-core/lib/llm-router";
import { getConfig } from "../legacy-core/lib/api";
import type { RouterResponse } from "../legacy-core/lib/types";
import { getHomeBrief, loadKv, saveKv } from "../bridge";
import { readSnapshot } from "../lib/noopSnapshot";
import type { NoopSnapshotPayload, RecordedSession, RevenueSummary } from "../types";
import { personalAdapter } from "../personal/adapters";
import { roadmapAdapter } from "../goals/roadmap";
import { agentsFacts, smartHomeFacts } from "./factsSurfaces";
import type { ActivityEntry, HermesRuntimeHealth, OutboxStatus } from "../types";
import type { HaDevicesResult } from "../bindings.gen";
import { businessFacts, lifeGoalFacts, fitnessFacts, homeFacts, mindBodyFacts, neuroFacts } from "./facts";
import type { TakeDeps } from "./generate";
import { appendTake, entryFromTake, takeLogEntries } from "../takes/takeLog";

const snapshot = (): Promise<NoopSnapshotPayload | null> => readSnapshot();

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
      case "agents": {
        // Each read is independent: one that fails is simply missing from the facts, never guessed.
        const [amara, crew, outbox, activity] = await Promise.all([
          invokeBound<HermesRuntimeHealth>("get_terrence_runtime_health").then((h) => h.status).catch(() => null),
          invokeBound<string>("crew_health").catch(() => null),
          invokeBound<OutboxStatus>("get_outbox_status").catch(() => null),
          invokeBound<ActivityEntry[]>("list_activity_entries", { limit: 30 }).catch(() => null),
        ]);
        return agentsFacts({ amara, crew, outbox, activity }, today);
      }
      case "smarthome": return smartHomeFacts(await invokeBound<HaDevicesResult>("ha_devices_list").catch(() => null), today);
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
