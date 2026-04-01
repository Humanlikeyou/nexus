import { useState, useEffect, useRef } from "react";
import {
  Play,
  Square,
  Circle,
  Radio,
  FileAudio,
  ChevronDown,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { ReplayMode, ReplayFileInfo } from "@/lib/types";

interface ReplayControlsProps {
  mode: ReplayMode;
  streaming: boolean;
  replayFiles: ReplayFileInfo[];
  speed: number;
  activeFilename: string | null;
  activeDemoMode: string | null;
  recordingStartedAt: number | null;
  onStartRecording: () => void;
  onStopRecording: () => void;
  onStartReplay: (filename: string, speed: number) => void;
  onStopReplay: () => void;
  onStartDemo: (mode: string, speed: number) => void;
  onStopDemo: () => void;
  onSetSpeed: (speed: number) => void;
  onRefreshFiles: () => void;
}

const SPEED_OPTIONS = [0.5, 1, 2, 4];
const DEMO_MODES = [
  { id: "eyes_closed", label: "Eyes Closed" },
  { id: "focus", label: "Focus" },
  { id: "mixed", label: "Mixed" },
];

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDuration(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = Math.round(secs % 60);
  return m > 0 ? `${m}m ${s}s` : `${s}s`;
}

function ElapsedTimer({ startedAt }: { startedAt: number }) {
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    const interval = setInterval(() => {
      setElapsed(Math.floor((Date.now() - startedAt) / 1000));
    }, 1000);
    return () => clearInterval(interval);
  }, [startedAt]);
  return <span className="tabular-nums">{formatDuration(elapsed)}</span>;
}

export function ReplayControls({
  mode,
  streaming,
  replayFiles,
  speed,
  activeFilename,
  activeDemoMode,
  recordingStartedAt,
  onStartRecording,
  onStopRecording,
  onStartReplay,
  onStopReplay,
  onStartDemo,
  onStopDemo,
  onSetSpeed,
  onRefreshFiles,
}: ReplayControlsProps) {
  const [showFilePicker, setShowFilePicker] = useState(false);
  const [showDemoMenu, setShowDemoMenu] = useState(false);
  const filePickerRef = useRef<HTMLDivElement>(null);
  const demoMenuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!showFilePicker && !showDemoMenu) return;
    const handleClick = (e: MouseEvent) => {
      if (showFilePicker && filePickerRef.current && !filePickerRef.current.contains(e.target as Node)) {
        setShowFilePicker(false);
      }
      if (showDemoMenu && demoMenuRef.current && !demoMenuRef.current.contains(e.target as Node)) {
        setShowDemoMenu(false);
      }
    };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [showFilePicker, showDemoMenu]);

  // Recording state
  if (mode === "recording") {
    return (
      <div className="panel p-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <Circle size={10} className="fill-red-400 text-red-400 animate-pulse" />
            <span className="text-sm font-medium text-red-400">Recording</span>
            {recordingStartedAt && (
              <span className="text-xs text-muted-foreground">
                <ElapsedTimer startedAt={recordingStartedAt} />
              </span>
            )}
          </div>
          <button
            onClick={onStopRecording}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-red-500/15 text-red-400 text-xs hover:bg-red-500/25 transition-colors"
          >
            <Square size={10} /> Stop Recording
          </button>
        </div>
      </div>
    );
  }

  // Replaying state
  if (mode === "replaying") {
    return (
      <div className="panel p-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="px-2 py-0.5 rounded bg-yellow-500/15 text-yellow-400 text-xs font-semibold uppercase tracking-wide">
              Replay
            </div>
            <span className="text-xs text-muted-foreground truncate max-w-[200px]">
              {activeFilename}
            </span>
            <span className="text-xs text-muted-foreground/60">{speed}x</span>
          </div>
          <button
            onClick={onStopReplay}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-destructive/15 text-destructive text-xs hover:bg-destructive/25 transition-colors"
          >
            <Square size={10} /> Stop
          </button>
        </div>
      </div>
    );
  }

  // Demo state
  if (mode === "demo") {
    return (
      <div className="panel p-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="px-2 py-0.5 rounded bg-blue-500/15 text-blue-400 text-xs font-semibold uppercase tracking-wide">
              Demo
            </div>
            <span className="text-xs text-muted-foreground capitalize">
              {activeDemoMode?.replace("_", " ")}
            </span>
            <span className="text-xs text-muted-foreground/60">{speed}x</span>
          </div>
          <div className="flex items-center gap-2">
            {/* Allow recording during demo */}
            <button
              onClick={onStartRecording}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-muted/20 text-muted-foreground text-xs hover:bg-muted/30 transition-colors"
            >
              <Circle size={8} /> Record
            </button>
            <button
              onClick={onStopDemo}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-destructive/15 text-destructive text-xs hover:bg-destructive/25 transition-colors"
            >
              <Square size={10} /> Stop
            </button>
          </div>
        </div>
      </div>
    );
  }

  // Idle state — show controls
  return (
    <div className="panel p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
          EEG Source
        </h3>
        {/* Speed selector */}
        <div className="flex items-center gap-1">
          {SPEED_OPTIONS.map((s) => (
            <button
              key={s}
              onClick={() => onSetSpeed(s)}
              className={cn(
                "px-2 py-0.5 rounded text-xs transition-colors",
                speed === s
                  ? "bg-accent/20 text-accent"
                  : "text-muted-foreground/50 hover:text-muted-foreground",
              )}
            >
              {s}x
            </button>
          ))}
        </div>
      </div>

      <div className="flex items-center gap-2">
        {/* Record button — only when live streaming */}
        {streaming && (
          <button
            onClick={onStartRecording}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-red-500/10 text-red-400 text-xs hover:bg-red-500/20 transition-colors"
          >
            <Circle size={8} /> Record
          </button>
        )}

        {/* Replay button */}
        <div className="relative" ref={filePickerRef}>
          <button
            onClick={() => {
              onRefreshFiles();
              setShowFilePicker(!showFilePicker);
              setShowDemoMenu(false);
            }}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-yellow-500/10 text-yellow-400 text-xs hover:bg-yellow-500/20 transition-colors"
          >
            <FileAudio size={12} /> Replay
            <ChevronDown size={10} />
          </button>

          {showFilePicker && (
            <div className="absolute top-full left-0 mt-1 w-72 rounded-lg border border-border/20 bg-background/95 backdrop-blur-sm shadow-lg z-50 max-h-48 overflow-y-auto">
              {replayFiles.length === 0 ? (
                <div className="p-3 text-xs text-muted-foreground/50 text-center">
                  No recordings yet
                </div>
              ) : (
                replayFiles.map((file) => (
                  <button
                    key={file.filename}
                    onClick={() => {
                      setShowFilePicker(false);
                      onStartReplay(file.filename, speed);
                    }}
                    className="w-full flex items-center justify-between px-3 py-2 hover:bg-muted/20 transition-colors text-left"
                  >
                    <div className="flex flex-col gap-0.5">
                      <span className="text-xs truncate max-w-[180px]">
                        {file.filename}
                      </span>
                      <span className="text-xs text-muted-foreground/50">
                        {formatBytes(file.size_bytes)}
                        {file.duration_secs != null &&
                          ` · ${formatDuration(file.duration_secs)}`}
                      </span>
                    </div>
                    <Play size={10} className="text-muted-foreground/60" />
                  </button>
                ))
              )}
            </div>
          )}
        </div>

        {/* Demo button */}
        <div className="relative" ref={demoMenuRef}>
          <button
            onClick={() => {
              setShowDemoMenu(!showDemoMenu);
              setShowFilePicker(false);
            }}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-blue-500/10 text-blue-400 text-xs hover:bg-blue-500/20 transition-colors"
          >
            <Radio size={12} /> Demo
            <ChevronDown size={10} />
          </button>

          {showDemoMenu && (
            <div className="absolute top-full left-0 mt-1 w-44 rounded-lg border border-border/20 bg-background/95 backdrop-blur-sm shadow-lg z-50">
              {DEMO_MODES.map((dm) => (
                <button
                  key={dm.id}
                  onClick={() => {
                    setShowDemoMenu(false);
                    onStartDemo(dm.id, speed);
                  }}
                  className="w-full flex items-center gap-2 px-3 py-2 hover:bg-muted/20 transition-colors text-left"
                >
                  <Play size={10} className="text-blue-400/60" />
                  <span className="text-xs">{dm.label}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
