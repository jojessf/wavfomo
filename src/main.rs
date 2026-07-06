//! wavfomo — a terminal audio player with waveform and spectrograph visualizers.

mod app;
mod audio;
mod config;
mod dsp;
mod hotkeys;
mod ui;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

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
    #[arg(short, long, value_name = "0-100")]
    volume: Option<u8>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

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

    let mut terminal = ratatui::init();
    let result = application.run(&mut terminal);
    ratatui::restore();

    if let Err(e) = result {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
