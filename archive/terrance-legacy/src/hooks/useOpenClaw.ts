import { useState, useEffect, useCallback } from "react";
import { readOpenClawStatus, readCronJobs, readSkills } from "@/lib/api";
import type { OpenClawStatus, CronJob, Skill } from "@/lib/types";

export function useOpenClaw(pollInterval = 30000) {
  const [status, setStatus] = useState<OpenClawStatus | null>(null);
  const [cronJobs, setCronJobs] = useState<CronJob[]>([]);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [s, c, sk] = await Promise.all([
        readOpenClawStatus(),
        readCronJobs(),
        readSkills(),
      ]);
      setStatus(s);
      setCronJobs(c);
      setSkills(sk);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, pollInterval);
    return () => clearInterval(interval);
  }, [refresh, pollInterval]);

  return { status, cronJobs, skills, loading, error, refresh };
}
