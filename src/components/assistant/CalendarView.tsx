import { Calendar, Clock } from "lucide-react";

interface CalendarEvent {
  time: string;
  title: string;
  duration: string;
}

export function CalendarView({ placeholder }: { placeholder?: boolean }) {
  // Placeholder events — will connect to Google Calendar via gog CLI
  const events: CalendarEvent[] = [
    { time: "9:00 AM", title: "Morning standup", duration: "15m" },
    { time: "11:00 AM", title: "Content review session", duration: "30m" },
    { time: "2:00 PM", title: "Product photography", duration: "2h" },
    { time: "5:00 PM", title: "Evening planning", duration: "30m" },
  ];

  return (
    <div className="glass p-6 space-y-5 relative">
      <div className="flex items-center gap-2">
        <Calendar size={16} className="text-accent" />
        <h3 className="text-base font-medium text-muted-foreground">Today's Schedule</h3>
      </div>

      <div className="space-y-2">
        {events.map((event, i) => (
          <div
            key={i}
            className="flex items-start gap-3 px-3 py-2.5 rounded-lg bg-muted/15 border border-border/15"
          >
            <div className="text-xs text-muted-foreground font-mono w-16 shrink-0 pt-0.5">
              {event.time}
            </div>
            <div className="flex-1 min-w-0">
              <p className="text-sm truncate">{event.title}</p>
              <div className="flex items-center gap-1 mt-0.5">
                <Clock size={10} className="text-muted-foreground/50" />
                <span className="text-xs text-muted-foreground/50">
                  {event.duration}
                </span>
              </div>
            </div>
          </div>
        ))}
      </div>

      {placeholder && (
        <div className="absolute inset-0 flex items-center justify-center bg-background/60 backdrop-blur-sm rounded-lg">
          <span className="text-xs text-muted-foreground/60 font-medium px-3 py-1.5 rounded-full bg-muted/20 border border-border/20">
            Not wired yet
          </span>
        </div>
      )}
    </div>
  );
}
