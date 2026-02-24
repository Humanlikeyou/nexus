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
      <div className="glass p-6">
        <h3 className="text-base font-medium text-muted-foreground mb-4">Task Board</h3>
        <div className="grid grid-cols-4 gap-4">
          {columns.map((col) => (
            <div key={col.id} className="space-y-2">
              <div className="h-4 bg-muted/30 rounded animate-pulse w-24" />
              <div className="h-16 bg-muted/20 rounded animate-pulse" />
              <div className="h-16 bg-muted/20 rounded animate-pulse" />
            </div>
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="glass p-6 space-y-5">
      <h3 className="text-base font-medium text-muted-foreground">Task Board</h3>

      <div className="grid grid-cols-4 gap-4">
        {columns.map((col) => {
          const Icon = col.icon;
          const colTasks = tasks.filter((t) => t.status === col.id);

          return (
            <div key={col.id} className="space-y-2">
              <div className="flex items-center gap-2 pb-2 border-b border-border/20">
                <Icon size={14} className={col.color} />
                <span className="text-xs font-medium text-muted-foreground">
                  {col.label}
                </span>
                <span className="ml-auto text-xs text-muted-foreground/50">
                  {colTasks.length}
                </span>
              </div>

              <div className="space-y-2 min-h-[100px]">
                {colTasks.map((task, i) => (
                  <div
                    key={i}
                    className={cn(
                      "p-3 rounded-lg bg-muted/15 border border-border/15",
                      "hover:bg-muted/25 transition-colors cursor-default",
                    )}
                  >
                    <p className="text-xs leading-relaxed">{task.title}</p>
                    {task.tags.length > 0 && (
                      <div className="flex flex-wrap gap-1 mt-2">
                        {task.tags.map((tag) => (
                          <span
                            key={tag}
                            className="text-[13px] px-2 py-0.5 rounded-md font-medium bg-accent/10 text-accent/70"
                          >
                            {tag}
                          </span>
                        ))}
                      </div>
                    )}
                    {task.priority && (
                      <span
                        className={cn(
                          "inline-block mt-1.5 text-[13px] px-1.5 py-0.5 rounded",
                          task.priority === "high"
                            ? "bg-destructive/10 text-destructive"
                            : task.priority === "medium"
                              ? "bg-warning/10 text-warning"
                              : "bg-muted/30 text-muted-foreground/50",
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
