import { TrendingUp, Flame, Clock, Award } from "lucide-react";
import type { NfSession, RecordedSession } from "@/lib/types";

interface ProgressStatsProps {
  sessions: RecordedSession[];
}

function isNfSession(s: RecordedSession): s is NfSession {
  return "protocol_id" in s && "total_score" in s;
}

export function ProgressStats({ sessions }: ProgressStatsProps) {
  const nfSessions = sessions.filter(isNfSession);

  const totalSessions = nfSessions.length;
  const totalMinutes = Math.round(
    nfSessions.reduce((sum, s) => sum + s.duration_secs, 0) / 60,
  );
  const bestScore = nfSessions.length > 0
    ? Math.max(...nfSessions.map((s) => s.total_score))
    : 0;

  // Streak: consecutive days with at least 1 NF session
  const streak = computeStreak(nfSessions);

  // Avg time-in-zone for last 10 sessions
  const last10 = nfSessions.slice(0, 10);
  const avgTimeInZone = last10.length > 0
    ? Math.round(
        last10.reduce((sum, s) => sum + s.time_in_zone_pct, 0) / last10.length,
      )
    : 0;

  // Sparkline data (last 10 scores)
  const sparklineData = last10.map((s) => s.total_score).reverse();

  return (
    <div className="glass p-6 space-y-3">
      <h3 className="text-base font-medium text-muted-foreground">Progress</h3>

      <div className="grid grid-cols-2 gap-3">
        <StatItem icon={<Clock size={12} />} label="Sessions" value={`${totalSessions}`} />
        <StatItem icon={<Clock size={12} />} label="Minutes" value={`${totalMinutes}`} />
        <StatItem icon={<Flame size={12} />} label="Streak" value={`${streak}d`} />
        <StatItem icon={<Award size={12} />} label="Best Score" value={`${bestScore}`} />
      </div>

      {last10.length > 0 && (
        <div className="space-y-1.5">
          <div className="flex justify-between text-xs text-muted-foreground/40">
            <span>Avg In-Zone: {avgTimeInZone}%</span>
            <span className="flex items-center gap-1">
              <TrendingUp size={9} /> Last {last10.length}
            </span>
          </div>
          <Sparkline data={sparklineData} />
        </div>
      )}
    </div>
  );
}

function StatItem({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="text-muted-foreground/40">{icon}</span>
      <div>
        <p className="text-sm font-bold tabular-nums">{value}</p>
        <p className="text-[13px] text-muted-foreground/50">{label}</p>
      </div>
    </div>
  );
}

function Sparkline({ data }: { data: number[] }) {
  if (data.length < 2) return null;

  const max = Math.max(...data, 1);
  const w = 200;
  const h = 28;
  const points = data.map((v, i) => {
    const x = (i / (data.length - 1)) * w;
    const y = h - (v / max) * (h - 4);
    return `${x},${y}`;
  });

  return (
    <svg viewBox={`0 0 ${w} ${h}`} className="w-full" style={{ height: h }}>
      <polyline
        points={points.join(" ")}
        fill="none"
        stroke="rgba(8, 145, 178, 0.4)"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function computeStreak(sessions: NfSession[]): number {
  if (sessions.length === 0) return 0;

  const daySet = new Set(
    sessions.map((s) => {
      const d = new Date(s.start);
      return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
    }),
  );

  let streak = 0;
  const now = new Date();
  for (let i = 0; i < 365; i++) {
    const d = new Date(now);
    d.setDate(d.getDate() - i);
    const key = `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
    if (daySet.has(key)) {
      streak++;
    } else if (i > 0) {
      break;
    }
  }
  return streak;
}
