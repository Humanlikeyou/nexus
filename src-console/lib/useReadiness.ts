import { useEffect, useState } from "react";
import { READINESS_COPY, readinessReading, shortDay } from "./mindBody";
import { isSnapshot, noopQuery, snapshotDays } from "./noop";

export type ReadinessLine = { score: number; label: string; tone: "success" | "warning" | "danger" | "neutral"; reason: string; earlierDay: string | null };

/** One read of NOOP's snapshot for Today's readiness line. Nothing readable means no line, never an error. */
export async function loadReadinessLine(): Promise<ReadinessLine | null> {
  const read = await noopQuery("health_snapshot", { days: 29 });
  if (!read.ok || !isSnapshot(read.value)) return null;
  const days = snapshotDays(read.value);
  const points = (pick: (row: (typeof days)[number]) => number | null) => days.map((row) => ({ day: row.day, value: pick(row) }));
  const { asOf, ready, recovery, earlier } = readinessReading({ hrv: points((d) => d.avgHrv), recovery: points((d) => d.recovery), sleep: points((d) => d.totalSleepMin) });
  if (ready.level === "none" || recovery === null) return null;
  const copy = READINESS_COPY[ready.level];
  return { score: Math.round(recovery), label: copy.label, tone: copy.tone, reason: copy.help, earlierDay: earlier && asOf ? shortDay(asOf) : null };
}

export function useReadinessLine(): ReadinessLine | null {
  const [line, setLine] = useState<ReadinessLine | null>(null);
  useEffect(() => {
    let live = true;
    loadReadinessLine().then((next) => { if (live) setLine(next); }, () => undefined);
    return () => { live = false; };
  }, []);
  return line;
}
