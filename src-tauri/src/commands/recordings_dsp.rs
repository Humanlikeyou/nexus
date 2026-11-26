//! Pure signal maths for the recording readers: Rust ports of the TypeScript originals, kept
//! numerically identical so a recording reads the same here as it did live. Nothing here does IO.
//!
//! - `fft`, `single_psd`: src/lib/fft.ts
//! - `relative_channel_band_powers`: src-console/neuro/fft.ts
//! - `remove_line_noise`: src/lib/lineNoise.ts
//! - `assess_channel`: src/hooks/useSignalQuality.ts
//! - `focus_relaxation`: src-console/neuro/fft.ts `bandFeatures` (focusScore, relaxationScore; decision 0060)
//! - `head_movement_events`: src-console/neuro/headMotion.ts thresholds

pub const SAMPLE_RATE: f64 = 256.0;
pub const FFT_SIZE: usize = 256;

pub const BAND_NAMES: [&str; 5] = ["delta", "theta", "alpha", "beta", "gamma"];

fn window() -> &'static [f64; FFT_SIZE] {
    use std::sync::OnceLock;
    static W: OnceLock<[f64; FFT_SIZE]> = OnceLock::new();
    W.get_or_init(|| {
        let mut w = [0.0; FFT_SIZE];
        for (i, v) in w.iter_mut().enumerate() {
            *v = 0.5
                * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / (FFT_SIZE as f64 - 1.0)).cos());
        }
        w
    })
}

fn window_energy() -> f64 {
    window().iter().map(|v| v * v).sum()
}

/// Cooley-Tukey radix-2 FFT, in place; the same loop and twiddle recurrence as the TypeScript.
pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f64::consts::PI / len as f64;
        let (w_im, w_re) = angle.sin_cos();
        let mut i = 0;
        while i < n {
            let (mut cur_re, mut cur_im) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (u_re, u_im) = (re[i + k], im[i + k]);
                let (x_re, x_im) = (re[i + k + len / 2], im[i + k + len / 2]);
                let v_re = x_re * cur_re - x_im * cur_im;
                let v_im = x_re * cur_im + x_im * cur_re;
                re[i + k] = u_re + v_re;
                im[i + k] = u_im + v_im;
                re[i + k + len / 2] = u_re - v_re;
                im[i + k + len / 2] = u_im - v_im;
                let next_re = cur_re * w_re - cur_im * w_im;
                cur_im = cur_re * w_im + cur_im * w_re;
                cur_re = next_re;
            }
            i += len;
        }
        len <<= 1;
    }
}

/// Hann-windowed power spectrum of the first 256 samples (no mean removal), as `singlePSD`.
pub fn single_psd(samples: &[f64]) -> [f64; FFT_SIZE / 2] {
    let w = window();
    let mut re = [0.0; FFT_SIZE];
    let mut im = [0.0; FFT_SIZE];
    for i in 0..FFT_SIZE {
        re[i] = samples.get(i).copied().filter(|v| *v != 0.0).unwrap_or(0.0) * w[i];
    }
    fft(&mut re, &mut im);
    let norm = window_energy() * SAMPLE_RATE;
    let mut psd = [0.0; FFT_SIZE / 2];
    for i in 0..FFT_SIZE / 2 {
        psd[i] = (re[i] * re[i] + im[i] * im[i]) / norm;
    }
    psd
}

/// Power spectrum of the newest 256 samples with the mean removed, as neuro/fft.ts `powerSpectrum`.
fn power_spectrum(samples: &[f64]) -> [f64; FFT_SIZE / 2] {
    let w = window();
    let source = &samples[samples.len().saturating_sub(FFT_SIZE)..];
    let mean = source.iter().sum::<f64>() / source.len().max(1) as f64;
    let mut re = [0.0; FFT_SIZE];
    let mut im = [0.0; FFT_SIZE];
    for i in 0..FFT_SIZE {
        re[i] = (source.get(i).copied().unwrap_or(mean) - mean) * w[i];
    }
    fft(&mut re, &mut im);
    let norm = window_energy() * SAMPLE_RATE;
    let mut p = [0.0; FFT_SIZE / 2];
    for i in 0..FFT_SIZE / 2 {
        p[i] = (re[i] * re[i] + im[i] * im[i]) / norm;
    }
    p
}

/// neuro/fft.ts `bandPower`: bins [ceil(min/bin), ceil(max/bin)).
fn band_power_exclusive(powers: &[f64], min: f64, max: f64) -> f64 {
    let bin = SAMPLE_RATE / FFT_SIZE as f64;
    let start = ((min / bin).ceil() as usize).max(1);
    let end = ((max / bin).ceil() as usize).min(powers.len());
    (start..end).map(|i| powers[i]).sum()
}

/// Relative band power (% of delta..gamma total) of the channels that have a full finite
/// 256-sample window, averaged as power. `None` when no channel qualifies. Order: delta, theta,
/// alpha, beta, gamma.
pub fn relative_channel_band_powers(channels: &[&[f64]]) -> Option<[f64; 5]> {
    let complete: Vec<&&[f64]> = channels
        .iter()
        .filter(|c| c.len() >= FFT_SIZE && c[c.len() - FFT_SIZE..].iter().all(|v| v.is_finite()))
        .collect();
    if complete.is_empty() {
        return None;
    }
    let mut powers = [0.0; FFT_SIZE / 2];
    for c in &complete {
        for (acc, p) in powers.iter_mut().zip(power_spectrum(c)) {
            *acc += p / complete.len() as f64;
        }
    }
    let raw = [
        band_power_exclusive(&powers, 1.0, 4.0),
        band_power_exclusive(&powers, 4.0, 8.0),
        band_power_exclusive(&powers, 8.0, 13.0),
        band_power_exclusive(&powers, 13.0, 30.0),
        band_power_exclusive(&powers, 30.0, 45.0),
    ];
    let sum: f64 = raw.iter().sum();
    let total = if sum == 0.0 { 1.0 } else { sum };
    Some(raw.map(|v| v / total * 100.0))
}

// ── Mains hum (src/lib/lineNoise.ts) ────────────────────────────

const MIN_PROMINENCE: f64 = 8.0;
const NOTCH_HALF_WIDTH_HZ: f64 = 3.0;

fn prominence(powers: &[f64], hz: usize) -> f64 {
    let peak = powers[hz - 1].max(powers[hz]).max(powers[hz + 1]);
    let mut floor = 0.0;
    for offset in [-6i64, -5, -4, 4, 5, 6] {
        floor += powers[(hz as i64 + offset) as usize] / 6.0;
    }
    if floor > 0.0 {
        peak / floor
    } else {
        0.0
    }
}

fn detect_line_hz(samples: &[f64]) -> Option<usize> {
    if samples.len() < FFT_SIZE {
        return None;
    }
    let recent = &samples[samples.len() - FFT_SIZE..];
    let mean = recent.iter().sum::<f64>() / FFT_SIZE as f64;
    let centred: Vec<f64> = recent.iter().map(|v| v - mean).collect();
    let powers = single_psd(&centred);
    let mut found = None;
    let mut best = MIN_PROMINENCE;
    for hz in [50usize, 60] {
        let score = prominence(&powers, hz);
        if score > best {
            best = score;
            found = Some(hz);
        }
    }
    found
}

/// The newest 256 or 512 samples with mains hum and its second harmonic removed, mean kept.
pub fn remove_line_noise(samples: &[f64]) -> Vec<f64> {
    if samples.len() < FFT_SIZE || samples.iter().any(|v| !v.is_finite()) {
        return samples.to_vec();
    }
    let hz = detect_line_hz(samples);
    let size = if samples.len() >= FFT_SIZE * 2 {
        FFT_SIZE * 2
    } else {
        FFT_SIZE
    };
    let block = &samples[samples.len() - size..];
    let Some(hz) = hz else {
        return block.to_vec();
    };
    let mean = block.iter().sum::<f64>() / size as f64;
    let mut re: Vec<f64> = block.iter().map(|v| v - mean).collect();
    let mut im = vec![0.0; size];
    fft(&mut re, &mut im);
    let bins_per_hz = size as f64 / SAMPLE_RATE;
    for center in [hz as f64, hz as f64 * 2.0] {
        let from = ((center - NOTCH_HALF_WIDTH_HZ) * bins_per_hz).round() as usize;
        let to = ((center + NOTCH_HALF_WIDTH_HZ) * bins_per_hz).round() as usize;
        let mut bin = from;
        while bin <= to && bin <= size / 2 {
            re[bin] = 0.0;
            im[bin] = 0.0;
            re[(size - bin) % size] = 0.0;
            im[(size - bin) % size] = 0.0;
            bin += 1;
        }
    }
    for v in im.iter_mut() {
        *v = -*v;
    }
    fft(&mut re, &mut im);
    re.iter().map(|v| v / size as f64 + mean).collect()
}

// ── Contact quality (src/hooks/useSignalQuality.ts) ─────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Good,
    Fair,
    Poor,
}

impl Quality {
    pub fn as_str(self) -> &'static str {
        match self {
            Quality::Good => "good",
            Quality::Fair => "fair",
            Quality::Poor => "poor",
        }
    }
}

const RAIL_VOLTAGE_UV: f64 = 900.0;
const FLATLINE_UV: f64 = 1.5;
const FAIR_SPREAD_UV: f64 = 40.0;
const POOR_SPREAD_UV: f64 = 100.0;

fn middle(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    v[values.len() >> 1]
}

fn robust_spread(samples: &[f64]) -> f64 {
    let center = middle(samples);
    let deviations: Vec<f64> = samples.iter().map(|v| (v - center).abs()).collect();
    1.4826 * middle(&deviations)
}

/// Contact quality of one channel from its newest samples (callers pass 256).
pub fn assess_channel(samples: &[f64]) -> Quality {
    if samples.len() < 32 {
        return Quality::Good;
    }
    let railed = samples.iter().filter(|v| v.abs() > RAIL_VOLTAGE_UV).count();
    if railed as f64 > samples.len() as f64 * 0.3 {
        return Quality::Poor;
    }
    let clean = remove_line_noise(samples);
    let spread = robust_spread(&clean);
    if !(FLATLINE_UV..=POOR_SPREAD_UV).contains(&spread) {
        return Quality::Poor;
    }
    if spread > FAIR_SPREAD_UV {
        return Quality::Fair;
    }
    let jumps = clean
        .windows(2)
        .filter(|w| (w[1] - w[0]).abs() > 100.0)
        .count();
    if jumps as f64 > clean.len() as f64 * 0.2 {
        return Quality::Fair;
    }
    Quality::Good
}

/// Focus (beta / (beta + theta)) and relaxation (alpha / (alpha + beta)) scores, 0-100, of a
/// mean-removed 256-sample window, as `neuro/fft.ts` `bandFeatures` returns them for one channel.
/// Decision 0060: half-open bins, so the 8 and 13 Hz bins are no longer counted in two bands.
/// Test anchor: the single-electrode parity check against the TypeScript (the app uses `focus_relaxation_channels`).
#[cfg(test)]
pub fn focus_relaxation(window_samples: &[f64]) -> (f64, f64) {
    let mean = window_samples.iter().sum::<f64>() / window_samples.len().max(1) as f64;
    let centred: Vec<f64> = window_samples.iter().map(|v| v - mean).collect();
    scores(&single_psd(&centred))
}

/// Decision 0060 (security review): several electrodes, as `bandFeatures` does it. Their power spectra are
/// averaged first and the scores taken from the average, never the scores averaged. `None` when no
/// electrode has a full finite window.
pub fn focus_relaxation_channels(channels: &[&[f64]]) -> Option<(f64, f64)> {
    let complete: Vec<&&[f64]> = channels
        .iter()
        .filter(|c| c.len() >= FFT_SIZE && c[c.len() - FFT_SIZE..].iter().all(|v| v.is_finite()))
        .collect();
    if complete.is_empty() {
        return None;
    }
    let mut powers = [0.0; FFT_SIZE / 2];
    for c in &complete {
        for (acc, p) in powers.iter_mut().zip(power_spectrum(c)) {
            *acc += p / complete.len() as f64;
        }
    }
    Some(scores(&powers))
}

fn scores(powers: &[f64]) -> (f64, f64) {
    let theta = band_power_exclusive(powers, 4.0, 8.0);
    let alpha = band_power_exclusive(powers, 8.0, 13.0);
    let beta = band_power_exclusive(powers, 13.0, 30.0);
    let focus = if beta + theta > 0.0 {
        (beta / (beta + theta) * 100.0).round()
    } else {
        0.0
    };
    let relax = if alpha + beta > 0.0 {
        (alpha / (alpha + beta) * 100.0).round()
    } else {
        0.0
    };
    (focus, relax)
}

// ── Filtering and reduction ─────────────────────────────────────

/// One-pole 0.5 Hz high-pass at 256 Hz. NaN gaps stay NaN and restart the filter after them.
pub fn high_pass_05hz(values: &[f64]) -> Vec<f64> {
    high_pass(values, SAMPLE_RATE, 0.5)
}

/// One-pole high-pass at `cutoff` Hz for a signal sampled at `rate` Hz.
pub fn high_pass(values: &[f64], rate: f64, cutoff: f64) -> Vec<f64> {
    let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff);
    let dt = 1.0 / rate;
    let a = rc / (rc + dt);
    let mut out = Vec::with_capacity(values.len());
    let mut prev: Option<(f64, f64)> = None; // (x, y)
    for &x in values {
        if !x.is_finite() {
            out.push(f64::NAN);
            prev = None;
            continue;
        }
        let y = match prev {
            Some((px, py)) => a * (py + x - px),
            None => 0.0,
        };
        prev = Some((x, y));
        out.push(y);
    }
    out
}

/// Mean per bucket, ignoring NaN; an all-NaN bucket stays NaN. Passes short input through.
pub fn bucket_mean(values: &[f64], points: usize) -> Vec<f64> {
    if points == 0 || values.len() <= points {
        return values.to_vec();
    }
    let step = values.len() as f64 / points as f64;
    (0..points)
        .map(|k| {
            let from = (k as f64 * step).floor() as usize;
            let to = (((k + 1) as f64 * step).floor() as usize).min(values.len());
            let (mut sum, mut n) = (0.0, 0usize);
            for v in &values[from..to] {
                if v.is_finite() {
                    sum += v;
                    n += 1;
                }
            }
            if n == 0 {
                f64::NAN
            } else {
                sum / n as f64
            }
        })
        .collect()
}

/// Heart rate (bpm) estimated from PPG counts at 64 Hz: high-passed, lightly smoothed, then the
/// median spacing of beat peaks. `None` unless at least 4 peaks give a rate of 40-180 bpm.
pub fn estimate_bpm(ppg: &[f64], rate: f64) -> Option<f64> {
    let finite: Vec<f64> = ppg.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.len() < (rate * 4.0) as usize {
        return None;
    }
    // Remove the slow baseline with a ~1.5 s moving average, smooth with ~0.1 s.
    let moving = |v: &[f64], w: usize| -> Vec<f64> {
        let mut out = Vec::with_capacity(v.len());
        let mut sum = 0.0;
        for i in 0..v.len() {
            sum += v[i];
            if i >= w {
                sum -= v[i - w];
            }
            out.push(sum / (i + 1).min(w) as f64);
        }
        out
    };
    let base = moving(&finite, (rate * 1.5) as usize);
    let detrended: Vec<f64> = finite.iter().zip(&base).map(|(x, b)| x - b).collect();
    let smooth = moving(&detrended, ((rate * 0.1) as usize).max(1));
    let mean = smooth.iter().sum::<f64>() / smooth.len() as f64;
    let sd = (smooth.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / smooth.len() as f64).sqrt();
    if sd <= 0.0 {
        return None;
    }
    let min_gap = (rate * 0.33) as usize;
    let mut peaks: Vec<usize> = Vec::new();
    for i in 1..smooth.len() - 1 {
        if smooth[i] > mean + 0.5 * sd && smooth[i] >= smooth[i - 1] && smooth[i] > smooth[i + 1] {
            match peaks.last() {
                Some(&last) if i - last < min_gap => {
                    if smooth[i] > smooth[last] {
                        *peaks.last_mut().unwrap() = i;
                    }
                }
                _ => peaks.push(i),
            }
        }
    }
    if peaks.len() < 4 {
        return None;
    }
    let mut gaps: Vec<f64> = peaks
        .windows(2)
        .map(|w| (w[1] - w[0]) as f64 / rate)
        .collect();
    gaps.sort_by(|a, b| a.total_cmp(b));
    let bpm = 60.0 / gaps[gaps.len() / 2];
    (40.0..=180.0).contains(&bpm).then_some(bpm.round())
}

const MOVED_DEG_PER_S: f64 = 6.0;
const MOVED_SAMPLES: usize = 4;

/// Gyroscope movement: each axis is read against its own median (the sensor has a fixed offset);
/// a movement is at least 4 consecutive samples more than 6 deg/s from it. Returns the deviation
/// magnitude per sample and the index where each movement starts.
pub fn head_movement(gyro: &[[f64; 3]]) -> (Vec<f64>, Vec<usize>) {
    if gyro.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let bias: Vec<f64> = (0..3)
        .map(|a| middle(&gyro.iter().map(|s| s[a]).collect::<Vec<_>>()))
        .collect();
    let dev: Vec<f64> = gyro
        .iter()
        .map(|s| {
            ((s[0] - bias[0]).powi(2) + (s[1] - bias[1]).powi(2) + (s[2] - bias[2]).powi(2)).sqrt()
        })
        .collect();
    let mut events = Vec::new();
    let mut run_start: Option<usize> = None;
    for (i, d) in dev.iter().chain(std::iter::once(&0.0)).enumerate() {
        if *d > MOVED_DEG_PER_S {
            run_start.get_or_insert(i);
        } else if let Some(s) = run_start.take() {
            if i - s >= MOVED_SAMPLES {
                events.push(s);
            }
        }
    }
    (dev, events)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f64, amp: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| amp * (2.0 * std::f64::consts::PI * hz * i as f64 / SAMPLE_RATE).sin())
            .collect()
    }

    #[test]
    fn a_10hz_wave_is_alpha() {
        let b = relative_channel_band_powers(&[&sine(10.0, 20.0, 256)]).unwrap();
        assert!(b[2] > 95.0, "alpha {b:?}");
        assert!((b.iter().sum::<f64>() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn short_or_non_finite_channels_are_skipped() {
        assert!(relative_channel_band_powers(&[&[1.0; 100]]).is_none());
        let mut bad = sine(10.0, 20.0, 256);
        bad[255] = f64::NAN;
        assert!(relative_channel_band_powers(&[&bad]).is_none());
    }

    #[test]
    fn flat_railed_and_normal_channels_are_assessed() {
        assert_eq!(assess_channel(&[0.0; 256]), Quality::Poor);
        assert_eq!(assess_channel(&[950.0; 256]), Quality::Poor);
        assert_eq!(assess_channel(&sine(10.0, 15.0, 256)), Quality::Good);
        assert_eq!(assess_channel(&sine(10.0, 200.0, 256)), Quality::Poor);
        assert_eq!(assess_channel(&[1.0; 10]), Quality::Good);
    }

    #[test]
    fn high_pass_removes_dc_and_keeps_gaps() {
        let mut v: Vec<f64> = sine(10.0, 10.0, 1024).iter().map(|x| x + 500.0).collect();
        v[300] = f64::NAN;
        let out = high_pass_05hz(&v);
        assert!(out[300].is_nan());
        let tail: f64 = out[800..].iter().sum::<f64>() / 224.0;
        assert!(tail.abs() < 1.0, "dc left {tail}");
    }

    #[test]
    fn bucketing_ignores_nan() {
        let b = bucket_mean(&[1.0, f64::NAN, 3.0, f64::NAN, f64::NAN, f64::NAN], 3);
        assert_eq!(b[0], 1.0);
        assert_eq!(b[1], 3.0);
        assert!(b[2].is_nan());
    }

    #[test]
    fn heart_rate_from_a_clean_pulse() {
        let rate = 64.0;
        let ppg: Vec<f64> = (0..(rate as usize * 12))
            .map(|i| {
                let ph = (i as f64 / rate * 72.0 / 60.0) % 1.0;
                1000.0 * (-((ph - 0.2) * (ph - 0.2)) / 0.004).exp() + 50000.0
            })
            .collect();
        let bpm = estimate_bpm(&ppg, rate).unwrap();
        assert!((bpm - 72.0).abs() <= 2.0, "bpm {bpm}");
        assert!(estimate_bpm(&[1.0; 64], rate).is_none());
    }

    #[test]
    fn a_sustained_turn_is_one_event_and_a_blip_is_not() {
        let mut gyro = vec![[2.0, 1.0, 0.0]; 200];
        for s in gyro.iter_mut().skip(50).take(8) {
            *s = [60.0, 0.0, 0.0];
        }
        gyro[120] = [90.0, 0.0, 0.0];
        let (dev, events) = head_movement(&gyro);
        assert_eq!(dev.len(), 200);
        assert_eq!(events, vec![50]);
    }
}

#[cfg(test)]
mod parity {
    use super::*;

    fn read(name: &str) -> serde_json::Value {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../test/fixtures/muse")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    /// The expected values come from the TypeScript originals (test/unit/museParity.test.ts).
    #[test]
    fn parity_with_typescript() {
        let inputs = read("parity_inputs.json");
        let expected = read("parity_expected.json");
        for (name, samples) in inputs.as_object().unwrap() {
            let x: Vec<f64> = samples
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let want = &expected[name];
            let bands = relative_channel_band_powers(&[&x]).unwrap_or([0.0; 5]);
            for (i, b) in bands.iter().enumerate() {
                let w = want["bands"][i].as_f64().unwrap();
                assert!((b - w).abs() < 1e-6, "{name} band {i}: {b} vs {w}");
            }
            assert_eq!(
                assess_channel(&x).as_str(),
                want["quality"],
                "{name} quality"
            );
            let mean = x.iter().sum::<f64>() / x.len() as f64;
            let centred: Vec<f64> = x.iter().map(|v| v - mean).collect();
            let (f, r) = focus_relaxation(&centred);
            assert_eq!(f, want["focus"].as_f64().unwrap(), "{name} focus");
            assert_eq!(r, want["relaxation"].as_f64().unwrap(), "{name} relaxation");
        }
        // Decision 0060: two electrodes, power averaged first, match the TypeScript bandFeatures.
        let pairs = expected["_pairs"].as_object().expect("pair fixtures");
        assert!(!pairs.is_empty());
        for (key, want) in pairs {
            let (a, b) = key.split_once('+').unwrap();
            let get = |n: &str| -> Vec<f64> {
                inputs[n]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect()
            };
            let (x, y) = (get(a), get(b));
            let (f, r) = focus_relaxation_channels(&[&x, &y]).unwrap();
            assert_eq!(f, want["focus"].as_f64().unwrap(), "{key} focus");
            assert_eq!(r, want["relaxation"].as_f64().unwrap(), "{key} relaxation");
        }
        assert_eq!(focus_relaxation_channels(&[&[1.0; 10]]), None);
    }
}
