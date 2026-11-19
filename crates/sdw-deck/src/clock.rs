//! the full-screen clock page.
//!
//! four styles share one canvas: `tide` follows the FLOW · Tide lock screen
//! (deep water, drifting light strips, a big moon and a tilted time), while
//! `digital`, `analog` and `minimal` are quieter takes. every style reads the
//! same [`ClockCfg`], so colours, fonts, elements and animations can all be
//! changed from the page itself.

use crate::app::App;
use crate::theme::{self, Tokens};
use eframe::egui::{
    self, epaint::TextShape, pos2, vec2, Align2, Color32, FontFamily, FontId, Pos2, Rect, Sense,
    Stroke, TextEdit, Ui,
};
use sdw_core::model::ClockCfg;
use sdw_core::{t, Lang};

const REF_W: f32 = 480.0;
const REF_H: f32 = 272.0;
const TAU: f32 = std::f32::consts::TAU;

pub fn ui(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let cfg = app.cfg.clock.clone();
    let lang = app.lang;
    let battery = app.battery;
    // One canvas that keeps the Tide proportions and fits the deck viewport.
    let width = ui.available_width().max(280.0);
    let viewport = ui.ctx().content_rect().height();
    let max_h = (viewport - 132.0).max(280.0);
    let height = (width * 0.566).min(max_h).max(260.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let time = ui.input(|i| i.time) as f32;

    match cfg.style.as_str() {
        "digital" => digital(ui, rect, t, &cfg, time, lang, battery),
        "analog" => analog(ui, rect, t, &cfg, lang, battery),
        "minimal" => minimal(ui, rect, t, &cfg, lang),
        _ => tide(ui, rect, t, &cfg, time, lang, battery),
    }

    ui.add_space(10.0);
    controls(app, ui, t);

    let animated = cfg.animate && cfg.anim != "off" && cfg.style != "minimal";
    let ms = if animated {
        33
    } else if cfg.seconds {
        250
    } else {
        1000
    };
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(ms));
}

// ---------------------------------------------------------------- tide

fn tide(
    ui: &mut Ui,
    rect: Rect,
    _t: &Tokens,
    cfg: &ClockCfg,
    time: f32,
    lang: Lang,
    battery: Option<u8>,
) {
    let painter = ui.painter_at(rect);
    let now = theme::local_now();
    let s = (rect.width() / REF_W).min(rect.height() / REF_H);
    let ox = rect.left() + (rect.width() - REF_W * s) * 0.5;
    let oy = rect.top() + (rect.height() - REF_H * s) * 0.5;
    let p = |x: f32, y: f32| pos2(ox + x * s, oy + y * s);
    let drift = cfg.animate && cfg.anim == "drift";
    let pulse = cfg.animate && cfg.anim == "pulse";
    let anim = if drift {
        time * cfg.speed.max(0.05)
    } else {
        0.0
    };

    let accent = if cfg.mono {
        _t.ink
    } else {
        color(&cfg.accent, Color32::from_rgb(63, 210, 255))
    };
    let bg_top = if cfg.mono {
        _t.bg
    } else {
        color(&cfg.bg_top, Color32::from_rgb(14, 95, 202))
    };
    let bg_bottom = if cfg.mono {
        _t.bg
    } else {
        color(&cfg.bg_bottom, Color32::from_rgb(2, 12, 40))
    };
    let water_mid = theme::mix(bg_top, bg_bottom, 0.62);

    painter.rect_filled(rect, 0.0, bg_bottom);
    vgrad(
        &painter,
        Rect::from_min_max(p(0.0, 0.0), p(REF_W, 150.0)),
        bg_top,
        water_mid,
    );
    vgrad(
        &painter,
        Rect::from_min_max(p(0.0, 150.0), p(REF_W, REF_H)),
        water_mid,
        bg_bottom,
    );

    // The house grid over the water: hard 32-unit lines, no glow.
    let step = 32.0;
    let grid = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 14);
    let mut gx = 0.0;
    while gx <= REF_W {
        painter.line_segment([p(gx, 0.0), p(gx, REF_H)], Stroke::new(1.0, grid));
        gx += step;
    }
    let mut gy = 0.0;
    while gy <= REF_H {
        painter.line_segment([p(0.0, gy), p(REF_W, gy)], Stroke::new(1.0, grid));
        gy += step;
    }

    // six hard, grid-aligned bars drifting through the water
    if cfg.show_stripes {
        let bars: [[f32; 4]; 6] = [
            [29.0, 96.0, 32.0, 22.0],
            [128.0, 32.0, 64.0, 13.0],
            [192.0, 128.0, 64.0, 17.0],
            [256.0, 64.0, 32.0, 6.0],
            [320.0, 160.0, 64.0, 12.0],
            [416.0, 96.0, 32.0, 12.0],
        ];
        for (k, b) in bars.iter().enumerate() {
            let kf = k as f32;
            let dx = (anim * 0.25 + kf * 1.7).sin() * 12.0;
            let rect = Rect::from_min_size(
                p(b[0] + dx, b[2]),
                vec2(b[1], if k % 2 == 0 { 3.0 } else { 2.0 }) * s,
            );
            painter.rect_filled(
                rect,
                0.0,
                Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), b[3] as u8),
            );
        }
    }

    // square ticks rise instead of bubbles
    if cfg.show_bubbles {
        for k in 0..12 {
            let kf = k as f32;
            let bx = (kf * 61.7).rem_euclid(300.0) + (anim * 0.7 + kf).sin() * 5.0;
            let by =
                REF_H - (anim * (8.0 + (k % 5) as f32 * 3.0) + kf * 37.0).rem_euclid(REF_H + 20.0);
            let side = (2.0 + (k % 3) as f32) * 2.0;
            painter.rect_stroke(
                Rect::from_min_size(p(bx, by), vec2(side, side) * s),
                0.0,
                Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 0x46),
                ),
                egui::StrokeKind::Inside,
            );
        }
    }

    // deepen the bottom and dim everything a little, like the PSP screen
    vgrad(
        &painter,
        Rect::from_min_max(p(0.0, 170.0), p(REF_W, 272.0)),
        Color32::from_rgba_unmultiplied(bg_bottom.r(), bg_bottom.g(), bg_bottom.b(), 0),
        Color32::from_rgba_unmultiplied(bg_bottom.r(), bg_bottom.g(), bg_bottom.b(), 0x96),
    );
    painter.rect_filled(
        Rect::from_min_max(p(0.0, 0.0), p(REF_W, REF_H)),
        0.0,
        Color32::from_black_alpha(0x28),
    );

    // the moon
    if cfg.show_moon {
        let breathe = if pulse {
            0.75 + 0.25 * (time * cfg.speed * 1.2).sin()
        } else {
            1.0
        };
        let mx = 410.0;
        let my = 128.0;
        let r = 118.0;
        painter.circle_filled(
            p(mx, my),
            (r + 60.0) * s,
            Color32::from_white_alpha((0x0c as f32 * breathe) as u8),
        );
        painter.circle_filled(
            p(mx, my),
            (r + 24.0) * s,
            Color32::from_white_alpha((0x0c as f32 * breathe) as u8),
        );
        moon(&painter, p(mx, my), r * s, now.day, s, bg_bottom);
    }

    // tilted time: white with an accent shadow to the lower right
    let txt = time_text(cfg, &now);
    let scale = cfg.font_scale.clamp(0.5, 2.0);
    let size = (if cfg.seconds { 66.0 } else { 88.0 }) * s * scale;
    let font = FontId::new(size, FontFamily::Proportional);
    let text_color = if cfg.mono { _t.strong } else { Color32::WHITE };
    let galley = painter.layout_no_wrap(txt.clone(), font.clone(), text_color);
    let tw = galley.size().x / s;
    let tx = match cfg.position.as_str() {
        "center" => (REF_W - tw) * 0.5,
        "right" => REF_W - tw - 24.0,
        _ => 26.0,
    };
    let pivot = p(120.0, 110.0);
    let top_left = p(tx, 80.0);
    let angle = -2.0_f32.to_radians();
    rotated_text(
        &painter,
        pivot,
        top_left + vec2(6.5, 4.3) * s,
        &txt,
        font.clone(),
        accent,
        angle,
    );
    rotated_text(&painter, pivot, top_left, &txt, font, text_color, angle);

    if cfg.date {
        // day-part plate
        let part = match now.hour / 6 {
            0 => t2(lang, "ночь", "night"),
            1 => t2(lang, "утро", "morning"),
            2 => t2(lang, "день", "afternoon"),
            _ => t2(lang, "вечер", "evening"),
        };
        let plate = Rect::from_min_size(p(44.0, 164.0), vec2(72.0, 20.8) * s);
        painter.rect_filled(
            plate.translate(vec2(4.0, 3.0) * s),
            0.0,
            Color32::from_rgba_unmultiplied(bg_bottom.r(), bg_bottom.g(), bg_bottom.b(), 0xC8),
        );
        painter.rect_filled(plate, 0.0, accent);
        painter.text(
            plate.center(),
            Align2::CENTER_CENTER,
            part.to_uppercase(),
            FontId::new(12.0 * s, FontFamily::Monospace),
            Color32::from_rgb(2, 12, 40),
        );

        // weekday · date · battery
        let mut line = String::new();
        if cfg.weekday {
            line.push_str(weekday_name(lang, now.weekday));
            line.push(' ');
        }
        line.push_str(&format!("{:02}.{:02}.{}", now.day, now.month, now.year));
        if cfg.battery {
            if let Some(bat) = battery {
                line.push_str(&format!(" · {}%", bat));
            }
        }
        painter.text(
            p(48.7, 212.0),
            Align2::LEFT_CENTER,
            line.to_uppercase(),
            FontId::new(11.0 * s * scale.min(1.3), FontFamily::Proportional),
            Color32::from_rgba_unmultiplied(196, 241, 255, 255),
        );
    }

    if !cfg.caption.trim().is_empty() {
        painter.text(
            p(REF_W * 0.5, 244.0),
            Align2::CENTER_CENTER,
            cfg.caption.trim(),
            FontId::new(13.0 * s, FontFamily::Proportional),
            Color32::from_rgba_unmultiplied(196, 241, 255, 220),
        );
    }
}

fn moon(painter: &egui::Painter, center: Pos2, r: f32, day: u32, s: f32, shadow_color: Color32) {
    // A simple synodic phase: new around day 1, full around day 15.
    let phase = (day as f32 / 29.53).fract();
    let lit = (1.0 - (phase * TAU).cos()) * 0.5;
    painter.circle_filled(center, r, Color32::WHITE);
    // Slide the shadow across to carve the lit shape.
    let dx = lit * 2.0 * r * if phase < 0.5 { 1.0 } else { -1.0 };
    painter.circle_filled(
        center + vec2(dx, 0.0),
        r,
        Color32::from_rgba_unmultiplied(shadow_color.r(), shadow_color.g(), shadow_color.b(), 0xC8),
    );
    for (cx, cy, cr) in [
        (-33.0, -26.0, 28.35),
        (26.0, 21.3, 21.25),
        (11.8, -56.6, 11.8),
        (-42.4, 40.2, 15.35),
        (49.6, -26.0, 9.45),
    ] {
        painter.circle_filled(
            center + vec2(cx * s, cy * s),
            cr * s,
            Color32::from_rgba_unmultiplied(178, 178, 178, 0x22),
        );
    }
    painter.circle_stroke(
        center,
        r + 0.6 * s,
        Stroke::new(1.2 * s, Color32::from_white_alpha(0x78)),
    );
}

// ---------------------------------------------------------------- digital

fn digital(
    ui: &mut Ui,
    rect: Rect,
    t: &Tokens,
    cfg: &ClockCfg,
    time: f32,
    lang: Lang,
    battery: Option<u8>,
) {
    let painter = ui.painter_at(rect);
    let now = theme::local_now();
    let accent = if cfg.mono {
        t.ink
    } else {
        color(&cfg.accent, t.accent)
    };
    let bg = if cfg.mono {
        t.bg
    } else {
        color(&cfg.bg_bottom, t.bg)
    };
    painter.rect_filled(rect, 0.0, bg);
    // a soft accent glow that breathes
    let breathe = if cfg.animate && cfg.anim != "off" {
        0.5 + 0.5 * (time * cfg.speed * 0.5).sin()
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
        FontId::new(size, FontFamily::Monospace),
        t.strong,
    );
    let mut line = String::new();
    if cfg.date {
        if cfg.weekday {
            line.push_str(weekday_name(lang, now.weekday));
            line.push(' ');
        }
        line.push_str(&format!("{:02}.{:02}.{}", now.day, now.month, now.year));
    }
    if cfg.battery {
        if let Some(bat) = battery {
            if !line.is_empty() {
                line.push_str(" · ");
            }
            line.push_str(&format!("{bat}%"));
        }
    }
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
    let bar_h = 3.0;
    painter.rect_filled(
        Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - bar_h),
            vec2(rect.width() * frac, bar_h),
        ),
        0.0,
        accent,
    );
}

// ---------------------------------------------------------------- analog

fn analog(ui: &mut Ui, rect: Rect, t: &Tokens, cfg: &ClockCfg, lang: Lang, battery: Option<u8>) {
    let painter = ui.painter_at(rect);
    let now = theme::local_now();
    let accent = if cfg.mono {
        t.ink
    } else {
        color(&cfg.accent, t.accent)
    };
    painter.rect_filled(
        rect,
        0.0,
        if cfg.mono {
            t.bg
        } else {
            color(&cfg.bg_top, t.bg)
        },
    );
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
            line.push_str(weekday_name(lang, now.weekday));
            line.push(' ');
        }
        line.push_str(&format!("{:02}.{:02}", now.day, now.month));
    }
    if cfg.battery {
        if let Some(bat) = battery {
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

fn minimal(ui: &mut Ui, rect: Rect, t: &Tokens, cfg: &ClockCfg, lang: Lang) {
    let painter = ui.painter_at(rect);
    let now = theme::local_now();
    let accent = if cfg.mono {
        t.ink
    } else {
        color(&cfg.accent, t.accent)
    };
    painter.rect_filled(
        rect,
        0.0,
        if cfg.mono {
            t.bg
        } else {
            color(&cfg.bg_bottom, t.bg)
        },
    );
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
        FontId::new(size, FontFamily::Proportional),
        t.strong,
    );
    if cfg.date {
        let mut line = String::new();
        if cfg.weekday {
            line.push_str(weekday_name(lang, now.weekday));
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

fn controls(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let mut cfg = app.cfg.clock.clone();
    let before = cfg.clone();
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "часы", "clock"), t.faint));
        ui.horizontal_wrapped(|ui| {
            for (key, ru, en) in [
                ("tide", "tide", "tide"),
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
            ] {
                theme::switch(ui, value, t);
                ui.label(rt(t!(app.lang, ru, en), 12.5, t.muted));
                ui.add_space(8.0);
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
                let active = cfg.anim == key;
                if ui
                    .selectable_label(active, rt(t!(app.lang, ru, en), 12.0, t.muted))
                    .clicked()
                {
                    cfg.anim = key.to_string();
                }
            }
            ui.add_space(10.0);
            ui.label(rt(t!(app.lang, "скорость", "speed"), 12.0, t.faint));
            ui.add(
                egui::Slider::new(&mut cfg.speed, 0.2..=3.0)
                    .show_value(false)
                    .trailing_fill(true),
            );
            ui.label(rt(format!("{:.1}×", cfg.speed), 11.5, t.faint).family(FontFamily::Monospace));
            ui.add_space(10.0);
            ui.label(rt(t!(app.lang, "размер", "size"), 12.0, t.faint));
            ui.add(
                egui::Slider::new(&mut cfg.font_scale, 0.6..=1.8)
                    .show_value(false)
                    .trailing_fill(true),
            );
            ui.label(
                rt(format!("{:.0}%", cfg.font_scale * 100.0), 11.5, t.faint)
                    .family(FontFamily::Monospace),
            );
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
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
            ui.add_space(10.0);
            ui.label(rt(t!(app.lang, "луна", "moon"), 12.0, t.faint));
            theme::switch(ui, &mut cfg.show_moon, t);
            ui.add_space(8.0);
            ui.label(rt(t!(app.lang, "волны", "waves"), 12.0, t.faint));
            theme::switch(ui, &mut cfg.show_stripes, t);
            ui.add_space(8.0);
            ui.label(rt(t!(app.lang, "пузыри", "bubbles"), 12.0, t.faint));
            theme::switch(ui, &mut cfg.show_bubbles, t);
        });
        ui.add_space(4.0);
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
                    .desired_width(260.0),
            );
            ui.add_space(10.0);
            if theme::ghost(ui, t!(app.lang, "сбросить", "reset"), t).clicked() {
                cfg = ClockCfg::default();
            }
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
        || cfg.show_moon != before.show_moon
        || cfg.show_bubbles != before.show_bubbles
        || cfg.show_stripes != before.show_stripes
        || cfg.position != before.position
        || cfg.accent != before.accent
        || cfg.bg_top != before.bg_top
        || cfg.bg_bottom != before.bg_bottom
        || (cfg.speed - before.speed).abs() > f32::EPSILON
        || (cfg.font_scale - before.font_scale).abs() > f32::EPSILON
    {
        app.cfg.clock = cfg;
        app.save();
    }
}

// ---------------------------------------------------------------- helpers

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

fn t2(lang: Lang, ru: &'static str, en: &'static str) -> &'static str {
    match lang {
        Lang::Ru => ru,
        _ => en,
    }
}

/// `RichText` under a local name so the control rows stay short.
fn rt(text: impl Into<String>, size: f32, color: Color32) -> egui::RichText {
    egui::RichText::new(text.into()).size(size).color(color)
}

fn vgrad(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    if rect.height() <= 0.0 || rect.width() <= 0.0 {
        return;
    }
    let steps = 48;
    let h = rect.height() / steps as f32;
    for i in 0..steps {
        let k = i as f32 / (steps - 1) as f32;
        let c = theme::mix(top, bottom, k);
        painter.rect_filled(
            Rect::from_min_size(
                pos2(rect.left(), rect.top() + i as f32 * h),
                vec2(rect.width(), h + 1.0),
            ),
            0.0,
            c,
        );
    }
}

/// Draw text rotated clockwise around `pivot`, with its top-left at `top_left`.
fn rotated_text(
    painter: &egui::Painter,
    pivot: Pos2,
    top_left: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    angle: f32,
) {
    let v = top_left - pivot;
    let (sin, cos) = angle.sin_cos();
    let rotated = pivot + vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
    let galley = painter.layout_no_wrap(text.to_owned(), font, color);
    painter.add(TextShape::new(rotated, galley, color).with_angle(angle));
}
