//! The little audio visualizer.
//!
//! When the computer's sound plays on the deck, the bands come from the real
//! PCM that passes through the player: a Goertzel filter per band, no FFT and
//! no extra dependency. When the sound stays on the computer but a player
//! reports a track, the bars follow the track instead, so the meter still
//! shows that something is playing.
//!
//! Everything is drawn in the Grid language: hard edges, a hairline baseline,
//! one accent colour.

use crate::app::App;
use crate::theme::Tokens;
use eframe::egui::{self, Rect, Sense, Ui, Vec2};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How many bands the analyzer produces. The visible bar count is a setting
/// between 6 and this; the bands are grouped when fewer are shown.
pub const BANDS: usize = 24;

/// Centre frequencies from bass to treble, roughly logarithmic.
const FREQS: [f32; BANDS] = [
    40.0, 55.0, 75.0, 100.0, 130.0, 170.0, 220.0, 290.0, 380.0, 500.0, 650.0, 850.0, 1100.0,
    1450.0, 1900.0, 2500.0, 3300.0, 4300.0, 5600.0, 7300.0, 9500.0, 12000.0, 15000.0, 18000.0,
];

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
    // 2048 frames is about 46 ms at 44.1 kHz: long enough for the bass bins,
    // short enough that the bars still follow the beat.
    let frames = (pcm.len() / 4).min(2048);
    let mut mono = Vec::with_capacity(frames);
    for i in 0..frames {
        let l = i16::from_le_bytes([pcm[i * 4], pcm[i * 4 + 1]]) as f32;
        let r = i16::from_le_bytes([pcm[i * 4 + 2], pcm[i * 4 + 3]]) as f32;
        mono.push((l + r) * 0.5 / 32768.0);
    }
    let n = frames as f32;
    let mut next = [0.0f32; BANDS];
    for (band, freq) in FREQS.iter().enumerate() {
        let w = 2.0 * std::f32::consts::PI * freq / rate as f32;
        let coeff = 2.0 * w.cos();
        let (mut s0, mut s1, mut s2) = (0.0f32, 0.0f32, 0.0f32);
        for &x in &mono {
            s0 = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let power = (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0);
        // A full-scale sine in the bin gives about half the window length.
        let amp = (power.sqrt() / (n * 0.5)).max(1e-6);
        // -60 dB..0 dB is the useful range for a little meter.
        let db = 20.0 * amp.log10();
        next[band] = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    }
    if let Ok(mut m) = shared.lock() {
        for band in 0..BANDS {
            // Fast attack, slow release: the bars jump on a hit and fall back
            // instead of strobing with every frame.
            let v = next[band];
            m.bands[band] = if v > m.bands[band] {
                v
            } else {
                m.bands[band] * 0.78 + v * 0.22
            };
        }
        m.live_at = Instant::now();
    }
}

/// The bars a player would show right now: real PCM first, the computer's
/// track second, silence last.
fn levels(app: &App, time: f32) -> Vec<f32> {
    let bars = app.cfg.viz.bars.clamp(6, BANDS as u8) as usize;
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
    let bands = match live {
        Some(bands) => bands,
        None if app.cfg.viz.media => synthetic(app, time),
        None => [0.0; BANDS],
    };
    group(&bands, bars)
}

/// Group the 24 analyzer bands into `n` visible bars by taking the loudest
/// member, so a narrow visible bar never loses a peak between two bands.
fn group(bands: &[f32; BANDS], n: usize) -> Vec<f32> {
    let n = n.clamp(2, BANDS);
    (0..n)
        .map(|i| {
            let from = i * BANDS / n;
            let to = ((i + 1) * BANDS / n).max(from + 1).min(BANDS);
            bands[from..to].iter().cloned().fold(0.0f32, f32::max)
        })
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

/// Draw the strip. `size` is the slot the caller reserved; the bars always
/// fill it from the hairline baseline up.
pub fn ui(ui: &mut Ui, app: &App, t: &Tokens, size: Vec2) {
    if !app.cfg.viz.enabled {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let time = ui.input(|i| i.time) as f32;
    draw(ui, t, rect, &levels(app, time));
}

fn draw(ui: &Ui, t: &Tokens, rect: Rect, bars: &[f32]) {
    if rect.width() < 8.0 || rect.height() < 4.0 || bars.is_empty() {
        return;
    }
    let painter = ui.painter_at(rect);
    let base = rect.bottom() - 1.0;
    // The baseline is always there: the strip never collapses or jumps when
    // the music stops.
    painter.rect_filled(
        Rect::from_min_size(egui::pos2(rect.left(), base), Vec2::new(rect.width(), 1.0)),
        0.0,
        t.line,
    );
    let gap = 2.0;
    let slot = (rect.width() - gap * (bars.len().saturating_sub(1)) as f32) / bars.len() as f32;
    if slot < 1.0 {
        return;
    }
    for (i, level) in bars.iter().enumerate() {
        let x = rect.left() + i as f32 * (slot + gap);
        let h = (rect.height() - 2.0) * level.clamp(0.0, 1.0);
        if h < 1.0 {
            continue;
        }
        painter.rect_filled(
            Rect::from_min_size(egui::pos2(x, base - h), Vec2::new(slot, h)),
            0.0,
            if *level > 0.72 {
                t.accent
            } else {
                t.accent_soft
            },
        );
        // A one-point cap makes even a quiet bar read as a bar.
        painter.rect_filled(
            Rect::from_min_size(egui::pos2(x, base - h), Vec2::new(slot, 1.0)),
            0.0,
            t.accent,
        );
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
    live || (app.cfg.viz.media && app.media.state.available && app.media.state.playing())
}
