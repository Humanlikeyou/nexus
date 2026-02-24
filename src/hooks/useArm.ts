import { useState, useEffect, useCallback } from "react";
import {
  armListStrategies,
  armGetRevenueSummary,
  armGetRiskStatus,
  armListDecisions,
  armPollAll,
  armGetConfig,
  armCreateStrategy,
  armUpdateStrategy,
  armDeleteStrategy,
  armActivateStrategy,
  armPauseStrategy,
  armKillStrategy,
  armKillSwitch,
  armLogDecision,
  armUpdateDecisionOutcome,
  armSetGlobalLimits,
  armSaveConfig,
} from "@/lib/api";
import type {
  ArmStrategy,
  ArmRevenueSummary,
  ArmRiskStatus,
  ArmDecision,
  ArmConfig,
  ArmStrategyConfig,
} from "@/lib/types";

export function useArm(pollIntervalMs = 300_000) {
  const [strategies, setStrategies] = useState<ArmStrategy[]>([]);
  const [revenueSummary, setRevenueSummary] = useState<ArmRevenueSummary | null>(null);
  const [riskStatus, setRiskStatus] = useState<ArmRiskStatus | null>(null);
  const [decisions, setDecisions] = useState<ArmDecision[]>([]);
  const [config, setConfig] = useState<ArmConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [polling, setPolling] = useState(false);
  const [fetchedAt, setFetchedAt] = useState<Date | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [strats, rev, risk, decs, cfg] = await Promise.all([
        armListStrategies(),
        armGetRevenueSummary(),
        armGetRiskStatus(),
        armListDecisions(undefined, 50),
        armGetConfig(),
      ]);
      setStrategies(strats);
      setRevenueSummary(rev);
      setRiskStatus(risk);
      setDecisions(decs);
      setConfig(cfg);
      setFetchedAt(new Date());
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, pollIntervalMs);
    return () => clearInterval(interval);
  }, [refresh, pollIntervalMs]);

  const pollWorkers = useCallback(async () => {
    setPolling(true);
    try {
      const updated = await armPollAll();
      setStrategies(updated);
      const [rev, risk] = await Promise.all([
        armGetRevenueSummary(),
        armGetRiskStatus(),
      ]);
      setRevenueSummary(rev);
      setRiskStatus(risk);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setPolling(false);
    }
  }, []);

  const createStrategy = useCallback(
    async (name: string, type: string, cfg: ArmStrategyConfig, capitalLimit: number, dailyLossCap: number) => {
      const strat = await armCreateStrategy(name, type, cfg, capitalLimit, dailyLossCap);
      setStrategies((prev) => [...prev, strat]);
      return strat;
    },
    [],
  );

  const updateStrategy = useCallback(
    async (id: string, name?: string, cfg?: ArmStrategyConfig, capitalLimit?: number, dailyLossCap?: number) => {
      const strat = await armUpdateStrategy(id, name, cfg, capitalLimit, dailyLossCap);
      setStrategies((prev) => prev.map((s) => (s.id === id ? strat : s)));
      return strat;
    },
    [],
  );

  const deleteStrategy = useCallback(async (id: string) => {
    await armDeleteStrategy(id);
    setStrategies((prev) => prev.filter((s) => s.id !== id));
  }, []);

  const activate = useCallback(async (id: string) => {
    const strat = await armActivateStrategy(id);
    setStrategies((prev) => prev.map((s) => (s.id === id ? strat : s)));
  }, []);

  const pause = useCallback(async (id: string, reasoning: string) => {
    const strat = await armPauseStrategy(id, reasoning);
    setStrategies((prev) => prev.map((s) => (s.id === id ? strat : s)));
  }, []);

  const kill = useCallback(async (id: string, reasoning: string) => {
    const strat = await armKillStrategy(id, reasoning);
    setStrategies((prev) => prev.map((s) => (s.id === id ? strat : s)));
  }, []);

  const toggleKillSwitch = useCallback(async (active: boolean, reasoning: string) => {
    await armKillSwitch(active, reasoning);
    await refresh();
  }, [refresh]);

  const logDecision = useCallback(
    async (strategyId: string, action: string, reasoning: string, context?: Record<string, unknown>) => {
      const dec = await armLogDecision(strategyId, action, reasoning, context);
      setDecisions((prev) => [dec, ...prev]);
      return dec;
    },
    [],
  );

  const updateOutcome = useCallback(
    async (id: string, outcome: string, notes?: string) => {
      const dec = await armUpdateDecisionOutcome(id, outcome, notes);
      setDecisions((prev) => prev.map((d) => (d.id === id ? dec : d)));
    },
    [],
  );

  const saveArmConfig = useCallback(async (cfg: ArmConfig) => {
    await armSaveConfig(cfg);
    setConfig(cfg);
  }, []);

  const setGlobalLimits = useCallback(async (capitalLimit: number, dailyLossCap: number) => {
    await armSetGlobalLimits(capitalLimit, dailyLossCap);
    await refresh();
  }, [refresh]);

  return {
    strategies,
    revenueSummary,
    riskStatus,
    decisions,
    config,
    loading,
    error,
    polling,
    fetchedAt,
    refresh,
    pollWorkers,
    createStrategy,
    updateStrategy,
    deleteStrategy,
    activate,
    pause,
    kill,
    toggleKillSwitch,
    logDecision,
    updateOutcome,
    saveArmConfig,
    setGlobalLimits,
  };
}
