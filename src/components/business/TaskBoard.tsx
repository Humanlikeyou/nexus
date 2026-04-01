import { CheckCircle2, Circle, Clock, AlertCircle } from "lucide-react";
import { cn } from "@/lib/utils";
import type { TaskItem } from "@/lib/types";

interface TaskBoardProps {
  tasks: TaskItem[];
  loading: boolean;
}

const columns = [
  { id: "active", label: "Active Sprint", icon: AlertCircle, color: "text-accent" },
  { id: "backlog", label: "Backlog", icon: Circle, color: "text-muted-foreground" },
  { id: "waiting", label: "Waiting", icon: Clock, color: "text-warning" },
  { id: "done", label: "Done", icon: CheckCircle2, color: "text-success" },
] as const;

export function TaskBoard({ tasks, loading }: TaskBoardProps) {
  if (loading) {
    return (
      <div className="panel p-4">
        <h3 className="heading-section mb-3">Task Board</h3>
        <div className="grid grid-cols-4 gap-3">
          {columns.map((col) => (
            <div key={col.id} className="space-y-1.5">
              <div className="h-3 bg-muted/15 rounded-sm animate-pulse w-20" />
              <div className="h-12 bg-muted/10 rounded-sm animate-pulse" />
              <div className="h-12 bg-muted/10 rounded-sm animate-pulse" />
            </div>
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="panel p-4 space-y-3">
      <h3 className="heading-section">Task Board</h3>

      <div className="grid grid-cols-4 gap-3">
        {columns.map((col) => {
          const Icon = col.icon;
          const colTasks = tasks.filter((t) => t.status === col.id);

          return (
            <div key={col.id} className="space-y-1.5">
              <div className="flex items-center gap-1.5 pb-1.5 border-b border-border/35">
                <Icon size={11} className={col.color} />
                <span className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground/60">
                  {col.label}
                </span>
                <span className="ml-auto text-[10px] font-mono text-muted-foreground/50">
                  {colTasks.length}
                </span>
              </div>

              <div className="space-y-1 min-h-[80px]">
                {colTasks.map((task, i) => (
                  <div
                    key={i}
                    className="p-2 rounded-sm bg-muted/5 border border-border/25 hover:bg-muted/10 transition-colors"
                  >
                    <p className="text-[11px] font-mono leading-relaxed text-foreground/85">{task.title}</p>
                    {task.tags.length > 0 && (
                      <div className="flex flex-wrap gap-1 mt-1.5">
                        {task.tags.map((tag) => (
                          <span
                            key={tag}
                            className="text-[9px] font-mono px-1.5 py-0.5 rounded-sm bg-accent/12 text-accent/70 uppercase tracking-wider"
                          >
                            {tag}
                          </span>
                        ))}
                      </div>
                    )}
                    {task.priority && (
                      <span
                        className={cn(
                          "inline-block mt-1 text-[9px] font-mono px-1.5 py-0.5 rounded-sm uppercase tracking-wider",
                          task.priority === "high"
                            ? "bg-destructive/12 text-destructive/80"
                            : task.priority === "medium"
                              ? "bg-warning/12 text-warning/80"
                              : "bg-muted/15 text-muted-foreground/50",
                        )}
                      >
                        {task.priority}
                      </span>
                    )}
                  </div>
                ))}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
