import type { StrainMetrics } from "@/lib/types";

/**
 * Strain calculator — inspired by OpenWhoop activity/exercise analysis
 * and WHOOP's public strain methodology.
 *
 * WHOOP Strain uses a 0-21 scale based on cardiovascular load (TRIMP-style).
 * The formula accumulates strain based on time spent in HR zones relative
 * to estimated max HR.
 *
 * HR Zones (% of max HR):
 *   Zone 1: 50-60% (Recovery)
 *   Zone 2: 60-70% (Light)
 *   Zone 3: 70-80% (Moderate)
 *   Zone 4: 80-90% (Hard)
 *   Zone 5: 90-100% (Max)
 *
 * Strain accumulates exponentially with higher zones.
 * The 0-21 scale is logarithmic — getting from 18 to 21 is much harder
 * than 0 to 3.
 */

const DEFAULT_MAX_HR = 200; // Conservative default, can be personalized
const ZONE_WEIGHTS = [0.5, 1.0, 2.0, 4.0, 8.0]; // exponential zone weighting
const STRAIN_SCALE = 21; // max strain value

interface StrainAccumulator {
  totalImpulse: number;
  maxHR: number;
  sumHR: number;
  count: number;
  zoneSecs: [number, number, number, number, number];
}

function hrToZone(hr: number, maxHR: number): number {
  const pct = hr / maxHR;
  if (pct >= 0.9) return 4;
  if (pct >= 0.8) return 3;
  if (pct >= 0.7) return 2;
  if (pct >= 0.6) return 1;
  return 0;
}

export function computeStrain(
  hrReadings: { heart_rate: number; timestamp: number }[],
  maxHR: number = DEFAULT_MAX_HR,
): StrainMetrics {
  if (hrReadings.length < 2) {
    return {
      strain: 0,
      maxHR: 0,
      avgHR: 0,
      durationSecs: 0,
      hrZones: [0, 0, 0, 0, 0],
    };
  }

  const acc: StrainAccumulator = {
    totalImpulse: 0,
    maxHR: 0,
    sumHR: 0,
    count: 0,
    zoneSecs: [0, 0, 0, 0, 0],
  };

  for (let i = 1; i < hrReadings.length; i++) {
    const hr = hrReadings[i].heart_rate;
    if (hr < 30 || hr > 220) continue; // sanity filter

    // Time delta between samples (seconds)
    const dt = Math.max(0, Math.min(10, (hrReadings[i].timestamp - hrReadings[i - 1].timestamp) / 1000));

    const zone = hrToZone(hr, maxHR);
    const pctMax = hr / maxHR;

    // Only accumulate strain when HR is above zone 1 threshold (50% max)
    if (pctMax >= 0.5) {
      acc.totalImpulse += ZONE_WEIGHTS[zone] * dt * pctMax;
      acc.zoneSecs[zone] += dt;
    }

    if (hr > acc.maxHR) acc.maxHR = hr;
    acc.sumHR += hr;
    acc.count++;
  }

  const durationSecs = Math.max(
    0,
    (hrReadings[hrReadings.length - 1].timestamp - hrReadings[0].timestamp) / 1000,
  );

  // Convert raw impulse to 0-21 scale using logarithmic mapping
  // A typical 1-hour moderate workout produces ~5000 impulse units → ~14 strain
  // The log scale means strain plateaus at high values (diminishing returns)
  let strain = 0;
  if (acc.totalImpulse > 0) {
    strain = STRAIN_SCALE * (1 - Math.exp(-acc.totalImpulse / 8000));
  }
  strain = Math.round(strain * 10) / 10;

  return {
    strain,
    maxHR: acc.maxHR,
    avgHR: acc.count > 0 ? Math.round(acc.sumHR / acc.count) : 0,
    durationSecs: Math.round(durationSecs),
    hrZones: acc.zoneSecs.map((s) => Math.round(s)) as [number, number, number, number, number],
  };
}
