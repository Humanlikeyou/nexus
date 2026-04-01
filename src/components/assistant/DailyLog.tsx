import { useState, useEffect } from "react";
import { FileText, ChevronLeft, ChevronRight } from "lucide-react";
import { readDailyLog } from "@/lib/api";

export function DailyLog() {
  const [date, setDate] = useState(() => {
    const d = new Date();
    return d.toISOString().split("T")[0];
  });
  const [content, setContent] = useState<string>("");
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    readDailyLog(date)
      .then(setContent)
      .catch(() => setContent("Failed to load daily log."))
      .finally(() => setLoading(false));
  }, [date]);

  const today = new Date().toISOString().split("T")[0];

  const changeDate = (delta: number) => {
    const d = new Date(date);
    d.setDate(d.getDate() + delta);
    setDate(d.toISOString().split("T")[0]);
  };

  return (
    <div className="panel p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="heading-section">Daily Log</h3>
        <div className="flex items-center gap-1.5">
          <button
            onClick={() => changeDate(-1)}
            className="p-1 rounded-sm hover:bg-muted/10 text-muted-foreground/50 transition-colors"
          >
            <ChevronLeft size={12} />
          </button>
          <span className="text-[10px] font-mono text-muted-foreground/60">{date}</span>
          <button
            onClick={() => changeDate(1)}
            disabled={date >= today}
            className="p-1 rounded-sm hover:bg-muted/10 text-muted-foreground/50 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          >
            <ChevronRight size={12} />
          </button>
        </div>
      </div>

      {loading ? (
        <div className="space-y-1.5">
          <div className="h-3 bg-muted/10 rounded-sm animate-pulse w-3/4" />
          <div className="h-3 bg-muted/10 rounded-sm animate-pulse w-full" />
          <div className="h-3 bg-muted/10 rounded-sm animate-pulse w-5/6" />
        </div>
      ) : (
        <div className="terminal-readout max-h-[280px] overflow-y-auto p-3">
          <pre className="whitespace-pre-wrap break-words">
            {content}
          </pre>
        </div>
      )}
    </div>
  );
}
