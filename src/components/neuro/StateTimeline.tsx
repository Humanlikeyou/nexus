import { useRef, useEffect } from "react";
import type { StateTimelineEntry } from "@/lib/types";

interface StateTimelineProps {
  timeline: StateTimelineEntry[];
  elapsed: number;
  duration: number;
}

const stateColors: Record<string, string> = {
  calm: "#10b981",
  neutral: "#f59e0b",
  active: "#f97316",
};

export function StateTimeline({ timeline, elapsed, duration }: StateTimelineProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    ctx.scale(dpr, dpr);

    const w = rect.width;
    const h = rect.height;

    ctx.clearRect(0, 0, w, h);

    // Background
    ctx.fillStyle = "rgba(255, 255, 255, 0.03)";
    ctx.beginPath();
    ctx.roundRect(0, 0, w, h, 4);
    ctx.fill();

    if (timeline.length === 0) return;

    const segWidth = duration > 0 ? w / (duration * 2) : w / timeline.length;

    for (let i = 0; i < timeline.length; i++) {
      const entry = timeline[i];
      const color = stateColors[entry.state] || "#6b7280";
      ctx.fillStyle = entry.inZone ? color : `${color}88`;
      ctx.fillRect(i * segWidth, 0, Math.max(segWidth, 1), h);
    }
  }, [timeline, elapsed, duration]);

  const formatTime = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m}:${s.toString().padStart(2, "0")}`;
  };

  return (
    <div className="space-y-1">
      <canvas
        ref={canvasRef}
        className="w-full rounded"
        style={{ height: 20 }}
      />
      <div className="flex justify-between text-xs text-muted-foreground/50">
        <span>{formatTime(elapsed)}</span>
        <span>{formatTime(duration)}</span>
      </div>
    </div>
  );
}
