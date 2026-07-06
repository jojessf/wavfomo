//! Runtime configuration, loaded from a TOML file with built-in defaults.
//!
//! The layout mirrors the documented `[audio]`, `[colors]`, `[seek]`,
//! `[waveform]`, `[spectrograph]`, and `[hotkeys]` sections. Every struct uses
//! `#[serde(default)]` so partial config files fall back to defaults per field.

use std::error::Error;
use std::path::{Path, PathBuf};

use serde::de::{self, Deserializer};
use serde::Deserialize;

use crate::hotkeys::{self, Action, Keymap};

/// How a visualizer treats multiple channels.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelMode {
    Mono,
    Stereo,
}

/// Spectrograph intensity mapping.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Magnitude {
    Db,
    Linear,
}

/// Spectrograph frequency axis.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FreqScale {
    Log,
    Linear,
}

/// Horizontal zoom for the waveform: `"fit"` or a numeric factor.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Zoom {
    Fit,
    Factor(f32),
}

impl<'de> Deserialize<'de> for Zoom {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Str(String),
            Num(f32),
        }
        match Repr::deserialize(deserializer)? {
            Repr::Str(s) if s.eq_ignore_ascii_case("fit") => Ok(Zoom::Fit),
            Repr::Str(s) => Err(de::Error::custom(format!("invalid zoom '{s}' (use \"fit\" or a number)"))),
            Repr::Num(n) => Ok(Zoom::Factor(n)),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub volume: u8,
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig { volume: 100 }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ColorsConfig {
    pub depth: String,
    pub background: String,
}

impl Default for ColorsConfig {
    fn default() -> Self {
        ColorsConfig {
            depth: "256".into(),
            background: "transparent".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct SeekConfig {
    pub fine: f32,
    pub normal: f32,
    pub large: f32,
}

impl Default for SeekConfig {
    fn default() -> Self {
        SeekConfig {
            fine: 0.1,
            normal: 1.0,
            large: 10.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct WaveformConfig {
    pub generate: bool,
    pub show: bool,
    pub channels: ChannelMode,
    pub zoom: Zoom,
    pub vertical_scale: f32,
    pub color_preset: String,
}

impl Default for WaveformConfig {
    fn default() -> Self {
        WaveformConfig {
            generate: true,
            show: true,
            channels: ChannelMode::Mono,
            zoom: Zoom::Fit,
            vertical_scale: 1.0,
            color_preset: "aurora".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct SpectrographConfig {
    pub generate: bool,
    pub show: bool,
    pub channels: ChannelMode,
    pub fft_size: usize,
    pub window: String,
    pub overlap: f32,
    pub magnitude: Magnitude,
    pub scale: FreqScale,
    pub color_preset: String,
}

impl Default for SpectrographConfig {
    fn default() -> Self {
        SpectrographConfig {
            generate: true,
            show: true,
            channels: ChannelMode::Mono,
            fft_size: 2048,
            window: "hann".into(),
            overlap: 0.5,
            magnitude: Magnitude::Db,
            scale: FreqScale::Log,
            color_preset: "inferno".into(),
        }
    }
}

impl SpectrographConfig {
    pub fn log_frequency(&self) -> bool {
        matches!(self.scale, FreqScale::Log)
    }
}

/// User-remappable key bindings; values are `"Modifier+Key"` strings.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct HotkeysConfig {
    pub play_pause: String,
    pub stop: String,
    pub seek_back: String,
    pub seek_forward: String,
    pub seek_back_large: String,
    pub seek_forward_large: String,
    pub seek_back_fine: String,
    pub seek_forward_fine: String,
    pub waveform_zoom_in: String,
    pub waveform_zoom_out: String,
    pub vscale_increase: String,
    pub vscale_decrease: String,
    pub volume_up: String,
    pub volume_down: String,
    pub mute: String,
    pub quit: String,
}

impl Default for HotkeysConfig {
    fn default() -> Self {
        HotkeysConfig {
            play_pause: "Space".into(),
            stop: "s".into(),
            seek_back: "Left".into(),
            seek_forward: "Right".into(),
            seek_back_large: "Shift+Left".into(),
            seek_forward_large: "Shift+Right".into(),
            seek_back_fine: "Alt+Left".into(),
            seek_forward_fine: "Alt+Right".into(),
            waveform_zoom_in: "Shift+Up".into(),
            waveform_zoom_out: "Shift+Down".into(),
            vscale_increase: "Alt+Up".into(),
            vscale_decrease: "Alt+Down".into(),
            volume_up: "Up".into(),
            volume_down: "Down".into(),
            mute: "m".into(),
            quit: "q".into(),
        }
    }
}

impl HotkeysConfig {
    fn bindings(&self) -> Vec<(String, Action)> {
        vec![
            (self.play_pause.clone(), Action::PlayPause),
            (self.stop.clone(), Action::Stop),
            (self.seek_back.clone(), Action::SeekBack),
            (self.seek_forward.clone(), Action::SeekForward),
            (self.seek_back_large.clone(), Action::SeekBackLarge),
            (self.seek_forward_large.clone(), Action::SeekForwardLarge),
            (self.seek_back_fine.clone(), Action::SeekBackFine),
            (self.seek_forward_fine.clone(), Action::SeekForwardFine),
            (self.waveform_zoom_in.clone(), Action::WaveformZoomIn),
            (self.waveform_zoom_out.clone(), Action::WaveformZoomOut),
            (self.vscale_increase.clone(), Action::VscaleIncrease),
            (self.vscale_decrease.clone(), Action::VscaleDecrease),
            (self.volume_up.clone(), Action::VolumeUp),
            (self.volume_down.clone(), Action::VolumeDown),
            (self.mute.clone(), Action::Mute),
            (self.quit.clone(), Action::Quit),
        ]
    }

    /// Parse all bindings into a lookup map.
    pub fn keymap(&self) -> Result<Keymap, String> {
        hotkeys::build_keymap(&self.bindings())
    }
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub audio: AudioConfig,
    pub colors: ColorsConfig,
    pub seek: SeekConfig,
    pub waveform: WaveformConfig,
    pub spectrograph: SpectrographConfig,
    pub hotkeys: HotkeysConfig,
}

impl Config {
    /// Load configuration. If `explicit` is given, that file must parse; missing
    /// files (explicit or default) fall back to built-in defaults.
    pub fn load(explicit: Option<&Path>) -> Result<Config, Box<dyn Error>> {
        let path = match explicit {
            Some(p) => Some(p.to_path_buf()),
            None => default_config_path(),
        };

        let Some(path) = path else {
            return Ok(Config::default());
        };
        if !path.exists() {
            return Ok(Config::default());
        }

        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("reading config '{}': {e}", path.display()))?;
        let config: Config = toml::from_str(&text)
            .map_err(|e| format!("parsing config '{}': {e}", path.display()))?;
        Ok(config)
    }
}

/// `$XDG_CONFIG_HOME/wavfomo/config.toml`, falling back to
/// `~/.config/wavfomo/config.toml`.
fn default_config_path() -> Option<PathBuf> {
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME")
        && !x.is_empty() {
            return Some(PathBuf::from(x).join("wavfomo").join("config.toml"));
        }
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty() {
            return Some(
                PathBuf::from(home)
                    .join(".config")
                    .join("wavfomo")
                    .join("config.toml"),
            );
        }
    None
}
