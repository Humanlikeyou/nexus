// Decision 0066: the cross-domain briefings. Thin wrappers over the Rust commands; nothing here
// computes a figure. Every number on screen comes from the backend's rules and arithmetic.
import type {
  BriefingLine, BriefingRuntime_Serialize as BriefingRuntime, BriefingSection_Serialize as BriefingSection,
  DailyBriefing_Serialize as DailyBriefing, EnterpriseBriefing_Serialize as EnterpriseBriefing, GoalTrajectory_Serialize as GoalTrajectory,
  NeuroSummary_Serialize as NeuroSummary, RecommendationDetail_Serialize as RecommendationDetail, TrainingSummary_Serialize as TrainingSummary,
  WeekCrossDomain_Serialize as WeekCrossDomain,
} from "../bindings.gen";
import { realInvoke, type Invoke } from "../lib/localStores";

export type { BriefingLine, BriefingRuntime, BriefingSection, DailyBriefing, EnterpriseBriefing, GoalTrajectory, NeuroSummary, RecommendationDetail, TrainingSummary, WeekCrossDomain };

export const briefingCalls = (call: Invoke = realInvoke) => ({
  daily: () => call<DailyBriefing>("briefing_daily_read"),
  dailyRefresh: () => call<DailyBriefing>("briefing_daily_refresh"),
  enterprise: (businessId: string) => call<EnterpriseBriefing>("briefing_enterprise_read", { businessId }),
  enterpriseRefresh: (businessId: string) => call<EnterpriseBriefing>("briefing_enterprise_refresh", { businessId }),
  week: (weekStart: string) => call<WeekCrossDomain>("briefing_week", { weekStart }),
  runtime: () => call<BriefingRuntime>("briefing_runtime"),
  recommendations: (scope: "personal" | "business", businessId: string | null = null) =>
    call<RecommendationDetail[]>("briefing_recommendations", { scope, businessId }),
});
export type BriefingCalls = ReturnType<typeof briefingCalls>;

/** The evaluation plan a cross-domain recommendation stores (`evaluationJson`). */
export type EvaluationPlan = {
  metric: string; target: number; baseline: number | null; direction: "at_least" | "at_most";
  unit: string; measure_after: string; window_days?: number; how?: string;
};

export function parsePlan(json: string | null | undefined): EvaluationPlan | null {
  if (!json) return null;
  try {
    const v = JSON.parse(json) as Partial<EvaluationPlan>;
    return typeof v.metric === "string" && typeof v.target === "number" && typeof v.unit === "string" && typeof v.measure_after === "string"
      && (v.direction === "at_least" || v.direction === "at_most")
      ? (v as EvaluationPlan) : null;
  } catch { return null; }
}

export const planText = (p: EvaluationPlan) =>
  `${p.direction === "at_most" ? "At most" : "At least"} ${p.target} ${p.unit}${p.baseline === null ? "" : ` (was ${p.baseline})`}, checked after ${p.measure_after}.`;

export const KIND_LABEL: Record<string, string> = {
  measured: "Measured", calculated: "Calculated", self_report: "You recorded it", unavailable: "Unavailable",
  interpretation: "Interpretation", hypothesis: "Hypothesis",
};

export const TRIGGER_LABEL: Record<string, string> = {
  timer: "on schedule", catch_up: "catching up after launch", opened: "when you opened it", manual: "when you asked",
};

export const MEASURED_LABEL: Record<string, string> = { met: "Target met", not_met: "Target not met", unknown: "Couldn't be measured" };
