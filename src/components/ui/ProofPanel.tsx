import { useState, useEffect, useRef } from "react";
import { Bug, ChevronDown, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { setProofMode, getProofLog } from "@/lib/api";
import type { BleProofEvent } from "@/lib/types";

function formatTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString("en-US", { hour12: false });
}

function eventColor(eventType: string): string {
  if (eventType.includes("success") || eventType.includes("stream_start")) return "text-success";
  if (eventType.includes("failed") || eventType.includes("error")) return "text-destructive";
  if (eventType.includes("scan") || eventType.includes("attempt")) return "text-warning";
  if (eventType === "state_change") return "text-accent";
  return "text-muted-foreground";
}

export function ProofPanel() {
  const [expanded, setExpanded] = useState(false);
  const [enabled, setEnabled] = useState(false);
  const [events, setEvents] = useState<BleProofEvent[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!enabled || !expanded) return;
    const interval = setInterval(() => {
      getProofLog().then(setEvents).catch(() => {});
    }, 2000);
    // Immediate fetch
    getProofLog().then(setEvents).catch(() => {});
    return () => clearInterval(interval);
  }, [enabled, expanded]);

  // Auto-scroll to bottom when new events arrive
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [events.length]);

  const handleToggle = async () => {
    const next = !enabled;
    try {
      await setProofMode(next);
      setEnabled(next);
      if (!next) setEvents([]);
    } catch {
      // ignore
    }
  };

  return (
    <div className="glass overflow-hidden">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center justify-between px-4 py-3 text-xs text-muted-foreground hover:text-foreground transition-colors"
      >
        <div className="flex items-center gap-2">
          <Bug size={14} />
          <span className="font-medium">BLE Proof Log</span>
          {enabled && events.length > 0 && (
            <span className="px-1.5 py-0.5 rounded-full bg-accent/15 text-accent text-xs font-mono">
              {events.length}
            </span>
          )}
        </div>
        {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
      </button>

      {expanded && (
        <div className="px-4 pb-4 space-y-3">
          <div className="flex items-center gap-3">
            <button
              onClick={handleToggle}
              className={cn(
                "relative w-9 h-5 rounded-full transition-colors",
                enabled ? "bg-success" : "bg-muted/30",
              )}
            >
              <div
                className={cn(
                  "absolute top-0.5 w-4 h-4 rounded-full bg-white shadow transition-transform",
                  enabled ? "translate-x-[18px]" : "translate-x-0.5",
                )}
              />
            </button>
            <span className="text-[13px] text-muted-foreground">
              {enabled ? "Proof mode ON — logging BLE events" : "Proof mode OFF"}
            </span>
          </div>

          {enabled && (
            <div
              ref={scrollRef}
              className="max-h-[200px] overflow-y-auto rounded-lg bg-background/50 border border-border/20 p-2 space-y-0.5"
            >
              {events.length === 0 ? (
                <p className="text-xs text-muted-foreground/60 text-center py-4">
                  No events yet. Scan or connect a device.
                </p>
              ) : (
                events.map((evt, i) => (
                  <div key={i} className="flex gap-2 font-mono text-xs leading-relaxed">
                    <span className="text-muted-foreground/50 shrink-0">
                      [{formatTime(evt.timestamp)}]
                    </span>
                    <span className="text-accent/70 shrink-0">[{evt.device}]</span>
                    <span className={cn("shrink-0", eventColor(evt.event_type))}>
                      {evt.event_type}:
                    </span>
                    <span className="text-muted-foreground/70 break-all">{evt.detail}</span>
                  </div>
                ))
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
