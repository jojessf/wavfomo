//! Application state, input handling, and the main event loop.

use std::error::Error;
use std::io;
use std::path::Path;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};

use crate::audio::{self, AudioData, Engine};
use crate::config::{Config, Zoom};
use crate::dsp::{self, Spectrogram};
use crate::hotkeys::{self, Action, Keymap};
use crate::ui;

const VOLUME_STEP: i32 = 5;
const ZOOM_FACTOR: f32 = 1.1;
const TICK: Duration = Duration::from_millis(50);

pub struct App {
    pub config: Config,
    pub audio: AudioData,
    pub engine: Engine,
    pub spectrogram: Option<Spectrogram>,
    /// Horizontal zoom factor, 1.0 == fit whole file.
    pub zoom: f32,
    pub vertical_scale: f32,
    keymap: Keymap,
    should_quit: bool,
}

impl App {
    /// Decode the file, precompute visualizer data, and open the audio device.
    pub fn load(config: Config, path: &Path) -> Result<Self, Box<dyn Error>> {
        let keymap = config.hotkeys.keymap()?;

        // Decoding the whole file into memory can take a while with no visible
        // sign of progress, so report it before the spectrogram pass runs.
        eprint!("Decoding audio... ");
        const SPINNER: [char; 4] = ['|', '/', '-', '\\'];
        let mut ticks = 0usize;
        let audio = audio::decode_to_memory(path, |frac| {
            match frac {
                Some(f) => eprint!("\rDecoding audio... {:>3}%", (f * 100.0) as u32),
                // Length unknown (MP3/Ogg): fall back to a spinner.
                None => {
                    eprint!("\rDecoding audio... {}", SPINNER[ticks % SPINNER.len()]);
                    ticks += 1;
                }
            }
        })?;
        eprintln!("\rDecoding audio... done   ");

        let spectrogram = if config.spectrograph.generate {
            eprint!("Analyzing spectrogram... ");
            let spec = dsp::compute_spectrogram(
                &audio.mono,
                config.spectrograph.fft_size,
                config.spectrograph.overlap,
                config.spectrograph.magnitude,
                |done, total| {
                    let pct = done * 100 / total.max(1);
                    eprint!("\rAnalyzing spectrogram... {pct:>3}%");
                },
            );
            eprintln!("\rAnalyzing spectrogram... done   ");
            Some(spec)
        } else {
            None
        };

        let engine = Engine::new(path, config.audio.volume)?;

        let zoom = match config.waveform.zoom {
            Zoom::Fit => 1.0,
            Zoom::Factor(f) => f.max(1.0),
        };
        let vertical_scale = config.waveform.vertical_scale.max(0.1);

        Ok(App {
            config,
            audio,
            engine,
            spectrogram,
            zoom,
            vertical_scale,
            keymap,
            should_quit: false,
        })
    }

    /// The visible sample range `[start, end)` of the waveform, given zoom and
    /// the current playhead (centered when zoomed in).
    pub fn visible_range(&self) -> (usize, usize) {
        let len = self.audio.mono.len();
        if self.zoom <= 1.0 || len == 0 {
            return (0, len);
        }
        let span = ((len as f32) / self.zoom) as usize;
        let total = self.audio.duration.as_secs_f32().max(0.001);
        let frac = (self.engine.position().as_secs_f32() / total).clamp(0.0, 1.0);
        let center = (frac * len as f32) as usize;
        let half = span / 2;
        let start = center.saturating_sub(half).min(len.saturating_sub(span));
        (start, (start + span).min(len))
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        let entry = hotkeys::normalize(key.code, key.modifiers);
        let Some(&action) = self.keymap.get(&entry) else {
            return;
        };
        self.dispatch(action);
    }

    fn dispatch(&mut self, action: Action) {
        let total = self.audio.duration;
        match action {
            Action::PlayPause => self.engine.toggle_pause(),
            Action::Stop => self.engine.stop(),
            Action::Mute => self.engine.toggle_mute(),
            Action::Quit => self.should_quit = true,

            Action::SeekBack => self.engine.seek_relative(-self.config.seek.normal, total),
            Action::SeekForward => self.engine.seek_relative(self.config.seek.normal, total),
            Action::SeekBackLarge => self.engine.seek_relative(-self.config.seek.large, total),
            Action::SeekForwardLarge => self.engine.seek_relative(self.config.seek.large, total),
            Action::SeekBackFine => self.engine.seek_relative(-self.config.seek.fine, total),
            Action::SeekForwardFine => self.engine.seek_relative(self.config.seek.fine, total),

            Action::WaveformZoomIn => self.zoom = (self.zoom * ZOOM_FACTOR).min(1024.0),
            Action::WaveformZoomOut => self.zoom = (self.zoom / ZOOM_FACTOR).max(1.0),
            Action::VscaleIncrease => {
                self.vertical_scale = (self.vertical_scale * ZOOM_FACTOR).min(100.0)
            }
            Action::VscaleDecrease => {
                self.vertical_scale = (self.vertical_scale / ZOOM_FACTOR).max(0.1)
            }

            Action::VolumeUp => self.engine.adjust_volume(VOLUME_STEP),
            Action::VolumeDown => self.engine.adjust_volume(-VOLUME_STEP),
        }
    }

    /// Run the TUI event loop until the user quits.
    pub fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| ui::render(frame, self))?;

            if event::poll(TICK)?
                && let Event::Key(key) = event::read()? {
                    self.handle_key(key);
                }
        }
        Ok(())
    }
}
