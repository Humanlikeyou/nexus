/**
 * Display hook: reads from the EegRingBuffer at a controlled FPS,
 * decimates samples for the chart, and computes dynamic Y-axis range.
 *
 * The rAF loop reads config from a ref so settings changes never
 * tear down / restart the animation loop (prevents freeze).
 */

import { useState, useEffect, useRef, useCallback } from "react";
import type { EegRingBuffer } from "@/lib/eegRingBuffer";
import type { EegDisplayConfig } from "@/lib/types";
import { DEFAULT_DISPLAY_CONFIG } from "@/lib/types";

const SAMPLE_RATE = 256;
/** Max points sent to chart per channel (keeps echarts fast) */
const MAX_CHART_POINTS = 512;

/** Auto-scale range limits (µV) */
const AUTO_SCALE_MIN = 50;
const AUTO_SCALE_MAX = 250;
/** Lerp factor for smooth Y-axis transitions (0-1, lower = smoother) */
const SCALE_LERP = 0.15;

export interface EegDisplayData {
  /** Decimated samples per channel for chart rendering */
  chartData: Record<string, number[]>;
  /** Time labels for x-axis (seconds from window start) */
  timeLabels: number[];
  /** Dynamic Y-axis bounds */
  yMin: number;
  yMax: number;
}

export function useEegDisplay(
  ringBuffer: EegRingBuffer | null,
  streaming: boolean,
  configOverride?: Partial<EegDisplayConfig>,
) {
  const [config, setConfig] = useState<EegDisplayConfig>({
    ...DEFAULT_DISPLAY_CONFIG,
    ...configOverride,
  });

  const [displayData, setDisplayData] = useState<EegDisplayData>({
    chartData: {},
    timeLabels: [],
    yMin: -200,
    yMax: 200,
  });

  const rafRef = useRef<number>(0);
  const lastFrameRef = useRef<number>(0);
  /** Smoothed auto-scale range (persists across frames) */
  const smoothedRangeRef = useRef<number>(AUTO_SCALE_MIN);
  /** Config ref — read by rAF tick so config changes don't restart the loop */
  const configRef = useRef(config);

  // Keep config ref in sync (no effect restart needed)
  useEffect(() => {
    configRef.current = config;
  }, [config]);

  const updateConfig = useCallback((patch: Partial<EegDisplayConfig>) => {
    setConfig((prev) => ({ ...prev, ...patch }));
  }, []);

  // Display loop: only depends on ringBuffer + streaming.
  // Config is read from ref inside tick — settings changes are picked up
  // on the next frame without tearing down the animation loop.
  useEffect(() => {
    if (!ringBuffer || !streaming) {
      if (!streaming) {
        const c = configRef.current;
        setDisplayData({ chartData: {}, timeLabels: [], yMin: -c.yRange, yMax: c.yRange });
        smoothedRangeRef.current = AUTO_SCALE_MIN;
      }
      return;
    }

    const tick = (now: number) => {
      const cfg = configRef.current;
      const frameInterval = 1000 / cfg.displayFps;

      if (now - lastFrameRef.current >= frameInterval) {
        lastFrameRef.current = now;

        const windowSamples = cfg.windowSecs * SAMPLE_RATE;
        const targetPoints = Math.min(MAX_CHART_POINTS, windowSamples);

        const chartData: Record<string, number[]> = {};
        let peakAbsValue = 0;

        for (const ch of cfg.visibleChannels) {
          const decimated = ringBuffer.readDecimated(ch, windowSamples, targetPoints);
          chartData[ch] = decimated;

          if (cfg.autoScale) {
            for (let i = 0; i < decimated.length; i++) {
              const abs = Math.abs(decimated[i]);
              if (abs > peakAbsValue) peakAbsValue = abs;
            }
          }
        }

        // Generate time labels (seconds)
        const pointCount = chartData[cfg.visibleChannels[0]]?.length || 0;
        const timeLabels = new Array<number>(pointCount);
        const timeStep = cfg.windowSecs / (pointCount || 1);
        for (let i = 0; i < pointCount; i++) {
          timeLabels[i] = Math.round(i * timeStep * 100) / 100;
        }

        let yMin: number, yMax: number;
        if (cfg.autoScale) {
          const targetRange = Math.min(
            Math.max(peakAbsValue * 1.2, AUTO_SCALE_MIN),
            AUTO_SCALE_MAX,
          );
          smoothedRangeRef.current = lerp(smoothedRangeRef.current, targetRange, SCALE_LERP);
          const range = Math.round(smoothedRangeRef.current);
          yMin = -range;
          yMax = range;
        } else {
          yMin = -cfg.yRange;
          yMax = cfg.yRange;
        }

        setDisplayData({ chartData, timeLabels, yMin, yMax });
      }

      rafRef.current = requestAnimationFrame(tick);
    };

    rafRef.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafRef.current);
  }, [ringBuffer, streaming]);

  return { displayData, config, updateConfig };
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}
