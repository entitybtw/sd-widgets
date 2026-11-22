//! The little audio visualizer.
//!
//! When the computer's sound plays on the deck, the bands come from the real
//! PCM that passes through the player. When the sound stays on the computer,
//! the daemon analyzes its sink monitor with the same code and the deck polls
//! those levels, so the meter follows the music even with the speakers remote.
//! A track that only reports metadata keeps falling back to a gentle motion.
//!
//! Everything is drawn in the Grid language: hard edges, a hairline baseline,
//! one accent colour.

use crate::app::App;
use crate::theme::Tokens;
use eframe::egui::{self, Color32, Rect, Sense, Stroke, Ui, Vec2};
use sdw_core::model::VizCfg;
use sdw_core::spectrum;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How many bands the analyzer produces (re-exported for the rest of the deck).
pub const BANDS: usize = spectrum::BANDS;

#[derive(Debug, Clone)]
pub struct Meter {
    bands: [f32; BANDS],
    /// When real PCM last arrived. Older than a second means silence or a
    /// player that stopped, and the synthetic track motion may take over.
    live_at: Instant,
}

impl Default for Meter {
    fn default() -> Self {
        Self {
            bands: [0.0; BANDS],
            live_at: Instant::now() - Duration::from_secs(10),
        }
    }
}

pub type Shared = Arc<Mutex<Meter>>;

pub fn shared() -> Shared {
    Arc::new(Mutex::new(Meter::default()))
}

/// Feed one chunk of the PCM stream (s16le, two channels) to the meter.
pub fn feed(shared: &Shared, pcm: &[u8], rate: u32) {
    if pcm.len() < 256 || rate < 8000 {
        return;
    }
    let next = spectrum::analyze(pcm, 2, rate);
    if let Ok(mut m) = shared.lock() {
        // Fast attack, slow release: the bars jump on a hit and fall back
        // instead of strobing with every frame.
        m.bands = spectrum::follow(&m.bands, &next, 0.78);
        m.live_at = Instant::now();
    }
}

/// The bars a player would show right now: the computer's own spectrum first
/// (the same picture cava draws there), real PCM on the deck second, the
/// track's motion third, silence last.
fn levels(app: &App, time: f32) -> Vec<f32> {
    let bars = app.cfg.viz.bars.clamp(4, BANDS as u8) as usize;
    let remote = app.viz_remote.as_ref().and_then(|(bands, at)| {
        if at.elapsed() < Duration::from_millis(700) && bands.len() == BANDS {
            let mut copy = [0.0f32; BANDS];
            copy.copy_from_slice(bands);
            Some(copy)
        } else {
            None
        }
    });
    let live = app
        .viz
        .lock()
        .ok()
        .map(|m| {
            if m.live_at.elapsed() < Duration::from_millis(900) {
                Some(m.bands)
            } else {
                None
            }
        })
        .flatten();
    let bands = match remote {
        Some(bands) => bands,
        None => match live {
            Some(bands) => bands,
            None if app.cfg.viz.media => synthetic(app, time),
            None => [0.0; BANDS],
        },
    };
    // Sensitivity: 100 % is the natural level, higher lifts quiet passages.
    let gain = app.cfg.viz.gain.clamp(50, 200) as f32 / 100.0;
    spectrum::group(&bands, bars)
        .into_iter()
        .map(|v| (v * gain).clamp(0.0, 1.0))
        .collect()
}

/// A track the deck can hear about but not hear: move the bars with the
/// track's own volume, using the title and artist as a seed so every song
/// looks a little different.
fn synthetic(app: &App, time: f32) -> [f32; BANDS] {
    let s = &app.media.state;
    let mut out = [0.0f32; BANDS];
    if !s.available || !s.playing() || s.muted {
        return out;
    }
    let amp = (0.45 + 0.55 * s.volume as f32 / 100.0).clamp(0.45, 1.0);
    let seed = s
        .title
        .bytes()
        .chain(s.artist.bytes())
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    for (i, v) in out.iter_mut().enumerate() {
        let f = i as f32 / BANDS as f32;
        let a = ((time * (1.3 + 2.4 * f) + seed as f32 * 0.0007 * (i as f32 + 1.0)).sin() * 0.5
            + 0.5)
            .powf(1.5);
        let b = ((time * (0.6 + 1.1 * f) + 2.1 + i as f32 * 0.7).sin() * 0.5 + 0.5).powf(1.2);
        // Treble bars stay shorter: a real mix has less energy up there.
        let tilt = (1.0 - 0.55 * f).max(0.2);
        *v = ((0.12 + 0.88 * a * b) * tilt * amp * 1.7).clamp(0.0, 1.0);
    }
    out
}

/// The strip's footprint: the width and height the user chose. `avail` lets a
/// caller hand in the slot it actually has (the top bar), so a wide strip is
/// shrunk there instead of pushing the shortcuts off the window.
pub fn size(app: &App, avail: Option<Vec2>) -> Vec2 {
    let cfg = &app.cfg.viz;
    let mut w = cfg.width.clamp(40, 400) as f32;
    let h = cfg.height.clamp(6, 48) as f32;
    if let Some(avail) = avail {
        if avail.x.is_finite() && avail.x > 0.0 && avail.y.is_finite() && avail.y > 0.0 {
            w = w.min(avail.x);
            // A very small scale leaves a few points in the top bar; never
            // ask for more than that, or the row would grow past the window.
            return Vec2::new(w.max(8.0).min(avail.x), h.min(avail.y.max(4.0)));
        }
    }
    Vec2::new(w.max(8.0), h)
}

/// The strip itself. `size` is the slot the caller reserved; the bars fill it
/// from the hairline baseline up, mirrored around the middle when asked.
pub fn ui(ui: &mut Ui, app: &App, t: &Tokens, size: Vec2) -> egui::Response {
    if !app.cfg.viz.enabled {
        return ui.allocate_response(Vec2::ZERO, Sense::hover());
    }
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    paint(ui, app, t, rect);
    resp
}

/// A framed preview for the settings page: a hairline well that holds the
/// strip at the very size and bar width it will have in the top bar. Stretching
/// it to the card's width made every bar three times wider than it really is,
/// so the preview showed a different object than the one being adjusted.
pub fn preview(ui: &mut Ui, app: &App, t: &Tokens) -> egui::Response {
    let strip = size(app, None);
    let stage = Vec2::new((strip.x + 48.0).max(220.0), strip.y + 40.0);
    let (rect, resp) = ui.allocate_exact_size(stage, Sense::hover());
    let p = ui.painter();
    // No fill: the stage must show the strip on the same surface as the top
    // bar, or the bars would look fainter here than they do there.
    p.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    // Centre the strip on its own baseline: the stage is a frame, the strip
    // inside is the real thing at its real size.
    let strip_rect = Rect::from_center_size(rect.center(), Vec2::new(strip.x, strip.y));
    paint(ui, app, t, strip_rect);
    resp
}

/// Paint the strip into a rect the caller already owns. The clock draws it
/// over its own canvas this way, without moving the page's layout.
pub fn paint(ui: &Ui, app: &App, t: &Tokens, rect: Rect) {
    if !app.cfg.viz.enabled || rect.width() < 8.0 || rect.height() < 4.0 {
        return;
    }
    let time = ui.input(|i| i.time) as f32;
    // One accent for the whole strip, like cava: the loud bars in full colour,
    // the quiet ones a little dimmer but still clearly there. The theme's
    // `accent_soft` is an alpha-30 wash and made every ordinary bar all but
    // invisible, which read as "the visualizer does not work".
    let base_color = accent(app, t);
    let hot = base_color;
    let soft = base_color.gamma_multiply(0.85);
    let lv = levels(app, time);
    draw(ui, t, rect, &lv, &app.cfg.viz, hot, soft);
}

/// The accent the bars use: the user's own colour, or the theme accent.
fn accent(app: &App, t: &Tokens) -> Color32 {
    crate::theme::parse_color(app.cfg.viz.color.trim()).unwrap_or(t.accent)
}

fn with_alpha(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

fn draw(ui: &Ui, t: &Tokens, rect: Rect, bars: &[f32], cfg: &VizCfg, hot: Color32, soft: Color32) {
    if rect.width() < 8.0 || rect.height() < 4.0 || bars.is_empty() {
        return;
    }
    let painter = ui.painter_at(rect);
    let alpha = (cfg.opacity.clamp(20, 100) as f32 / 100.0 * 255.0) as u8;
    let hot = with_alpha(hot, alpha);
    let soft = with_alpha(soft, alpha);
    let base = rect.bottom() - 1.0;
    // The baseline is always there: the strip never collapses or jumps when
    // the music stops.
    painter.rect_filled(
        Rect::from_min_size(egui::pos2(rect.left(), base), Vec2::new(rect.width(), 1.0)),
        0.0,
        t.line,
    );
    let gap = (cfg.gap.min(8) as f32).min(rect.width() / 8.0);
    let style = cfg.style.as_str();
    let mirror = cfg.mirror && style != "wave";
    // A mirrored strip grows up and down from the middle line.
    let mid = rect.center().y;
    let full = if mirror {
        (rect.height() * 0.5 - 1.0).max(1.0)
    } else {
        (rect.height() - 2.0).max(1.0)
    };
    // A narrow slot shows fewer bars instead of squeezing every one into a
    // sub-pixel: a squeezed strip read as a clipped stump. The shape and the
    // order stay, only the count adapts to the room the caller gave.
    let mut n = bars.len().max(1);
    let mut slot = (rect.width() - gap * (n.saturating_sub(1)) as f32) / n as f32;
    while slot < 1.5 && n > 1 {
        n -= 1;
        slot = (rect.width() - gap * (n.saturating_sub(1)) as f32) / n as f32;
    }
    if slot < 1.0 {
        return;
    }
    let bars = &bars[..n];
    if style == "wave" {
        // A hairline through the tips turns them into one line of sound.
        let pts: Vec<egui::Pos2> = bars
            .iter()
            .enumerate()
            .map(|(i, level)| {
                let x = rect.left() + i as f32 * (slot + gap) + slot * 0.5;
                egui::pos2(x, base - full * level.clamp(0.0, 1.0))
            })
            .collect();
        if pts.len() > 1 {
            painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, hot)));
        }
    }
    for (i, level) in bars.iter().enumerate() {
        let x = rect.left() + i as f32 * (slot + gap);
        let level = level.clamp(0.0, 1.0);
        let h = full * level;
        if h < 1.0 && style != "wave" {
            continue;
        }
        let color = if level > 0.72 { hot } else { soft };
        if style == "dots" {
            let side = slot.max(1.0).min(8.0);
            let top = (base - h - side).clamp(rect.top(), base - side);
            painter.rect_filled(
                Rect::from_min_size(egui::pos2(x, top), Vec2::new(side, side)),
                0.0,
                color,
            );
            if mirror {
                let below = (mid + h).min(base - side);
                painter.rect_filled(
                    Rect::from_min_size(egui::pos2(x, below), Vec2::new(side, side)),
                    0.0,
                    color,
                );
            }
            continue;
        }
        if style != "wave" {
            let (y, hh) = if mirror {
                (mid - h.round(), h)
            } else {
                (base - h, h)
            };
            painter.rect_filled(
                Rect::from_min_size(egui::pos2(x, y), Vec2::new(slot, hh)),
                0.0,
                color,
            );
            if mirror {
                painter.rect_filled(
                    Rect::from_min_size(egui::pos2(x, mid), Vec2::new(slot, h)),
                    0.0,
                    color,
                );
            }
        }
        if cfg.peak && style != "wave" {
            // A one-point cap makes even a quiet bar read as a bar.
            let cap = if mirror { mid - h.round() } else { base - h };
            painter.rect_filled(
                Rect::from_min_size(egui::pos2(x, cap), Vec2::new(slot, 1.0)),
                0.0,
                hot,
            );
            if mirror {
                painter.rect_filled(
                    Rect::from_min_size(egui::pos2(x, mid + h - 1.0), Vec2::new(slot, 1.0)),
                    0.0,
                    hot,
                );
            }
        }
    }
}

/// Whether anything is playing, so a caller can keep the slot out of the
/// layout when it would only be a flat line.
pub fn active(app: &App) -> bool {
    if !app.cfg.viz.enabled {
        return false;
    }
    let live = app
        .viz
        .lock()
        .ok()
        .is_some_and(|m| m.live_at.elapsed() < Duration::from_millis(900));
    let remote = app
        .viz_remote
        .as_ref()
        .is_some_and(|(_, at)| at.elapsed() < Duration::from_millis(700));
    live || remote || (app.cfg.viz.media && app.media.state.available && app.media.state.playing())
}
