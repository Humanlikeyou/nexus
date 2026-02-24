import type { NfProtocol } from "@/lib/types";

export const PROTOCOLS: NfProtocol[] = [
  // ── Core Protocols ──────────────────────────────────────────
  {
    id: "focus",
    name: "Focus Training",
    description: "Enhance sensorimotor rhythm (SMR) to sharpen concentration and reduce mental noise.",
    icon: "Crosshair",
    rewardBands: [{ label: "SMR", minHz: 12, maxHz: 15 }],
    inhibitBands: [
      { label: "Theta", minHz: 4, maxHz: 8 },
      { label: "High Beta", minHz: 20, maxHz: 30 },
    ],
    audioMode: "soundscape",
    eyesClosed: false,
    defaultDurationMin: 15,
    category: "focus",
    sessionPrompt: "Gently direct your attention. Let distractions pass without engagement.",
    coachingCues: [
      "Settle into your breath. Let attention sharpen naturally.",
      "If your mind wanders, gently return. The feedback will guide you.",
      "You're building sustained attention. Stay with the process.",
    ],
  },
  {
    id: "calm",
    name: "Calm & Relax",
    description: "Boost alpha waves to enter a relaxed, present state of awareness.",
    icon: "Leaf",
    rewardBands: [{ label: "Alpha", minHz: 8, maxHz: 13 }],
    inhibitBands: [{ label: "High Beta", minHz: 20, maxHz: 30 }],
    audioMode: "soundscape",
    eyesClosed: false,
    defaultDurationMin: 10,
    category: "calm",
    sessionPrompt: "Soften your gaze. Let your body relax while staying aware.",
    coachingCues: [
      "Release tension in your jaw and shoulders.",
      "Let thoughts float by like clouds. Stay present.",
    ],
  },
  {
    id: "deep-relaxation",
    name: "Deep Relaxation",
    description: "Alpha-theta crossover training for deep meditative states. Eyes closed, audio-guided.",
    icon: "Moon",
    rewardBands: [
      { label: "Theta", minHz: 4, maxHz: 8 },
      { label: "Alpha", minHz: 8, maxHz: 13 },
    ],
    inhibitBands: [{ label: "High Beta", minHz: 20, maxHz: 30 }],
    audioMode: "tone",
    eyesClosed: true,
    defaultDurationMin: 20,
    category: "relaxation",
    sessionPrompt: "Close your eyes. Let go of effort and sink inward.",
    coachingCues: [
      "Release all doing. Simply be.",
      "If you notice you're trying, let that go too.",
    ],
  },
  {
    id: "peak-performance",
    name: "Peak Performance",
    description: "Strengthen low-beta activity to sustain alertness and cognitive performance.",
    icon: "Zap",
    rewardBands: [{ label: "Low Beta", minHz: 15, maxHz: 20 }],
    inhibitBands: [{ label: "Theta", minHz: 4, maxHz: 8 }],
    audioMode: "soundscape",
    eyesClosed: false,
    defaultDurationMin: 15,
    category: "performance",
    sessionPrompt: "Stay alert and engaged. Channel your mental energy.",
    coachingCues: [
      "Maintain crisp awareness. You're in the zone.",
      "Sustain this state. Your brain is learning the pattern.",
    ],
  },

  // ── Advanced State-Training Protocols ────────────────────────
  {
    id: "relaxed-flow",
    name: "Relaxed Flow",
    description:
      "Alpha + SMR co-training for calm alertness. Suppresses theta drift to prevent drowsiness while maintaining relaxed focus.",
    icon: "Wind",
    rewardBands: [
      { label: "Alpha", minHz: 8, maxHz: 13 },
      { label: "SMR", minHz: 12, maxHz: 15 },
    ],
    inhibitBands: [{ label: "High Beta", minHz: 20, maxHz: 30 }],
    suppressOnDrift: [{ label: "Theta", minHz: 4, maxHz: 8 }],
    audioMode: "soundscape",
    eyesClosed: false,
    defaultDurationMin: 15,
    category: "calm",
    progressionOrder: 1,
    sessionPrompt:
      "Let your awareness settle. Notice thoughts without following them.",
    coachingCues: [
      "Find the balance — relaxed but present.",
      "If you feel drowsy, gently re-engage your attention.",
      "You're training calm alertness. Stay with the flow.",
    ],
  },
  {
    id: "creative-insight",
    name: "Creative Insight",
    description:
      "Alpha/theta coupling stability training. Rewards stable ratio between creative theta and grounding alpha. Inhibits mental noise and delta drift.",
    icon: "Lightbulb",
    rewardBands: [
      { label: "Alpha", minHz: 8, maxHz: 13 },
      { label: "Theta", minHz: 4, maxHz: 8 },
    ],
    couplingReward: { bandA: "Alpha", bandB: "Theta", rewardStability: true },
    inhibitBands: [
      { label: "High Beta", minHz: 20, maxHz: 30 },
      { label: "Delta", minHz: 0.5, maxHz: 4 },
    ],
    passthroughBands: ["Gamma"],
    audioMode: "soundscape",
    eyesClosed: false,
    defaultDurationMin: 20,
    category: "performance",
    progressionOrder: 3,
    sessionPrompt:
      "Allow ideas to surface without forcing. Stay present with the flow.",
    coachingCues: [
      "Let your mind wander purposefully. Don't chase thoughts.",
      "The sweet spot is between thinking and dreaming.",
      "Creative insight often arrives in the spaces between effort.",
    ],
  },
  {
    id: "deep-meditation",
    name: "Deep Meditation",
    description:
      "Alpha coherence stability training. Rewards sustained, stable alpha — not just peaks. Allows gamma bursts without reward logic. Inhibits volatility.",
    icon: "Mountain",
    rewardBands: [{ label: "Alpha", minHz: 8, maxHz: 13 }],
    rewardStability: true,
    inhibitBands: [{ label: "High Beta", minHz: 20, maxHz: 30 }],
    suppressOnDrift: [{ label: "High Beta", minHz: 20, maxHz: 30 }],
    passthroughBands: ["Gamma"],
    audioMode: "tone",
    eyesClosed: true,
    defaultDurationMin: 25,
    category: "relaxation",
    progressionOrder: 4,
    sessionPrompt:
      "Rest in stillness. Let the breath carry your attention inward.",
    coachingCues: [
      "Settle deeper. There is nowhere to go.",
      "Notice the space between breaths.",
      "Let awareness rest in itself, without an object.",
    ],
  },
  {
    id: "hypnagogic",
    name: "Hypnagogic Imagery",
    description:
      "Theta increase with alpha presence — training the edge-of-sleep state. Strict artifact gating. Inhibits delta dominance and arousal.",
    icon: "Eye",
    rewardBands: [
      { label: "Theta", minHz: 4, maxHz: 8 },
      { label: "Alpha", minHz: 8, maxHz: 13 },
    ],
    inhibitBands: [
      { label: "Delta", minHz: 0.5, maxHz: 4 },
      { label: "High Beta", minHz: 20, maxHz: 30 },
    ],
    gateOverrides: { maxArtifactRate: 8, minConfidence: 0.4 },
    audioMode: "tone",
    eyesClosed: true,
    defaultDurationMin: 20,
    category: "relaxation",
    progressionOrder: 5,
    sessionPrompt:
      "Drift toward the edge of sleep. Let imagery arise without grasping.",
    coachingCues: [
      "Hover at the threshold. Don't fall asleep, but don't wake up.",
      "If images appear, observe them without attachment.",
      "You're in the liminal space. Trust the process.",
    ],
  },
];

/** Suggested session progression — easiest to most advanced */
export const PROGRESSION_ORDER: string[] = [
  "relaxed-flow",
  "calm",
  "creative-insight",
  "deep-meditation",
  "hypnagogic",
  "focus",
  "deep-relaxation",
  "peak-performance",
];

export function getProtocol(id: string): NfProtocol | undefined {
  return PROTOCOLS.find((p) => p.id === id);
}
