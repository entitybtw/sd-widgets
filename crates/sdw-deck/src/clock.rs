//! the clock page.
//!
//! four faces share one canvas, all drawn in the hard-edged Grid language:
//! `grid` is the big time on the house grid (the default), `digital` adds a
//! soft accent glow, `analog` draws hands and `minimal` strips everything but
//! the time. every face reads the same [`ClockCfg`], so colours, font, size,
//! position and the little status chips are all editable on the page itself.

use crate::app::App;
use crate::theme::{self, Tokens};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontFamily, FontId, Rect, Sense, Stroke, TextEdit, Ui,
};
use sdw_core::model::ClockCfg;
use sdw_core::{t, Lang};

const TAU: f32 = std::f32::consts::TAU;

pub fn ui(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let cfg = app.cfg.clock.clone();
    let lang = app.lang;
    // A wide canvas that fits the deck viewport and keeps the big time big.
    let width = ui.available_width().max(280.0);
    let viewport = ui.ctx().content_rect().height();
    let max_h = (viewport - 150.0).max(240.0);
    let height = (width * 0.5).min(max_h).max(220.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let time = ui.input(|i| i.time) as f32;
    {
        let face = face_of(app, lang, time);
        canvas(ui, rect, t, &cfg, &face);
    }

    // The same clock, one tap away from the whole screen.
    let button = Rect::from_min_size(
        pos2(rect.right() - 42.0, rect.top() + 8.0),
        vec2(34.0, 34.0),
    );
    let resp = ui.put(
        button,
        egui::Button::new(theme::icon(theme::icon::FULLSCREEN, 20.0, t.strong))
            .fill(t.bg2)
            .stroke(Stroke::new(1.0, t.line))
            .corner_radius(0.0),
    );
    if resp
        .on_hover_text(t!(lang, "на весь экран", "full screen"))
        .clicked()
    {
        app.clock_full = true;
    }

    ui.add_space(10.0);
    controls(app, ui, t);
    repaint(ui.ctx(), &cfg);
}

/// Everything the face needs to know about the outside world. Built by the page
/// and by the full-screen overlay so both draw exactly the same picture.
pub struct Face<'a> {
    pub lang: Lang,
    pub time: f32,
    pub battery: Option<u8>,
    /// Headset battery: `(percent, name)`.
    pub headset: Option<(u8, &'a str)>,
    pub notices: usize,
    pub notice: Option<&'a str>,
}

/// Paint the selected face into `rect`. The page and the full-bleed overlay
/// share this so both look the same at any size.
pub fn canvas(ui: &mut Ui, rect: Rect, t: &Tokens, cfg: &ClockCfg, f: &Face) {
    match cfg.style.as_str() {
        "digital" => digital(ui, rect, t, cfg, f),
        "analog" => analog(ui, rect, t, cfg, f),
        "minimal" => minimal(ui, rect, t, cfg, f),
        _ => grid(ui, rect, t, cfg, f),
    }
    let pal = palette(cfg, t);
    info_strip(ui, rect, t, cfg, f, &pal);
}

/// The clock over the whole window, with the chrome hidden. Any tap, a click
/// or Escape brings the normal page back.
pub fn fullscreen(app: &mut App, root: &mut Ui, ctx: &egui::Context) {
    let t = Tokens::get(ctx);
    let cfg = app.cfg.clock.clone();
    let lang = app.lang;
    let time = ctx.input(|i| i.time) as f32;
    let face = face_of(app, lang, time);
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(t.bg))
        .show(root, |ui| {
            let rect = ui.available_rect_before_wrap();
            canvas(ui, rect, &t, &cfg, &face);
            ui.painter().text(
                pos2(rect.center().x, rect.bottom() - 16.0),
                Align2::CENTER_CENTER,
                match lang {
                    Lang::Ru => "нажмите или esc, чтобы выйти",
                    _ => "tap or press esc to exit",
                },
                FontId::new(11.5, FontFamily::Monospace),
                Color32::from_white_alpha(90),
            );
        });
    if ctx.input(|i| i.pointer.any_click()) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.clock_full = false;
    }
    repaint(ctx, &cfg);
}

fn face_of(app: &App, lang: Lang, time: f32) -> Face<'_> {
    let latest = app.notifications.last();
    Face {
        lang,
        time,
        battery: app.battery,
        headset: app.headset.as_ref().map(|h| (h.percent, h.name.as_str())),
        notices: app.notif_unread as usize,
        notice: latest
            .map(|n| {
                if n.summary.trim().is_empty() {
                    n.app.as_str()
                } else {
                    n.summary.as_str()
                }
            })
            .filter(|s| !s.trim().is_empty()),
    }
}

fn repaint(ctx: &egui::Context, cfg: &ClockCfg) {
    let animated = cfg.animate && cfg.anim != "off" && cfg.style != "minimal";
    let ms = if animated {
        40
    } else if cfg.seconds {
        250
    } else {
        1000
    };
    ctx.request_repaint_after(std::time::Duration::from_millis(ms));
}

// ---------------------------------------------------------------- grid

/// The default face: a big time over the house grid, an accent bar underneath,
/// the date in mono, and nothing else that is not asked for.
fn grid(ui: &mut Ui, rect: Rect, t: &Tokens, cfg: &ClockCfg, f: &Face) {
    let painter = ui.painter_at(rect);
    let now = theme::local_now();
    let pal = palette(cfg, t);
    painter.rect_filled(rect, 0.0, pal.bg_bottom);

    // The same hairline grid as the rest of the app, scaled to the canvas.
    let st = theme::style();
    if st.grid {
        let step = (st.grid_step * (rect.height() / 272.0)).max(16.0);
        let alpha = (st.grid_alpha * 2.6).clamp(8.0, 60.0) as u8;
        let line =
            Color32::from_rgba_unmultiplied(pal.accent.r(), pal.accent.g(), pal.accent.b(), alpha);
        let mut x = rect.left() + step;
        while x < rect.right() {
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.0, line),
            );
            x += step;
        }
        let mut y = rect.top() + step;
        while y < rect.bottom() {
            painter.line_segment(
                [pos2(rect.left(), y), pos2(rect.right(), y)],
                Stroke::new(1.0, line),
            );
            y += step;
        }
    }

    // A slow accent sweep keeps the face alive without moving the numbers.
    let sweep = cfg.animate && cfg.anim == "drift";
    let pulse = cfg.animate && cfg.anim == "pulse";
    if sweep {
        let k = (f.time * cfg.speed.max(0.05) * 0.18).fract();
        let x = rect.left() + rect.width() * k;
        painter.rect_filled(
            Rect::from_min_size(pos2(x - 1.0, rect.top()), vec2(2.0, rect.height())),
            0.0,
            Color32::from_rgba_unmultiplied(pal.accent.r(), pal.accent.g(), pal.accent.b(), 26),
        );
    }

    let scale = cfg.font_scale.clamp(0.5, 2.0);
    let size = rect.height() * 0.44 * scale;
    let font = FontId::new(size, face(cfg, "wide"));
    let text = time_text(cfg, &now);
    let probe = painter.layout_no_wrap(text.clone(), font.clone(), t.strong);
    let tw = probe.size().x;
    let (x, align) = match cfg.position.as_str() {
        "center" => (rect.center().x, Align2::CENTER_CENTER),
        "right" => (rect.right() - 26.0, Align2::RIGHT_CENTER),
        _ => (rect.left() + 26.0, Align2::LEFT_CENTER),
    };
    let ytime = rect.center().y - rect.height() * 0.04;
    painter.text(pos2(x, ytime), align, text, font, t.strong);

    // The accent bar marks the time block, hard edges only.
    let bw = (tw * 0.28).clamp(32.0, rect.width() * 0.32);
    let bx = match align {
        Align2::CENTER_CENTER => x - bw * 0.5,
        Align2::RIGHT_CENTER => x - bw,
        _ => x,
    };
    let breathing = if pulse {
        (0.65 + 0.35 * (f.time * cfg.speed.max(0.05)).sin()).clamp(0.0, 1.0)
    } else {
        1.0
    };
    painter.rect_filled(
        Rect::from_min_size(pos2(bx, ytime + size * 0.46), vec2(bw, 4.0)),
        0.0,
        pal.accent.gamma_multiply(breathing),
    );

    // date · weekday · battery, in mono under the bar
    let mut line = String::new();
    if cfg.date {
        if cfg.weekday {
            line.push_str(&weekday_name(f.lang, now.weekday));
            line.push(' ');
        }
        line.push_str(&format!("{:02}.{:02}.{}", now.day, now.month, now.year));
    }
    if cfg.battery {
        if let Some(bat) = f.battery {
            if !line.is_empty() {
                line.push_str(" · ");
            }
            line.push_str(&format!("{bat}%"));
        }
    }
    if !line.is_empty() {
        let (dx, dalign) = match align {
            Align2::CENTER_CENTER => (x, Align2::CENTER_CENTER),
            Align2::RIGHT_CENTER => (x, Align2::RIGHT_CENTER),
            _ => (x, Align2::LEFT_CENTER),
        };
        painter.text(
            pos2(dx, ytime + size * 0.58),
            dalign,
            line.to_uppercase(),
            FontId::new(
                (rect.height() * 0.036).clamp(11.0, 18.0),
                FontFamily::Monospace,
            ),
            t.muted,
        );
    }
    if !cfg.caption.trim().is_empty() {
        painter.text(
            pos2(x, ytime + size * 0.72),
            align,
            cfg.caption.trim(),
            FontId::new(
                (rect.height() * 0.032).clamp(10.0, 16.0),
                FontFamily::Proportional,
            ),
            t.faint,
        );
    }

    // A hairline progress bar along the bottom edge: seconds, but quiet.
    let frac = now.second as f32 / 60.0;
    painter.rect_filled(
        Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - 2.0),
            vec2(rect.width() * frac, 2.0),
        ),
        0.0,
        pal.accent.gamma_multiply(0.7),
    );
}

// ---------------------------------------------------------------- digital

fn digital(ui: &mut Ui, rect: Rect, t: &Tokens, cfg: &ClockCfg, f: &Face) {
    let painter = ui.painter_at(rect);
    let now = theme::local_now();
    let pal = palette(cfg, t);
    let accent = pal.accent;
    painter.rect_filled(rect, 0.0, pal.bg_bottom);
    // a soft accent glow that breathes
    let breathe = if cfg.animate && cfg.anim != "off" {
        0.5 + 0.5 * (f.time * cfg.speed * 0.5).sin()
    } else {
        0.6
    };
    let center = rect.center();
    for k in (1..=6).rev() {
        let rr = rect.height() * 0.055 * k as f32;
        painter.circle_filled(
            center,
            rr,
            Color32::from_rgba_unmultiplied(
                accent.r(),
                accent.g(),
                accent.b(),
                (4.0 * breathe) as u8,
            ),
        );
    }
    let scale = cfg.font_scale.clamp(0.5, 2.0);
    let size = rect.height() * 0.24 * scale;
    let (align, x) = match cfg.position.as_str() {
        "center" => (Align2::CENTER_CENTER, rect.center().x),
        "right" => (Align2::RIGHT_CENTER, rect.right() - 24.0),
        _ => (Align2::LEFT_CENTER, rect.left() + 24.0),
    };
    painter.text(
        pos2(x, center.y - rect.height() * 0.04),
        align,
        time_text(cfg, &now),
        FontId::new(size, face(cfg, "mono")),
        t.strong,
    );
    let line = date_line(cfg, &now, f);
    if !line.is_empty() {
        painter.text(
            pos2(x, center.y + rect.height() * 0.16),
            align,
            line.to_uppercase(),
            FontId::new(
                rect.height() * 0.038 * scale.min(1.4),
                FontFamily::Proportional,
            ),
            t.muted,
        );
    }
    if !cfg.caption.trim().is_empty() {
        painter.text(
            pos2(rect.center().x, rect.bottom() - 30.0),
            Align2::CENTER_CENTER,
            cfg.caption.trim(),
            FontId::new(rect.height() * 0.036, FontFamily::Proportional),
            accent,
        );
    }
    let frac = now.second as f32 / 60.0;
    painter.rect_filled(
        Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - 3.0),
            vec2(rect.width() * frac, 3.0),
        ),
        0.0,
        accent,
    );
}

// ---------------------------------------------------------------- analog

fn analog(ui: &mut Ui, rect: Rect, t: &Tokens, cfg: &ClockCfg, f: &Face) {
    let painter = ui.painter_at(rect);
    let now = theme::local_now();
    let pal = palette(cfg, t);
    let accent = pal.accent;
    painter.rect_filled(rect, 0.0, pal.bg_top);
    let center = rect.center();
    let r = ((rect.height() * 0.40).min(rect.width() * 0.30) * cfg.font_scale.clamp(0.6, 1.6))
        .min(rect.height() * 0.46);
    painter.circle_stroke(center, r + 8.0, Stroke::new(1.0, t.line));
    painter.circle_filled(center, r, theme::mix(t.well, accent, 0.06));
    for k in 0..60 {
        let a = k as f32 / 60.0 * TAU - TAU / 4.0;
        let outer = center + vec2(a.cos(), a.sin()) * r;
        let inner = center + vec2(a.cos(), a.sin()) * (r - if k % 5 == 0 { 16.0 } else { 8.0 });
        let col = if k % 5 == 0 { t.muted } else { t.faint };
        painter.line_segment(
            [inner, outer],
            Stroke::new(if k % 5 == 0 { 2.0 } else { 1.0 }, col),
        );
    }
    let hand = |angle: f32, len: f32, width: f32, color: Color32| {
        let a = angle - TAU / 4.0;
        painter.line_segment(
            [
                center - vec2(a.cos(), a.sin()) * (r * 0.10),
                center + vec2(a.cos(), a.sin()) * len,
            ],
            Stroke::new(width, color),
        );
    };
    let hour = (now.hour % 12) as f32 + now.minute as f32 / 60.0;
    let minute = now.minute as f32 + now.second as f32 / 60.0;
    let second = now.second as f32;
    hand(hour / 12.0 * TAU, r * 0.52, 6.0, t.strong);
    hand(minute / 60.0 * TAU, r * 0.76, 4.0, t.ink);
    if cfg.seconds {
        hand(second / 60.0 * TAU, r * 0.88, 2.0, accent);
    }
    painter.circle_filled(center, 5.0, accent);
    let mut line = String::new();
    if cfg.date {
        if cfg.weekday {
            line.push_str(&weekday_name(f.lang, now.weekday));
            line.push(' ');
        }
        line.push_str(&format!("{:02}.{:02}", now.day, now.month));
    }
    if cfg.battery {
        if let Some(bat) = f.battery {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&format!("{bat}%"));
        }
    }
    if !line.is_empty() {
        painter.text(
            center + vec2(0.0, r * 0.45),
            Align2::CENTER_CENTER,
            line,
            FontId::new(r * 0.10, FontFamily::Monospace),
            t.muted,
        );
    }
    if !cfg.caption.trim().is_empty() {
        painter.text(
            pos2(rect.center().x, rect.bottom() - 24.0),
            Align2::CENTER_CENTER,
            cfg.caption.trim(),
            FontId::new(rect.height() * 0.04, FontFamily::Proportional),
            t.faint,
        );
    }
}

// ---------------------------------------------------------------- minimal

fn minimal(ui: &mut Ui, rect: Rect, t: &Tokens, cfg: &ClockCfg, f: &Face) {
    let painter = ui.painter_at(rect);
    let now = theme::local_now();
    let pal = palette(cfg, t);
    let accent = pal.accent;
    painter.rect_filled(rect, 0.0, pal.bg_bottom);
    let scale = cfg.font_scale.clamp(0.5, 2.0);
    let size = rect.height() * 0.34 * scale;
    let (align, x) = match cfg.position.as_str() {
        "center" => (Align2::CENTER_CENTER, rect.center().x),
        "right" => (Align2::RIGHT_CENTER, rect.right() - 24.0),
        _ => (Align2::LEFT_CENTER, rect.left() + 24.0),
    };
    painter.text(
        pos2(x, rect.center().y),
        align,
        time_text(cfg, &now),
        FontId::new(size, face(cfg, "wide")),
        t.strong,
    );
    if cfg.date {
        let mut line = String::new();
        if cfg.weekday {
            line.push_str(&weekday_name(f.lang, now.weekday));
            line.push(' ');
        }
        line.push_str(&format!("{:02}.{:02}.{}", now.day, now.month, now.year));
        painter.text(
            pos2(rect.center().x, rect.center().y + rect.height() * 0.24),
            Align2::CENTER_CENTER,
            line,
            FontId::new(rect.height() * 0.04, FontFamily::Proportional),
            accent,
        );
    }
    if !cfg.caption.trim().is_empty() {
        painter.text(
            pos2(rect.center().x, rect.center().y + rect.height() * 0.32),
            Align2::CENTER_CENTER,
            cfg.caption.trim(),
            FontId::new(rect.height() * 0.035, FontFamily::Proportional),
            t.faint,
        );
    }
}

// ---------------------------------------------------------------- controls

/// Material symbols offered as the clock badge: time, weather, a mood.
const CLOCK_ICONS: [(&str, &str, &str); 12] = [
    (theme::icon::CLOCK, "часы", "clock"),
    (theme::icon::SUN, "солнце", "sun"),
    (theme::icon::MOON, "луна", "moon"),
    (theme::icon::STAR, "звезда", "star"),
    (theme::icon::BULB, "идея", "idea"),
    (theme::icon::MUSIC, "музыка", "music"),
    (theme::icon::CODE, "код", "code"),
    (theme::icon::GAMEPAD, "игры", "games"),
    (theme::icon::PERSON, "человек", "person"),
    (theme::icon::HEADPHONES, "наушники", "headphones"),
    (theme::icon::HOME, "дом", "home"),
    (theme::icon::SCENE, "сцена", "scene"),
];

/// A stable, readable name for a clock preset: the caption when it is set,
/// otherwise the face and the day.
fn preset_name(cfg: &ClockCfg) -> String {
    let caption = cfg.caption.trim();
    let ascii_name = !caption.is_empty()
        && caption
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'));
    if ascii_name {
        return caption.to_string();
    }
    format!("{}-{}", cfg.style, crate::addons_ui::now_tag())
}

fn controls(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let mut cfg = app.cfg.clock.clone();
    let before = cfg.clone();
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::hud(t!(app.lang, "часы", "clock"), t.faint));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::icon_button(ui, theme::icon::FULLSCREEN, t, 30.0, false)
                    .on_hover_text(t!(app.lang, "на весь экран", "full screen"))
                    .clicked()
                {
                    app.clock_full = true;
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            for (key, ru, en) in [
                ("grid", "сетка", "grid"),
                ("digital", "цифры", "digital"),
                ("analog", "стрелки", "analog"),
                ("minimal", "минимум", "minimal"),
            ] {
                let active = cfg.style == key;
                let resp = ui.add(
                    egui::Button::new(rt(
                        t!(app.lang, ru, en),
                        13.5,
                        if active { t.strong } else { t.muted },
                    ))
                    .fill(if active { t.accent_soft } else { t.well })
                    .stroke(Stroke::new(1.0, if active { t.accent } else { t.line }))
                    .corner_radius(0.0)
                    .min_size(vec2(0.0, 36.0)),
                );
                if resp.clicked() {
                    cfg.style = key.to_string();
                }
            }
            // Clock presets shipped by enabled addons replace the whole config.
            for addon in sdw_core::addons::list() {
                if !app.cfg.addons.is_on(addon.id()) {
                    continue;
                }
                let Some(preset) = addon.clock() else {
                    continue;
                };
                let label = if addon.manifest.name.is_empty() {
                    addon.id().to_string()
                } else {
                    addon.manifest.name.clone()
                };
                let resp = ui.add(
                    egui::Button::new(rt(&label, 13.5, t.muted))
                        .fill(t.well)
                        .stroke(Stroke::new(1.0, t.line))
                        .corner_radius(0.0)
                        .min_size(vec2(0.0, 36.0)),
                );
                if resp.clicked() {
                    cfg = preset;
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (value, ru, en) in [
                (&mut cfg.seconds, "секунды", "seconds"),
                (&mut cfg.date, "дата", "date"),
                (&mut cfg.weekday, "день", "weekday"),
                (&mut cfg.battery, "батарея", "battery"),
                (&mut cfg.animate, "анимация", "animation"),
                (&mut cfg.mono, "моно", "mono"),
                (&mut cfg.h24, "24 часа", "24 hour"),
                (&mut cfg.follow_theme, "из темы", "follow theme"),
            ] {
                theme::switch(ui, value, t);
                ui.label(rt(t!(app.lang, ru, en), 12.5, t.muted));
                ui.add_space(8.0);
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (value, ru, en) in [
                (&mut cfg.show_headset, "наушники", "headset"),
                (&mut cfg.show_notifications, "уведомления", "notifications"),
            ] {
                theme::switch(ui, value, t);
                ui.label(rt(t!(app.lang, ru, en), 12.5, t.muted));
                ui.add_space(8.0);
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(rt(t!(app.lang, "шрифт", "font"), 12.0, t.faint));
            for (key, ru, en) in [
                ("auto", "авто", "auto"),
                ("wide", "широкий", "wide"),
                ("mono", "моно", "mono"),
            ] {
                if ui
                    .selectable_label(cfg.font == key, rt(t!(app.lang, ru, en), 12.0, t.muted))
                    .clicked()
                {
                    cfg.font = key.to_string();
                }
            }
            ui.add_space(10.0);
            ui.label(rt(t!(app.lang, "время", "position"), 12.0, t.faint));
            for (key, ru, en) in [
                ("left", "слева", "left"),
                ("center", "центр", "center"),
                ("right", "справа", "right"),
            ] {
                if ui
                    .selectable_label(cfg.position == key, rt(t!(app.lang, ru, en), 12.0, t.muted))
                    .clicked()
                {
                    cfg.position = key.to_string();
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(rt(t!(app.lang, "анимация", "animation"), 12.0, t.faint));
            for (key, ru, en) in [
                ("drift", "дрейф", "drift"),
                ("pulse", "пульс", "pulse"),
                ("off", "нет", "off"),
            ] {
                if ui
                    .selectable_label(cfg.anim == key, rt(t!(app.lang, ru, en), 12.0, t.muted))
                    .clicked()
                {
                    cfg.anim = key.to_string();
                }
            }
        });
        if theme::slider(
            ui,
            t,
            t!(app.lang, "скорость", "speed"),
            &mut cfg.speed,
            0.2..=3.0,
            |v| format!("{v:.1}×"),
        ) {}
        if theme::slider(
            ui,
            t,
            t!(app.lang, "размер", "size"),
            &mut cfg.font_scale,
            0.6..=1.8,
            |v| format!("{:.0}%", v * 100.0),
        ) {}
        ui.add_space(4.0);
        // Colours are the theme's unless the clock was taken off it: a disabled
        // row says so instead of silently ignoring the pickers.
        let custom = !cfg.follow_theme && !cfg.mono;
        if !custom {
            theme::small(
                ui,
                t!(
                    app.lang,
                    "цвета берутся из активной темы; выключите «из темы», чтобы задать свои.",
                    "colours come from the active theme; switch “follow theme” off to pick your own."
                ),
                t.faint,
            );
        }
        ui.add_enabled_ui(custom, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(rt(t!(app.lang, "акцент", "accent"), 12.0, t.faint));
                color_edit(ui, &mut cfg.accent, t);
                ui.add_space(10.0);
                ui.label(rt(
                    t!(app.lang, "фон сверху", "background top"),
                    12.0,
                    t.faint,
                ));
                color_edit(ui, &mut cfg.bg_top, t);
                ui.add_space(10.0);
                ui.label(rt(
                    t!(app.lang, "фон снизу", "background bottom"),
                    12.0,
                    t.faint,
                ));
                color_edit(ui, &mut cfg.bg_bottom, t);
            });
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(rt(t!(app.lang, "подпись", "caption"), 12.0, t.faint));
            ui.add(
                TextEdit::singleline(&mut cfg.caption)
                    .hint_text(t!(
                        app.lang,
                        "имя, город, что угодно",
                        "name, city, anything"
                    ))
                    .desired_width(220.0),
            );
            ui.add_space(10.0);
            if theme::ghost(ui, t!(app.lang, "сбросить", "reset"), t).clicked() {
                cfg = ClockCfg::default();
            }
        });
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(rt(t!(app.lang, "значок", "icon"), 12.0, t.faint));
            let none = cfg.icon.trim().is_empty();
            if ui
                .selectable_label(none, rt(t!(app.lang, "нет", "none"), 12.0, t.muted))
                .clicked()
            {
                cfg.icon.clear();
            }
            for (glyph, ru, en) in CLOCK_ICONS {
                let active = cfg.icon == glyph;
                let resp = ui
                    .add(
                        egui::Button::new(theme::icon(
                            glyph,
                            18.0,
                            if active { t.strong } else { t.muted },
                        ))
                        .fill(if active { t.accent_soft } else { t.well })
                        .stroke(Stroke::new(1.0, if active { t.accent } else { t.line }))
                        .corner_radius(0.0)
                        .min_size(vec2(34.0, 30.0)),
                    )
                    .on_hover_text(t!(app.lang, ru, en));
                if resp.clicked() {
                    cfg.icon = if active {
                        String::new()
                    } else {
                        glyph.to_string()
                    };
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if theme::ghost(ui, t!(app.lang, "сохранить часы", "save clock"), t).clicked()
            {
                let name = preset_name(&cfg);
                match sdw_core::addons::create_clock(&name, &cfg) {
                    Ok(addon) => {
                        app.cfg.addons.set(addon.id(), true);
                        app.save();
                        app.toast(
                            t!(
                                app.lang,
                                "часы сохранены как аддон",
                                "clock saved as an addon"
                            ),
                            true,
                        );
                    }
                    Err(e) => app.toast(e, false),
                }
            }
            if theme::ghost(ui, t!(app.lang, "экспорт часов", "export clock"), t).clicked()
            {
                match sdw_core::addons::export_clock(
                    &preset_name(&cfg),
                    &cfg,
                    &sdw_core::addons::export_dir(),
                ) {
                    Ok(path) => app.toast(
                        format!(
                            "{} {}",
                            t!(app.lang, "экспортировано:", "exported:"),
                            path.display()
                        ),
                        true,
                    ),
                    Err(e) => app.toast(e, false),
                }
            }
            if theme::ghost(ui, t!(app.lang, "экспорт темы", "export theme"), t).clicked()
            {
                match sdw_core::addons::export_theme(
                    &format!("theme-{}", crate::addons_ui::now_tag()),
                    &app.cfg.style.css,
                    &sdw_core::addons::export_dir(),
                ) {
                    Ok(path) => app.toast(
                        format!(
                            "{} {}",
                            t!(app.lang, "экспортировано:", "exported:"),
                            path.display()
                        ),
                        true,
                    ),
                    Err(e) => app.toast(e, false),
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(rt(
                    &format!(
                        "{} {}",
                        t!(app.lang, "папка:", "folder:"),
                        sdw_core::addons::export_dir().display()
                    ),
                    10.5,
                    t.faint,
                ));
            });
        });
    });
    if cfg.style != before.style
        || cfg.seconds != before.seconds
        || cfg.date != before.date
        || cfg.weekday != before.weekday
        || cfg.battery != before.battery
        || cfg.animate != before.animate
        || cfg.anim != before.anim
        || cfg.mono != before.mono
        || cfg.h24 != before.h24
        || cfg.caption != before.caption
        || cfg.show_headset != before.show_headset
        || cfg.show_notifications != before.show_notifications
        || cfg.follow_theme != before.follow_theme
        || cfg.font != before.font
        || cfg.position != before.position
        || cfg.accent != before.accent
        || cfg.bg_top != before.bg_top
        || cfg.bg_bottom != before.bg_bottom
        || cfg.icon != before.icon
        || (cfg.speed - before.speed).abs() > f32::EPSILON
        || (cfg.font_scale - before.font_scale).abs() > f32::EPSILON
    {
        app.cfg.clock = cfg;
        app.save_soon();
    }
}

// ---------------------------------------------------------------- palette and chips

struct Palette {
    accent: Color32,
    bg_top: Color32,
    bg_bottom: Color32,
}

/// Three colours per face: the accent and the two background ends. `mono`
/// keeps the clock in the theme ink, `follow_theme` takes the live palette, and
/// otherwise the three fields of the config win.
fn palette(cfg: &ClockCfg, t: &Tokens) -> Palette {
    if cfg.mono {
        Palette {
            accent: t.ink,
            bg_top: t.bg,
            bg_bottom: t.bg,
        }
    } else if cfg.follow_theme {
        Palette {
            accent: t.accent,
            bg_top: t.bg2,
            bg_bottom: t.bg,
        }
    } else {
        Palette {
            accent: color(&cfg.accent, t.accent),
            bg_top: color(&cfg.bg_top, t.bg2),
            bg_bottom: color(&cfg.bg_bottom, t.bg),
        }
    }
}

/// The face of the big time. `auto` keeps each style's own look, `wide` is the
/// humanist UI face and `mono` the technical one.
fn face(cfg: &ClockCfg, natural: &str) -> FontFamily {
    match cfg.font.as_str() {
        "mono" => FontFamily::Monospace,
        "wide" => FontFamily::Proportional,
        _ => {
            if natural == "mono" {
                FontFamily::Monospace
            } else {
                FontFamily::Proportional
            }
        }
    }
}

fn brightness(c: Color32) -> f32 {
    0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32
}

/// Headset battery and a one-line notification summary, as hairline chips in
/// the top-left corner of the canvas. Every part can be switched off.
fn info_strip(ui: &Ui, rect: Rect, t: &Tokens, cfg: &ClockCfg, f: &Face, pal: &Palette) {
    let badge = cfg.icon.trim();
    if !cfg.show_headset && !cfg.show_notifications && badge.is_empty() {
        return;
    }
    let painter = ui.painter_at(rect);
    let ink = if brightness(pal.bg_bottom) < 128.0 {
        Color32::WHITE
    } else {
        t.strong
    };
    let text_font = FontId::new(10.5, FontFamily::Monospace);
    let mut x = rect.left() + 12.0;
    let y = rect.top() + 10.0;
    // Leave the corner free for the full-screen button on the page.
    let avail = (rect.width() - 60.0).max(80.0);

    if !badge.is_empty() {
        let side = 26.0;
        let box_rect = Rect::from_min_size(pos2(x, y - 2.0), vec2(side, side));
        painter.rect_filled(
            box_rect,
            0.0,
            Color32::from_rgba_unmultiplied(
                pal.bg_bottom.r(),
                pal.bg_bottom.g(),
                pal.bg_bottom.b(),
                0xD2,
            ),
        );
        painter.rect_stroke(
            box_rect,
            0.0,
            Stroke::new(1.0, pal.accent.gamma_multiply(0.55)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            box_rect.center(),
            Align2::CENTER_CENTER,
            badge,
            FontId::new(17.0, theme::icon_family()),
            pal.accent,
        );
        x += side + 6.0;
    }

    if cfg.show_headset {
        if let Some((pct, name)) = f.headset {
            let label = format!("{pct}% · {name}");
            let cap = (avail * 0.5).max(64.0);
            x += chip(
                &painter,
                x,
                y,
                theme::icon::HEADPHONES,
                &label,
                pal,
                ink,
                &text_font,
                cap,
            ) + 6.0;
        }
    }
    if cfg.show_notifications && f.notices > 0 {
        let text = match f.notice {
            Some(title) => format!("{} · {title}", f.notices),
            None => f.notices.to_string(),
        };
        let cap = (rect.right() - 12.0 - x).max(40.0);
        chip(
            &painter,
            x,
            y,
            theme::icon::NOTIFICATIONS,
            &text,
            pal,
            ink,
            &text_font,
            cap,
        );
    }
}

/// One chip: an accent glyph, then text, on a translucent panel of the clock
/// background. Returns its width.
#[allow(clippy::too_many_arguments)]
fn chip(
    painter: &egui::Painter,
    x: f32,
    y: f32,
    icon: &str,
    text: &str,
    pal: &Palette,
    ink: Color32,
    text_font: &FontId,
    max_w: f32,
) -> f32 {
    let label = fit(painter, text, text_font, (max_w - 36.0).max(0.0));
    let measured = painter
        .layout_no_wrap(label.clone(), text_font.clone(), ink)
        .size()
        .x;
    let w = (10.0 + 14.0 + 6.0 + measured + 10.0).min(max_w.max(40.0));
    let bg = Rect::from_min_size(pos2(x, y), vec2(w, 22.0));
    painter.rect_filled(
        bg,
        0.0,
        Color32::from_rgba_unmultiplied(
            pal.bg_bottom.r(),
            pal.bg_bottom.g(),
            pal.bg_bottom.b(),
            0xD2,
        ),
    );
    painter.rect_stroke(
        bg,
        0.0,
        Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(pal.accent.r(), pal.accent.g(), pal.accent.b(), 0x60),
        ),
        egui::StrokeKind::Inside,
    );
    painter.text(
        pos2(x + 10.0, y + 11.0),
        Align2::LEFT_CENTER,
        icon,
        FontId::new(14.0, theme::icon_family()),
        pal.accent,
    );
    painter.text(
        pos2(x + 30.0, y + 11.0),
        Align2::LEFT_CENTER,
        label,
        text_font.clone(),
        ink,
    );
    w
}

/// Trim `text` (measuring with `font`) until it fits `max_w`, with an ellipsis.
fn fit(painter: &egui::Painter, text: &str, font: &FontId, max_w: f32) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    let width = |probe: &str| {
        painter
            .layout_no_wrap(probe.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
    };
    if width(text) <= max_w {
        return text.to_owned();
    }
    let mut cut: String = text.to_owned();
    while !cut.is_empty() {
        cut.pop();
        let probe = format!("{cut}…");
        if width(&probe) <= max_w {
            return probe;
        }
    }
    String::new()
}

// ---------------------------------------------------------------- helpers

fn date_line(cfg: &ClockCfg, now: &theme::NowParts, f: &Face) -> String {
    let mut line = String::new();
    if cfg.date {
        if cfg.weekday {
            line.push_str(&weekday_name(f.lang, now.weekday));
            line.push(' ');
        }
        line.push_str(&format!("{:02}.{:02}.{}", now.day, now.month, now.year));
    }
    if cfg.battery {
        if let Some(bat) = f.battery {
            if !line.is_empty() {
                line.push_str(" · ");
            }
            line.push_str(&format!("{bat}%"));
        }
    }
    line
}

fn time_text(cfg: &ClockCfg, now: &theme::NowParts) -> String {
    let mut out = if cfg.h24 {
        format!("{:02}:{:02}", now.hour, now.minute)
    } else {
        let h = match now.hour % 12 {
            0 => 12,
            h => h,
        };
        format!("{:02}:{:02}", h, now.minute)
    };
    if cfg.seconds {
        out.push_str(&format!(":{:02}", now.second));
    }
    if !cfg.h24 {
        out.push_str(if now.hour < 12 { " am" } else { " pm" });
    }
    out
}

fn color(raw: &str, fallback: Color32) -> Color32 {
    theme::parse_color(raw.trim()).unwrap_or(fallback)
}

fn color_edit(ui: &mut Ui, value: &mut String, t: &Tokens) {
    let mut current = color(value, t.accent);
    if ui.color_edit_button_srgba(&mut current).changed() {
        *value = format!("#{:02x}{:02x}{:02x}", current.r(), current.g(), current.b());
    }
}

fn weekday_name(lang: Lang, weekday: u32) -> &'static str {
    let ru = ["вс", "пн", "вт", "ср", "чт", "пт", "сб"];
    let en = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
    let idx = (weekday as usize).min(6);
    match lang {
        Lang::Ru => ru[idx],
        _ => en[idx],
    }
}

/// `RichText` under a local name so the control rows stay short.
fn rt(text: impl Into<String>, size: f32, color: Color32) -> egui::RichText {
    egui::RichText::new(text.into()).size(size).color(color)
}
