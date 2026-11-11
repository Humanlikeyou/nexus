import { useEffect, useState } from "react";
import { UNWIRED } from "../lib/sampleAdapters";
import { liveDeps } from "./live";
import { runTake, type TakeDeps } from "./generate";
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

/** Amara's take for a page today: unwired until that day's data exists and a valid take is generated. Not wired into any screen yet. */
export function useAmaraTake(page: TakePage, deps?: TakeDeps): TakeResult {
  const [result, setResult] = useState<{ page: TakePage; value: TakeResult } | null>(null);
  useEffect(() => {
    let live = true;
    const check = () => { checkTake(page, deps).then((value) => { if (live) setResult({ page, value }); }); };
    check();
    const timer = window.setInterval(check, TAKE_POLL_MS);
    window.addEventListener("focus", check);
    return () => { live = false; window.clearInterval(timer); window.removeEventListener("focus", check); };
  }, [page, deps]);
  return result?.page === page ? result.value : UNWIRED;
}
