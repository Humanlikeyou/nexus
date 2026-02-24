import { useState, useEffect, useCallback } from "react";
import { readGoals, saveGoals, deleteGoal as deleteGoalApi, insertEvents } from "@/lib/api";
import { goalToEvents } from "@/lib/events";
import type { Goal } from "@/lib/types";

export function useGoals() {
  const [goals, setGoals] = useState<Goal[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    try {
      const data = await readGoals();
      setGoals(data);
    } catch {
      // If backend not available, keep empty
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

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
      await saveGoals(updated).catch(() => {});
      insertEvents(goalToEvents(newGoal)).catch(() => {});
      return newGoal;
    },
    [goals],
  );

  const updateGoal = useCallback(
    async (id: string, changes: Partial<Goal>) => {
      const updated = goals.map((g) =>
        g.id === id ? { ...g, ...changes, updated_at: new Date().toISOString() } : g,
      );
      setGoals(updated);
      await saveGoals(updated).catch(() => {});
      const changedGoal = updated.find((g) => g.id === id);
      if (changedGoal) {
        insertEvents(goalToEvents(changedGoal)).catch(() => {});
      }
    },
    [goals],
  );

  const removeGoal = useCallback(
    async (id: string) => {
      try {
        const remaining = await deleteGoalApi(id);
        setGoals(remaining);
      } catch {
        setGoals((prev) => prev.filter((g) => g.id !== id));
      }
    },
    [],
  );

  return { goals, loading, addGoal, updateGoal, removeGoal, refresh: load };
}
