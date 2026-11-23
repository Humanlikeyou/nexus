import { inTauri } from "../lib/localStores";
import { makeLiveCrewsSchedule } from "./liveSchedule";
import { NEEDS_CONVERSION, UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";

/** A request from a crew that waits for Taisun (separate from Terrance's approvals). */
export interface CrewApproval { id: string; runId?: string; title: string; crew: string; agents: string; impact: string; requested: string; risk: "Low" | "Medium" | "High"; reason: string; amount: Wired<number> }
export interface CrewDecision { id: string; runId?: string; title: string; crew: string; when: string; approved: boolean }
export interface CrewRunMetrics { durationS: number; tokens: number; requests: number; costUsd: number }
export interface CrewStep { id: string; label: string; agent: string; startPct: number; endPct: number; status: "ok" | "failed" | "skipped"; took: string }
export interface CrewHandoff { from: string; to: string }
export interface CrewRoster { name: string; ok: number; total: number; nextRun: string | null }
export interface N8nStatus { connected: boolean; workflows: number; failures7d: number }

/** Claude wires these (HANDOFF-1110d.md). Every method is synchronous over already-loaded data. */
export interface CrewsAdapter {
  approvals(): Wired<CrewApproval[]>;
  decided(): Wired<CrewDecision[]>;
  runMetrics(runId: string): Wired<CrewRunMetrics>;
  steps(runId: string): Wired<CrewStep[]>;
  handoffs(runId: string): Wired<CrewHandoff[]>;
  roster(): Wired<CrewRoster[]>;
  n8n(): Wired<N8nStatus>;
  nextSchedule(): Wired<string>;
}

export const unwiredCrews: CrewsAdapter = {
  approvals: () => UNWIRED, decided: () => UNWIRED, runMetrics: () => UNWIRED, steps: () => UNWIRED,
  handoffs: () => UNWIRED, roster: () => UNWIRED, n8n: () => UNWIRED, nextSchedule: () => UNWIRED,
};

/** Preview/test only: the design-1110d mockup's sample values. */
export const sampleCrews: CrewsAdapter = {
  ...unwiredCrews,
  approvals: () => sample([
    { id: "ca1", title: "Send weekly supplier email", crew: "research crew", agents: "writer → mailer", impact: "Outbound email · 1 recipient", requested: "10 min ago", risk: "Low", reason: "the action leaves the Mac", amount: UNWIRED },
    { id: "ca2", title: "Buy sample ingredients", crew: "Taiyo Skin crew", agents: "buyer", impact: "Spend", requested: "32 min ago", risk: "Medium", reason: "the action spends money", amount: NEEDS_CONVERSION },
    { id: "ca3", title: "Publish launch note draft", crew: "content crew", agents: "writer", impact: "Public post · blog", requested: "1 h ago", risk: "High", reason: "the action is public", amount: UNWIRED },
  ]),
  decided: () => sample([
    { id: "cd1", title: "Book sample courier pickup", crew: "Taiyo Skin crew", when: "yesterday 18:02", approved: true },
    { id: "cd2", title: "Post routine reel", crew: "content crew", when: "yesterday 11:40", approved: false },
  ]),
  runMetrics: () => sample({ durationS: 48, tokens: 3210, requests: 4, costUsd: 0 }),
  steps: () => sample([
    { id: "s1", label: "Plan task", agent: "planner", startPct: 0, endPct: 14, status: "ok", took: "6.2 s" },
    { id: "s2", label: "kb_query", agent: "tool", startPct: 14, endPct: 22, status: "ok", took: "3.1 s" },
    { id: "s3", label: "Draft facts", agent: "writer", startPct: 22, endPct: 96, status: "failed", took: "35 s" },
    { id: "s4", label: "Review", agent: "reviewer", startPct: 96, endPct: 100, status: "skipped", took: "skipped" },
  ]),
  handoffs: () => sample([{ from: "planner", to: "writer" }, { from: "writer", to: "reviewer" }, { from: "writer", to: "kb_query" }]),
};

export const crewsAdapter = (): CrewsAdapter => (sampleAdaptersEnabled() ? sampleCrews : inTauri() ? makeLiveCrewsSchedule(unwiredCrews) : unwiredCrews);
