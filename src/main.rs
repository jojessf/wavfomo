//! wavfomo — a terminal audio player with waveform and spectrograph visualizers.

mod app;
mod audio;
mod config;
mod dsp;
mod ui;

use std::path::PathBuf;
use std::process::ExitCode;

use app::App;
use config::Config;

const USAGE: &str = "\
wavfomo — terminal audio player

USAGE:
    wavfomo <FILE>

ARGS:
    <FILE>    Audio file to play (WAV, FLAC, Ogg Vorbis, MP3)

Full CLI options and config-file support are coming; see README.md.";

fn main() -> ExitCode {
    // Minimal argument handling for now (a full clap-based CLI + TOML config
    // are pending dependency approval — see README).
    let mut args = std::env::args().skip(1);
    let first = args.next();

    let path = match first.as_deref() {
        None | Some("-h") | Some("--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V") | Some("--version") => {
            println!("wavfomo {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(p) => PathBuf::from(p),
    };

    if !path.exists() {
        eprintln!("error: file not found: {}", path.display());
        return ExitCode::FAILURE;
    }

    let config = Config::default();

    // Decode + precompute happen here, before entering the TUI, so any failure
    // is reported as a plain CLI error.
    let mut application = match App::load(config, &path) {
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
