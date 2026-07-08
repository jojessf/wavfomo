//! Application state, input handling, and the main event loop.

use std::error::Error;
use std::io;
use std::path::Path;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};

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
    /// Active "goto timestamp" prompt, if the user is typing one.
    pub goto: Option<GotoInput>,
    keymap: Keymap,
    should_quit: bool,
}

/// State for the inline "goto timestamp" text prompt (opened with `g`).
pub struct GotoInput {
    pub buffer: String,
    /// Set when the last Enter failed validation, so the UI can flag it.
    pub error: bool,
}

/// Parse an `m:ss` timestamp (e.g. `1:23`) into a duration. Requires one or
/// more minute digits, a colon, then exactly two second digits — matching
/// `^\d+:\d{2}$`. Returns `None` on any other shape.
fn parse_timestamp(s: &str) -> Option<Duration> {
    let (mins, secs) = s.split_once(':')?;
    if mins.is_empty() || !mins.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if secs.len() != 2 || !secs.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mins: u64 = mins.parse().ok()?;
    let secs: u64 = secs.parse().ok()?;
    Some(Duration::from_secs(mins * 60 + secs))
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
            goto: None,
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

    /// The visible time window `[start, end)` in seconds, matching the
    /// waveform's zoom/seek window (whole track when zoomed out).
    pub fn visible_time_range(&self) -> (f32, f32) {
        let (s, e) = self.visible_range();
        let sr = self.audio.sample_rate.max(1) as f32;
        (s as f32 / sr, e as f32 / sr)
    }

    /// The visible spectrogram frame range `[start, end)`, mapped from the
    /// waveform's visible sample window so both panes zoom and seek together.
    pub fn visible_frame_range(&self, n_frames: usize) -> (usize, usize) {
        let len = self.audio.mono.len();
        if len == 0 || n_frames == 0 {
            return (0, n_frames);
        }
        let (s, e) = self.visible_range();
        let fstart = s * n_frames / len;
        // Round the end up so the window never collapses below the sample span.
        let fend = e.saturating_mul(n_frames).div_ceil(len).min(n_frames);
        (fstart, fend.max(fstart + 1).min(n_frames))
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        // While the goto prompt is open, keystrokes edit its text instead of
        // triggering hotkeys.
        if self.goto.is_some() {
            self.handle_goto_key(key);
            return;
        }
        let entry = hotkeys::normalize(key.code, key.modifiers);
        let Some(&action) = self.keymap.get(&entry) else {
            return;
        };
        self.dispatch(action);
    }

    /// Editing keys for the open goto prompt: type `m:ss`, Enter to jump, Esc to
    /// cancel. Enter on an invalid string flags the error and keeps the prompt.
    fn handle_goto_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.goto = None,
            KeyCode::Enter => {
                let text = self.goto.as_ref().map(|g| g.buffer.clone()).unwrap_or_default();
                if let Some(target) = parse_timestamp(&text) {
                    self.engine.seek_to(target, self.audio.duration);
                    self.goto = None;
                } else if let Some(g) = self.goto.as_mut() {
                    g.error = true;
                }
            }
            KeyCode::Backspace => {
                if let Some(g) = self.goto.as_mut() {
                    g.buffer.pop();
                    g.error = false;
                }
            }
            KeyCode::Char(c) => {
                if let Some(g) = self.goto.as_mut() {
                    g.buffer.push(c);
                    g.error = false;
                }
            }
            _ => {}
        }
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
            Action::SeekStart => self.engine.seek_to_start(),
            Action::SeekEnd => self.engine.seek_to_end(total),
            Action::Goto => {
                self.goto = Some(GotoInput {
                    buffer: String::new(),
                    error: false,
                })
            }

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
