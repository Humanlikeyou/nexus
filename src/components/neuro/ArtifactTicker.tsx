import { Eye, Move, Volume2 } from "lucide-react";
import type { ArtifactEvent, ArtifactType } from "@/lib/types";

interface ArtifactTickerProps {
  artifacts: ArtifactEvent[];
  artifactRate: number;
  streaming: boolean;
}

const ARTIFACT_CONFIG: Record<ArtifactType, { icon: typeof Eye; label: string }> = {
  blink: { icon: Eye, label: "Blink" },
  motion: { icon: Move, label: "Motion" },
  jaw_clench: { icon: Volume2, label: "Jaw" },
};

export function ArtifactTicker({ artifacts, artifactRate, streaming }: ArtifactTickerProps) {
  if (!streaming || artifacts.length === 0) return null;

  const last5 = artifacts.slice(-5).reverse();
  const now = Date.now();

  return (
    <div className="panel p-2">
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs text-muted-foreground uppercase tracking-wider">
          Artifacts
        </span>
        <span className="text-xs text-muted-foreground">
          {artifactRate}/min
        </span>
      </div>
      <div className="flex flex-wrap gap-1.5">
        {last5.map((event, i) => {
          const config = ARTIFACT_CONFIG[event.type];
          const Icon = config.icon;
          const secsAgo = Math.round((now - event.timestamp) / 1000);

          return (
            <div
              key={`${event.timestamp}-${i}`}
              className="flex items-center gap-1 px-2 py-1 rounded bg-muted/15 border border-border/18"
            >
              <Icon size={10} className="text-muted-foreground/70" />
              <span className="text-xs text-muted-foreground">
                {config.label}
              </span>
              <span className="text-xs text-muted-foreground/50">
                {secsAgo}s
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
