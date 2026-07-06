//! Runtime configuration.
//!
//! For now these are constructed from built-in defaults only. Loading them from
//! a TOML config file (per the README) requires the `serde` + `toml` crates;
//! once those are added, these structs get `#[derive(Deserialize)]` and a
//! `Config::load()` path. The field layout already mirrors the documented
//! `[audio]`, `[colors]`, `[seek]`, `[waveform]`, `[spectrograph]` sections.

/// How a visualizer treats multiple channels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelMode {
    /// Downmix all channels to one trace (default).
    Mono,
    /// Show channels separately.
    Stereo,
}

/// Horizontal zoom for the waveform.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Zoom {
    /// Fit the whole file across the pane width.
    Fit,
    /// Zoom factor relative to `Fit` (2.0 shows half the track, etc.).
    Factor(f32),
}

/// Spectrograph intensity mapping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Magnitude {
    /// Logarithmic (decibel) mapping (default).
    Db,
    /// Linear magnitude.
    Linear,
}

#[derive(Clone, Debug)]
pub struct AudioConfig {
    /// Starting volume, 0–100.
    pub volume: u8,
}

#[derive(Clone, Debug)]
pub struct SeekConfig {
    /// `Alt+←/→` step, seconds.
    pub fine: f32,
    /// `←/→` step, seconds.
    pub normal: f32,
    /// `Shift+←/→` step, seconds.
    pub large: f32,
}

#[derive(Clone, Debug)]
pub struct WaveformConfig {
    pub generate: bool,
    pub show: bool,
    pub channels: ChannelMode,
    pub zoom: Zoom,
    /// Amplitude magnification.
    pub vertical_scale: f32,
}

#[derive(Clone, Debug)]
pub struct SpectrographConfig {
    pub generate: bool,
    pub show: bool,
    pub channels: ChannelMode,
    /// FFT window size (power of two).
    pub fft_size: usize,
    /// Window overlap fraction between frames (0.0–1.0).
    pub overlap: f32,
    pub magnitude: Magnitude,
    /// Logarithmic frequency axis when true.
    pub log_frequency: bool,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub audio: AudioConfig,
    pub seek: SeekConfig,
    pub waveform: WaveformConfig,
    pub spectrograph: SpectrographConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            audio: AudioConfig { volume: 100 },
            seek: SeekConfig {
                fine: 0.1,
                normal: 1.0,
                large: 10.0,
            },
            waveform: WaveformConfig {
                generate: true,
                show: true,
                channels: ChannelMode::Mono,
                zoom: Zoom::Fit,
                vertical_scale: 1.0,
            },
            spectrograph: SpectrographConfig {
                generate: true,
                show: true,
                channels: ChannelMode::Mono,
                fft_size: 2048,
                overlap: 0.5,
                magnitude: Magnitude::Db,
                log_frequency: true,
            },
        }
    }
}
