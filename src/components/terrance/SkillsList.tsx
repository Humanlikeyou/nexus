import { Zap, Circle } from "lucide-react";
import { cn } from "@/lib/utils";
import type { Skill } from "@/lib/types";

interface SkillsListProps {
  skills: Skill[];
  loading: boolean;
}

export function SkillsList({ skills, loading }: SkillsListProps) {
  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center gap-2">
        <Zap size={16} className="text-accent" />
        <h3 className="text-sm font-medium text-muted-foreground/70 uppercase tracking-wide">Active Skills</h3>
        <span className="ml-auto text-xs text-muted-foreground/60">{(skills ?? []).length}</span>
      </div>

      {loading ? (
        <div className="space-y-2">
          {[1, 2, 3].map((i) => (
            <div key={i} className="h-8 bg-muted/30 rounded animate-pulse" />
          ))}
        </div>
      ) : (
        <div className="space-y-1.5">
          {skills.map((skill, i) => (
            <div
              key={i}
              className="flex items-center gap-3 px-3 py-2 rounded-lg hover:bg-muted/20 transition-colors"
            >
              <Circle
                size={8}
                fill="currentColor"
                className={cn(
                  skill.status === "active" ? "text-success" : "text-muted-foreground/30",
                )}
              />
              <div className="flex-1 min-w-0">
                <p className="text-sm truncate">{skill.name}</p>
                {skill.description && (
                  <p className="text-xs text-muted-foreground/60 truncate">
                    {skill.description}
                  </p>
                )}
              </div>
              <span
                className={cn(
                  "text-[11px] font-mono uppercase tracking-wider px-1.5 py-0.5 rounded",
                  skill.status === "active"
                    ? "bg-success/10 text-success"
                    : "bg-muted/30 text-muted-foreground/50",
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
