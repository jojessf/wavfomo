//! Application state, input handling, and the main event loop.

use std::io;
use std::path::Path;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::audio::{self, AudioData, AudioError, Engine};
use crate::config::{Config, Zoom};
use crate::dsp::{self, Spectrogram};
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
    should_quit: bool,
}

impl App {
    /// Decode the file, precompute visualizer data, and open the audio device.
    pub fn load(config: Config, path: &Path) -> Result<Self, AudioError> {
        let audio = audio::decode_to_memory(path)?;

        let spectrogram = if config.spectrograph.generate {
            eprint!("Analyzing spectrogram... ");
            let spec = dsp::compute_spectrogram(
                &audio.mono,
                config.spectrograph.fft_size,
                config.spectrograph.overlap,
                config.spectrograph.magnitude,
                |done, total| {
                    if total > 0 {
                        eprint!("\rAnalyzing spectrogram... {:>3}%", done * 100 / total);
                    }
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
        let m = key.modifiers;
        let shift = m.contains(KeyModifiers::SHIFT);
        let alt = m.contains(KeyModifiers::ALT);
        let total = self.audio.duration;

        match key.code {
            KeyCode::Char(' ') => self.engine.toggle_pause(),
            KeyCode::Char('s') => self.engine.stop(),
            KeyCode::Char('m') => self.engine.toggle_mute(),
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,

            KeyCode::Left | KeyCode::Right => {
                let sign = if matches!(key.code, KeyCode::Left) {
                    -1.0
                } else {
                    1.0
                };
                let step = if shift {
                    self.config.seek.large
                } else if alt {
                    self.config.seek.fine
                } else {
                    self.config.seek.normal
                };
                self.engine.seek_relative(sign * step, total);
            }

            KeyCode::Up | KeyCode::Down => {
                let up = matches!(key.code, KeyCode::Up);
                if shift {
                    // Waveform zoom.
                    self.zoom = if up {
                        (self.zoom * ZOOM_FACTOR).min(1024.0)
                    } else {
                        (self.zoom / ZOOM_FACTOR).max(1.0)
                    };
                } else if alt {
                    // Waveform vertical scale.
                    self.vertical_scale = if up {
                        (self.vertical_scale * ZOOM_FACTOR).min(100.0)
                    } else {
                        (self.vertical_scale / ZOOM_FACTOR).max(0.1)
                    };
                } else {
                    // Volume.
                    self.engine
                        .adjust_volume(if up { VOLUME_STEP } else { -VOLUME_STEP });
                }
            }
            _ => {}
        }
    }

    /// Run the TUI event loop until the user quits.
    pub fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| ui::render(frame, self))?;

            if event::poll(TICK)? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key);
                }
            }
        }
        Ok(())
    }
}
