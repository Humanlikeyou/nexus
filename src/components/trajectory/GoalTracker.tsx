import { useState } from "react";
import {
  Target,
  DollarSign,
  Heart,
  Code,
  User,
  Plus,
  Trash2,
  Edit3,
  X,
  Check,
} from "lucide-react";
import * as Dialog from "@radix-ui/react-dialog";
import { cn } from "@/lib/utils";
import { useGoals } from "@/hooks/useGoals";
import { useNexus } from "@/context/NexusContext";
import type { Goal } from "@/lib/types";

const categoryIcons: Record<string, typeof Target> = {
  revenue: DollarSign,
  health: Heart,
  project: Code,
  personal: User,
};

const categoryColors: Record<string, string> = {
  revenue: "text-success",
  health: "text-red-400",
  project: "text-accent",
  personal: "text-warning",
};

type GoalFormData = Omit<Goal, "id" | "created_at" | "updated_at">;

const emptyForm: GoalFormData = {
  title: "",
  category: "project",
  target: 100,
  current: 0,
  unit: "%",
  deadline: null,
};

export function GoalTracker() {
  const { goals, addGoal, updateGoal, removeGoal } = useGoals();
  const { toast } = useNexus();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [form, setForm] = useState<GoalFormData>(emptyForm);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingProgress, setEditingProgress] = useState<{ id: string; value: string } | null>(null);

  const handleOpenAdd = () => {
    setForm(emptyForm);
    setEditingId(null);
    setDialogOpen(true);
  };

  const handleOpenEdit = (goal: Goal) => {
    setForm({
      title: goal.title,
      category: goal.category,
      target: goal.target,
      current: goal.current,
      unit: goal.unit,
      deadline: goal.deadline,
    });
    setEditingId(goal.id);
    setDialogOpen(true);
  };

  const handleSave = async () => {
    if (!form.title.trim()) return;
    if (editingId) {
      await updateGoal(editingId, form);
      toast({ type: "success", title: "Goal Updated" });
    } else {
      await addGoal(form);
      toast({ type: "success", title: "Goal Added", description: form.title });
    }
    setDialogOpen(false);
  };

  const handleDelete = async (id: string) => {
    await removeGoal(id);
    toast({ type: "info", title: "Goal Removed" });
  };

  const handleProgressSubmit = async (id: string, value: string) => {
    const num = parseFloat(value);
    if (!isNaN(num)) {
      await updateGoal(id, { current: num });
    }
    setEditingProgress(null);
  };

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Target size={16} className="text-accent" />
          <h3 className="text-base font-medium text-muted-foreground">Goals & Milestones</h3>
        </div>
        <button
          onClick={handleOpenAdd}
          className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg bg-accent/15 text-accent text-xs hover:bg-accent/25 transition-colors"
        >
          <Plus size={12} /> Add Goal
        </button>
      </div>

      {goals.length === 0 ? (
        <div className="py-8 text-center">
          <Target size={24} className="text-muted-foreground/20 mx-auto mb-2" />
          <p className="text-xs text-muted-foreground/40">No goals yet. Add one to start tracking.</p>
        </div>
      ) : (
        <div className="space-y-3">
          {goals.map((goal) => {
            const Icon = categoryIcons[goal.category] || Target;
            const progress = goal.target > 0 ? (goal.current / goal.target) * 100 : 0;

            return (
              <div
                key={goal.id}
                className="group px-4 py-3 rounded-lg bg-muted/15 border border-border/15 space-y-2"
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <Icon size={14} className={cn(categoryColors[goal.category])} />
                    <span className="text-sm">{goal.title}</span>
                  </div>
                  <div className="flex items-center gap-2">
                    {editingProgress?.id === goal.id ? (
                      <div className="flex items-center gap-1">
                        <input
                          type="number"
                          value={editingProgress.value}
                          onChange={(e) => setEditingProgress({ id: goal.id, value: e.target.value })}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") handleProgressSubmit(goal.id, editingProgress.value);
                            if (e.key === "Escape") setEditingProgress(null);
                          }}
                          className="w-16 px-2 py-0.5 rounded bg-muted/30 border border-border/30 text-xs font-mono focus:outline-none focus:border-accent/40"
                          autoFocus
                        />
                        <button onClick={() => handleProgressSubmit(goal.id, editingProgress.value)}>
                          <Check size={12} className="text-success" />
                        </button>
                        <button onClick={() => setEditingProgress(null)}>
                          <X size={12} className="text-muted-foreground" />
                        </button>
                      </div>
                    ) : (
                      <button
                        onClick={() => setEditingProgress({ id: goal.id, value: String(goal.current) })}
                        className="text-xs text-muted-foreground hover:text-foreground transition-colors"
                      >
                        {goal.current}{goal.unit === "$" ? "" : ` ${goal.unit}`}
                        {" / "}
                        {goal.unit === "$" ? `$${goal.target.toLocaleString()}` : `${goal.target} ${goal.unit}`}
                      </button>
                    )}
                    <div className="opacity-0 group-hover:opacity-100 flex items-center gap-1 transition-opacity">
                      <button onClick={() => handleOpenEdit(goal)}>
                        <Edit3 size={12} className="text-muted-foreground/50 hover:text-accent" />
                      </button>
                      <button onClick={() => handleDelete(goal.id)}>
                        <Trash2 size={12} className="text-muted-foreground/50 hover:text-destructive" />
                      </button>
                    </div>
                  </div>
                </div>

                <div className="w-full h-2 rounded-full bg-muted/30 overflow-hidden">
                  <div
                    className={cn(
                      "h-full rounded-full transition-all duration-500",
                      progress >= 100
                        ? "bg-success"
                        : progress >= 50
                          ? "bg-accent"
                          : "bg-accent/60",
                    )}
                    style={{ width: `${Math.min(100, progress)}%` }}
                  />
                </div>

                <div className="flex items-center justify-between">
                  <span className="text-xs text-muted-foreground/50">
                    {Math.round(progress)}% complete
                  </span>
                  {goal.deadline && (
                    <span className="text-xs text-muted-foreground/50">
                      Due {new Date(goal.deadline).toLocaleDateString()}
                    </span>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* Add/Edit Dialog */}
      <Dialog.Root open={dialogOpen} onOpenChange={setDialogOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 z-[85] bg-black/50 backdrop-blur-sm" />
          <Dialog.Content className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-[86] w-[420px] glass rounded-xl border border-border/30 shadow-2xl p-6 space-y-5">
            <Dialog.Title className="text-sm font-semibold">
              {editingId ? "Edit Goal" : "Add Goal"}
            </Dialog.Title>

            <div className="space-y-3">
              <div>
                <label className="text-[13px] text-muted-foreground mb-1 block">Title</label>
                <input
                  type="text"
                  value={form.title}
                  onChange={(e) => setForm((f) => ({ ...f, title: e.target.value }))}
                  placeholder="What are you working toward?"
                  className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
                />
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="text-[13px] text-muted-foreground mb-1 block">Category</label>
                  <select
                    value={form.category}
                    onChange={(e) => setForm((f) => ({ ...f, category: e.target.value as Goal["category"] }))}
                    className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm focus:outline-none focus:border-accent/40 transition-colors"
                  >
                    <option value="revenue">Revenue</option>
                    <option value="health">Health</option>
                    <option value="project">Project</option>
                    <option value="personal">Personal</option>
                  </select>
                </div>
                <div>
                  <label className="text-[13px] text-muted-foreground mb-1 block">Unit</label>
                  <input
                    type="text"
                    value={form.unit}
                    onChange={(e) => setForm((f) => ({ ...f, unit: e.target.value }))}
                    placeholder="%"
                    className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
                  />
                </div>
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="text-[13px] text-muted-foreground mb-1 block">Target</label>
                  <input
                    type="number"
                    value={form.target}
                    onChange={(e) => setForm((f) => ({ ...f, target: parseFloat(e.target.value) || 0 }))}
                    className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm font-mono focus:outline-none focus:border-accent/40 transition-colors"
                  />
                </div>
                <div>
                  <label className="text-[13px] text-muted-foreground mb-1 block">Current</label>
                  <input
                    type="number"
                    value={form.current}
                    onChange={(e) => setForm((f) => ({ ...f, current: parseFloat(e.target.value) || 0 }))}
                    className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm font-mono focus:outline-none focus:border-accent/40 transition-colors"
                  />
                </div>
              </div>

              <div>
                <label className="text-[13px] text-muted-foreground mb-1 block">
                  Deadline (optional)
                </label>
                <input
                  type="date"
                  value={form.deadline || ""}
                  onChange={(e) => setForm((f) => ({ ...f, deadline: e.target.value || null }))}
                  className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm focus:outline-none focus:border-accent/40 transition-colors"
                />
              </div>
            </div>

            <div className="flex items-center justify-end gap-3 pt-2">
              <Dialog.Close asChild>
                <button className="px-3 py-2 rounded-lg text-sm text-muted-foreground hover:bg-muted/20 transition-colors">
                  Cancel
                </button>
              </Dialog.Close>
              <button
                onClick={handleSave}
                disabled={!form.title.trim()}
                className="px-4 py-2 rounded-lg bg-accent/15 text-accent text-sm font-medium hover:bg-accent/25 transition-colors disabled:opacity-40"
              >
                {editingId ? "Save Changes" : "Add Goal"}
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </div>
  );
}
