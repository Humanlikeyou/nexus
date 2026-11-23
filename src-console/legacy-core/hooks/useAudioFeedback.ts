import { useState, useRef, useCallback } from "react";
import type { BrainState } from "../lib/types";

type AudioMode = "soundscape" | "tone" | "silent";

export function useAudioFeedback() {
  const [muted, setMuted] = useState(false);
  const ctxRef = useRef<AudioContext | null>(null);
  const modeRef = useRef<AudioMode>("soundscape");

  // Nodes
  const ambientGainRef = useRef<GainNode | null>(null);
  const noiseSourceRef = useRef<AudioBufferSourceNode | null>(null);
  const noiseFilterRef = useRef<BiquadFilterNode | null>(null);
  const droneOscRef = useRef<OscillatorNode | null>(null);
  const rewardGainRef = useRef<GainNode | null>(null);
  const toneGainRef = useRef<GainNode | null>(null);
  const feedbackOscRef = useRef<OscillatorNode | null>(null);

  // Cooldowns
  const lastRewardRef = useRef(0);
  const nodesCreatedRef = useRef(false);

  const init = useCallback(() => {
    if (ctxRef.current) return;
    const ctx = new AudioContext();
    ctxRef.current = ctx;

    // Ambient gain
    const ambientGain = ctx.createGain();
    ambientGain.gain.value = 0.2;
    ambientGain.connect(ctx.destination);
    ambientGainRef.current = ambientGain;

    // Brown noise
    const bufferSize = ctx.sampleRate * 2;
    const noiseBuffer = ctx.createBuffer(1, bufferSize, ctx.sampleRate);
    const output = noiseBuffer.getChannelData(0);
    let lastOut = 0;
    for (let i = 0; i < bufferSize; i++) {
      const white = Math.random() * 2 - 1;
      lastOut = (lastOut + 0.02 * white) / 1.02;
      output[i] = lastOut * 3.5;
    }

    const noiseSource = ctx.createBufferSource();
    noiseSource.buffer = noiseBuffer;
    noiseSource.loop = true;

    const filter = ctx.createBiquadFilter();
    filter.type = "lowpass";
    filter.frequency.value = 500;
    filter.Q.value = 0.7;
    noiseFilterRef.current = filter;

    noiseSource.connect(filter);
    filter.connect(ambientGain);
    noiseSource.start();
    noiseSourceRef.current = noiseSource;

    // Low drone
    const drone = ctx.createOscillator();
    drone.type = "sine";
    drone.frequency.value = 80;
    const droneGain = ctx.createGain();
    droneGain.gain.value = 0.04;
    drone.connect(droneGain);
    droneGain.connect(ambientGain);
    drone.start();
    droneOscRef.current = drone;

    // Reward gain
    const rGain = ctx.createGain();
    rGain.gain.value = 0.3;
    rGain.connect(ctx.destination);
    rewardGainRef.current = rGain;

    // Tone gain + osc (for tone mode)
    const tGain = ctx.createGain();
    tGain.gain.value = 0.02;
    tGain.connect(ctx.destination);
    toneGainRef.current = tGain;

    const feedbackOsc = ctx.createOscillator();
    feedbackOsc.type = "sine";
    feedbackOsc.frequency.value = 432;
    feedbackOsc.connect(tGain);
    feedbackOsc.start();
    feedbackOscRef.current = feedbackOsc;

    nodesCreatedRef.current = true;
  }, []);

  const setMode = useCallback((mode: AudioMode) => {
    modeRef.current = mode;
    const ctx = ctxRef.current;
    if (!ctx || !nodesCreatedRef.current) return;

    const now = ctx.currentTime;
    if (mode === "soundscape") {
      ambientGainRef.current?.gain.linearRampToValueAtTime(0.2, now + 0.5);
      toneGainRef.current?.gain.linearRampToValueAtTime(0.02, now + 0.5);
    } else if (mode === "tone") {
      ambientGainRef.current?.gain.linearRampToValueAtTime(0.05, now + 0.5);
      toneGainRef.current?.gain.linearRampToValueAtTime(0.2, now + 0.5);
    } else {
      ambientGainRef.current?.gain.linearRampToValueAtTime(0, now + 0.5);
      toneGainRef.current?.gain.linearRampToValueAtTime(0, now + 0.5);
    }
  }, []);

  const updateState = useCallback((state: BrainState, inZone: boolean) => {
    const ctx = ctxRef.current;
    if (!ctx || !nodesCreatedRef.current || muted) return;

    const now = ctx.currentTime;

    if (modeRef.current === "soundscape") {
      // Rain intensity mapped to brain state
      const filterFreq = state === "calm" ? 200 : state === "neutral" ? 500 : 1200;
      const gain = state === "calm" ? 0.15 : state === "neutral" ? 0.25 : 0.4;
      noiseFilterRef.current?.frequency.linearRampToValueAtTime(filterFreq, now + 1);
      ambientGainRef.current?.gain.linearRampToValueAtTime(gain, now + 1);
    }

    if (modeRef.current === "tone") {
      const vol = inZone ? 0.2 : 0.02;
      toneGainRef.current?.gain.linearRampToValueAtTime(vol, now + 0.5);
    }
  }, [muted]);

  const triggerReward = useCallback(() => {
    const ctx = ctxRef.current;
    if (!ctx || muted) return;

    const now = Date.now();
    if (now - lastRewardRef.current < 8000) return;
    lastRewardRef.current = now;

    const t = ctx.currentTime;

    // Two chirps
    for (let c = 0; c < 2; c++) {
      const offset = c * 0.2;
      const osc = ctx.createOscillator();
      osc.type = "sine";
      const gain = ctx.createGain();
      gain.gain.value = 0;
      osc.connect(gain);
      gain.connect(rewardGainRef.current || ctx.destination);

      // Frequency sweep: 2000 → 4000 → 2000 over 150ms
      osc.frequency.setValueAtTime(2000, t + offset);
      osc.frequency.linearRampToValueAtTime(4000, t + offset + 0.075);
      osc.frequency.linearRampToValueAtTime(2000, t + offset + 0.15);

      gain.gain.setValueAtTime(0.3, t + offset);
      gain.gain.linearRampToValueAtTime(0, t + offset + 0.15);

      osc.start(t + offset);
      osc.stop(t + offset + 0.2);
    }
  }, [muted]);

  const playBell = useCallback((type: "start" | "end" | "countdown") => {
    const ctx = ctxRef.current;
    if (!ctx || muted) return;

    const t = ctx.currentTime;

    if (type === "start") {
      const osc = ctx.createOscillator();
      osc.type = "sine";
      osc.frequency.value = 528;
      const gain = ctx.createGain();
      gain.gain.setValueAtTime(0.25, t);
      gain.gain.exponentialRampToValueAtTime(0.001, t + 0.8);
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.start(t);
      osc.stop(t + 0.8);
    } else if (type === "end") {
      const freqs = [528, 660, 792];
      for (const freq of freqs) {
        const osc = ctx.createOscillator();
        osc.type = "sine";
        osc.frequency.value = freq;
        const gain = ctx.createGain();
        gain.gain.setValueAtTime(0.15, t);
        gain.gain.exponentialRampToValueAtTime(0.001, t + 1.5);
        osc.connect(gain);
        gain.connect(ctx.destination);
        osc.start(t);
        osc.stop(t + 1.5);
      }
    } else if (type === "countdown") {
      for (let i = 0; i < 3; i++) {
        const osc = ctx.createOscillator();
        osc.type = "sine";
        osc.frequency.value = 440;
        const gain = ctx.createGain();
        gain.gain.setValueAtTime(0.2, t + i);
        gain.gain.exponentialRampToValueAtTime(0.001, t + i + 0.1);
        osc.connect(gain);
        gain.connect(ctx.destination);
        osc.start(t + i);
        osc.stop(t + i + 0.15);
      }
    }
  }, [muted]);

  const cleanup = useCallback(() => {
    noiseSourceRef.current?.stop();
    droneOscRef.current?.stop();
    feedbackOscRef.current?.stop();
    ctxRef.current?.close();
    ctxRef.current = null;
    nodesCreatedRef.current = false;
  }, []);

  return {
    init,
    setMode,
    updateState,
    triggerReward,
    playBell,
    cleanup,
    muted,
    setMuted,
  };
}
