import { cn } from "@/lib/utils";

interface BleStatusDotProps {
  connected: boolean;
  streaming: boolean;
  label: string;
}

export function BleStatusDot({ connected, streaming, label }: BleStatusDotProps) {
  return (
    <div className="flex items-center gap-2">
      <div
        className={cn(
          "w-2 h-2 rounded-full transition-colors",
          connected
            ? streaming
              ? "bg-success animate-pulse-dot"
              : "bg-success"
            : "bg-muted-foreground/30",
        )}
      />
      <span className="text-xs text-muted-foreground/60">{label}</span>
    </div>
  );
}
