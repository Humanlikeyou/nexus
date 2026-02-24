import { useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { Crosshair, Leaf, Moon, Zap, Wind, Lightbulb, Mountain, Eye, X } from "lucide-react";
import { cn } from "@/lib/utils";
import type { NfProtocol } from "@/lib/types";
import { PROTOCOLS } from "@/lib/neurofeedback/protocols";

const iconMap: Record<string, React.ComponentType<{ size?: number; className?: string }>> = {
  Crosshair,
  Leaf,
  Moon,
  Zap,
  Wind,
  Lightbulb,
  Mountain,
  Eye,
};

const categoryColors: Record<string, string> = {
  focus: "border-cyan-500/30 hover:border-cyan-500/60",
  calm: "border-emerald-500/30 hover:border-emerald-500/60",
  relaxation: "border-blue-500/30 hover:border-blue-500/60",
  performance: "border-amber-500/30 hover:border-amber-500/60",
};

const categoryAccents: Record<string, string> = {
  focus: "text-cyan-400",
  calm: "text-emerald-400",
  relaxation: "text-blue-400",
  performance: "text-amber-400",
};

interface ProtocolPickerProps {
  museConnected: boolean;
  museStreaming: boolean;
  onSelect: (protocol: NfProtocol, durationMin: number) => void;
}

const DURATION_OPTIONS = [5, 10, 15, 20, 30];

export function ProtocolPicker({ museConnected, museStreaming, onSelect }: ProtocolPickerProps) {
  const [selectedProtocol, setSelectedProtocol] = useState<NfProtocol | null>(null);
  const [selectedDuration, setSelectedDuration] = useState(15);
  const [dialogOpen, setDialogOpen] = useState(false);

  const handleCardClick = (protocol: NfProtocol) => {
    setSelectedProtocol(protocol);
    setSelectedDuration(protocol.defaultDurationMin);
    setDialogOpen(true);
  };

  const handleBeginSession = () => {
    if (selectedProtocol) {
      setDialogOpen(false);
      onSelect(selectedProtocol, selectedDuration);
    }
  };

  return (
    <div className="space-y-3">
      <h3 className="text-base font-medium text-muted-foreground">Training Protocols</h3>
      <div className="grid grid-cols-2 gap-3">
        {PROTOCOLS.map((protocol) => {
          const Icon = iconMap[protocol.icon] || Crosshair;
          const accent = categoryAccents[protocol.category];
          const border = categoryColors[protocol.category];

          return (
            <button
              key={protocol.id}
              onClick={() => handleCardClick(protocol)}
              className={cn(
                "glass p-6 text-left rounded-xl border transition-all duration-200 relative",
                border,
              )}
            >
              {protocol.progressionOrder != null && (
                <span className="absolute top-2 right-2 w-5 h-5 rounded-full bg-muted/20 border border-border/20 flex items-center justify-center text-[13px] text-muted-foreground/50 font-medium">
                  {protocol.progressionOrder}
                </span>
              )}
              <div className="flex items-start gap-3">
                <div className={cn("mt-0.5", accent)}>
                  <Icon size={18} />
                </div>
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium">{protocol.name}</p>
                  <p className="text-[13px] text-muted-foreground/60 mt-1 line-clamp-2">
                    {protocol.description}
                  </p>
                  <div className="flex items-center gap-2 mt-2">
                    <span className="text-xs px-2 py-0.5 rounded-md font-medium bg-success/10 text-success">
                      {protocol.rewardBands.map((b) => b.label).join("+")}
                    </span>
                    <span className="text-xs text-muted-foreground/40">
                      {protocol.defaultDurationMin}min
                    </span>
                  </div>
                </div>
              </div>
            </button>
          );
        })}
      </div>

      <Dialog.Root open={dialogOpen} onOpenChange={setDialogOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 bg-black/60 backdrop-blur-sm z-50" />
          <Dialog.Content className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[380px] glass-elevated rounded-2xl p-6 border border-border/30 shadow-2xl">
            <div className="flex items-center justify-between mb-4">
              <Dialog.Title className="text-base font-semibold">
                {selectedProtocol?.name}
              </Dialog.Title>
              <Dialog.Close className="text-muted-foreground/40 hover:text-muted-foreground transition-colors">
                <X size={16} />
              </Dialog.Close>
            </div>

            <p className="text-xs text-muted-foreground/60 mb-4">
              {selectedProtocol?.description}
            </p>

            {selectedProtocol?.sessionPrompt && (
              <p className="text-[13px] text-muted-foreground/80 italic mb-4 px-3 py-2 rounded-lg bg-muted/10 border border-border/10">
                {selectedProtocol.sessionPrompt}
              </p>
            )}

            {selectedProtocol?.eyesClosed && (
              <p className="text-[13px] text-blue-400 mb-4">
                Eyes-closed protocol — audio-guided feedback
              </p>
            )}

            <div className="space-y-3 mb-5">
              <p className="text-xs text-muted-foreground">Duration</p>
              <div className="flex gap-2">
                {DURATION_OPTIONS.map((d) => (
                  <button
                    key={d}
                    onClick={() => setSelectedDuration(d)}
                    className={cn(
                      "flex-1 py-2 rounded-lg text-xs transition-colors",
                      selectedDuration === d
                        ? "bg-accent/20 text-accent border border-accent/40"
                        : "bg-muted/15 text-muted-foreground border border-border/15 hover:bg-muted/25",
                    )}
                  >
                    {d}m
                  </button>
                ))}
              </div>
            </div>

            {!museConnected || !museStreaming ? (
              <div className="py-3 px-4 rounded-lg bg-warning/10 border border-warning/20 text-center">
                <p className="text-xs text-warning">
                  {!museConnected
                    ? "Connect your Muse headband to begin"
                    : "Start EEG stream to begin"}
                </p>
              </div>
            ) : (
              <button
                onClick={handleBeginSession}
                className="w-full py-2.5 rounded-lg bg-accent/15 text-accent text-sm font-medium hover:bg-accent/25 transition-colors border border-accent/30"
              >
                Begin Session
              </button>
            )}
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </div>
  );
}
