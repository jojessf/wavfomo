//! Terminal rendering with ratatui.
//!
//! The waveform and spectrograph are drawn directly into the cell buffer so we
//! can keep the background transparent (only foreground glyphs are colored) and
//! target 256-color output.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::config::MarkerMode;
use crate::dsp::{self};

const PLAYHEAD: Color = Color::Indexed(226); // bright yellow
const WAVE_COLOR: Color = Color::Indexed(37);

/// 256-color heat ramp stops (inferno-ish): dark → purple → red → orange →
/// yellow → white.
const HEAT_STOPS: [u8; 8] = [17, 54, 91, 160, 196, 202, 214, 231];

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
    let mut wave_area = None;
    let mut spec_area = None;
    if show_wave {
        let a = chunks[idx];
        draw_waveform(frame, a, app);
        wave_area = Some(a);
        idx += 1;
    }
    if show_spec {
        let a = chunks[idx];
        draw_spectrogram(frame, a, app);
        spec_area = Some(a);
        idx += 1;
    }
    draw_footer(frame, chunks[idx], app);

    // Overlay a time axis on the border between the panes. When both are shown
    // it lands on the waveform's bottom border (the divider between them);
    // otherwise it annotates whichever single pane is visible.
    if let Some(a) = wave_area {
        draw_time_axis(frame, a.x + 1, a.width.saturating_sub(2), a.bottom().saturating_sub(1), app);
    } else if let Some(a) = spec_area {
        draw_time_axis(frame, a.x + 1, a.width.saturating_sub(2), a.y, app);
    }
}

/// Draw evenly spaced `m:ss` labels across a border row, spanning the currently
/// visible time window. Always includes a flush-left and flush-right label, and
/// keeps at least `MIN_GAP` blank cells between adjacent labels; the number of
/// labels scales with the available width.
fn draw_time_axis(frame: &mut Frame, x0: u16, width: u16, y: u16, app: &App) {
    const MIN_GAP: usize = 6;
    let w = width as usize;
    if w < 4 {
        return;
    }
    let (t0, t1) = app.visible_time_range();

    // The widest label (largest time) sets the layout box width.
    let label_w = fmt_duration(t1).len().max(4);

    // Most labels that fit as N boxes of `label_w` with >= MIN_GAP between them:
    // N*label_w + (N-1)*MIN_GAP <= w  ⇒  N <= (w + MIN_GAP) / (label_w + MIN_GAP).
    let mut n = (w + MIN_GAP) / (label_w + MIN_GAP);
    if n < 2 {
        // Too tight for spacing; still show both ends if they don't overlap.
        n = if 2 * label_w <= w { 2 } else { 1 };
    }

    let style = Style::default().fg(Color::Indexed(250));
    let buf = frame.buffer_mut();

    // Box left edges run flush-left (0) to flush-right (w - label_w), evenly.
    let span = w.saturating_sub(label_w);
    for i in 0..n {
        let f = if n <= 1 { 0.0 } else { i as f32 / (n - 1) as f32 };
        let label = fmt_duration(t0 + f * (t1 - t0));
        let len = label.len();
        let start = if i == 0 {
            0
        } else if i == n - 1 {
            w.saturating_sub(len)
        } else {
            // Center the label within its evenly placed box.
            let box_left = ((i * span) as f32 / (n - 1) as f32).round() as usize;
            box_left + label_w.saturating_sub(len) / 2
        };
        for (j, ch) in label.char_indices() {
            let x = x0 + (start + j) as u16;
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_symbol(&ch.to_string()).set_style(style);
            }
        }
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let name = app
        .engine
        .path()
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "?".into());

    let state = if app.engine.finished() {
        "✓ done uwu"
    } else if app.engine.is_paused() {
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
    match app.config.waveform.marker {
        MarkerMode::Block => draw_waveform_block(frame, area, app),
        MarkerMode::Dot => draw_waveform_canvas(frame, area, app, Marker::Dot),
        MarkerMode::Braille => draw_waveform_canvas(frame, area, app, Marker::Braille),
    }
}

/// Default renderer: filled amplitude bars drawn straight into the cell buffer.
fn draw_waveform_block(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(" waveform ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let (start, end) = app.visible_range();
    let cols = inner.width as usize;
    let peaks = dsp::waveform_peaks_view(&app.audio.mono, &app.wave_peaks, start, end, cols);

    let mid = inner.y + inner.height / 2;
    let half = (inner.height / 2).max(1) as f32;
    let buf = frame.buffer_mut();

    for (i, (lo, hi)) in peaks.iter().enumerate() {
        let x = inner.x + i as u16;
        let amp = (lo.abs().max(hi.abs()) * app.vertical_scale).clamp(0.0, 1.0);
        let h = (amp * half).round() as u16;
        for dy in 0..=h {
            for y in [mid.saturating_sub(dy), (mid + dy).min(inner.bottom() - 1)] {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_symbol("█").set_style(Style::default().fg(WAVE_COLOR));
                }
            }
        }
    }

    // Playhead: waveform indices span the whole file.
    let full_len = app.audio.mono.len();
    draw_playhead(buf, inner, start, end, full_len, app);
}

/// Canvas renderer: min/max envelope as vertical segments at sub-cell
/// resolution via `symbols::Marker` (Dot or Braille).
fn draw_waveform_canvas(frame: &mut Frame, area: Rect, app: &App, marker: Marker) {
    let inner_w = area.width.saturating_sub(2);
    let inner_h = area.height.saturating_sub(2);
    if inner_w == 0 || inner_h == 0 {
        return;
    }

    let (start, end) = app.visible_range();
    // Oversample horizontally so Braille's 2-per-cell resolution is used.
    let res = inner_w as usize * marker_res(marker).0;
    let peaks = dsp::waveform_peaks_view(&app.audio.mono, &app.wave_peaks, start, end, res);
    let vscale = app.vertical_scale;

    // Playhead x in canvas coordinates, if it falls in the visible window.
    let full_len = app.audio.mono.len();
    let total = app.audio.duration.as_secs_f32().max(0.001);
    let pos = app.engine.position().as_secs_f32().min(total);
    let idx = ((pos / total).clamp(0.0, 1.0) * full_len as f32) as usize;
    let playhead_x = if end > start && idx >= start && idx < end {
        Some((idx - start) as f64 * res as f64 / (end - start) as f64)
    } else {
        None
    };

    let canvas = Canvas::default()
        .block(Block::default().borders(Borders::ALL).title(" waveform "))
        .marker(marker)
        .x_bounds([0.0, res.max(1) as f64])
        .y_bounds([-1.0, 1.0])
        .paint(move |ctx| {
            for (i, (lo, hi)) in peaks.iter().enumerate() {
                let x = i as f64;
                let y1 = (lo * vscale).clamp(-1.0, 1.0) as f64;
                let y2 = (hi * vscale).clamp(-1.0, 1.0) as f64;
                ctx.draw(&CanvasLine {
                    x1: x,
                    y1,
                    x2: x,
                    y2,
                    color: WAVE_COLOR,
                });
            }
            if let Some(px) = playhead_x {
                ctx.draw(&CanvasLine {
                    x1: px,
                    y1: -1.0,
                    x2: px,
                    y2: 1.0,
                    color: PLAYHEAD,
                });
            }
        });
    frame.render_widget(canvas, area);
}

fn draw_spectrogram(frame: &mut Frame, area: Rect, app: &App) {
    match app.config.spectrograph.marker {
        MarkerMode::Block => draw_spectrogram_block(frame, area, app),
        MarkerMode::Dot => draw_spectrogram_canvas(frame, area, app, Marker::Dot),
        MarkerMode::Braille => draw_spectrogram_canvas(frame, area, app, Marker::Braille),
    }
}

/// Map a display row (0 = top) to a frequency bin, honoring the log/linear axis.
fn row_to_bin(cy: usize, height: usize, bins: usize, log_freq: bool) -> usize {
    let frac_from_bottom = if height <= 1 {
        0.0
    } else {
        (height - 1 - cy) as f32 / (height - 1) as f32
    };
    if log_freq {
        ((bins as f32).powf(frac_from_bottom) - 1.0).clamp(0.0, (bins - 1) as f32) as usize
    } else {
        (frac_from_bottom * (bins - 1) as f32) as usize
    }
}

/// Default renderer: one colored cell per (time, frequency) bucket.
fn draw_spectrogram_block(frame: &mut Frame, area: Rect, app: &App) {
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
    let (fstart, fend) = app.visible_frame_range(n_frames);
    let span = fend - fstart;
    let buf = frame.buffer_mut();

    for cx in 0..w {
        let frame_idx = if w <= 1 || span <= 1 {
            fstart
        } else {
            fstart + cx * (span - 1) / (w - 1)
        };
        let column = &spec.frames[frame_idx];
        for cy in 0..h {
            let bin = row_to_bin(cy, h, bins, log_freq);
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

    // Playhead: mapped onto the visible frame window.
    draw_playhead(buf, inner, fstart, fend, n_frames, app);
}

/// Canvas renderer: sample the spectrogram at the marker's sub-cell resolution
/// and plot points grouped by heat level. Note a Braille/Dot cell carries a
/// single color, so within one cell the strongest level wins (drawn last),
/// while the sub-dots add time/frequency detail.
fn draw_spectrogram_canvas(frame: &mut Frame, area: Rect, app: &App, marker: Marker) {
    let inner_w = area.width.saturating_sub(2) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" spectrograph ");
    if inner_w == 0 || inner_h == 0 {
        frame.render_widget(block, area);
        return;
    }
    let Some(spec) = &app.spectrogram else {
        frame.render_widget(block, area);
        return;
    };
    if spec.frames.is_empty() {
        frame.render_widget(block, area);
        return;
    }

    let n_frames = spec.frames.len();
    let bins = spec.bins.max(1);
    let log_freq = app.config.spectrograph.log_frequency();
    let (rx, ry) = marker_res(marker);
    let sw = inner_w * rx;
    let sh = inner_h * ry;
    let (fstart, fend) = app.visible_frame_range(n_frames);
    let span = fend - fstart;

    // Bucket sub-cell points by heat level so each level draws in its color.
    let mut buckets: Vec<Vec<(f64, f64)>> = vec![Vec::new(); HEAT_STOPS.len()];
    for sx in 0..sw {
        let frame_idx = if sw <= 1 || span <= 1 {
            fstart
        } else {
            fstart + sx * (span - 1) / (sw - 1)
        };
        let column = &spec.frames[frame_idx];
        for sy in 0..sh {
            // sy = 0 is the bottom (low frequency); reuse the row mapping with
            // an inverted index since row_to_bin expects 0 = top.
            let bin = row_to_bin(sh - 1 - sy, sh, bins, log_freq);
            let v = column.get(bin).copied().unwrap_or(0.0);
            if v <= 0.02 {
                continue;
            }
            buckets[heat_level(v)].push((sx as f64, sy as f64));
        }
    }

    // Playhead x in sub-cell coordinates, mapped onto the visible window.
    let total = app.audio.duration.as_secs_f32().max(0.001);
    let pos = app.engine.position().as_secs_f32().min(total);
    let ph_frame = (pos / total).clamp(0.0, 1.0) * n_frames as f32;
    let playhead_x = if ph_frame >= fstart as f32 && ph_frame < fend as f32 {
        Some((ph_frame - fstart as f32) as f64 / span as f64 * sw as f64)
    } else {
        None
    };

    let canvas = Canvas::default()
        .block(block)
        .marker(marker)
        .x_bounds([0.0, sw.max(1) as f64])
        .y_bounds([0.0, sh.max(1) as f64])
        .paint(move |ctx| {
            // Draw low levels first so the strongest color wins each cell.
            for (level, coords) in buckets.iter().enumerate() {
                if coords.is_empty() {
                    continue;
                }
                ctx.draw(&Points {
                    coords,
                    color: Color::Indexed(HEAT_STOPS[level]),
                });
            }
            if let Some(px) = playhead_x {
                ctx.draw(&CanvasLine {
                    x1: px,
                    y1: 0.0,
                    x2: px,
                    y2: sh as f64,
                    color: PLAYHEAD,
                });
            }
        });
    frame.render_widget(canvas, area);
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

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    // When the goto prompt is open, the footer becomes the input line.
    if let Some(goto) = &app.goto {
        let line = if goto.error {
            Line::from(vec![
                Span::styled("goto ", Style::default().fg(Color::Indexed(203))),
                Span::styled(
                    format!("{}▌", goto.buffer),
                    Style::default().fg(Color::Indexed(231)),
                ),
                Span::styled(
                    "   invalid — use m:ss (e.g. 1:23)",
                    Style::default().fg(Color::Indexed(203)),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled("goto ", Style::default().fg(Color::Indexed(150))),
                Span::styled(
                    format!("{}▌", goto.buffer),
                    Style::default().fg(Color::Indexed(231)),
                ),
                Span::styled(
                    "   m:ss · Enter to jump · Esc to cancel",
                    Style::default().fg(Color::Indexed(240)),
                ),
            ])
        };
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let hint = "Space play/pause  ·  s stop  ·  ←→ seek (Shift ±10s, Alt ±0.1s)  ·  Home/End start/end  ·  g goto  ·  ↑↓ vol  ·  Ctrl+PgUp/PgDn zoom  ·  Alt+↑↓ vscale  ·  m mute  ·  q quit";
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::Indexed(240)),
        ))),
        area,
    );
}

/// Quantize an intensity 0.0–1.0 to an index into [`HEAT_STOPS`].
fn heat_level(v: f32) -> usize {
    let i = (v.clamp(0.0, 1.0) * (HEAT_STOPS.len() - 1) as f32).round() as usize;
    i.min(HEAT_STOPS.len() - 1)
}

/// Map an intensity 0.0–1.0 to a 256-color heat ramp (inferno-ish).
fn heat_color(v: f32) -> Color {
    Color::Indexed(HEAT_STOPS[heat_level(v)])
}

/// Sub-cell resolution (x, y) a marker provides on a Canvas.
fn marker_res(marker: Marker) -> (usize, usize) {
    match marker {
        Marker::Braille => (2, 4),
        Marker::HalfBlock => (1, 2),
        _ => (1, 1), // Dot, Block, Bar
    }
}

fn fmt_duration(secs: f32) -> String {
    let secs = secs.max(0.0) as u32;
    format!("{}:{:02}", secs / 60, secs % 60)
}
