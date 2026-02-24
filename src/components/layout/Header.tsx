import { Settings, RefreshCw, Sun, Moon } from "lucide-react";
import { cn } from "@/lib/utils";
import { useNexus } from "@/context/NexusContext";
import { useTheme } from "@/context/ThemeContext";
import { NotificationCenter } from "@/components/ui/NotificationCenter";
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
    <header className="glass-subtle flex items-center justify-between px-7 py-5 border-b border-border/20">
      <div>
        <div className="flex items-center gap-3">
          <h2 className="text-xl font-semibold tracking-tight">
            {pageTitles[activePage]}
          </h2>
          {activePage === "terrance" && (
            <div className="flex items-center gap-1.5">
              <div
                className={cn(
                  "w-2 h-2 rounded-full",
                  terranceOnline
                    ? "bg-success animate-pulse-dot"
                    : "bg-muted-foreground/30",
                )}
              />
              <span className="text-xs text-muted-foreground">
                {terranceOnline ? "Online" : "Offline"}
              </span>
            </div>
          )}
        </div>
        <p className="text-sm text-muted-foreground mt-0.5">
          {pageDescriptions[activePage]}
        </p>
      </div>

      <div className="flex items-center gap-3">
        {onRefresh && (
          <button
            onClick={onRefresh}
            className="p-2.5 rounded-xl text-muted-foreground hover:text-foreground hover:bg-muted/50 transition-colors"
          >
            <RefreshCw size={16} />
          </button>
        )}
        <NotificationCenter />
        <button
          onClick={() => setMode(resolved === "dark" ? "light" : "dark")}
          className="p-2.5 rounded-xl text-muted-foreground hover:text-foreground hover:bg-muted/50 transition-colors"
          title={resolved === "dark" ? "Switch to light mode" : "Switch to dark mode"}
        >
          {resolved === "dark" ? <Sun size={16} /> : <Moon size={16} />}
        </button>
        <button
          onClick={() => navigate("settings")}
          className={cn(
            "p-2.5 rounded-xl transition-colors",
            activePage === "settings"
              ? "text-accent bg-accent/10"
              : "text-muted-foreground hover:text-foreground hover:bg-muted/50",
          )}
        >
          <Settings size={16} />
        </button>
      </div>
    </header>
  );
}
