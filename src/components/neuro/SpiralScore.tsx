import { useRef, useEffect } from "react";
import type { StateTimelineEntry } from "@/lib/types";

interface SpiralScoreProps {
  timeline: StateTimelineEntry[];
  durationMin: number;
  size?: number;
}

const stateColors: Record<string, string> = {
  calm: "#10b981",
  neutral: "#f59e0b",
  active: "#f97316",
};

export function SpiralScore({ timeline, durationMin, size = 240 }: SpiralScoreProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    canvas.width = size * dpr;
    canvas.height = size * dpr;
    ctx.scale(dpr, dpr);

    const cx = size / 2;
    const cy = size / 2;

    ctx.clearRect(0, 0, size, size);

    const dotCount = Math.max(durationMin * 10, 30);
    const goldenAngle = Math.PI * (3 - Math.sqrt(5));
    const maxRadius = size * 0.42;

    for (let i = 0; i < dotCount; i++) {
      const angle = i * goldenAngle;
      const radius = maxRadius * Math.sqrt(i / dotCount);
      const x = cx + Math.cos(angle) * radius;
      const y = cy + Math.sin(angle) * radius;

      // Map to timeline entry
      const timelineIdx = Math.floor((i / dotCount) * timeline.length);
      const entry = timeline[timelineIdx];

      if (entry) {
        const color = stateColors[entry.state] || "#6b7280";
        const dotSize = entry.state === "calm" ? 4 : entry.state === "neutral" ? 3 : 2;
        const alpha = entry.inZone ? 1.0 : 0.4;

        ctx.globalAlpha = alpha;
        ctx.fillStyle = color;
        ctx.beginPath();
        ctx.arc(x, y, dotSize, 0, Math.PI * 2);
        ctx.fill();
      } else {
        ctx.globalAlpha = 0.1;
        ctx.fillStyle = "#6b7280";
        ctx.beginPath();
        ctx.arc(x, y, 2, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    ctx.globalAlpha = 1;
  }, [timeline, durationMin, size]);

  return (
    <canvas
      ref={canvasRef}
      style={{ width: size, height: size }}
      className="rounded-lg"
    />
  );
}
