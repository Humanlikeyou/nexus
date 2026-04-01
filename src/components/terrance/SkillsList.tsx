import { Zap, Circle } from "lucide-react";
import { cn } from "@/lib/utils";
import type { Skill } from "@/lib/types";

interface SkillsListProps {
  skills: Skill[];
  loading: boolean;
}

export function SkillsList({ skills, loading }: SkillsListProps) {
  return (
    <div className="panel p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="heading-section">Active Skills</h3>
        <span className="text-[10px] font-mono text-muted-foreground/60">{(skills ?? []).length}</span>
      </div>

      {loading ? (
        <div className="space-y-1.5">
          {[1, 2, 3].map((i) => (
            <div key={i} className="h-7 bg-muted/15 rounded-sm animate-pulse" />
          ))}
        </div>
      ) : (
        <div className="space-y-px">
          {skills.map((skill, i) => (
            <div
              key={i}
              className="flex items-center gap-2.5 px-2.5 py-1.5 rounded-sm hover:bg-muted/10 transition-colors"
            >
              <Circle
                size={5}
                fill="currentColor"
                className={cn(
                  skill.status === "active" ? "text-success" : "text-muted-foreground/35",
                )}
              />
              <div className="flex-1 min-w-0">
                <p className="text-[11px] font-mono truncate text-foreground/85">{skill.name}</p>
              </div>
              <span
                className={cn(
                  "text-[9px] font-mono uppercase tracking-wider px-1.5 py-0.5 rounded-sm",
                  skill.status === "active"
                    ? "bg-success/12 text-success"
                    : "bg-muted/15 text-muted-foreground/50",
                )}
              >
                {skill.status}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
