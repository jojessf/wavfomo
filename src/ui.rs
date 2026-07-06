//! Terminal rendering with ratatui.
//!
//! The waveform and spectrograph are drawn directly into the cell buffer so we
//! can keep the background transparent (only foreground glyphs are colored) and
//! target 256-color output.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::dsp::{self};

const PLAYHEAD: Color = Color::Indexed(226); // bright yellow

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Decide which panes are visible.
    let show_wave = app.config.waveform.show && app.config.waveform.generate;
    let show_spec = app.config.spectrograph.show
        && app.config.spectrograph.generate
        && app.spectrogram.is_some();

    let mut constraints = vec![Constraint::Length(2), Constraint::Length(1)];
    if show_wave {
        constraints.push(Constraint::Ratio(1, 2));
    }
    if show_spec {
        constraints.push(Constraint::Ratio(1, 2));
    }
    constraints.push(Constraint::Length(1));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut idx = 0;
    draw_header(frame, chunks[idx], app);
    idx += 1;
    draw_progress(frame, chunks[idx], app);
    idx += 1;
    if show_wave {
        draw_waveform(frame, chunks[idx], app);
        idx += 1;
    }
    if show_spec {
        draw_spectrogram(frame, chunks[idx], app);
        idx += 1;
    }
    draw_footer(frame, chunks[idx]);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let name = app
        .engine
        .path()
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "?".into());

    let state = if app.engine.is_paused() {
        "⏸ paused"
    } else {
        "▶ playing"
    };
    let vol = if app.engine.is_muted() {
        "muted".to_string()
    } else {
        format!("{}%", app.engine.volume_percent())
    };

    let meta = format!(
        "{} Hz · {} ch · {}",
        app.audio.sample_rate,
        app.audio.channels,
        fmt_duration(app.audio.duration.as_secs_f32()),
    );

    let lines = vec![
        Line::from(vec![
            Span::styled(
                name,
                Style::default()
                    .fg(Color::Indexed(45))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(meta, Style::default().fg(Color::Indexed(244))),
        ]),
        Line::from(vec![
            Span::styled(state, Style::default().fg(Color::Indexed(150))),
            Span::raw("   vol "),
            Span::styled(vol, Style::default().fg(Color::Indexed(180))),
            Span::raw(format!(
                "   zoom {:.1}x   vscale {:.1}x",
                app.zoom, app.vertical_scale
            )),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_progress(frame: &mut Frame, area: Rect, app: &App) {
    let total = app.audio.duration.as_secs_f32().max(0.001);
    let pos = app.engine.position().as_secs_f32().min(total);
    let ratio = (pos / total).clamp(0.0, 1.0) as f64;
    let label = format!("{} / {}", fmt_duration(pos), fmt_duration(total));
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Indexed(39)))
        .ratio(ratio)
        .label(label);
    frame.render_widget(gauge, area);
}

fn draw_waveform(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(" waveform ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let (start, end) = app.visible_range();
    let cols = inner.width as usize;
    let peaks = dsp::waveform_peaks(&app.audio.mono, start, end, cols);

    let mid = inner.y + inner.height / 2;
    let half = (inner.height / 2).max(1) as f32;
    let buf = frame.buffer_mut();
    let wave_color = Color::Indexed(37);

    for (i, (lo, hi)) in peaks.iter().enumerate() {
        let x = inner.x + i as u16;
        let amp = (lo.abs().max(hi.abs()) * app.vertical_scale).clamp(0.0, 1.0);
        let h = (amp * half).round() as u16;
        for dy in 0..=h {
            for y in [mid.saturating_sub(dy), (mid + dy).min(inner.bottom() - 1)] {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_symbol("█").set_style(Style::default().fg(wave_color));
                }
            }
        }
    }

    // Playhead: waveform indices span the whole file.
    let full_len = app.audio.mono.len();
    draw_playhead(buf, inner, start, end, full_len, app);
}

fn draw_spectrogram(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" spectrograph ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let Some(spec) = &app.spectrogram else {
        return;
    };
    if spec.frames.is_empty() {
        return;
    }

    let n_frames = spec.frames.len();
    let bins = spec.bins.max(1);
    let w = inner.width as usize;
    let h = inner.height as usize;
    let log_freq = app.config.spectrograph.log_frequency();
    let buf = frame.buffer_mut();

    for cx in 0..w {
        // Map column to a spectrogram frame.
        let frame_idx = if w <= 1 {
            0
        } else {
            cx * (n_frames - 1) / (w - 1)
        };
        let column = &spec.frames[frame_idx];
        for cy in 0..h {
            // Row 0 is the top (high frequency); map to a bin.
            let frac_from_bottom = if h <= 1 {
                0.0
            } else {
                (h - 1 - cy) as f32 / (h - 1) as f32
            };
            let bin = if log_freq {
                ((bins as f32).powf(frac_from_bottom) - 1.0)
                    .clamp(0.0, (bins - 1) as f32) as usize
            } else {
                (frac_from_bottom * (bins - 1) as f32) as usize
            };
            let v = column.get(bin).copied().unwrap_or(0.0);
            if v <= 0.02 {
                continue; // leave transparent
            }
            let x = inner.x + cx as u16;
            let y = inner.y + cy as u16;
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_symbol("█")
                    .set_style(Style::default().fg(heat_color(v)));
            }
        }
    }

    // Playhead: spectrogram indices span all frames.
    draw_playhead(buf, inner, 0, n_frames, n_frames, app);
}

/// Draw a vertical playhead marker mapped from playback position onto a pane
/// spanning sample/frame indices `[start, end)`.
fn draw_playhead(
    buf: &mut ratatui::buffer::Buffer,
    inner: Rect,
    start: usize,
    end: usize,
    full_len: usize,
    app: &App,
) {
    if end <= start || inner.width == 0 || full_len == 0 {
        return;
    }
    let total = app.audio.duration.as_secs_f32().max(0.001);
    let pos = app.engine.position().as_secs_f32().min(total);
    let frac = (pos / total).clamp(0.0, 1.0);
    let idx = (frac * full_len as f32) as usize;
    if idx < start || idx >= end {
        return;
    }
    let col = (idx - start) * inner.width as usize / (end - start);
    let x = inner.x + col.min(inner.width as usize - 1) as u16;
    for y in inner.y..inner.bottom() {
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_symbol("│").set_style(Style::default().fg(PLAYHEAD));
        }
    }
}

fn draw_footer(frame: &mut Frame, area: Rect) {
    let hint = "Space play/pause  ·  s stop  ·  ←→ seek (Shift ±10s, Alt ±0.1s)  ·  ↑↓ vol (Shift zoom, Alt vscale)  ·  m mute  ·  q quit";
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::Indexed(240)),
        ))),
        area,
    );
}

/// Map an intensity 0.0–1.0 to a 256-color heat ramp (inferno-ish).
fn heat_color(v: f32) -> Color {
    // A handful of stops through the 256-color cube: dark → purple → red →
    // orange → yellow → white.
    const STOPS: [u8; 8] = [17, 54, 91, 160, 196, 202, 214, 231];
    let v = v.clamp(0.0, 1.0);
    let i = (v * (STOPS.len() - 1) as f32).round() as usize;
    Color::Indexed(STOPS[i.min(STOPS.len() - 1)])
}

fn fmt_duration(secs: f32) -> String {
    let secs = secs.max(0.0) as u32;
    format!("{}:{:02}", secs / 60, secs % 60)
}
