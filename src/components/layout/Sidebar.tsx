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

const easing = [0.4, 0, 0.2, 1];

export function Sidebar({ activePage, onNavigate }: SidebarProps) {
  const { museState, whoopState } = useNexus();
  const [expanded, setExpanded] = useState(false);

  const museConnected = museState.connected;
  const whoopConnected = whoopState.connected;

  return (
    <Tooltip.Provider delayDuration={300} skipDelayDuration={100}>
      <motion.aside
        animate={{ width: expanded ? 200 : 56 }}
        transition={{ duration: 0.3, ease: easing }}
        onMouseEnter={() => setExpanded(true)}
        onMouseLeave={() => setExpanded(false)}
        className="relative flex flex-col h-screen border-r border-border/55 overflow-hidden flex-shrink-0 bg-[#0C1018]"
        style={{ willChange: "width" }}
      >
        {/* Logo */}
        <div className="flex items-center h-[48px] px-3 flex-shrink-0 border-b border-border/35">
          <div className="w-7 h-7 rounded gradient-solar flex items-center justify-center flex-shrink-0">
            <span className="text-xs font-bold text-white/90 font-mono">N</span>
          </div>
          <AnimatePresence>
            {expanded && (
              <motion.div
                initial={{ opacity: 0, x: -6 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0, x: -6 }}
                transition={{ duration: 0.2, ease: easing }}
                className="ml-2.5 overflow-hidden whitespace-nowrap"
              >
                <p className="text-[11px] font-semibold font-mono uppercase tracking-[0.1em] text-foreground/85">Nexus</p>
                <p className="text-[10px] font-mono text-muted-foreground/55 tracking-wide">Command System</p>
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        {/* Navigation */}
        <nav className="flex flex-col gap-px flex-1 px-1.5 py-2">
          {navItems.map((item) => {
            const Icon = item.icon;
            const active = activePage === item.id;
            return (
              <Tooltip.Root key={item.id}>
                <Tooltip.Trigger asChild>
                  <button
                    onClick={() => onNavigate(item.id)}
                    className={cn(
                      "relative flex items-center gap-2.5 rounded text-xs transition-all duration-150 overflow-hidden",
                      "h-9 px-2.5",
                      active
                        ? "bg-accent/8 text-accent"
                        : "text-muted-foreground hover:text-foreground/85 hover:bg-muted/15",
                    )}
                  >
                    {/* Active indicator bar */}
                    {active && (
                      <motion.div
                        layoutId="active-bar"
                        className="absolute left-0 top-1/2 -translate-y-1/2 w-[2px] h-3.5 rounded-r-sm bg-accent"
                        transition={{ duration: 0.2, ease: easing }}
                      />
                    )}
                    <Icon
                      size={16}
                      className={cn("flex-shrink-0", active ? "text-accent" : "")}
                    />
                    <AnimatePresence>
                      {expanded && (
                        <motion.span
                          initial={{ opacity: 0, x: -4 }}
                          animate={{ opacity: 1, x: 0 }}
                          exit={{ opacity: 0, x: -4 }}
                          transition={{ duration: 0.2, ease: easing }}
                          className="whitespace-nowrap overflow-hidden font-mono text-[11px] font-medium uppercase tracking-wider"
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
                      className="panel-elevated px-2 py-1 text-[10px] font-mono font-medium text-foreground/85 select-none z-50 uppercase tracking-wider"
                    >
                      {item.label}
                      <Tooltip.Arrow className="fill-[#172030]" />
                    </Tooltip.Content>
                  </Tooltip.Portal>
                )}
              </Tooltip.Root>
            );
          })}
        </nav>

        {/* BLE Status */}
        <div className="px-1.5 py-1.5 border-t border-border/35 flex-shrink-0">
          {expanded ? (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.2 }}
              className="px-2 py-1.5 space-y-1.5"
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
            <div className="flex flex-col items-center gap-1 py-1.5">
              <div
                className={cn(
                  "w-1.5 h-1.5 rounded-full",
                  museConnected
                    ? museState.streaming
                      ? "bg-accent animate-pulse-dot"
                      : "bg-success"
                    : "bg-muted-foreground/35",
                )}
              />
              <div
                className={cn(
                  "w-1.5 h-1.5 rounded-full",
                  whoopConnected
                    ? whoopState.streaming
                      ? "bg-accent animate-pulse-dot"
                      : "bg-success"
                    : "bg-muted-foreground/35",
                )}
              />
            </div>
          )}
        </div>

        {/* Settings */}
        <div className="px-1.5 pb-1.5 flex-shrink-0">
          <Tooltip.Root>
            <Tooltip.Trigger asChild>
              <button
                onClick={() => onNavigate("settings")}
                className={cn(
                  "relative flex items-center gap-2.5 rounded text-xs transition-all duration-150 overflow-hidden",
                  "h-9 w-full px-2.5",
                  activePage === "settings"
                    ? "bg-accent/8 text-accent"
                    : "text-muted-foreground hover:text-foreground/85 hover:bg-muted/15",
                )}
              >
                {activePage === "settings" && (
                  <motion.div
                    layoutId="active-bar"
                    className="absolute left-0 top-1/2 -translate-y-1/2 w-[2px] h-3.5 rounded-r-sm bg-accent"
                    transition={{ duration: 0.2, ease: easing }}
                  />
                )}
                <Settings size={16} className="flex-shrink-0" />
                <AnimatePresence>
                  {expanded && (
                    <motion.span
                      initial={{ opacity: 0, x: -4 }}
                      animate={{ opacity: 1, x: 0 }}
                      exit={{ opacity: 0, x: -4 }}
                      transition={{ duration: 0.2, ease: easing }}
                      className="whitespace-nowrap overflow-hidden font-mono text-[11px] font-medium uppercase tracking-wider"
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
                  className="panel-elevated px-2 py-1 text-[10px] font-mono font-medium text-foreground/85 select-none z-50 uppercase tracking-wider"
                >
                  Settings
                  <Tooltip.Arrow className="fill-[#172030]" />
                </Tooltip.Content>
              </Tooltip.Portal>
            )}
          </Tooltip.Root>

          {/* Version */}
          <AnimatePresence>
            {expanded && (
              <motion.p
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.2 }}
                className="px-2.5 pt-1.5 text-[10px] font-mono text-muted-foreground/40 whitespace-nowrap tracking-wider"
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
