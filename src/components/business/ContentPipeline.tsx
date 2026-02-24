import { ArrowRight, FileText, Video, Image } from "lucide-react";
import { cn } from "@/lib/utils";
import type { ContentPipelineItem } from "@/lib/types";

interface ContentPipelineProps {
  items: ContentPipelineItem[];
  loading: boolean;
}

const stages = [
  { id: "ideation", label: "Ideation", color: "bg-accent/20 text-accent" },
  { id: "drafting", label: "Drafting", color: "bg-warning/20 text-warning" },
  { id: "review", label: "Review", color: "bg-blue-500/20 text-blue-400" },
  { id: "scheduled", label: "Scheduled", color: "bg-success/20 text-success" },
  { id: "published", label: "Published", color: "bg-success/30 text-success" },
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
      <div className="glass p-6">
        <div className="h-4 bg-muted/30 rounded animate-pulse w-40 mb-4" />
        <div className="h-20 bg-muted/20 rounded animate-pulse" />
      </div>
    );
  }

  return (
    <div className="glass p-6 space-y-5">
      <h3 className="text-base font-medium text-muted-foreground">Content Pipeline</h3>

      {/* Stage indicators */}
      <div className="flex items-center gap-1">
        {stages.map((stage, i) => {
          const count = items.filter((item) => item.stage === stage.id).length;
          return (
            <div key={stage.id} className="flex items-center">
              <div className="flex items-center gap-1.5">
                <div
                  className={cn(
                    "px-2.5 py-1 rounded-md text-xs font-medium",
                    stage.color,
                  )}
                >
                  {stage.label} ({count})
                </div>
              </div>
              {i < stages.length - 1 && (
                <ArrowRight size={12} className="text-muted-foreground/30 mx-1" />
              )}
            </div>
          );
        })}
      </div>

      {/* Content items */}
      <div className="space-y-2">
        {items.map((item, i) => {
          const Icon = typeIcons[item.content_type] || FileText;
          const stage = stages.find((s) => s.id === item.stage);

          return (
            <div
              key={i}
              className="flex items-center gap-3 px-3 py-2.5 rounded-lg bg-muted/15 border border-border/15"
            >
              <Icon size={14} className="text-muted-foreground/60 shrink-0" />
              <span className="text-xs flex-1 truncate">{item.title}</span>
              <span
                className={cn(
                  "text-[13px] px-2 py-0.5 rounded shrink-0",
                  stage?.color || "bg-muted/30 text-muted-foreground",
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
