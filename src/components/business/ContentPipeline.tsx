import { ArrowRight, FileText, Video, Image } from "lucide-react";
import { cn } from "@/lib/utils";
import type { ContentPipelineItem } from "@/lib/types";

interface ContentPipelineProps {
  items: ContentPipelineItem[];
  loading: boolean;
}

const stages = [
  { id: "ideation", label: "Ideation", color: "bg-accent/15 text-accent/80" },
  { id: "drafting", label: "Drafting", color: "bg-warning/15 text-warning/80" },
  { id: "review", label: "Review", color: "bg-data/10 text-data/60" },
  { id: "scheduled", label: "Scheduled", color: "bg-success/15 text-success/80" },
  { id: "published", label: "Published", color: "bg-success/15 text-success/70" },
] as const;

const typeIcons: Record<string, typeof FileText> = {
  blog: FileText,
  video: Video,
  carousel: Image,
  post: FileText,
};

export function ContentPipeline({ items, loading }: ContentPipelineProps) {
  if (loading) {
    return (
      <div className="panel p-4">
        <div className="h-3 bg-muted/15 rounded-sm animate-pulse w-32 mb-3" />
        <div className="h-16 bg-muted/10 rounded-sm animate-pulse" />
      </div>
    );
  }

  return (
    <div className="panel p-4 space-y-3">
      <h3 className="heading-section">Content Pipeline</h3>

      {/* Stage indicators */}
      <div className="flex items-center gap-1">
        {stages.map((stage, i) => {
          const count = items.filter((item) => item.stage === stage.id).length;
          return (
            <div key={stage.id} className="flex items-center">
              <div
                className={cn(
                  "px-2 py-0.5 rounded-sm text-[9px] font-mono uppercase tracking-wider",
                  stage.color,
                )}
              >
                {stage.label} ({count})
              </div>
              {i < stages.length - 1 && (
                <ArrowRight size={9} className="text-muted-foreground/35 mx-0.5" />
              )}
            </div>
          );
        })}
      </div>

      {/* Content items */}
      <div className="space-y-1">
        {items.map((item, i) => {
          const Icon = typeIcons[item.content_type] || FileText;
          const stage = stages.find((s) => s.id === item.stage);

          return (
            <div
              key={i}
              className="flex items-center gap-2.5 px-2.5 py-2 rounded-sm bg-muted/5 border border-border/25"
            >
              <Icon size={11} className="text-muted-foreground/50 shrink-0" />
              <span className="text-[11px] font-mono flex-1 truncate text-foreground/80">{item.title}</span>
              <span
                className={cn(
                  "text-[9px] font-mono px-1.5 py-0.5 rounded-sm shrink-0 uppercase tracking-wider",
                  stage?.color || "bg-muted/15 text-muted-foreground/50",
                )}
              >
                {item.stage}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
