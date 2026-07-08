//! Precomputed visualizer data: waveform peaks and a full-track spectrogram.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

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

    // The plan is `Send + Sync`; workers share it and keep their own scratch.
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(fft_size);

    let n_frames = if mono.len() < fft_size {
        1
    } else {
        (mono.len() - fft_size) / hop + 1
    };

    // Decibel floor for normalization.
    const DB_FLOOR: f32 = -80.0;
    let norm = 2.0 / fft_size as f32;

    // One column per frame. Each FFT is independent, so we split the frames
    // across worker threads that fill disjoint slices in parallel.
    let mut frames: Vec<Vec<f32>> = vec![Vec::new(); n_frames];

    let threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(n_frames.max(1));
    let chunk = n_frames.div_ceil(threads.max(1)).max(1);
    let done = AtomicUsize::new(0);

    // Copyable references the worker closures capture by value.
    let window: &[f32] = &window;
    let fft = &fft;
    let done = &done;

    thread::scope(|scope| {
        for (ci, slice) in frames.chunks_mut(chunk).enumerate() {
            let base = ci * chunk;
            scope.spawn(move || {
                let mut input = fft.make_input_vec();
                let mut output = fft.make_output_vec();
                let mut scratch = fft.make_scratch_vec();
                for (local, out) in slice.iter_mut().enumerate() {
                    let start = (base + local) * hop;
                    for i in 0..fft_size {
                        input[i] = mono.get(start + i).copied().unwrap_or(0.0) * window[i];
                    }
                    if fft
                        .process_with_scratch(&mut input, &mut output, &mut scratch)
                        .is_err()
                    {
                        break;
                    }
                    let mut column = Vec::with_capacity(bins);
                    for c in &output {
                        let mag = c.norm() * norm;
                        let value = match magnitude {
                            Magnitude::Db => {
                                let db = 20.0 * (mag + 1e-9).log10();
                                ((db - DB_FLOOR) / -DB_FLOOR).clamp(0.0, 1.0)
                            }
                            Magnitude::Linear => mag.clamp(0.0, 1.0),
                        };
                        column.push(value);
                    }
                    *out = column;
                    done.fetch_add(1, Ordering::Relaxed);
                }
            });
        }

        // Drive the progress bar from the main thread while workers run.
        loop {
            let d = done.load(Ordering::Relaxed);
            progress(d, n_frames);
            if d >= n_frames {
                break;
            }
            thread::sleep(Duration::from_millis(4));
        }
    });

    Spectrogram { frames, bins }
}

/// Precompute a fixed-resolution (min, max) summary over the whole track. This
/// is the render-time "mip": once built, drawing the waveform downsamples from
/// it instead of rescanning millions of raw samples every frame. Cheap to build
/// (one O(len) pass at load) and tiny to keep (`resolution` pairs).
pub fn peak_summary(mono: &[f32], resolution: usize) -> Vec<(f32, f32)> {
    let n = resolution.max(1).min(mono.len().max(1));
    waveform_peaks(mono, 0, mono.len(), n)
}

/// Reduce the precomputed peak `summary` to `columns` (min, max) pairs over the
/// visible sample window `[start, end)`. Falls back to a raw scan only when
/// zoomed in finer than the summary's resolution — where the window is small,
/// so the scan is cheap. Either way, per-frame work is bounded by `columns` (or
/// the visible span), never by the full track length.
pub fn waveform_peaks_view(
    mono: &[f32],
    summary: &[(f32, f32)],
    start: usize,
    end: usize,
    columns: usize,
) -> Vec<(f32, f32)> {
    let len = mono.len();
    let end = end.min(len);
    if columns == 0 || start >= end {
        return vec![(0.0, 0.0); columns];
    }
    let n = summary.len();
    // Samples covered per summary bucket vs. per output column. If a column is
    // coarser than a bucket, the summary has enough detail; otherwise we're
    // zoomed past it and must read raw samples for accuracy.
    let per_bucket = len as f64 / n.max(1) as f64;
    let per_col = (end - start) as f64 / columns as f64;
    if n == 0 || per_col < per_bucket {
        return waveform_peaks(mono, start, end, columns);
    }

    // Map the sample window onto summary buckets, then min/max-downsample to
    // `columns`.
    let bstart = start * n / len;
    let bend = (end * n / len).max(bstart + 1).min(n);
    let bspan = bend - bstart;
    let mut peaks = Vec::with_capacity(columns);
    for col in 0..columns {
        let a = bstart + bspan * col / columns;
        let b = (bstart + bspan * (col + 1) / columns).max(a + 1).min(bend);
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for &(l, h) in &summary[a..b] {
            lo = lo.min(l);
            hi = hi.max(h);
        }
        if lo > hi {
            lo = 0.0;
            hi = 0.0;
        }
        peaks.push((lo, hi));
    }
    peaks
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

#[cfg(test)]
mod tests {
    use super::*;

    // A sine-ish signal so peaks vary across the track.
    fn signal(len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (i as f32 * 0.001).sin() * (0.2 + 0.8 * (i as f32 / len as f32)))
            .collect()
    }

    #[test]
    fn view_envelope_contains_true_envelope() {
        let mono = signal(2_000_000);
        let summary = peak_summary(&mono, 1 << 15);
        // Zoomed-out full-track view at a typical terminal width.
        let (start, end, cols) = (0, mono.len(), 200);
        let truth = waveform_peaks(&mono, start, end, cols);
        let view = waveform_peaks_view(&mono, &summary, start, end, cols);
        assert_eq!(truth.len(), view.len());
        // The summary is coarser than the raw scan, so bucket/column edges don't
        // align exactly — the view may be a hair narrower or wider than truth.
        // The error is bounded by roughly one summary bucket's worth of signal
        // change, which is far below one terminal cell of vertical resolution.
        for (i, ((tl, th), (vl, vh))) in truth.iter().zip(&view).enumerate() {
            assert!((vl - tl).abs() < 0.02, "col {i}: view lo {vl} vs true lo {tl}");
            assert!((vh - th).abs() < 0.02, "col {i}: view hi {vh} vs true hi {th}");
        }
    }

    #[test]
    fn view_falls_back_to_raw_when_zoomed_past_summary() {
        let mono = signal(2_000_000);
        let summary = peak_summary(&mono, 1 << 15);
        // Tiny window, high column count → finer than the summary → raw scan.
        let (start, end, cols) = (1000, 1400, 200);
        let truth = waveform_peaks(&mono, start, end, cols);
        let view = waveform_peaks_view(&mono, &summary, start, end, cols);
        assert_eq!(truth, view);
    }
}
