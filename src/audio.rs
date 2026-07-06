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
pub fn decode_to_memory(path: &Path) -> Result<AudioData, AudioError> {
    let file = File::open(path).map_err(|e| {
        AudioError::new(format!("cannot open '{}': {}", path.display(), e))
    })?;

    let decoder = Decoder::try_from(file).map_err(|e| {
        AudioError::new(format!(
            "cannot decode '{}': {e}. Unsupported or unrecognized format \
             (supported: WAV, FLAC, Ogg Vorbis, MP3) — this format would need \
             to be added.",
            path.display()
        ))
    })?;

    let channels = decoder.channels().get();
    let sample_rate = decoder.sample_rate().get();

    // Collect interleaved samples, then downmix to mono.
    let interleaved: Vec<f32> = decoder.collect();
    let ch = channels.max(1) as usize;
    let frames = interleaved.len() / ch;
    let mut mono = Vec::with_capacity(frames);
    for frame in interleaved.chunks_exact(ch) {
        let sum: f32 = frame.iter().copied().sum();
        mono.push(sum / ch as f32);
    }

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
        let sink = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| AudioError::new(format!("cannot open audio device: {e}")))?;
        let player = Player::connect_new(sink.mixer());

        let file = File::open(path)
            .map_err(|e| AudioError::new(format!("cannot open '{}': {}", path.display(), e)))?;
        let decoder = Decoder::try_from(file)
            .map_err(|e| AudioError::new(format!("cannot decode '{}': {e}", path.display())))?;
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

    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    pub fn toggle_pause(&self) {
        if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
        }
    }

    /// Stop: pause and return to the start of the track.
    pub fn stop(&self) {
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
        let _ = self.player.try_seek(Duration::from_secs_f32(target));
    }

    pub fn seek_to(&self, pos: Duration) {
        let _ = self.player.try_seek(pos);
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
