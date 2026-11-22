//! The visualizer as a page of its own.
//!
//! The strip also lives in the top bar, the control centre and on the clock
//! face, where it is small by necessity. This page hands it the whole canvas:
//! the bars are scaled, placed and put on a surface of their own, and the same
//! canvas fills the window in full-screen mode. The look is still one
//! [`VizCfg`], so a change here is the same change everywhere.

use crate::app::App;
use crate::clock::{color_edit, rt};
use crate::theme::{self, Tokens};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontFamily, FontId, Rect, Sense, Stroke, Ui, Vec2,
};
use sdw_core::model::VizCfg;
use sdw_core::t;
use std::time::Duration;

/// The page: the stage on top, every setting underneath.
pub fn page(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let width = ui.available_width().max(280.0);
    let viewport = ui.ctx().content_rect().height();
    let max_h = (viewport - 190.0).max(240.0);
    let height = (width * 0.56).min(max_h).max(220.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    canvas(ui, rect, app, t);

    // The whole window, one tap away, like the clock face.
    let button = Rect::from_min_size(
        pos2(rect.right() - 42.0, rect.top() + 8.0),
        vec2(34.0, 34.0),
    );
    // `Ui::put` advances the cursor past the button (egui 0.37), so the
    // controls below would move up over the canvas. A child UI keeps the
    // page cursor where it was.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(button));
    if child
        .add(
            egui::Button::new(theme::icon(theme::icon::FULLSCREEN, 20.0, t.strong))
                .fill(t.bg2)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(0.0),
        )
        .on_hover_text(t!(app.lang, "на весь экран", "full screen"))
        .clicked()
    {
        app.viz_full = true;
    }

    ui.add_space(10.0);
    controls(app, ui, t);
}

/// Paint the stage into `rect`: its own background, the optional house grid,
/// a hairline frame and the bars inside. The page and the full-screen overlay
/// share this, so the picture is the same at any size.
pub fn canvas(ui: &mut Ui, rect: Rect, app: &App, t: &Tokens) {
    let cfg = &app.cfg.viz;
    let accent = theme::parse_color(cfg.color.trim()).unwrap_or(t.accent);
    let stage = stage_rect(cfg, rect);
    let bg = match cfg.stage_bg.as_str() {
        "black" => Color32::BLACK,
        "accent" => theme::mix(t.bg, accent, 0.14),
        _ => t.bg,
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(stage, 0.0, bg);
    if cfg.stage_grid {
        grid(&painter, stage, accent);
    }
    painter.rect_stroke(
        stage,
        0.0,
        Stroke::new(1.0, t.line_strong),
        egui::StrokeKind::Inside,
    );
    if !cfg.enabled {
        painter.text(
            stage.center(),
            Align2::CENTER_CENTER,
            t!(
                app.lang,
                "визуализатор выключен — включите его ниже",
                "the visualizer is off — turn it on below"
            ),
            FontId::new(13.0, FontFamily::Proportional),
            t.faint,
        );
        return;
    }
    // A little air between the bars and the frame: at a full-size stage the
    // outer bars would otherwise weld themselves to the hairline.
    let pad = (stage.width().min(stage.height()) * 0.03).clamp(4.0, 16.0);
    crate::viz::paint(ui, app, t, stage.shrink(pad));
}

/// Where the stage sits: a percentage of the canvas, one of nine anchors and
/// a nudge in percent of the canvas. The result is always kept inside the
/// canvas, so no setting can push the bars out of the picture.
pub fn stage_rect(cfg: &VizCfg, canvas: Rect) -> Rect {
    let scale = cfg.stage_scale.clamp(10, 100) as f32 / 100.0;
    let size = if cfg.style == "circular" {
        // A circle in a wide slot would be a dot; keep it square.
        Vec2::splat((canvas.width().min(canvas.height()) * scale).max(40.0))
    } else {
        vec2(
            (canvas.width() * scale).max(40.0),
            (canvas.height() * scale).max(6.0),
        )
    };
    let size = vec2(
        size.x.min(canvas.width()).max(1.0),
        size.y.min(canvas.height()).max(1.0),
    );
    let (ax, ay) = crate::viz::anchor_xy(&cfg.stage_place);
    let ox = canvas.width() * cfg.stage_offset_x.clamp(-50, 50) as f32 / 100.0;
    let oy = canvas.height() * cfg.stage_offset_y.clamp(-50, 50) as f32 / 100.0;
    let x = canvas.left() + (canvas.width() - size.x) * ax + ox;
    let y = canvas.top() + (canvas.height() - size.y) * ay + oy;
    let min = pos2(
        x.clamp(canvas.left(), (canvas.right() - size.x).max(canvas.left())),
        y.clamp(canvas.top(), (canvas.bottom() - size.y).max(canvas.top())),
    );
    Rect::from_min_size(min, size)
}

/// The house hairline grid, scaled to the stage and tinted with the bars'
/// accent so it belongs to them instead of to the page.
fn grid(painter: &egui::Painter, rect: Rect, accent: Color32) {
    let st = theme::style();
    if !st.grid {
        return;
    }
    let step = (st.grid_step * (rect.height() / 272.0)).max(16.0);
    let alpha = (st.grid_alpha * 2.6).clamp(8.0, 60.0) as u8;
    let line = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha);
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

/// The stage over the whole window, without chrome. Any tap or Escape brings
/// the page back; the two shortcuts stay in the corner like on the clock.
pub fn fullscreen(app: &mut App, root: &mut Ui, ctx: &egui::Context) {
    let t = Tokens::get(ctx);
    let lang = app.lang;
    let mut consumed = false;
    let mut action: Option<Action> = None;
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(t.bg))
        .show(root, |ui| {
            let rect = ui.available_rect_before_wrap();
            canvas(ui, rect, app, &t);

            let side = 40.0;
            let gap = 20.0;
            let top = rect.top() + 16.0;
            let mut x = rect.right() - 20.0 - side;
            let button = |ui: &mut Ui, x: f32, glyph: &str, active: bool, tip: &str| {
                let r = Rect::from_min_size(pos2(x, top), vec2(side, side));
                ui.put(
                    r,
                    egui::Button::new(theme::icon(
                        glyph,
                        21.0,
                        if active { t.accent } else { t.strong },
                    ))
                    .fill(t.bg2)
                    .stroke(Stroke::new(1.0, if active { t.accent } else { t.line }))
                    .corner_radius(0.0),
                )
                .on_hover_text(tip)
            };
            if button(
                ui,
                x,
                theme::icon::NOTIFICATIONS,
                app.notif_unread > 0,
                t!(lang, "уведомления", "notifications"),
            )
            .clicked()
            {
                consumed = true;
                action = Some(Action::Notifications);
            }
            x -= side + gap;
            if button(
                ui,
                x,
                theme::icon::TUNE,
                app.quick_open,
                t!(lang, "центр управления", "control centre"),
            )
            .clicked()
            {
                consumed = true;
                action = Some(Action::Quick);
            }

            ui.painter().text(
                pos2(rect.center().x, rect.bottom() - 16.0),
                Align2::CENTER_CENTER,
                t!(
                    lang,
                    "нажмите или esc, чтобы выйти",
                    "tap or press esc to exit"
                ),
                FontId::new(11.5, FontFamily::Monospace),
                Color32::from_white_alpha(90),
            );
        });
    match action {
        Some(Action::Notifications) => app.set_page(crate::app::Page::Notifications),
        Some(Action::Quick) => {
            let open = !app.quick_open;
            app.set_quick(open);
        }
        None => {}
    }
    if (ctx.input(|i| i.pointer.any_click()) || ctx.input(|i| i.key_pressed(egui::Key::Escape)))
        && !consumed
        && !app.quick_clicked
        && !app.quick_open
    {
        app.viz_full = false;
    }
    ctx.request_repaint_after(Duration::from_millis(app.viz_interval_ms()));
}

/// What a shortcut on the full-screen stage asked for, applied after the
/// canvas borrow of `App` is released.
enum Action {
    Notifications,
    Quick,
}

/// Every setting of the visualizer in one card. The look page and the
/// visualizer page show the same card, so the two can never drift apart.
pub fn controls(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let mut viz = app.cfg.viz.clone();
    let mut viz_dirty = false;
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::hud(
                t!(app.lang, "визуализатор", "visualizer"),
                t.faint,
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::icon_button(ui, theme::icon::FULLSCREEN, t, 30.0, false)
                    .on_hover_text(t!(app.lang, "на весь экран", "full screen"))
                    .clicked()
                {
                    app.viz_full = true;
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            let before = viz.enabled;
            theme::switch(ui, &mut viz.enabled, t);
            if viz.enabled != before {
                viz_dirty = true;
            }
            ui.label(rt(t!(app.lang, "включен", "enabled"), 13.0, t.muted));
            ui.add_space(14.0);
            let before = viz.nav;
            theme::switch(ui, &mut viz.nav, t);
            if viz.nav != before {
                viz_dirty = true;
            }
            ui.label(rt(t!(app.lang, "навбар", "nav bar"), 13.0, t.muted));
            ui.add_space(14.0);
            let before = viz.shade;
            theme::switch(ui, &mut viz.shade, t);
            if viz.shade != before {
                viz_dirty = true;
            }
            ui.label(rt(
                t!(app.lang, "центр управления", "control centre"),
                13.0,
                t.muted,
            ));
            ui.add_space(14.0);
            let before = viz.clock;
            theme::switch(ui, &mut viz.clock, t);
            if viz.clock != before {
                viz_dirty = true;
            }
            ui.label(rt(t!(app.lang, "циферблат", "clock face"), 13.0, t.muted));
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let mut bars = viz.bars as f32;
            if theme::slider(
                ui,
                t,
                t!(app.lang, "полосы", "bars"),
                &mut bars,
                4.0..=24.0,
                |v| format!("{:.0}", v),
            ) {
                viz.bars = bars.round() as u8;
                viz_dirty = true;
            }
            let mut height = viz.height as f32;
            if theme::slider(
                ui,
                t,
                t!(app.lang, "высота", "height"),
                &mut height,
                6.0..=240.0,
                |v| format!("{:.0}", v),
            ) {
                viz.height = height.round() as u8;
                viz_dirty = true;
            }
            let mut width = viz.width as f32;
            if theme::slider(
                ui,
                t,
                t!(app.lang, "ширина", "width"),
                &mut width,
                40.0..=1000.0,
                |v| format!("{:.0}", v),
            ) {
                viz.width = width.round() as u16;
                viz_dirty = true;
            }
            let mut gap = viz.gap as f32;
            if theme::slider(
                ui,
                t,
                t!(app.lang, "шаг", "gap"),
                &mut gap,
                0.0..=8.0,
                |v| format!("{:.0}", v),
            ) {
                viz.gap = gap.round() as u8;
                viz_dirty = true;
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let mut opacity = viz.opacity as f32;
            if theme::slider(
                ui,
                t,
                t!(app.lang, "прозрачность", "opacity"),
                &mut opacity,
                20.0..=100.0,
                |v| format!("{:.0}%", v),
            ) {
                viz.opacity = opacity.round() as u8;
                viz_dirty = true;
            }
            let mut gain = viz.gain as f32;
            if theme::slider(
                ui,
                t,
                t!(app.lang, "чувствительность", "gain"),
                &mut gain,
                50.0..=200.0,
                |v| format!("{:.0}%", v),
            ) {
                viz.gain = gain.round() as u8;
                viz_dirty = true;
            }
            let mut fps = viz.fps as f32;
            if theme::slider(
                ui,
                t,
                t!(app.lang, "fps", "fps"),
                &mut fps,
                10.0..=60.0,
                |v| format!("{:.0}", v),
            ) {
                viz.fps = fps.round() as u8;
                viz_dirty = true;
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (key, ru, en) in [
                ("bars", "полосы", "bars"),
                ("dots", "точки", "dots"),
                ("wave", "волна", "wave"),
                ("circular", "кругом", "circular"),
            ] {
                if ui
                    .selectable_label(viz.style == key, rt(t!(app.lang, ru, en), 13.0, t.muted))
                    .clicked()
                {
                    viz.style = key.to_string();
                    viz_dirty = true;
                }
            }
            ui.add_space(8.0);
            let before = viz.mirror;
            theme::switch(ui, &mut viz.mirror, t);
            if viz.mirror != before {
                viz_dirty = true;
            }
            ui.label(rt(t!(app.lang, "зеркало", "mirror"), 13.0, t.muted));
            ui.add_space(8.0);
            let before = viz.peak;
            theme::switch(ui, &mut viz.peak, t);
            if viz.peak != before {
                viz_dirty = true;
            }
            ui.label(rt(t!(app.lang, "пик", "peak"), 13.0, t.muted));
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(rt(t!(app.lang, "акцент", "accent"), 12.0, t.faint));
            color_edit(ui, &mut viz.color, t);
            ui.add_space(10.0);
            ui.label(rt(
                t!(app.lang, "навбар — сторона", "nav bar side"),
                12.0,
                t.faint,
            ));
            for (key, ru, en) in [("left", "лево", "left"), ("right", "право", "right")] {
                if ui
                    .selectable_label(viz.nav_side == key, rt(t!(app.lang, ru, en), 12.0, t.muted))
                    .clicked()
                {
                    viz.nav_side = key.to_string();
                    viz_dirty = true;
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(rt(
                t!(app.lang, "якорь на циферблате", "clock anchor"),
                12.0,
                t.faint,
            ));
            for (key, ru, en) in [
                ("tl", "вл", "tl"),
                ("tc", "вц", "tc"),
                ("tr", "вп", "tr"),
                ("ml", "цл", "ml"),
                ("mc", "цц", "mc"),
                ("mr", "цп", "mr"),
                ("bl", "нл", "bl"),
                ("bc", "нц", "bc"),
                ("br", "нп", "br"),
            ] {
                if ui
                    .selectable_label(viz.anchor == key, rt(t!(app.lang, ru, en), 12.0, t.muted))
                    .clicked()
                {
                    viz.anchor = key.to_string();
                    viz_dirty = true;
                }
            }
            ui.add_space(8.0);
            let mut offset_x = viz.offset_x as f32;
            if theme::slider(
                ui,
                t,
                t!(app.lang, "сдвиг X", "offset X"),
                &mut offset_x,
                -50.0..=50.0,
                |v| format!("{:.0}", v),
            ) {
                viz.offset_x = offset_x.round() as i16;
                viz_dirty = true;
            }
            let mut offset_y = viz.offset_y as f32;
            if theme::slider(
                ui,
                t,
                t!(app.lang, "сдвиг Y", "offset Y"),
                &mut offset_y,
                -50.0..=50.0,
                |v| format!("{:.0}", v),
            ) {
                viz.offset_y = offset_y.round() as i16;
                viz_dirty = true;
            }
        });
        ui.add_space(8.0);
        // The page and the full-screen stage: their own canvas, not the strip
        // in a row. Size, place and background are separate from the clock's,
        // because the two surfaces are not the same shape.
        ui.label(theme::hud(t!(app.lang, "полотно", "canvas"), t.faint));
        ui.horizontal_wrapped(|ui| {
            let mut scale = viz.stage_scale as f32;
            if theme::slider(
                ui,
                t,
                t!(app.lang, "размер полотна", "canvas size"),
                &mut scale,
                10.0..=100.0,
                |v| format!("{:.0}%", v),
            ) {
                viz.stage_scale = scale.round() as u8;
                viz_dirty = true;
            }
            let mut offset_x = viz.stage_offset_x as f32;
            if theme::slider(
                ui,
                t,
                t!(app.lang, "сдвиг X", "offset X"),
                &mut offset_x,
                -50.0..=50.0,
                |v| format!("{:.0}%", v),
            ) {
                viz.stage_offset_x = offset_x.round() as i16;
                viz_dirty = true;
            }
            let mut offset_y = viz.stage_offset_y as f32;
            if theme::slider(
                ui,
                t,
                t!(app.lang, "сдвиг Y", "offset Y"),
                &mut offset_y,
                -50.0..=50.0,
                |v| format!("{:.0}%", v),
            ) {
                viz.stage_offset_y = offset_y.round() as i16;
                viz_dirty = true;
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(rt(t!(app.lang, "место", "place"), 12.0, t.faint));
            for (key, ru, en) in [
                ("tl", "вл", "tl"),
                ("tc", "вц", "tc"),
                ("tr", "вп", "tr"),
                ("ml", "цл", "ml"),
                ("mc", "цц", "mc"),
                ("mr", "цп", "mr"),
                ("bl", "нл", "bl"),
                ("bc", "нц", "bc"),
                ("br", "нп", "br"),
            ] {
                if ui
                    .selectable_label(
                        viz.stage_place == key,
                        rt(t!(app.lang, ru, en), 12.0, t.muted),
                    )
                    .clicked()
                {
                    viz.stage_place = key.to_string();
                    viz_dirty = true;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(rt(t!(app.lang, "фон", "background"), 12.0, t.faint));
            for (key, ru, en) in [
                ("theme", "тема", "theme"),
                ("black", "чёрный", "black"),
                ("accent", "акцент", "accent"),
            ] {
                if ui
                    .selectable_label(viz.stage_bg == key, rt(t!(app.lang, ru, en), 12.0, t.muted))
                    .clicked()
                {
                    viz.stage_bg = key.to_string();
                    viz_dirty = true;
                }
            }
            ui.add_space(10.0);
            let before = viz.stage_grid;
            theme::switch(ui, &mut viz.stage_grid, t);
            if viz.stage_grid != before {
                viz_dirty = true;
            }
            ui.label(rt(t!(app.lang, "сетка", "grid"), 12.5, t.muted));
        });
        ui.add_space(8.0);
        if theme::ghost(ui, t!(app.lang, "сбросить", "reset"), t).clicked() {
            viz = VizCfg::default();
            viz_dirty = true;
        }
        if theme::primary(ui, t!(app.lang, "применить", "apply"), t).clicked() {
            viz_dirty = true;
        }
        // The preview must show the draft, not the value saved last frame.
        app.cfg.viz = viz.clone();
        crate::viz::preview(ui, app, t);
    });
    app.cfg.viz = viz;
    if viz_dirty {
        app.save_soon();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 600.0))
    }

    #[test]
    fn a_full_stage_fills_the_canvas() {
        let mut cfg = VizCfg::default();
        cfg.stage_scale = 100;
        cfg.stage_place = "mc".into();
        let r = stage_rect(&cfg, canvas());
        assert!((r.width() - 1200.0).abs() < 0.01);
        assert!((r.height() - 600.0).abs() < 0.01);
    }

    #[test]
    fn a_nudge_can_never_push_the_stage_out() {
        let mut cfg = VizCfg::default();
        cfg.stage_scale = 60;
        cfg.stage_place = "mc".into();
        cfg.stage_offset_x = 50;
        cfg.stage_offset_y = -50;
        let c = canvas();
        let r = stage_rect(&cfg, c);
        assert!(r.left() >= c.left() - 0.01 && r.right() <= c.right() + 0.01);
        assert!(r.top() >= c.top() - 0.01 && r.bottom() <= c.bottom() + 0.01);
    }

    #[test]
    fn a_circle_stays_square_on_a_wide_canvas() {
        let mut cfg = VizCfg::default();
        cfg.style = "circular".into();
        cfg.stage_scale = 80;
        let r = stage_rect(&cfg, canvas());
        assert!((r.width() - r.height()).abs() < 0.01);
        assert!((r.width() - 480.0).abs() < 0.01);
    }
}
