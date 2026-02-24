import { useState } from "react";
import {
  Brain,
  Briefcase,
  Activity,
  User,
  Target,
  Cpu,
  Settings,
  Wallet,
  type LucideIcon,
} from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import * as Tooltip from "@radix-ui/react-tooltip";
import { cn } from "@/lib/utils";
import { useNexus } from "@/context/NexusContext";
import { BleStatusDot } from "@/components/ui/BleStatusDot";
import type { PageId } from "@/lib/types";

interface NavItem {
  id: PageId;
  label: string;
  icon: LucideIcon;
}

const navItems: NavItem[] = [
  { id: "terrance", label: "Terrance", icon: Cpu },
  { id: "business", label: "Business", icon: Briefcase },
  { id: "revenue", label: "Revenue", icon: Wallet },
  { id: "assistant", label: "Assistant", icon: User },
  { id: "neuro", label: "Neuro", icon: Brain },
  { id: "fitness", label: "Fitness", icon: Activity },
  { id: "trajectory", label: "Trajectory", icon: Target },
];

interface SidebarProps {
  activePage: PageId;
  onNavigate: (page: PageId) => void;
}

const easing = [0.25, 0.1, 0.25, 1];

export function Sidebar({ activePage, onNavigate }: SidebarProps) {
  const { museState, whoopState } = useNexus();
  const [expanded, setExpanded] = useState(false);

  const museConnected = museState.connected;
  const whoopConnected = whoopState.connected;

  return (
    <Tooltip.Provider delayDuration={300} skipDelayDuration={100}>
      <motion.aside
        animate={{ width: expanded ? 220 : 64 }}
        transition={{ duration: 0.25, ease: easing }}
        onMouseEnter={() => setExpanded(true)}
        onMouseLeave={() => setExpanded(false)}
        className="relative flex flex-col h-screen glass border-r border-border/30 overflow-hidden flex-shrink-0"
        style={{ willChange: "width" }}
      >
        {/* Logo */}
        <div className="flex items-center h-[60px] px-4 flex-shrink-0">
          <div className="w-8 h-8 rounded-lg gradient-accent-vivid flex items-center justify-center flex-shrink-0">
            <span className="text-sm font-bold text-white">N</span>
          </div>
          <AnimatePresence>
            {expanded && (
              <motion.div
                initial={{ opacity: 0, x: -8 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0, x: -8 }}
                transition={{ duration: 0.18, ease: easing }}
                className="ml-3 overflow-hidden whitespace-nowrap"
              >
                <p className="text-sm font-semibold tracking-tight leading-none">Nexus</p>
                <p className="text-[13px] text-muted-foreground mt-0.5">Command Center</p>
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        {/* Navigation */}
        <nav className="flex flex-col gap-0.5 flex-1 px-2 py-2">
          {navItems.map((item) => {
            const Icon = item.icon;
            const active = activePage === item.id;
            return (
              <Tooltip.Root key={item.id}>
                <Tooltip.Trigger asChild>
                  <button
                    onClick={() => onNavigate(item.id)}
                    className={cn(
                      "relative flex items-center gap-3 rounded-xl text-sm transition-all duration-200 overflow-hidden",
                      "h-10 px-3",
                      active
                        ? "bg-accent/12 text-accent"
                        : "text-muted-foreground hover:text-foreground hover:bg-muted/40",
                      active && "animate-glow-pulse",
                    )}
                  >
                    {/* Active indicator bar */}
                    {active && (
                      <motion.div
                        layoutId="active-bar"
                        className="absolute left-0 top-1/2 -translate-y-1/2 w-[3px] h-5 rounded-r-full bg-accent"
                        transition={{ duration: 0.22, ease: easing }}
                      />
                    )}
                    <Icon
                      size={20}
                      className={cn("flex-shrink-0", active ? "text-accent" : "")}
                    />
                    <AnimatePresence>
                      {expanded && (
                        <motion.span
                          initial={{ opacity: 0, x: -6 }}
                          animate={{ opacity: 1, x: 0 }}
                          exit={{ opacity: 0, x: -6 }}
                          transition={{ duration: 0.16, ease: easing }}
                          className="whitespace-nowrap overflow-hidden font-medium"
                        >
                          {item.label}
                        </motion.span>
                      )}
                    </AnimatePresence>
                  </button>
                </Tooltip.Trigger>
                {!expanded && (
                  <Tooltip.Portal>
                    <Tooltip.Content
                      side="right"
                      sideOffset={8}
                      className="glass-elevated px-2.5 py-1.5 text-xs font-medium text-foreground select-none z-50"
                    >
                      {item.label}
                      <Tooltip.Arrow className="fill-card" />
                    </Tooltip.Content>
                  </Tooltip.Portal>
                )}
              </Tooltip.Root>
            );
          })}
        </nav>

        {/* BLE Status */}
        <div className="px-2 py-2 border-t border-border/20 flex-shrink-0">
          {expanded ? (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.16 }}
              className="px-2 py-2 space-y-2"
            >
              <BleStatusDot
                connected={museConnected}
                streaming={museState.streaming}
                label={museConnected ? (museState.device_name || "Muse") : "Muse"}
              />
              <BleStatusDot
                connected={whoopConnected}
                streaming={whoopState.streaming}
                label={whoopConnected ? (whoopState.device_name || "Whoop") : "Whoop"}
              />
            </motion.div>
          ) : (
            <div className="flex flex-col items-center gap-1.5 py-2">
              {/* Muse dot */}
              <div
                className={cn(
                  "w-1.5 h-1.5 rounded-full",
                  museConnected
                    ? museState.streaming
                      ? "bg-accent animate-pulse-dot"
                      : "bg-success"
                    : "bg-muted-foreground/25",
                )}
              />
              {/* Whoop dot */}
              <div
                className={cn(
                  "w-1.5 h-1.5 rounded-full",
                  whoopConnected
                    ? whoopState.streaming
                      ? "bg-accent animate-pulse-dot"
                      : "bg-success"
                    : "bg-muted-foreground/25",
                )}
              />
            </div>
          )}
        </div>

        {/* Settings */}
        <div className="px-2 pb-2 flex-shrink-0">
          <Tooltip.Root>
            <Tooltip.Trigger asChild>
              <button
                onClick={() => onNavigate("settings")}
                className={cn(
                  "relative flex items-center gap-3 rounded-xl text-sm transition-all duration-200 overflow-hidden",
                  "h-10 w-full px-3",
                  activePage === "settings"
                    ? "bg-accent/12 text-accent animate-glow-pulse"
                    : "text-muted-foreground hover:text-foreground hover:bg-muted/40",
                )}
              >
                {activePage === "settings" && (
                  <motion.div
                    layoutId="active-bar"
                    className="absolute left-0 top-1/2 -translate-y-1/2 w-[3px] h-5 rounded-r-full bg-accent"
                    transition={{ duration: 0.22, ease: easing }}
                  />
                )}
                <Settings size={20} className="flex-shrink-0" />
                <AnimatePresence>
                  {expanded && (
                    <motion.span
                      initial={{ opacity: 0, x: -6 }}
                      animate={{ opacity: 1, x: 0 }}
                      exit={{ opacity: 0, x: -6 }}
                      transition={{ duration: 0.16, ease: easing }}
                      className="whitespace-nowrap overflow-hidden font-medium"
                    >
                      Settings
                    </motion.span>
                  )}
                </AnimatePresence>
              </button>
            </Tooltip.Trigger>
            {!expanded && (
              <Tooltip.Portal>
                <Tooltip.Content
                  side="right"
                  sideOffset={8}
                  className="glass-elevated px-2.5 py-1.5 text-xs font-medium text-foreground select-none z-50"
                >
                  Settings
                  <Tooltip.Arrow className="fill-card" />
                </Tooltip.Content>
              </Tooltip.Portal>
            )}
          </Tooltip.Root>

          {/* Version — expanded only */}
          <AnimatePresence>
            {expanded && (
              <motion.p
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.16 }}
                className="px-3 pt-2 text-[13px] text-muted-foreground/40 whitespace-nowrap"
              >
                v0.1.0
              </motion.p>
            )}
          </AnimatePresence>
        </div>
      </motion.aside>
    </Tooltip.Provider>
  );
}
