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

/// How a visualizer pane is drawn. `Block` is the built-in cell renderer;
/// `Dot`/`Braille` use ratatui's `Canvas` (`symbols::Marker`) for sub-cell
/// resolution.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkerMode {
    Block,
    Dot,
    Braille,
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
    pub marker: MarkerMode,
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
            marker: MarkerMode::Block,
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
    pub marker: MarkerMode,
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
            marker: MarkerMode::Block,
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
    pub seek_start: String,
    pub seek_end: String,
    pub goto: String,
    pub waveform_zoom_in: String,
    pub waveform_zoom_out: String,
    pub vscale_increase: String,
    pub vscale_decrease: String,
    pub volume_up: String,
    pub volume_down: String,
    pub mute: String,
    pub menu: String,
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
            seek_start: "Home".into(),
            seek_end: "End".into(),
            goto: "g".into(),
            waveform_zoom_in: "Ctrl+PageUp".into(),
            waveform_zoom_out: "Ctrl+PageDown".into(),
            vscale_increase: "Alt+Up".into(),
            vscale_decrease: "Alt+Down".into(),
            volume_up: "Up".into(),
            volume_down: "Down".into(),
            mute: "m".into(),
            menu: "F1".into(),
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
            (self.seek_start.clone(), Action::SeekStart),
            (self.seek_end.clone(), Action::SeekEnd),
            (self.goto.clone(), Action::Goto),
            (self.waveform_zoom_in.clone(), Action::WaveformZoomIn),
            (self.waveform_zoom_out.clone(), Action::WaveformZoomOut),
            (self.vscale_increase.clone(), Action::VscaleIncrease),
            (self.vscale_decrease.clone(), Action::VscaleDecrease),
            (self.volume_up.clone(), Action::VolumeUp),
            (self.volume_down.clone(), Action::VolumeDown),
            (self.mute.clone(), Action::Mute),
            (self.menu.clone(), Action::ToggleMenu),
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
    /// Load configuration. An explicit `--config` path (if it exists) wins;
    /// otherwise the first existing file in [`candidate_paths`] is used. A
    /// missing file (explicit or discovered) falls back to built-in defaults; an
    /// existing file that fails to parse is an error.
    pub fn load(explicit: Option<&Path>) -> Result<Config, Box<dyn Error>> {
        let Some(path) = Self::resolve_path(explicit) else {
            return Ok(Config::default());
        };

        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("reading config '{}': {e}", path.display()))?;
        let config: Config = toml::from_str(&text)
            .map_err(|e| format!("parsing config '{}': {e}", path.display()))?;
        Ok(config)
    }

    /// The config file that [`load`](Self::load) would use, if any exists.
    pub fn resolve_path(explicit: Option<&Path>) -> Option<PathBuf> {
        match explicit {
            Some(p) if p.exists() => Some(p.to_path_buf()),
            Some(_) => None,
            None => candidate_paths().into_iter().find(|p| p.exists()),
        }
    }
}

/// Config-file search locations, highest priority first:
///
/// 1. `./wavfomo.config` (current directory)
/// 2. `~/.wavfomo.config`
/// 3. `$XDG_CONFIG_HOME/wavfomo/config.toml`
/// 4. `~/.config/wavfomo/config.toml`
fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from("wavfomo.config")];

    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty());
    if let Some(home) = &home {
        paths.push(PathBuf::from(home).join(".wavfomo.config"));
    }
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME")
        && !x.is_empty()
    {
        paths.push(PathBuf::from(x).join("wavfomo").join("config.toml"));
    }
    if let Some(home) = &home {
        paths.push(
            PathBuf::from(home)
                .join(".config")
                .join("wavfomo")
                .join("config.toml"),
        );
    }
    paths
}
