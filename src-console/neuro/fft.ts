export const SAMPLE_RATE = 256;
export const FFT_SIZE = 256;

const WINDOW = (() => {
  const values = new Float64Array(FFT_SIZE);
  for (let index = 0; index < FFT_SIZE; index += 1) {
    values[index] = 0.5 * (1 - Math.cos((2 * Math.PI * index) / (FFT_SIZE - 1)));
  }
  return values;
})();

const WINDOW_ENERGY = WINDOW.reduce((sum, value) => sum + value * value, 0);

function fft(real: Float64Array, imaginary: Float64Array) {
  const length = real.length;
  for (let index = 1, reversed = 0; index < length; index += 1) {
    let bit = length >> 1;
    for (; reversed & bit; bit >>= 1) reversed ^= bit;
    reversed ^= bit;
    if (index < reversed) {
      [real[index], real[reversed]] = [real[reversed], real[index]];
      [imaginary[index], imaginary[reversed]] = [imaginary[reversed], imaginary[index]];
    }
  }

  for (let span = 2; span <= length; span <<= 1) {
    const angle = (-2 * Math.PI) / span;
    const stepReal = Math.cos(angle);
    const stepImaginary = Math.sin(angle);
    for (let offset = 0; offset < length; offset += span) {
      let currentReal = 1;
      let currentImaginary = 0;
      for (let index = 0; index < span / 2; index += 1) {
        const upperReal = real[offset + index];
        const upperImaginary = imaginary[offset + index];
        const lowerReal = real[offset + index + span / 2] * currentReal - imaginary[offset + index + span / 2] * currentImaginary;
        const lowerImaginary = real[offset + index + span / 2] * currentImaginary + imaginary[offset + index + span / 2] * currentReal;
        real[offset + index] = upperReal + lowerReal;
        imaginary[offset + index] = upperImaginary + lowerImaginary;
        real[offset + index + span / 2] = upperReal - lowerReal;
        imaginary[offset + index + span / 2] = upperImaginary - lowerImaginary;
        const nextReal = currentReal * stepReal - currentImaginary * stepImaginary;
        currentImaginary = currentReal * stepImaginary + currentImaginary * stepReal;
        currentReal = nextReal;
      }
    }
  }
}

export function powerSpectrum(samples: number[]) {
  const real = new Float64Array(FFT_SIZE);
  const imaginary = new Float64Array(FFT_SIZE);
  const source = samples.slice(-FFT_SIZE);
  const mean = source.reduce((sum, value) => sum + value, 0) / Math.max(source.length, 1);
  for (let index = 0; index < FFT_SIZE; index += 1) {
    real[index] = ((source[index] ?? mean) - mean) * WINDOW[index];
  }
  fft(real, imaginary);
  const powers = new Float64Array(FFT_SIZE / 2);
  const normalization = WINDOW_ENERGY * SAMPLE_RATE;
  for (let index = 0; index < powers.length; index += 1) {
    powers[index] = (real[index] ** 2 + imaginary[index] ** 2) / normalization;
  }
  return powers;
}

function bandPower(powers: Float64Array, minimum: number, maximum: number) {
  const binSize = SAMPLE_RATE / FFT_SIZE;
  const start = Math.max(1, Math.ceil(minimum / binSize));
  const end = Math.min(powers.length, Math.ceil(maximum / binSize));
  let total = 0;
  for (let index = start; index < end; index += 1) total += powers[index];
  return total;
}

export type FrequencyBands = Record<"delta" | "theta" | "alpha" | "beta" | "gamma", number>;

export function relativeBandPowers(samples: number[]): FrequencyBands {
  return relativeChannelBandPowers([samples]);
}

export function relativeChannelBandPowers(channels: number[][]): FrequencyBands {
  const complete = channels.filter((samples) => samples.length >= FFT_SIZE && samples.slice(-FFT_SIZE).every(Number.isFinite));
  if (!complete.length) return { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 };
  // Combine electrode power, not phase-sensitive waveforms, to avoid cancellation.
  const powers = new Float64Array(FFT_SIZE / 2);
  for (const samples of complete) {
    powerSpectrum(samples).forEach((power, index) => { powers[index] += power / complete.length; });
  }
  const raw = {
    delta: bandPower(powers, 1, 4),
    theta: bandPower(powers, 4, 8),
    alpha: bandPower(powers, 8, 13),
    beta: bandPower(powers, 13, 30),
    gamma: bandPower(powers, 30, 45),
  };
  const total = Object.values(raw).reduce((sum, value) => sum + value, 0) || 1;
  return Object.fromEntries(Object.entries(raw).map(([key, value]) => [key, value / total * 100])) as FrequencyBands;
}
