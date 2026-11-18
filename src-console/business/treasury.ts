import { commands } from "../bindings.gen";
import type {
  FxRate, TreasuryAttentionRow, TreasuryPerHour_Serialize, TreasuryStreamMonth_Serialize,
  TreasuryStream_Serialize, TreasuryTrendRow_Serialize,
} from "../bindings.gen";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";

/**
 * Treasury live adapter (phase 2): backend and typed bridge only, no markup. Amounts are integer minor
 * units (JPY yen, USD cents) in the row's own currency; a missing figure is `null`, never 0. The only
 * exchange rate is the ledger's dated one (`fx`); no other rate is read. Values are `live` only when
 * real rows exist, otherwise `unwired`. There is no sample data here.
 */
export type TreasuryStream = TreasuryStream_Serialize;
export type TreasuryStreamMonth = TreasuryStreamMonth_Serialize;
export type TreasuryTrendRow = TreasuryTrendRow_Serialize;
export type TreasuryPerHour = TreasuryPerHour_Serialize;
export type TreasuryAttention = TreasuryAttentionRow;
export interface TreasuryFx { rate: FxRate | null; stale: boolean }

export interface TreasurySnapshot {
  streams: TreasuryStream[];
  months: TreasuryStreamMonth[];
  trend: TreasuryTrendRow[];
  perHour: TreasuryPerHour[];
  fx: TreasuryFx | null;
}

export interface TreasuryRange { fromMonth: string; toMonth: string; from: string; to: string }

/** The slice of the generated bridge this loader calls, so tests can inject it. */
export type TreasuryCalls = Pick<
  typeof commands,
  "treasuryStreamsList" | "treasuryIncomeMonth" | "treasuryIncomeTrend" | "treasuryPerHour" | "treasuryFxStatus"
>;

/** One failed read leaves that part empty; it never hides the others (same rule as the system snapshot). */
async function part<T>(read: () => Promise<T>, fallback: T): Promise<T> {
  try { return await read(); } catch { return fallback; }
}

export async function loadTreasury(range: TreasuryRange, calls: TreasuryCalls = commands): Promise<TreasurySnapshot> {
  const [streams, months, trend, perHour, fx] = await Promise.all([
    part(() => calls.treasuryStreamsList() as Promise<TreasuryStream[]>, []),
    part(() => calls.treasuryIncomeMonth(range.fromMonth, range.toMonth) as Promise<TreasuryStreamMonth[]>, []),
    part(() => calls.treasuryIncomeTrend(null) as Promise<TreasuryTrendRow[]>, []),
    part(() => calls.treasuryPerHour(range.from, range.to) as Promise<TreasuryPerHour[]>, []),
    part(() => calls.treasuryFxStatus() as Promise<TreasuryFx | null>, null),
  ]);
  return { streams, months, trend, perHour, fx };
}

export interface TreasuryAdapter {
  streams(): Wired<TreasuryStream[]>;
  /** Per stream, currency and month. `totalMinor` is null for a month with no events. */
  incomeMonths(): Wired<TreasuryStreamMonth[]>;
  trend(): Wired<TreasuryTrendRow[]>;
  perHour(): Wired<TreasuryPerHour[]>;
  fx(): Wired<TreasuryFx>;
}

export const unwiredTreasury: TreasuryAdapter = {
  streams: () => UNWIRED, incomeMonths: () => UNWIRED, trend: () => UNWIRED, perHour: () => UNWIRED, fx: () => UNWIRED,
};

const live = <T,>(value: T): Wired<T> => ({ state: "live", value });
const isArr = (x: unknown): x is unknown[] => Array.isArray(x);
const num = (x: unknown) => typeof x === "number" && Number.isFinite(x);

/** True only for a snapshot object whose parts are the right shape (the IPC reply is untrusted). */
export function isTreasurySnapshot(x: unknown): x is TreasurySnapshot {
  if (!x || typeof x !== "object" || Array.isArray(x)) return false;
  const s = x as Record<string, unknown>;
  return ["streams", "months", "trend", "perHour"].every((k) => isArr(s[k]));
}

/** Live adapter over a loaded snapshot. No streams (or a malformed snapshot): the unwired adapter, unchanged. */
export function liveTreasury(snapshot: unknown): TreasuryAdapter {
  if (!isTreasurySnapshot(snapshot) || snapshot.streams.length === 0) return unwiredTreasury;
  const { streams, months, trend, perHour, fx } = snapshot;
  const hasFigures = (rows: { totalMinor: number | null }[]) => rows.some((r) => num(r.totalMinor));
  return {
    streams: () => live(streams),
    incomeMonths: () => (hasFigures(months) ? live(months) : UNWIRED),
    trend: () => (trend.some((t) => t.avgMinor !== null && num(t.avgMinor)) ? live(trend) : UNWIRED),
    perHour: () => (perHour.some((p) => num(p.incomeMinor) || num(p.minutes)) ? live(perHour) : UNWIRED),
    fx: () => (fx && typeof fx === "object" && typeof fx.stale === "boolean" ? live(fx) : UNWIRED),
  };
}
