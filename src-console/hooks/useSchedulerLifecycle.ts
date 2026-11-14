/**
 * Starts the Amara scheduler once for the console shell and stops it on
 * unmount. A window-level guard keeps StrictMode double-mounts, duplicate
 * mounts and HMR re-evaluation from starting a second instance.
 */
import { useEffect } from "react";
import { startScheduler, stopScheduler } from "../legacy-core/terrance/system/scheduler";

declare global {
  interface Window {
    __nexusSchedulerStarted?: symbol;
  }
}

export function useSchedulerLifecycle(): void {
  useEffect(() => {
    if (window.__nexusSchedulerStarted) return; // another owner already runs it
    const owner = Symbol("nexus.scheduler.owner");
    window.__nexusSchedulerStarted = owner;
    startScheduler();
    return () => {
      if (window.__nexusSchedulerStarted !== owner) return;
      stopScheduler();
      delete window.__nexusSchedulerStarted;
    };
  }, []);
}
