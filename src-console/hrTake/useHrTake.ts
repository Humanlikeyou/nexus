import { useEffect, useState } from "react";
import type { Wired } from "../lib/sampleAdapters";
import { hrTakeAdapter, type HrTakeAdapter, type HrTakeStatus, type HrTrigger } from "./adapters";

export type HrTakeState = { w: Wired<HrTakeStatus> | null; reading: boolean };

/**
 * design-1111u: no button, no timer. Home asks when it is opened, when the window gets focus back and when
 * he navigates to it. The backend says whether a take is due (30-minute rule) and writes at most one.
 */
export function useHrTake(trigger: HrTrigger = "open", adapter: HrTakeAdapter = hrTakeAdapter()): HrTakeState {
  const [w, setW] = useState<Wired<HrTakeStatus> | null>(null);
  const [reading, setReading] = useState(false);
  useEffect(() => {
    let live = true;
    let busy = false;
    const run = async (why: HrTrigger) => {
      if (busy) return;
      busy = true;
      try {
        const s = await adapter.status();
        if (!live) return;
        setW(s);
        if (s.state !== "unwired" && s.value.due) {
          setReading(true);
          const r = await adapter.refresh(why);
          if (live) setW(r);
        }
      } catch {
        if (live) setW({ state: "unwired" });
      } finally {
        busy = false;
        if (live) setReading(false);
      }
    };
    void run(trigger);
    const onFocus = () => { void run("focus"); };
    const onVisible = () => { if (!document.hidden) void run("focus"); };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisible);
    return () => { live = false; window.removeEventListener("focus", onFocus); document.removeEventListener("visibilitychange", onVisible); };
  }, [adapter, trigger]);
  return { w, reading };
}
