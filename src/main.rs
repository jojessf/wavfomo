//! wavfomo — a terminal audio player with waveform and spectrograph visualizers.

mod app;
mod audio;
mod config;
mod dsp;
mod hotkeys;
mod ui;

use std::io::stdout;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;

use app::App;
use config::Config;

/// Terminal audio player with waveform and spectrograph visualizers.
#[derive(Parser, Debug)]
#[command(name = "wavfomo", version, about)]
struct Cli {
    /// Audio file to play (WAV, FLAC, Ogg Vorbis, MP3).
    file: PathBuf,

    /// Use a specific config file (overrides the default path).
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// FFT window size for the spectrograph (power of two).
    #[arg(long, value_name = "N")]
    fft_size: Option<usize>,

    /// Disable both visualizers for this run.
    #[arg(long)]
    no_viz: bool,

    /// Initial volume, 0–100.
    #[arg(long, visible_alias = "vol", value_name = "0-100")]
    volume: Option<u8>,

    /// Print extra diagnostic output.
    #[arg(short = 'v', long)]
    verbose: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.verbose {
        match Config::resolve_path(cli.config.as_deref()) {
            Some(p) => eprintln!("config: {}", p.display()),
            None => eprintln!("config: built-in defaults (no file found)"),
        }
    }

    // Load config (built-in defaults if no file), then apply CLI overrides.
    let mut config = match Config::load(cli.config.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(v) = cli.volume {
        config.audio.volume = v.min(100);
    }
    if let Some(n) = cli.fft_size {
        config.spectrograph.fft_size = n;
    }
    if cli.no_viz {
        config.waveform.show = false;
        config.waveform.generate = false;
        config.spectrograph.show = false;
        config.spectrograph.generate = false;
    }

    if !cli.file.exists() {
        eprintln!("error: file not found: {}", cli.file.display());
        return ExitCode::FAILURE;
    }

    // Decode + precompute happen here, before entering the TUI, so any failure
    // is reported as a plain CLI error.
    let mut application = match App::load(config, &cli.file) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if cli.verbose {
        let a = &application.audio;
        eprintln!(
            "loaded: {} — {} Hz, {} ch, {:.1}s, {} spectrogram frames",
            cli.file.display(),
            a.sample_rate,
            a.channels,
            a.duration.as_secs_f32(),
            application.spectrogram.as_ref().map_or(0, |s| s.frames.len()),
        );
    }

    let mut terminal = ratatui::init();
    // Ask the terminal to disambiguate modified keys (Shift+Arrow, etc.) via the
    // enhanced keyboard protocol, where supported. Without this, many terminals
    // don't deliver Shift+↑/↓ to the app at all.
    let enhanced = enable_keyboard_enhancement();
    let result = application.run(&mut terminal);
    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    ratatui::restore();

    if let Err(e) = result {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Enable the enhanced keyboard protocol if the terminal supports it. Returns
/// whether flags were pushed (so they can be popped on exit).
fn enable_keyboard_enhancement() -> bool {
    match crossterm::terminal::supports_keyboard_enhancement() {
        Ok(true) => execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok(),
        _ => false,
    }
}
