import { useState, useRef, useCallback, useEffect } from "react";
import { Camera, Loader2 } from "lucide-react";
import { computeBands } from "@/hooks/useFFT";
import type { BrainSnapshot, FrequencyBandPowers } from "@/lib/types";

interface BrainSnapshotCardProps {
  streaming: boolean;
  eegData: Record<string, number[]>;
  onSnapshotComplete?: (snapshot: BrainSnapshot) => void;
}

const FFT_SIZE = 256;

export function BrainSnapshotCard({ streaming, eegData, onSnapshotComplete }: BrainSnapshotCardProps) {
  const [recording, setRecording] = useState(false);
  const [progress, setProgress] = useState(0);
  const [snapshot, setSnapshot] = useState<BrainSnapshot | null>(null);
  const samplesRef = useRef<FrequencyBandPowers[]>([]);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const finalizeSnapshot = useCallback(() => {
    const s = samplesRef.current;
    if (s.length === 0) {
      setRecording(false);
      return;
    }

    const avg = (arr: number[]) =>
      arr.length > 0 ? arr.reduce((a, b) => a + b, 0) / arr.length : 0;

    const avgDelta = avg(s.map((b) => b.delta));
    const avgAlpha = avg(s.map((b) => b.alpha));
    const avgTheta = avg(s.map((b) => b.theta));
    const avgBeta = avg(s.map((b) => b.beta));
    const avgHighBeta = avg(s.map((b) => b.highBeta));
    const totalBeta = avgBeta;
    const avgAlphaPeak = avg(s.map((b) => b.alphaPeak));

    // Fatigue: delta/alpha ratio, inverted to 0-100
    const rawFatigue = avgAlpha > 0 ? avgDelta / avgAlpha : 5;
    const fatigue = Math.round(Math.max(0, Math.min(100, 100 - rawFatigue * 20)));

    // Engagement: inverted theta/beta ratio
    const rawEng = avgBeta > 0 ? avgTheta / avgBeta : 5;
    const engagement = Math.round(Math.max(0, Math.min(100, 100 - rawEng * 20)));

    // Beta balance: highBeta / totalBeta (lower is better for calm)
    const betaBalance = totalBeta > 0
      ? Math.round((avgHighBeta / totalBeta) * 100)
      : 50;

    const snap: BrainSnapshot = {
      id: crypto.randomUUID(),
      timestamp: new Date().toISOString(),
      fatigue,
      engagement,
      alphaPeak: Math.round(avgAlphaPeak * 10) / 10,
      betaBalance,
      focusScore: Math.round(avg(s.map((b) => b.focusScore))),
      relaxationScore: Math.round(avg(s.map((b) => b.relaxationScore))),
    };

    setSnapshot(snap);
    onSnapshotComplete?.(snap);
  }, [onSnapshotComplete]);

  const takeSnapshot = useCallback(() => {
    if (!streaming) return;
    setRecording(true);
    setProgress(0);
    samplesRef.current = [];

    let elapsed = 0;
    const total = 30000; // 30 seconds

    intervalRef.current = setInterval(() => {
      elapsed += 500;
      setProgress(Math.min(100, (elapsed / total) * 100));

      // Compute bands from current EEG data
      const af7 = eegData.AF7 || [];
      const af8 = eegData.AF8 || [];
      if (af7.length >= FFT_SIZE || af8.length >= FFT_SIZE) {
        const samples = new Array(FFT_SIZE);
        const af7Slice = af7.slice(-FFT_SIZE);
        const af8Slice = af8.slice(-FFT_SIZE);
        for (let i = 0; i < FFT_SIZE; i++) {
          samples[i] = ((af7Slice[i] ?? 0) + (af8Slice[i] ?? 0)) / 2;
        }
        const bands = computeBands(samples);
        samplesRef.current.push(bands);
      }

      if (elapsed >= total || !streaming) {
        if (intervalRef.current) clearInterval(intervalRef.current);
        setRecording(false);
        if (streaming) finalizeSnapshot();
      }
    }, 500);
  }, [streaming, eegData, finalizeSnapshot]);

  useEffect(() => {
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, []);

  const getColor = (value: number, inverted = false) => {
    const v = inverted ? 100 - value : value;
    if (v >= 70) return "text-success";
    if (v >= 40) return "text-warning";
    return "text-destructive";
  };

  return (
    <div className="panel p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-muted-foreground/70 uppercase tracking-wide">Brain Snapshot</h3>
        <button
          onClick={takeSnapshot}
          disabled={!streaming || recording}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs bg-accent/15 text-accent hover:bg-accent/25 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
        >
          {recording ? (
            <>
              <Loader2 size={10} className="animate-spin" />
              {Math.round(progress)}%
            </>
          ) : (
            <>
              <Camera size={10} />
              Take Snapshot
            </>
          )}
        </button>
      </div>

      {recording && (
        <div className="w-full h-1 rounded-full bg-muted/20 overflow-hidden">
          <div
            className="h-full bg-accent/60 transition-all duration-500"
            style={{ width: `${progress}%` }}
          />
        </div>
      )}

      {snapshot && !recording && (
        <div className="grid grid-cols-2 gap-2">
          <MetricCell
            label="Fatigue"
            value={snapshot.fatigue}
            unit=""
            colorClass={getColor(snapshot.fatigue)}
          />
          <MetricCell
            label="Engagement"
            value={snapshot.engagement}
            unit=""
            colorClass={getColor(snapshot.engagement)}
          />
          <MetricCell
            label="Alpha Peak"
            value={snapshot.alphaPeak}
            unit="Hz"
            colorClass="text-cyan-400"
          />
          <MetricCell
            label="Beta Balance"
            value={snapshot.betaBalance}
            unit="%"
            colorClass={getColor(snapshot.betaBalance, true)}
          />
        </div>
      )}

      {!snapshot && !recording && (
        <p className="text-[13px] text-muted-foreground/50 text-center py-3">
          30s recording to analyze brain state
        </p>
      )}
    </div>
  );
}

function MetricCell({
  label,
  value,
  unit,
  colorClass,
}: {
  label: string;
  value: number;
  unit: string;
  colorClass: string;
}) {
  return (
    <div className="panel p-2.5 rounded-lg text-center">
      <p className={`text-lg font-bold tabular-nums ${colorClass}`}>
        {value}
        {unit && <span className="text-xs font-normal ml-0.5">{unit}</span>}
      </p>
      <p className="text-[13px] text-muted-foreground/50">{label}</p>
    </div>
  );
}
