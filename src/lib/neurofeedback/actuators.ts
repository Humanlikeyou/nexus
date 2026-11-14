import type { FeedbackSignal, BrainState } from "../../../src-console/legacy-core/lib/types";
import type { useAudioFeedback } from "../../../src-console/legacy-core/hooks/useAudioFeedback";

// ============================================================
// Actuator Interface
// ============================================================

export interface FeedbackActuator {
  name: string;
  init(): void;
  update(signal: FeedbackSignal): void;
  cleanup(): void;
}

// ============================================================
// Visual Actuator State (read by React components)
// ============================================================

export interface VisualFeedbackState {
  glowColor: string;
  glowIntensity: number; // 0-1
  pulseActive: boolean;
  gated: boolean;
}

const INITIAL_VISUAL_STATE: VisualFeedbackState = {
  glowColor: "transparent",
  glowIntensity: 0,
  pulseActive: false,
  gated: false,
};

// ============================================================
// MockActuator — console logging for dev/debug
// ============================================================

export class MockActuator implements FeedbackActuator {
  name = "mock";

  init(): void {
    console.log("[mock-actuator] initialized");
  }

  update(signal: FeedbackSignal): void {
    console.log("[mock-actuator]", signal);
  }

  cleanup(): void {
    console.log("[mock-actuator] cleaned up");
  }
}

// ============================================================
// AudioActuator — wraps existing useAudioFeedback
// ============================================================

export class AudioActuator implements FeedbackActuator {
  name = "audio";
  private audio: ReturnType<typeof useAudioFeedback>;

  constructor(audio: ReturnType<typeof useAudioFeedback>) {
    this.audio = audio;
  }

  init(): void {
    // Audio init is handled externally during calibration
  }

  update(signal: FeedbackSignal): void {
    if (signal.gated) {
      // When gated, mute ambient — neutral state, out of zone
      this.audio.updateState("neutral", false);
      return;
    }

    // Map action to BrainState for audio
    const stateMap: Record<string, BrainState> = {
      reward: "calm",
      inhibit: "active",
      neutral: "neutral",
    };
    const brainState = stateMap[signal.action] ?? "neutral";
    const inZone = signal.intensity > 0.5;

    this.audio.updateState(brainState, inZone);

    // Trigger reward chirp for strong reward signals
    if (signal.action === "reward" && signal.intensity > 0.8) {
      this.audio.triggerReward();
    }
  }

  cleanup(): void {
    // Audio cleanup is handled externally
  }
}

// ============================================================
// VisualActuator — stores state for React components to read
// ============================================================

export class VisualActuator implements FeedbackActuator {
  name = "visual";
  state: VisualFeedbackState = { ...INITIAL_VISUAL_STATE };

  init(): void {
    this.state = { ...INITIAL_VISUAL_STATE };
  }

  update(signal: FeedbackSignal): void {
    if (signal.gated) {
      this.state = {
        glowColor: "transparent",
        glowIntensity: 0,
        pulseActive: false,
        gated: true,
      };
      return;
    }

    let glowColor = "transparent";
    if (signal.action === "reward") {
      glowColor = "#10b981"; // emerald
    } else if (signal.action === "inhibit") {
      glowColor = "#f97316"; // orange
    }

    this.state = {
      glowColor,
      glowIntensity: signal.intensity,
      pulseActive: signal.action === "reward" && signal.intensity > 0.7,
      gated: false,
    };
  }

  cleanup(): void {
    this.state = { ...INITIAL_VISUAL_STATE };
  }
}
