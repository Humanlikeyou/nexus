/**
 * Head movement from the Muse gyroscope (about 52 Hz), an artifact gate for the EEG window.
 *
 * The sensor sits at a fixed offset of several deg/s even on a still head, so each axis is read
 * against its own recent median. One packet is three samples and the stream's first packet can
 * read over 100 deg/s, so a movement has to outlast a single packet.
 */

export const GYRO_BUFFER = 260;
const MOVED_DEG_PER_S = 6;
const MOVED_SAMPLES = 4;
/** The EEG window is one second long, so a movement stays in it for that long. */
const RECENT_SAMPLES = 52;

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[sorted.length >> 1];
}

export function headMoved(gyro: number[][]): boolean {
  if (gyro.length < RECENT_SAMPLES) return false;
  const bias = [0, 1, 2].map((axis) => median(gyro.map((sample) => sample[axis])));
  const moving = gyro.slice(-RECENT_SAMPLES).filter((sample) => Math.hypot(sample[0] - bias[0], sample[1] - bias[1], sample[2] - bias[2]) > MOVED_DEG_PER_S);
  return moving.length >= MOVED_SAMPLES;
}
