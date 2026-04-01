import { Settings, RefreshCw, Sun, Moon } from "lucide-react";
import { cn } from "@/lib/utils";
import { useNexus } from "@/context/NexusContext";
import { useTheme } from "@/context/ThemeContext";
import type { PageId } from "@/lib/types";

const pageTitles: Record<PageId, string> = {
  terrance: "Terrance System",
  business: "Business — TaiyoSkin",
  revenue: "Autonomous Revenue",
  assistant: "Personal Assistant",
  neuro: "Neurofeedback — Muse",
  fitness: "Fitness & Recovery — Whoop",
  trajectory: "Life Trajectory",
  settings: "Settings",
};

const pageDescriptions: Record<PageId, string> = {
  terrance: "System status, cron jobs, token usage, and active skills",
  business: "Task board, content pipeline, and revenue metrics",
  revenue: "Strategy management, revenue tracking, and risk controls",
  assistant: "Calendar, email, reminders, and daily logs",
  neuro: "Real-time EEG, frequency bands, and focus scoring",
  fitness: "Heart rate, HRV, recovery, and strain tracking",
  trajectory: "Goals, wellness score, and weekly review",
  settings: "Telegram, data paths, and portable backup",
};

interface HeaderProps {
  activePage: PageId;
  terranceOnline?: boolean;
  onRefresh?: () => void;
}

export function Header({ activePage, terranceOnline = false, onRefresh }: HeaderProps) {
  const { navigate } = useNexus();
  const { resolved, setMode } = useTheme();

  return (
    <header className="flex items-center justify-between px-4 py-3 border-b border-border/55">
      <div>
        <div className="flex items-center gap-3">
          <h2 className="text-xs font-semibold uppercase font-mono tracking-[0.12em] text-foreground/90">
            {pageTitles[activePage]}
          </h2>
          {activePage === "terrance" && (
            <div className="flex items-center gap-1.5">
              <div
                className={cn(
                  "w-1.5 h-1.5 rounded-full",
                  terranceOnline
                    ? "bg-success animate-status-glow"
                    : "bg-muted-foreground/35",
                )}
              />
              <span className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground">
                {terranceOnline ? "Online" : "Offline"}
              </span>
            </div>
          )}
        </div>
        <p className="text-[10px] font-mono tracking-wide text-muted-foreground/65 mt-0.5">
          {pageDescriptions[activePage]}
        </p>
      </div>

      <div className="flex items-center gap-1.5">
        {onRefresh && (
          <button
            onClick={onRefresh}
            className="p-2 rounded text-muted-foreground/65 hover:text-foreground/80 hover:bg-muted/25 transition-colors"
          >
            <RefreshCw size={14} />
          </button>
        )}
        <button
          onClick={() => setMode(resolved === "dark" ? "light" : "dark")}
          className="p-2 rounded text-muted-foreground/65 hover:text-foreground/80 hover:bg-muted/25 transition-colors"
          title={resolved === "dark" ? "Switch to light mode" : "Switch to dark mode"}
        >
          {resolved === "dark" ? <Sun size={14} /> : <Moon size={14} />}
        </button>
        <button
          onClick={() => navigate("settings")}
          className={cn(
            "p-2 rounded transition-colors",
            activePage === "settings"
              ? "text-accent bg-accent/8"
              : "text-muted-foreground/65 hover:text-foreground/80 hover:bg-muted/25",
          )}
        >
          <Settings size={14} />
        </button>
      </div>
    </header>
  );
}
