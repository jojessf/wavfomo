//! Precomputed visualizer data: waveform peaks and a full-track spectrogram.

use realfft::RealFftPlanner;

use crate::config::Magnitude;

/// A full-track spectrogram: one column of frequency bins per STFT frame.
pub struct Spectrogram {
    /// `frames[t][bin]` normalized to 0.0–1.0. `bin` 0 is the lowest frequency.
    pub frames: Vec<Vec<f32>>,
    /// Number of frequency bins per frame (`fft_size / 2 + 1`).
    pub bins: usize,
}

/// Compute a short-time FFT across the whole mono signal.
///
/// `progress(done, total)` is called periodically so the caller can drive a
/// progress bar during this (potentially slow) analysis pass.
pub fn compute_spectrogram(
    mono: &[f32],
    fft_size: usize,
    overlap: f32,
    magnitude: Magnitude,
    mut progress: impl FnMut(usize, usize),
) -> Spectrogram {
    let fft_size = fft_size.max(2);
    let overlap = overlap.clamp(0.0, 0.95);
    let hop = ((fft_size as f32) * (1.0 - overlap)).max(1.0) as usize;
    let bins = fft_size / 2 + 1;

    // Precompute a Hann window.
    let window: Vec<f32> = (0..fft_size)
        .map(|n| {
            let x = std::f32::consts::PI * 2.0 * n as f32 / (fft_size as f32 - 1.0);
            0.5 * (1.0 - x.cos())
        })
        .collect();

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(fft_size);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();

    let n_frames = if mono.len() < fft_size {
        1
    } else {
        (mono.len() - fft_size) / hop + 1
    };

    // Decibel floor for normalization.
    const DB_FLOOR: f32 = -80.0;
    let norm = 2.0 / fft_size as f32;

    let mut frames = Vec::with_capacity(n_frames);
    for f in 0..n_frames {
        let start = f * hop;
        for i in 0..fft_size {
            let s = mono.get(start + i).copied().unwrap_or(0.0);
            input[i] = s * window[i];
        }
        // realfft requires the input length to match; process in place.
        if fft.process(&mut input, &mut output).is_err() {
            break;
        }

        let mut column = Vec::with_capacity(bins);
        for c in &output {
            let mag = (c.norm()) * norm;
            let value = match magnitude {
                Magnitude::Db => {
                    let db = 20.0 * (mag + 1e-9).log10();
                    ((db - DB_FLOOR) / -DB_FLOOR).clamp(0.0, 1.0)
                }
                Magnitude::Linear => mag.clamp(0.0, 1.0),
            };
            column.push(value);
        }
        frames.push(column);

        if f % 32 == 0 {
            progress(f, n_frames);
        }
    }
    progress(n_frames, n_frames);

    Spectrogram { frames, bins }
}

/// Reduce a slice of mono samples to `columns` (min, max) pairs for waveform
/// rendering. `[start, end)` bounds the visible region of the track.
pub fn waveform_peaks(mono: &[f32], start: usize, end: usize, columns: usize) -> Vec<(f32, f32)> {
    let end = end.min(mono.len());
    if columns == 0 || start >= end {
        return vec![(0.0, 0.0); columns];
    }
    let span = end - start;
    let mut peaks = Vec::with_capacity(columns);
    for col in 0..columns {
        let a = start + span * col / columns;
        let b = (start + span * (col + 1) / columns).max(a + 1).min(end);
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for &s in &mono[a..b] {
            lo = lo.min(s);
            hi = hi.max(s);
        }
        if lo > hi {
            lo = 0.0;
            hi = 0.0;
        }
        peaks.push((lo, hi));
    }
    peaks
}
