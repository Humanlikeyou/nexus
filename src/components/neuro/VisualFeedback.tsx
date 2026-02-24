import { motion, AnimatePresence } from "framer-motion";

interface VisualFeedbackProps {
  glowColor: string;
  glowIntensity: number; // 0-1
  pulseActive: boolean;
  gated: boolean;
}

export function VisualFeedback({
  glowColor,
  glowIntensity,
  pulseActive,
  gated,
}: VisualFeedbackProps) {
  if (gated) {
    return (
      <div
        className="absolute inset-0 pointer-events-none rounded-xl"
        style={{ backgroundColor: "rgba(128, 128, 128, 0.02)" }}
      />
    );
  }

  const opacity = glowIntensity * 0.15;

  return (
    <AnimatePresence>
      {glowIntensity > 0 && (
        <motion.div
          key="glow"
          className="absolute inset-0 pointer-events-none rounded-xl"
          initial={{ opacity: 0 }}
          animate={{
            opacity: 1,
            scale: pulseActive ? [1, 1.02, 1] : 1,
          }}
          exit={{ opacity: 0 }}
          transition={{
            opacity: { duration: 1, ease: "easeOut" },
            scale: pulseActive
              ? { duration: 2, repeat: Infinity, ease: "easeInOut" }
              : { duration: 1 },
          }}
          style={{
            background: `radial-gradient(ellipse at center, ${glowColor} 0%, transparent 70%)`,
            opacity,
          }}
        />
      )}
    </AnimatePresence>
  );
}
