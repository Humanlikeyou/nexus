interface GateStatusIndicatorProps {
  allowed: boolean;
  gated: boolean;
  reason?: string;
  confidence?: number;
}

export function GateStatusIndicator({
  allowed,
  gated,
  reason,
  confidence,
}: GateStatusIndicatorProps) {
  const isPaused = confidence != null && confidence < 0.3;

  let dotColor: string;
  let label: string;

  if (isPaused) {
    dotColor = "#ef4444"; // red
    label = "Feedback Paused";
  } else if (gated) {
    dotColor = "#eab308"; // yellow
    label = `Gated: ${reason || "unknown"}`;
  } else {
    dotColor = "#10b981"; // green
    label = "Feedback Active";
  }

  return (
    <div className="flex items-center gap-1.5">
      <div
        className="w-1.5 h-1.5 rounded-full"
        style={{ backgroundColor: dotColor }}
      />
      <span className="text-xs text-muted-foreground/50">{label}</span>
    </div>
  );
}
