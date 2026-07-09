//! Audio decoding and playback.
//!
//! Decoding happens twice by design: once eagerly into memory (mono) to feed the
//! visualizers, and again lazily through rodio's `Player` for playback (which
//! supports native seeking on the `Decoder` source).

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rodio::source::Source;
use rodio::{Decoder, MixerDeviceSink, Player};

/// The whole track decoded to a mono `f32` buffer, plus stream metadata.
pub struct AudioData {
    /// Interleaved channels downmixed to a single mono trace, range ~[-1, 1].
    pub mono: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration: Duration,
}

/// A decode/open failure with a user-facing message.
#[derive(Debug)]
pub struct AudioError {
    pub message: String,
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for AudioError {}

impl AudioError {
    fn new(message: impl Into<String>) -> Self {
        AudioError {
            message: message.into(),
        }
    }
}

/// Decode the entire file into a mono buffer for analysis.
///
/// `progress(fraction)` is called periodically during the decode. `fraction` is
/// `Some(0.0..=1.0)` when the total duration is known (WAV, FLAC), or `None` for
/// formats that don't report a length up front (MP3, Ogg Vorbis) — in which case
/// the caller can only show an indeterminate spinner.
pub fn decode_to_memory(
    path: &Path,
    mut progress: impl FnMut(Option<f32>),
) -> Result<AudioData, AudioError> {
    let file = File::open(path).map_err(|e| {
        AudioError::new(format!("cannot open '{}': {}", path.display(), e))
    })?;

    // Passing the byte length lets the decoder report a total duration for
    // formats that otherwise can't (MP3, Ogg Vorbis), so decode progress can
    // show a real percentage instead of an indeterminate spinner.
    let byte_len = file.metadata().ok().map(|m| m.len());

    let mut builder = Decoder::builder().with_data(file);
    if let Some(len) = byte_len {
        builder = builder.with_byte_len(len);
    }
    let decoder = builder.build().map_err(|e| {
        AudioError::new(format!(
            "cannot decode '{}': {e}. Unsupported or unrecognized format \
             (supported: WAV, FLAC, Ogg Vorbis, MP3) — this format would need \
             to be added.",
            path.display()
        ))
    })?;

    let channels = decoder.channels().get();
    let sample_rate = decoder.sample_rate().get();
    let ch = channels.max(1) as usize;

    // Total length when the format reports it, so we can show real progress and
    // preallocate. `None` for MP3/Vorbis without a byte-length hint.
    let total = decoder.total_duration();
    let total_secs = total.map(|d| d.as_secs_f64()).filter(|s| *s > 0.0);
    let est_frames = total_secs
        .map(|s| (s * sample_rate as f64) as usize)
        .unwrap_or(0);
    let mut mono = Vec::with_capacity(est_frames);

    // Downmix to mono on the fly instead of buffering the interleaved signal.
    // Report progress roughly every ~1.5s of decoded audio.
    let report_every = (sample_rate as usize).saturating_mul(3) / 2;
    let report_every = report_every.max(1);
    let mut next_report = report_every;
    let mut sum = 0.0f32;
    let mut in_frame = 0usize;
    for s in decoder {
        sum += s;
        in_frame += 1;
        if in_frame == ch {
            mono.push(sum / ch as f32);
            sum = 0.0;
            in_frame = 0;
            if mono.len() >= next_report {
                next_report = mono.len() + report_every;
                let frac = total_secs.map(|secs| {
                    (mono.len() as f64 / sample_rate as f64 / secs).clamp(0.0, 1.0) as f32
                });
                progress(frac);
            }
        }
    }
    progress(Some(1.0));

    let frames = mono.len();
    let duration = if sample_rate > 0 {
        Duration::from_secs_f64(frames as f64 / sample_rate as f64)
    } else {
        Duration::ZERO
    };

    Ok(AudioData {
        mono,
        sample_rate,
        channels,
        duration,
    })
}

/// Playback engine: owns the audio device sink and a `Player`.
pub struct Engine {
    // Kept alive for the lifetime of playback; dropping it stops audio.
    _sink: MixerDeviceSink,
    player: Player,
    path: PathBuf,
    /// Volume as a 0.0–1.0 fraction; preserved across mute toggles.
    volume: f32,
    muted: bool,
}

impl Engine {
    /// Open the default device and queue the file, starting paused.
    pub fn new(path: &Path, volume_percent: u8) -> Result<Self, AudioError> {
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| AudioError::new(format!("cannot open audio device: {e}")))?;
        // We stop playback intentionally on quit; silence rodio's drop warning.
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());

        let decoder = Self::open_decoder(path)?;
        player.append(decoder);
        player.pause();

        let volume = (volume_percent.min(100) as f32) / 100.0;
        player.set_volume(volume);

        Ok(Engine {
            _sink: sink,
            player,
            path: path.to_path_buf(),
            volume,
            muted: false,
        })
    }

    /// Build a fresh decoder for the loaded file.
    fn open_decoder(path: &Path) -> Result<Decoder<std::io::BufReader<File>>, AudioError> {
        let file = File::open(path)
            .map_err(|e| AudioError::new(format!("cannot open '{}': {}", path.display(), e)))?;
        Decoder::try_from(file)
            .map_err(|e| AudioError::new(format!("cannot decode '{}': {e}", path.display())))
    }

    /// True once playback has run past the end of the track: the queue has
    /// drained, so seeking and play/pause no longer have a source to act on
    /// until we re-queue one.
    pub fn finished(&self) -> bool {
        self.player.empty()
    }

    /// Re-queue a fresh decoder after the track has finished, restoring volume
    /// and optionally seeking to `pos`. `play` resumes immediately when true.
    fn requeue(&self, pos: Duration, play: bool) {
        let Ok(decoder) = Self::open_decoder(&self.path) else {
            return;
        };
        self.player.append(decoder);
        self.player
            .set_volume(if self.muted { 0.0 } else { self.volume });
        if play {
            self.player.play();
        } else {
            self.player.pause();
        }
        if pos > Duration::ZERO {
            let _ = self.player.try_seek(pos);
        }
    }

    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    pub fn toggle_pause(&self) {
        if self.finished() {
            // Nothing left in the queue — restart the track from the top.
            self.requeue(Duration::ZERO, true);
        } else if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
        }
    }

    /// Stop: pause and return to the start of the track.
    pub fn stop(&self) {
        if self.finished() {
            self.requeue(Duration::ZERO, false);
            return;
        }
        let _ = self.player.try_seek(Duration::ZERO);
        self.player.pause();
    }

    pub fn position(&self) -> Duration {
        self.player.get_pos()
    }

    /// Seek by a signed number of seconds, clamped to `[0, total]`.
    pub fn seek_relative(&self, delta_secs: f32, total: Duration) {
        let cur = self.player.get_pos().as_secs_f32();
        let target = (cur + delta_secs).clamp(0.0, total.as_secs_f32());
        let target = Duration::from_secs_f32(target);
        if self.finished() {
            // Ran off the end — re-queue before seeking so the position sticks,
            // staying paused so completed playback doesn't spontaneously resume.
            self.requeue(target, false);
        } else {
            let _ = self.player.try_seek(target);
        }
    }

    /// Jump to the start of the track, preserving the current play/pause state.
    pub fn seek_to_start(&self) {
        if self.finished() {
            // Re-queue paused so a completed track doesn't resume on its own.
            self.requeue(Duration::ZERO, false);
        } else {
            let _ = self.player.try_seek(Duration::ZERO);
        }
    }

    /// Jump to the end of the track. Nothing to do if it already finished.
    pub fn seek_to_end(&self, total: Duration) {
        if !self.finished() {
            let _ = self.player.try_seek(total);
        }
    }

    /// Jump to an absolute position, clamped to `[0, total]`, preserving the
    /// current play/pause state (re-queues paused if the track had finished).
    pub fn seek_to(&self, target: Duration, total: Duration) {
        let target = target.min(total);
        if self.finished() {
            self.requeue(target, false);
        } else {
            let _ = self.player.try_seek(target);
        }
    }

    pub fn volume_percent(&self) -> u8 {
        (self.volume * 100.0).round() as u8
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn adjust_volume(&mut self, delta_percent: i32) {
        let cur = (self.volume * 100.0).round() as i32;
        let next = (cur + delta_percent).clamp(0, 100) as f32 / 100.0;
        self.volume = next;
        if !self.muted {
            self.player.set_volume(next);
        }
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        self.player
            .set_volume(if self.muted { 0.0 } else { self.volume });
    }

    /// Path of the currently loaded file (for display).
    pub fn path(&self) -> &Path {
        &self.path
    }
}
