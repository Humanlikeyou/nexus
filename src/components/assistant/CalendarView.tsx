import { Calendar } from "lucide-react";

export function CalendarView() {
  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center gap-2">
        <Calendar size={16} className="text-accent" />
        <h3 className="text-base font-medium text-muted-foreground">Today's Schedule</h3>
      </div>

      <div className="py-8 flex flex-col items-center justify-center gap-2">
        <Calendar size={28} className="text-muted-foreground/20" />
        <p className="text-xs text-muted-foreground/50 text-center">
          Google Calendar not connected
        </p>
        <p className="text-[11px] text-muted-foreground/30 text-center">
          Configure OAuth credentials in Settings to see your schedule here
        </p>
      </div>
    </div>
  );
}
