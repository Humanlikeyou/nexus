import { Mail, Inbox } from "lucide-react";

export function EmailSummary({ placeholder }: { placeholder?: boolean }) {
  // Placeholder — will connect via gog CLI or Gmail API
  const emails = [
    { subject: "TaiyoSkin supplier update", from: "supplier@example.com", time: "2h ago" },
    { subject: "Weekly metrics digest", from: "analytics@example.com", time: "4h ago" },
    { subject: "Content collaboration request", from: "creator@example.com", time: "6h ago" },
  ];

  return (
    <div className="glass p-6 space-y-5 relative">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Mail size={16} className="text-accent" />
          <h3 className="text-base font-medium text-muted-foreground">Email</h3>
        </div>
        <div className="flex items-center gap-1.5 px-2 py-0.5 rounded-full bg-accent/15">
          <Inbox size={10} className="text-accent" />
          <span className="text-xs font-medium text-accent">{emails.length}</span>
        </div>
      </div>

      <div className="space-y-2">
        {emails.map((email, i) => (
          <div
            key={i}
            className="px-3 py-2.5 rounded-lg bg-muted/15 border border-border/15"
          >
            <p className="text-xs font-medium truncate">{email.subject}</p>
            <div className="flex items-center justify-between mt-1">
              <span className="text-xs text-muted-foreground/60 truncate">
                {email.from}
              </span>
              <span className="text-xs text-muted-foreground/40 shrink-0 ml-2">
                {email.time}
              </span>
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
