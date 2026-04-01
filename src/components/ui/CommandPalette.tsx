import { useState, useEffect, useRef, useMemo } from "react";
import { motion, AnimatePresence } from "framer-motion";
import * as Dialog from "@radix-ui/react-dialog";
import {
  Search,
  Cpu,
  Briefcase,
  User,
  Brain,
  Activity,
  Target,
  Settings,
  Wallet,
  BluetoothSearching,
  Package,
  RefreshCw,
  Command,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useNexus } from "@/context/NexusContext";
import type { PageId } from "@/lib/types";

interface PaletteCommand {
  id: string;
  label: string;
  icon: typeof Search;
  shortcut?: string;
  action: () => void;
  category: "Navigation" | "Actions" | "Devices";
}

const pageIcons: Record<PageId, typeof Search> = {
  terrance: Cpu,
  business: Briefcase,
  revenue: Wallet,
  assistant: User,
  neuro: Brain,
  fitness: Activity,
  trajectory: Target,
  settings: Settings,
};

const pageLabels: Record<PageId, string> = {
  terrance: "Terrance System",
  business: "Business",
  revenue: "Autonomous Revenue",
  assistant: "Personal Assistant",
  neuro: "Neurofeedback",
  fitness: "Fitness & Recovery",
  trajectory: "Life Trajectory",
  settings: "Settings",
};

export function CommandPalette() {
  const { commandPaletteOpen, setCommandPaletteOpen, navigate } = useNexus();
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const commands = useMemo<PaletteCommand[]>(() => {
    const pages: PageId[] = [
      "terrance",
      "business",
      "revenue",
      "assistant",
      "neuro",
      "fitness",
      "trajectory",
      "settings",
    ];

    const navCommands: PaletteCommand[] = pages.map((page, i) => ({
      id: `nav-${page}`,
      label: pageLabels[page],
      icon: pageIcons[page],
      shortcut: page === "settings" ? "\u2318," : `\u2318${i + 1}`,
      action: () => {
        navigate(page);
        setCommandPaletteOpen(false);
      },
      category: "Navigation" as const,
    }));

    const actionCommands: PaletteCommand[] = [
      {
        id: "action-refresh",
        label: "Refresh Data",
        icon: RefreshCw,
        shortcut: "\u2318R",
        action: () => {
          setCommandPaletteOpen(false);
          window.dispatchEvent(new CustomEvent("nexus:refresh"));
        },
        category: "Actions",
      },
      {
        id: "action-pack",
        label: "Pack It Up (Backup)",
        icon: Package,
        action: () => {
          navigate("settings");
          setCommandPaletteOpen(false);
        },
        category: "Actions",
      },
    ];

    const deviceCommands: PaletteCommand[] = [
      {
        id: "device-muse",
        label: "Scan for Muse Headband",
        icon: BluetoothSearching,
        action: () => {
          navigate("neuro");
          setCommandPaletteOpen(false);
        },
        category: "Devices",
      },
      {
        id: "device-whoop",
        label: "Scan for Whoop Band",
        icon: BluetoothSearching,
        action: () => {
          navigate("fitness");
          setCommandPaletteOpen(false);
        },
        category: "Devices",
      },
    ];

    return [...navCommands, ...actionCommands, ...deviceCommands];
  }, [navigate, setCommandPaletteOpen]);

  const filtered = useMemo(() => {
    if (!query.trim()) return commands;
    const q = query.toLowerCase();
    return commands.filter(
      (cmd) =>
        cmd.label.toLowerCase().includes(q) ||
        cmd.category.toLowerCase().includes(q),
    );
  }, [commands, query]);

  const grouped = useMemo(() => {
    const groups: Record<string, PaletteCommand[]> = {};
    for (const cmd of filtered) {
      (groups[cmd.category] ??= []).push(cmd);
    }
    return groups;
  }, [filtered]);

  useEffect(() => {
    if (commandPaletteOpen) {
      setQuery("");
      setSelectedIndex(0);
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  }, [commandPaletteOpen]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((i) => Math.min(i + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter" && filtered[selectedIndex]) {
      e.preventDefault();
      filtered[selectedIndex].action();
    }
  };

  // Scroll selected item into view
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const item = list.querySelector(`[data-index="${selectedIndex}"]`);
    if (item) {
      item.scrollIntoView({ block: "nearest" });
    }
  }, [selectedIndex]);

  return (
    <Dialog.Root open={commandPaletteOpen} onOpenChange={setCommandPaletteOpen}>
      <AnimatePresence>
        {commandPaletteOpen && (
          <Dialog.Portal forceMount>
            <Dialog.Overlay asChild>
              <motion.div
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.15 }}
                className="fixed inset-0 z-[90] bg-black/50 backdrop-blur-sm"
              />
            </Dialog.Overlay>
            <Dialog.Content asChild>
              <motion.div
                initial={{ opacity: 0, scale: 0.96, y: -10 }}
                animate={{ opacity: 1, scale: 1, y: 0 }}
                exit={{ opacity: 0, scale: 0.96, y: -10 }}
                transition={{ duration: 0.15, ease: "easeOut" }}
                className="fixed left-1/2 top-[20%] -translate-x-1/2 z-[91] w-[560px] max-h-[440px] glass-elevated rounded-xl border border-border/45 shadow-2xl overflow-hidden"
              >
                <div className="flex items-center gap-3 px-4 py-3 border-b border-border/20">
                  <Search size={16} className="text-muted-foreground/50" />
                  <input
                    ref={inputRef}
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder="Search commands..."
                    className="flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground/50"
                  />
                  <kbd className="text-xs text-muted-foreground/60 px-2 py-0.5 rounded-md font-medium bg-muted/20 border border-border/20">
                    ESC
                  </kbd>
                </div>

                <div ref={listRef} className="overflow-y-auto max-h-[360px] py-2">
                  {filtered.length === 0 ? (
                    <p className="text-sm text-muted-foreground/50 text-center py-8">
                      No commands found
                    </p>
                  ) : (
                    Object.entries(grouped).map(([category, cmds]) => (
                      <div key={category}>
                        <p className="text-xs font-medium text-muted-foreground/60 uppercase tracking-wider px-4 py-1.5">
                          {category}
                        </p>
                        {cmds.map((cmd) => {
                          const globalIdx = filtered.indexOf(cmd);
                          const Icon = cmd.icon;
                          return (
                            <button
                              key={cmd.id}
                              data-index={globalIdx}
                              onClick={cmd.action}
                              onMouseEnter={() => setSelectedIndex(globalIdx)}
                              className={cn(
                                "flex items-center gap-3 w-full px-4 py-2.5 text-sm transition-colors",
                                globalIdx === selectedIndex
                                  ? "bg-accent/15 text-accent"
                                  : "text-muted-foreground hover:bg-muted/20",
                              )}
                            >
                              <Icon size={16} />
                              <span className="flex-1 text-left">{cmd.label}</span>
                              {cmd.shortcut && (
                                <kbd className="text-xs text-muted-foreground/60 px-2 py-0.5 rounded-md font-medium bg-muted/15 border border-border/25">
                                  {cmd.shortcut}
                                </kbd>
                              )}
                            </button>
                          );
                        })}
                      </div>
                    ))
                  )}
                </div>

                <div className="flex items-center gap-4 px-4 py-2 border-t border-border/20 text-xs text-muted-foreground/50">
                  <span className="flex items-center gap-1">
                    <Command size={10} /> K to toggle
                  </span>
                  <span>Arrow keys to navigate</span>
                  <span>Enter to select</span>
                </div>
              </motion.div>
            </Dialog.Content>
          </Dialog.Portal>
        )}
      </AnimatePresence>
    </Dialog.Root>
  );
}
