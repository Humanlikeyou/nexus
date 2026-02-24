import { useState } from "react";
import { Clock, Brain, Trash2, ChevronDown, ChevronUp } from "lucide-react";
import { deleteSession } from "@/lib/api";
import { useNexus } from "@/context/NexusContext";
import { SpiralScore } from "./SpiralScore";
import { StateTimeline } from "./StateTimeline";
import type { RecordedSession, NfSession } from "@/lib/types";

interface SessionHistoryProps {
  sessions: RecordedSession[];
}

function isNfSession(s: RecordedSession): s is NfSession {
  return "protocol_id" in s && "total_score" in s;
}

export function SessionHistory({ sessions }: SessionHistoryProps) {
  const { toast } = useNexus();
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const handleDelete = async (id: string) => {
    try {
      await deleteSession(id);
      toast({ type: "info", title: "Session Deleted" });
      window.dispatchEvent(new CustomEvent("nexus:refresh"));
    } catch (e) {
      toast({ type: "error", title: "Delete Failed", description: String(e) });
    }
  };

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center gap-2">
        <Brain size={16} className="text-accent" />
        <h3 className="text-base font-medium text-muted-foreground">Session History</h3>
      </div>

      {sessions.length === 0 ? (
        <div className="py-6 text-center">
          <Clock size={20} className="text-muted-foreground/20 mx-auto mb-2" />
          <p className="text-xs text-muted-foreground/40">
            No sessions recorded yet. Start streaming and hit Record.
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {sessions.map((session) => {
            const isNf = isNfSession(session);
            const expanded = expandedId === session.id;

            return (
              <div key={session.id} className="rounded-lg bg-muted/15 border border-border/15">
                <div
                  className="group flex items-center justify-between px-3 py-2.5 cursor-pointer"
                  onClick={() => setExpandedId(expanded ? null : session.id)}
                >
                  <div className="flex items-center gap-3">
                    <Clock size={12} className="text-muted-foreground/50" />
                    <div>
                      <div className="flex items-center gap-2">
                        <p className="text-xs">
                          {new Date(session.start).toLocaleDateString()}
                        </p>
                        {isNf && (
                          <span className="text-[13px] px-1.5 py-0.5 rounded-full bg-accent/10 text-accent">
                            {session.protocol_name}
                          </span>
                        )}
                      </div>
                      <p className="text-xs text-muted-foreground/50">
                        {Math.round(session.duration_secs / 60)}min · {session.data_points} pts
                      </p>
                    </div>
                  </div>
                  <div className="flex items-center gap-4">
                    <div className="flex gap-4 text-right">
                      {isNf && (
                        <>
                          <div>
                            <p className="text-xs font-medium text-accent">
                              {session.total_score}
                            </p>
                            <p className="text-[13px] text-muted-foreground/50">Score</p>
                          </div>
                          <div>
                            <p className="text-xs font-medium text-success">
                              {session.time_in_zone_pct}%
                            </p>
                            <p className="text-[13px] text-muted-foreground/50">In Zone</p>
                          </div>
                        </>
                      )}
                      {!isNf && session.summary.avg_focus != null && (
                        <div>
                          <p className="text-xs font-medium text-accent">
                            {Math.round(session.summary.avg_focus)}
                          </p>
                          <p className="text-[13px] text-muted-foreground/50">Focus</p>
                        </div>
                      )}
                      {!isNf && session.summary.avg_relaxation != null && (
                        <div>
                          <p className="text-xs font-medium text-success">
                            {Math.round(session.summary.avg_relaxation)}
                          </p>
                          <p className="text-[13px] text-muted-foreground/50">Relax</p>
                        </div>
                      )}
                      {session.summary.avg_hr != null && (
                        <div>
                          <p className="text-xs font-medium text-red-400">
                            {Math.round(session.summary.avg_hr)}
                          </p>
                          <p className="text-[13px] text-muted-foreground/50">Avg HR</p>
                        </div>
                      )}
                    </div>

                    {isNf && (
                      expanded
                        ? <ChevronUp size={12} className="text-muted-foreground/30" />
                        : <ChevronDown size={12} className="text-muted-foreground/30" />
                    )}

                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        handleDelete(session.id);
                      }}
                      className="opacity-0 group-hover:opacity-100 transition-opacity"
                    >
                      <Trash2 size={12} className="text-muted-foreground/30 hover:text-destructive" />
                    </button>
                  </div>
                </div>

                {/* Expanded view for NF sessions */}
                {expanded && isNf && (
                  <div className="px-3 pb-3 space-y-3 border-t border-border/10 pt-3">
                    <div className="flex gap-4">
                      <SpiralScore
                        timeline={session.state_timeline}
                        durationMin={Math.round(session.duration_secs / 60)}
                        size={80}
                      />
                      <div className="flex-1 space-y-2">
                        <StateTimeline
                          timeline={session.state_timeline}
                          elapsed={session.duration_secs}
                          duration={session.duration_secs}
                        />
                        {session.avg_alpha_peak && (
                          <p className="text-xs text-muted-foreground/40">
                            Alpha Peak: {session.avg_alpha_peak.toFixed(1)} Hz
                          </p>
                        )}
                      </div>
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
