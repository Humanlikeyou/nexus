import { useEffect, useState } from "react";
import { UNWIRED } from "../lib/sampleAdapters";
import { liveDeps } from "./live";
import { runTake, type TakeDeps } from "./generate";
import { tokyoDay } from "../lib/mindBody";
import { recordTakeShown } from "../takes/events";
import { takeEntryId } from "../takes/takeLog";
import type { TakePage, TakeResult } from "./types";

/** Checked at launch, when the window regains focus, and every 30 minutes. A check is cheap: it only calls the router for a miss or a changed fingerprint. */
export const TAKE_POLL_MS = 30 * 60_000;

const inflight = new Map<TakePage, Promise<TakeResult>>();
/** One pass per page at a time, shared by every caller. */
export function checkTake(page: TakePage, deps: TakeDeps = liveDeps): Promise<TakeResult> {
  const running = inflight.get(page);
  if (running) return running;
  const p = runTake(page, deps).catch((): TakeResult => UNWIRED).finally(() => { inflight.delete(page); });
  inflight.set(page, p);
  return p;
}

const shownThisSession = new Set<string>();
export const resetShownForTest = () => shownThisSession.clear();
/** Decision 0020: a live take reaching a component counts as shown, once per take per day. Never throws into React. */
function noteShown(value: TakeResult): void {
  if (value.state !== "live") return;
  const t = value.value;
  const takeId = takeEntryId(t.page, t.generatedAt);
  const day = tokyoDay(new Date().toISOString()) ?? t.day; // the JST day it was shown, which can be later than the take's own day
  const key = `${takeId}|${day}`;
  if (shownThisSession.has(key)) return;
  shownThisSession.add(key);
  try { void recordTakeShown(t.page, takeId, day).catch(() => { shownThisSession.delete(key); }); } catch { shownThisSession.delete(key); }
}

/** Amara's take for a page today plus whether the first check is still running (design-1110f "Generating"). */
export function useAmaraTakeStatus(page: TakePage, deps?: TakeDeps): { value: TakeResult; checking: boolean } {
  const [result, setResult] = useState<{ page: TakePage; value: TakeResult } | null>(null);
  useEffect(() => {
    let live = true;
    const check = () => { checkTake(page, deps).then((value) => { if (live) setResult({ page, value }); }); };
    check();
    const timer = window.setInterval(check, TAKE_POLL_MS);
    window.addEventListener("focus", check);
    return () => { live = false; window.clearInterval(timer); window.removeEventListener("focus", check); };
  }, [page, deps]);
  useEffect(() => { if (result?.page === page) noteShown(result.value); }, [result, page]);
  return result?.page === page ? { value: result.value, checking: false } : { value: UNWIRED, checking: true };
}

/** Amara's take for a page today: unwired until that day's data exists and a valid take is generated. */
export function useAmaraTake(page: TakePage, deps?: TakeDeps): TakeResult {
  return useAmaraTakeStatus(page, deps).value;
}
