import * as Dialog from "@radix-ui/react-dialog";
import { X, Trophy, Download } from "lucide-react";
import { SpiralScore } from "./SpiralScore";
import { StateTimeline } from "./StateTimeline";
import { getMilestone, MILESTONE_COLORS } from "@/lib/neurofeedback/scoring";
import type { NfSession } from "@/lib/types";

interface SessionReportProps {
  session: NfSession;
  open: boolean;
  onClose: () => void;
  onSave: (session: NfSession) => void;
}

function downloadSessionJson(session: NfSession) {
  const blob = new Blob([JSON.stringify(session, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  const dateStr = new Date(session.start).toISOString().slice(0, 10);
  a.href = url;
  a.download = `nexus-session-${dateStr}-${session.protocol_name}.json`;
  a.click();
  URL.revokeObjectURL(url);
}

export function SessionReport({ session, open, onClose, onSave }: SessionReportProps) {
  const milestone = getMilestone(session.time_in_zone_pct);
  const milestoneColor = MILESTONE_COLORS[milestone];
  const durationMin = Math.round(session.duration_secs / 60);

  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/70 backdrop-blur-sm z-50" />
        <Dialog.Content className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[480px] max-h-[85vh] overflow-y-auto glass-elevated rounded-2xl p-6 border border-border/45 shadow-2xl">
          <div className="flex items-center justify-between mb-5">
            <Dialog.Title className="text-base font-semibold">
              Session Complete
            </Dialog.Title>
            <Dialog.Close className="text-muted-foreground/60 hover:text-muted-foreground transition-colors">
              <X size={16} />
            </Dialog.Close>
          </div>

          {/* Top section: Spiral + Stats */}
          <div className="flex gap-5 mb-5">
            <div className="flex-shrink-0">
              <SpiralScore
                timeline={session.state_timeline}
                durationMin={durationMin}
                size={160}
              />
            </div>
            <div className="flex-1 space-y-3">
              <div className="flex items-center gap-2">
                {milestone !== "none" && (
                  <Trophy size={14} style={{ color: milestoneColor }} />
                )}
                <span
                  className="text-xs font-medium capitalize"
                  style={{ color: milestoneColor }}
                >
                  {milestone !== "none" ? milestone : ""}
                </span>
              </div>

              <div className="space-y-2">
                <StatRow label="Score" value={session.total_score.toLocaleString()} />
                <StatRow label="Time in Zone" value={`${session.time_in_zone_pct}%`} />
                <StatRow label="Duration" value={`${durationMin}:${(session.duration_secs % 60).toString().padStart(2, "0")}`} />
                <StatRow label="Avg Focus" value={`${Math.round(session.summary.avg_focus || 0)}`} />
                <StatRow label="Avg Relax" value={`${Math.round(session.summary.avg_relaxation || 0)}`} />
                {session.avg_alpha_peak && (
                  <StatRow label="Alpha Peak" value={`${session.avg_alpha_peak.toFixed(1)} Hz`} />
                )}
              </div>
            </div>
          </div>

          {/* Protocol badge */}
          <div className="mb-4">
            <span className="text-[13px] px-2 py-1 rounded-full bg-accent/15 text-accent border border-accent/20">
              {session.protocol_name}
            </span>
          </div>

          {/* Full timeline */}
          <div className="mb-4 space-y-2">
            <p className="text-xs text-muted-foreground">State Timeline</p>
            <StateTimeline
              timeline={session.state_timeline}
              elapsed={session.duration_secs}
              duration={session.duration_secs}
            />
          </div>

          {/* Band averages */}
          {Object.keys(session.band_averages).length > 0 && (
            <div className="mb-5">
              <p className="text-xs text-muted-foreground mb-2">Band Averages</p>
              <div className="grid grid-cols-4 gap-2">
                {Object.entries(session.band_averages).map(([band, value]) => (
                  <div
                    key={band}
                    className="glass p-2 rounded-lg text-center"
                  >
                    <p className="text-xs font-medium tabular-nums">
                      {typeof value === "number" ? value.toFixed(1) : value}%
                    </p>
                    <p className="text-[13px] text-muted-foreground/50 capitalize">{band}</p>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Actions */}
          <div className="flex gap-3">
            <button
              onClick={() => onSave(session)}
              className="flex-1 py-2.5 rounded-lg bg-accent/15 text-accent text-sm font-medium hover:bg-accent/25 transition-colors border border-accent/30"
            >
              Save Session
            </button>
            <button
              onClick={() => downloadSessionJson(session)}
              className="flex items-center justify-center gap-1.5 px-3 py-2.5 rounded-lg bg-muted/20 text-muted-foreground text-sm hover:bg-muted/30 transition-colors"
              title="Export session as JSON"
            >
              <Download size={13} />
              Export
            </button>
            <button
              onClick={onClose}
              className="flex-1 py-2.5 rounded-lg bg-muted/20 text-muted-foreground text-sm hover:bg-muted/30 transition-colors"
            >
              Close
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function StatRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between text-xs">
      <span className="text-muted-foreground/60">{label}</span>
      <span className="font-medium tabular-nums">{value}</span>
    </div>
  );
}
