import { useState, useEffect, useCallback } from "react";
import { readGoals, saveGoals, deleteGoal as deleteGoalApi, insertEvents } from "../../src-console/legacy-core/lib/api";
import { goalToEvents } from "@/lib/events";
import { loadContext, saveContext } from "../../src-console/legacy-core/terrance/system/continuity";
import type { Goal } from "../../src-console/legacy-core/lib/types";

/** Sync goal titles into TerranceContext so Amara is goal-aware in every prompt */
async function syncGoalsToTerrance(goals: Goal[]): Promise<void> {
  try {
    const ctx = await loadContext();
    ctx.activeGoals = goals.map((g) => g.title);
    await saveContext(ctx);
  } catch {
    // Non-fatal — context will catch up on next load
  }
}

/** User-safe banner text for a failed goals save (never the raw error). */
export const GOALS_SAVE_ERROR = "Failed to save goals. Please try again.";

export function useGoals() {
  const [goals, setGoals] = useState<Goal[]>([]);
  const [loading, setLoading] = useState(true);
  const [saveError, setSaveError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const data = await readGoals();
      setGoals(data);
    } catch {
      // Backend unavailable on first load — start empty
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const persist = useCallback(async (updated: Goal[]): Promise<boolean> => {
    try {
      await saveGoals(updated);
      await syncGoalsToTerrance(updated);
      setSaveError(null);
      return true;
    } catch (e) {
      // The raw error can carry file paths or provider text: log it locally, show fixed text.
      console.error("[goals] save failed:", e);
      setSaveError(GOALS_SAVE_ERROR);
      return false;
    }
  }, []);

  const addGoal = useCallback(
    async (goal: Omit<Goal, "id" | "created_at" | "updated_at">) => {
      const now = new Date().toISOString();
      const newGoal: Goal = {
        ...goal,
        id: crypto.randomUUID(),
        created_at: now,
        updated_at: now,
      };
      const updated = [...goals, newGoal];
      setGoals(updated);
      const ok = await persist(updated);
      if (ok) insertEvents(goalToEvents(newGoal)).catch(() => {});
      return newGoal;
    },
    [goals, persist],
  );

  const updateGoal = useCallback(
    async (id: string, changes: Partial<Goal>) => {
      const updated = goals.map((g) =>
        g.id === id ? { ...g, ...changes, updated_at: new Date().toISOString() } : g,
      );
      setGoals(updated);
      const ok = await persist(updated);
      if (ok) {
        const changedGoal = updated.find((g) => g.id === id);
        if (changedGoal) insertEvents(goalToEvents(changedGoal)).catch(() => {});
      }
    },
    [goals, persist],
  );

  const removeGoal = useCallback(
    async (id: string) => {
      try {
        const remaining = await deleteGoalApi(id);
        setGoals(remaining);
        setSaveError(null);
      } catch {
        // Optimistic remove if backend call fails
        setGoals((prev) => prev.filter((g) => g.id !== id));
      }
    },
    [],
  );

  return { goals, loading, saveError, addGoal, updateGoal, removeGoal, refresh: load };
}
