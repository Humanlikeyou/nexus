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

  const changeDate = (delta: number) => {
    const d = new Date(date);
    d.setDate(d.getDate() + delta);
    setDate(d.toISOString().split("T")[0]);
  };

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <FileText size={16} className="text-accent" />
          <h3 className="text-base font-medium text-muted-foreground">Daily Log</h3>
        </div>

        <div className="flex items-center gap-2">
          <button
            onClick={() => changeDate(-1)}
            className="p-1 rounded hover:bg-muted/50 text-muted-foreground"
          >
            <ChevronLeft size={14} />
          </button>
          <span className="text-xs font-mono text-muted-foreground">{date}</span>
          <button
            onClick={() => changeDate(1)}
            className="p-1 rounded hover:bg-muted/50 text-muted-foreground"
          >
            <ChevronRight size={14} />
          </button>
        </div>
      </div>

      {loading ? (
        <div className="space-y-2">
          <div className="h-4 bg-muted/30 rounded animate-pulse w-3/4" />
          <div className="h-4 bg-muted/30 rounded animate-pulse w-full" />
          <div className="h-4 bg-muted/30 rounded animate-pulse w-5/6" />
        </div>
      ) : (
        <div className="max-h-[300px] overflow-y-auto">
          <pre className="text-xs text-muted-foreground/80 whitespace-pre-wrap font-mono leading-relaxed">
            {content}
          </pre>
        </div>
      )}
    </div>
  );
}
