import { useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { X, CheckCircle2, AlertCircle, Info, AlertTriangle } from "lucide-react";
import { cn } from "@/lib/utils";
import { useNexus } from "@/context/NexusContext";

const icons = {
  success: CheckCircle2,
  error: AlertCircle,
  warning: AlertTriangle,
  info: Info,
};

const borderColors = {
  success: "border-l-success",
  error: "border-l-destructive",
  warning: "border-l-warning",
  info: "border-l-accent",
};

const iconColors = {
  success: "text-success",
  error: "text-destructive",
  warning: "text-warning",
  info: "text-accent",
};

export function ToastContainer() {
  const { toasts, dismissToast } = useNexus();

  const handleMouseEnter = useCallback(
    (_id: string) => {
      // Pause auto-dismiss is handled by the context timer
      // For simplicity we just let hovering work as-is
    },
    [],
  );

  return (
    <div className="fixed bottom-4 right-4 z-[100] flex flex-col gap-2 w-[320px]">
      <AnimatePresence mode="popLayout">
        {toasts.map((t) => {
          const Icon = icons[t.type];
          return (
            <motion.div
              key={t.id}
              layout
              initial={{ opacity: 0, x: 80, scale: 0.95 }}
              animate={{ opacity: 1, x: 0, scale: 1 }}
              exit={{ opacity: 0, x: 80, scale: 0.95 }}
              transition={{ duration: 0.25, ease: "easeOut" }}
              onMouseEnter={() => handleMouseEnter(t.id)}
              className={cn(
                "glass-subtle rounded-lg border-l-[3px] px-4 py-3 shadow-lg",
                borderColors[t.type],
              )}
            >
              <div className="flex items-start gap-3">
                <Icon size={16} className={cn("mt-0.5 shrink-0", iconColors[t.type])} />
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium">{t.title}</p>
                  {t.description && (
                    <p className="text-xs text-muted-foreground mt-0.5">
                      {t.description}
                    </p>
                  )}
                  {t.action && (
                    <button
                      onClick={t.action.onClick}
                      className="text-xs text-accent hover:underline mt-1"
                    >
                      {t.action.label}
                    </button>
                  )}
                </div>
                <button
                  onClick={() => dismissToast(t.id)}
                  className="text-muted-foreground/50 hover:text-foreground transition-colors shrink-0"
                >
                  <X size={14} />
                </button>
              </div>
            </motion.div>
          );
        })}
      </AnimatePresence>
    </div>
  );
}
