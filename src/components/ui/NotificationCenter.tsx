import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Bell, Check, Trash2, CheckCircle2, AlertCircle, Info, AlertTriangle } from "lucide-react";
import { cn } from "@/lib/utils";
import { useNexus } from "@/context/NexusContext";
import { formatRelativeTime } from "@/lib/utils";

const typeIcons = {
  success: CheckCircle2,
  error: AlertCircle,
  warning: AlertTriangle,
  info: Info,
};

const typeColors = {
  success: "text-success",
  error: "text-destructive",
  warning: "text-warning",
  info: "text-accent",
};

export function NotificationCenter() {
  const { notifications, markRead, markAllRead, clearNotifications } = useNexus();
  const unreadCount = notifications.filter((n) => !n.read).length;

  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button className="relative p-2.5 rounded-xl text-muted-foreground hover:text-foreground hover:bg-muted/50 transition-colors">
          <Bell size={16} />
          {unreadCount > 0 && (
            <span className="absolute -top-0.5 -right-0.5 w-4 h-4 rounded-full bg-accent text-[13px] font-bold text-white flex items-center justify-center">
              {unreadCount > 9 ? "9+" : unreadCount}
            </span>
          )}
        </button>
      </DropdownMenu.Trigger>

      <DropdownMenu.Portal>
        <DropdownMenu.Content
          side="bottom"
          align="end"
          sideOffset={8}
          className="w-[360px] max-h-[480px] glass-elevated rounded-xl border border-border/30 shadow-2xl z-[80] overflow-hidden"
        >
          <div className="flex items-center justify-between px-4 py-3 border-b border-border/20">
            <h3 className="text-base font-medium">Notifications</h3>
            <div className="flex items-center gap-2">
              {unreadCount > 0 && (
                <button
                  onClick={markAllRead}
                  className="text-xs text-accent hover:underline flex items-center gap-1"
                >
                  <Check size={10} /> Mark all read
                </button>
              )}
              {notifications.length > 0 && (
                <button
                  onClick={clearNotifications}
                  className="text-xs text-muted-foreground/50 hover:text-destructive flex items-center gap-1"
                >
                  <Trash2 size={10} /> Clear
                </button>
              )}
            </div>
          </div>

          <div className="overflow-y-auto max-h-[400px]">
            {notifications.length === 0 ? (
              <div className="py-12 text-center">
                <Bell size={24} className="text-muted-foreground/20 mx-auto mb-2" />
                <p className="text-xs text-muted-foreground/40">No notifications yet</p>
              </div>
            ) : (
              notifications.map((n) => {
                const Icon = typeIcons[n.type];
                return (
                  <DropdownMenu.Item
                    key={n.id}
                    onSelect={(e) => {
                      e.preventDefault();
                      markRead(n.id);
                    }}
                    className={cn(
                      "flex items-start gap-3 px-4 py-3 cursor-pointer outline-none transition-colors",
                      "hover:bg-muted/20 focus:bg-muted/20",
                      !n.read && "bg-accent/5",
                    )}
                  >
                    <Icon
                      size={14}
                      className={cn("mt-0.5 shrink-0", typeColors[n.type])}
                    />
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <p className={cn("text-xs", !n.read && "font-medium")}>
                          {n.title}
                        </p>
                        {!n.read && (
                          <div className="w-1.5 h-1.5 rounded-full bg-accent shrink-0" />
                        )}
                      </div>
                      <p className="text-[13px] text-muted-foreground/60 mt-0.5">
                        {n.message}
                      </p>
                      <p className="text-[13px] text-muted-foreground/30 mt-1">
                        {formatRelativeTime(n.timestamp)} · {n.source}
                      </p>
                    </div>
                  </DropdownMenu.Item>
                );
              })
            )}
          </div>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
