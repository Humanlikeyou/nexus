/**
 * Pure FFT math functions extracted from useFFT.
 * Shared by useFFT (band powers) and artifactDetection (jaw-clench detection).
 */

export const SAMPLE_RATE = 256;
export const FFT_SIZE = 256;

// Hanning window (precomputed)
export const WINDOW = (() => {
  const w = new Float64Array(FFT_SIZE);
  for (let i = 0; i < FFT_SIZE; i++) {
    w[i] = 0.5 * (1 - Math.cos((2 * Math.PI * i) / (FFT_SIZE - 1)));
  }
  return w;
})();

// Window energy for normalization
export const WINDOW_ENERGY = (() => {
  let sum = 0;
  for (let i = 0; i < FFT_SIZE; i++) sum += WINDOW[i] * WINDOW[i];
  return sum;
})();

/** Cooley-Tukey radix-2 FFT (in-place) */
export function fft(re: Float64Array, im: Float64Array): void {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      let tmp = re[i]; re[i] = re[j]; re[j] = tmp;
      tmp = im[i]; im[i] = im[j]; im[j] = tmp;
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const angle = (-2 * Math.PI) / len;
    const wRe = Math.cos(angle);
    const wIm = Math.sin(angle);
    for (let i = 0; i < n; i += len) {
      let curRe = 1, curIm = 0;
      for (let j = 0; j < len / 2; j++) {
        const uRe = re[i + j];
        const uIm = im[i + j];
        const vRe = re[i + j + len / 2] * curRe - im[i + j + len / 2] * curIm;
        const vIm = re[i + j + len / 2] * curIm + im[i + j + len / 2] * curRe;
        re[i + j] = uRe + vRe;
        im[i + j] = uIm + vIm;
        re[i + j + len / 2] = uRe - vRe;
        im[i + j + len / 2] = uIm - vIm;
        const newCurRe = curRe * wRe - curIm * wIm;
        curIm = curRe * wIm + curIm * wRe;
        curRe = newCurRe;
      }
    }
  }
}

/** Single-segment PSD: window, FFT, compute power spectrum. */
export function singlePSD(samples: number[]): Float64Array {
  const re = new Float64Array(FFT_SIZE);
  const im = new Float64Array(FFT_SIZE);
  for (let i = 0; i < FFT_SIZE; i++) {
    re[i] = (samples[i] || 0) * WINDOW[i];
    im[i] = 0;
  }
  fft(re, im);
  const psd = new Float64Array(FFT_SIZE / 2);
  const norm = WINDOW_ENERGY * SAMPLE_RATE;
  for (let i = 0; i < FFT_SIZE / 2; i++) {
    psd[i] = (re[i] * re[i] + im[i] * im[i]) / norm;
  }
  return psd;
}

/**
 * Welch PSD: split buffer into overlapping segments, window each, FFT,
 * average the periodograms. More stable than single-segment FFT.
 */
export function welchPSD(samples: number[], overlap = 0.5): Float64Array {
  const n = samples.length;
  const step = Math.floor(FFT_SIZE * (1 - overlap));
  const numSegments = Math.floor((n - FFT_SIZE) / step) + 1;

  if (numSegments < 1) {
    return singlePSD(samples);
  }

  const psd = new Float64Array(FFT_SIZE / 2);
  const re = new Float64Array(FFT_SIZE);
  const im = new Float64Array(FFT_SIZE);

  for (let seg = 0; seg < numSegments; seg++) {
    const offset = seg * step;
    for (let i = 0; i < FFT_SIZE; i++) {
      re[i] = (samples[offset + i] || 0) * WINDOW[i];
      im[i] = 0;
    }
    fft(re, im);
    for (let i = 0; i < FFT_SIZE / 2; i++) {
      psd[i] += (re[i] * re[i] + im[i] * im[i]);
    }
  }

  const norm = numSegments * WINDOW_ENERGY * SAMPLE_RATE;
  for (let i = 0; i < FFT_SIZE / 2; i++) {
    psd[i] /= norm;
  }

  return psd;
}

/** Sum power in a frequency range from a PSD array. */
export function bandPower(powers: Float64Array, fMin: number, fMax: number): number {
  const binSize = SAMPLE_RATE / FFT_SIZE;
  const iMin = Math.max(1, Math.floor(fMin / binSize));
  const iMax = Math.min(powers.length - 1, Math.ceil(fMax / binSize));
  let sum = 0;
  for (let i = iMin; i <= iMax; i++) sum += powers[i];
  return sum;
}
