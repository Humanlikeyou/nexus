import { motion } from "framer-motion";
import type { BrainState } from "@/lib/types";

interface BrainStateCircleProps {
  state: BrainState;
  inZone: boolean;
}

const stateConfig: Record<BrainState, { color: string; bg: string; label: string }> = {
  calm: { color: "#10b981", bg: "rgba(16, 185, 129, 0.15)", label: "CALM" },
  neutral: { color: "#f59e0b", bg: "rgba(245, 158, 11, 0.15)", label: "NEUTRAL" },
  active: { color: "#f97316", bg: "rgba(249, 115, 22, 0.15)", label: "ACTIVE" },
};

export function BrainStateCircle({ state, inZone }: BrainStateCircleProps) {
  const cfg = stateConfig[state];

  return (
    <div className="relative flex items-center justify-center">
      {/* Pulsing ring when in zone */}
      {inZone && (
        <motion.div
          className="absolute rounded-full"
          style={{
            width: 176,
            height: 176,
            border: `2px solid ${cfg.color}`,
          }}
          animate={{
            scale: [1, 1.08, 1],
            opacity: [0.7, 0.3, 0.7],
          }}
          transition={{
            duration: 2,
            repeat: Infinity,
            ease: "easeInOut",
          }}
        />
      )}

      {/* Main circle */}
      <motion.div
        className="flex flex-col items-center justify-center rounded-full"
        style={{
          width: 160,
          height: 160,
          backgroundColor: cfg.bg,
          border: `2px solid ${cfg.color}`,
        }}
        animate={{
          borderColor: cfg.color,
          backgroundColor: cfg.bg,
        }}
        transition={{ duration: 0.5 }}
      >
        <motion.span
          className="text-2xl font-bold tracking-wider"
          style={{ color: cfg.color }}
          animate={{ color: cfg.color }}
          transition={{ duration: 0.5 }}
        >
          {cfg.label}
        </motion.span>
        {inZone && (
          <span className="text-xs text-muted-foreground mt-1">IN ZONE</span>
        )}
      </motion.div>
    </div>
  );
}
