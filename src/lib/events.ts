import type {
  CanonicalEvent,
  BrainSnapshot,
  RecordedSession,
  NfSession,
  Goal,
} from "./types";

export function snapshotToEvents(
  snapshot: BrainSnapshot,
  rawRef?: string,
): CanonicalEvent[] {
  const base = {
    session_id: snapshot.id,
    timestamp: snapshot.timestamp,
    source: "snapshot" as const,
    raw_ref: rawRef ?? "",
  };

  return [
    { ...base, metric: "focus_score", value: snapshot.focusScore, unit: "score" },
    { ...base, metric: "relaxation_score", value: snapshot.relaxationScore, unit: "score" },
    { ...base, metric: "alpha_peak", value: snapshot.alphaPeak, unit: "hz" },
    { ...base, metric: "beta_balance", value: snapshot.betaBalance, unit: "ratio" },
    { ...base, metric: "fatigue", value: snapshot.fatigue, unit: "score" },
    { ...base, metric: "engagement", value: snapshot.engagement, unit: "score" },
  ];
}

export function sessionToEvents(
  session: RecordedSession,
  rawRef?: string,
): CanonicalEvent[] {
  const base = {
    session_id: session.id,
    timestamp: session.start,
    source: session.device_type,
    raw_ref: rawRef ?? session.file_path ?? "",
  };

  const events: CanonicalEvent[] = [
    { ...base, metric: "duration_secs", value: session.duration_secs, unit: "seconds" },
  ];

  if (session.summary.avg_focus != null) {
    events.push({ ...base, metric: "avg_focus", value: session.summary.avg_focus, unit: "score" });
  }
  if (session.summary.avg_relaxation != null) {
    events.push({ ...base, metric: "avg_relaxation", value: session.summary.avg_relaxation, unit: "score" });
  }
  if (session.summary.avg_hr != null) {
    events.push({ ...base, metric: "avg_hr", value: session.summary.avg_hr, unit: "bpm" });
  }
  if (session.summary.max_hr != null) {
    events.push({ ...base, metric: "max_hr", value: session.summary.max_hr, unit: "bpm" });
  }
  if (session.summary.avg_hrv != null) {
    events.push({ ...base, metric: "avg_hrv", value: session.summary.avg_hrv, unit: "ms" });
  }

  return events;
}

export function nfSessionToEvents(
  session: NfSession,
  rawRef?: string,
): CanonicalEvent[] {
  const base = sessionToEvents(session, rawRef);

  const extras: CanonicalEvent[] = [
    {
      session_id: session.id,
      timestamp: session.start,
      source: session.device_type,
      metric: "total_score",
      value: session.total_score,
      unit: "score",
      raw_ref: rawRef ?? session.file_path ?? "",
      tags: [session.protocol_name],
    },
    {
      session_id: session.id,
      timestamp: session.start,
      source: session.device_type,
      metric: "time_in_zone_pct",
      value: session.time_in_zone_pct,
      unit: "percent",
      raw_ref: rawRef ?? session.file_path ?? "",
      tags: [session.protocol_name],
    },
  ];

  return [...base, ...extras];
}

export function armRevenueToEvents(
  strategyId: string,
  strategyName: string,
  strategyType: string,
  revenue: number,
  expenses: number,
  workerUrl: string,
): CanonicalEvent[] {
  const now = new Date().toISOString();
  const base = {
    session_id: strategyId,
    timestamp: now,
    source: "arm" as const,
    tags: ["autonomous", "arm", strategyType, strategyName],
    raw_ref: workerUrl,
  };

  const events: CanonicalEvent[] = [];

  if (revenue > 0) {
    events.push({ ...base, metric: "revenue", value: revenue, unit: "usd" });
  }
  if (expenses > 0) {
    events.push({ ...base, metric: "expense", value: expenses, unit: "usd" });
  }
  if (revenue > 0 || expenses > 0) {
    events.push({ ...base, metric: "net_income", value: revenue - expenses, unit: "usd" });
  }

  return events;
}

export function armHealthToEvents(
  strategyId: string,
  strategyName: string,
  healthScore: number,
  workerUrl: string,
): CanonicalEvent[] {
  return [
    {
      session_id: strategyId,
      timestamp: new Date().toISOString(),
      source: "arm",
      metric: "strategy_health",
      value: healthScore,
      unit: "score",
      confidence: healthScore,
      tags: ["autonomous", "arm", strategyName],
      raw_ref: workerUrl,
    },
  ];
}

export function goalToEvents(goal: Goal): CanonicalEvent[] {
  const base = {
    session_id: goal.id,
    timestamp: goal.updated_at,
    source: "goal" as const,
    tags: [goal.category, goal.title],
  };

  return [
    { ...base, metric: "current", value: goal.current, unit: goal.unit },
    { ...base, metric: "target", value: goal.target, unit: goal.unit },
  ];
}
