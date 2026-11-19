//! hard edges, hairlines, monospace captions. two appearances, one set of tokens.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Response,
    RichText, Stroke, TextStyle, Ui, Vec2, Visuals,
};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Tokens {
    pub dark: bool,
    pub bg: Color32,
    pub well: Color32,
    pub ink: Color32,
    pub strong: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub accent: Color32,
    pub accent_ink: Color32,
    pub inv_bg: Color32,
    pub inv_fg: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
}

const fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

const fn alpha(c: u32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const((c >> 16) as u8, (c >> 8) as u8, c as u8, a)
}

impl Tokens {
    pub const DARK: Self = Self {
        dark: true,
        bg: rgb(0x111214),
        well: rgb(0x08090a),
        ink: rgb(0xeceae6),
        strong: rgb(0xffffff),
        muted: alpha(0xeceae6, 150),
        faint: alpha(0xeceae6, 95),
        line: alpha(0xeceae6, 36),
        line_strong: alpha(0xeceae6, 100),
        accent: rgb(0x3b82f6),
        accent_ink: rgb(0x7fb0ff),
        inv_bg: rgb(0xffffff),
        inv_fg: rgb(0x0b0b0c),
        good: rgb(0x34d3b0),
        warn: rgb(0xffc24a),
        bad: rgb(0xff6159),
    };

    pub const LIGHT: Self = Self {
        dark: false,
        bg: rgb(0xf3f2ef),
        well: rgb(0xe8e7e3),
        ink: rgb(0x121216),
        strong: rgb(0x000000),
        muted: alpha(0x121216, 160),
        faint: alpha(0x121216, 110),
        line: alpha(0x121216, 40),
        line_strong: alpha(0x121216, 100),
        accent: rgb(0x2f6fe4),
        accent_ink: rgb(0x1a56c0),
        inv_bg: rgb(0x121216),
        inv_fg: rgb(0xf3f2ef),
        good: rgb(0x0b7a64),
        warn: rgb(0x8c5a00),
        bad: rgb(0xb42a20),
    };

    pub fn get(ctx: &egui::Context) -> Self {
        if ctx.global_style().visuals.dark_mode {
            Self::DARK
        } else {
            Self::LIGHT
        }
    }

    pub fn hairline(&self) -> Stroke {
        Stroke::new(1.0, self.line)
    }

    pub fn status(&self, ok: bool) -> Color32 {
        if ok {
            self.good
        } else {
            self.bad
        }
    }
}

fn visuals(t: &Tokens) -> Visuals {
    let mut v = if t.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = t.bg;
    v.window_fill = t.bg;
    v.extreme_bg_color = t.well;
    v.faint_bg_color = t.well;
    v.code_bg_color = t.well;
    v.window_stroke = Stroke::new(1.0, t.line_strong);
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.override_text_color = Some(t.ink);
    v.hyperlink_color = t.accent_ink;
    v.selection.bg_fill = t.accent;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.warn_fg_color = t.warn;
    v.error_fg_color = t.bad;
    for (w, fill, stroke, fg) in [
        (&mut v.widgets.noninteractive, t.bg, t.line, t.muted),
        (&mut v.widgets.inactive, t.bg, t.line, t.ink),
        (&mut v.widgets.hovered, t.well, t.line_strong, t.strong),
        (&mut v.widgets.active, t.well, t.line_strong, t.strong),
        (&mut v.widgets.open, t.well, t.line_strong, t.strong),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
        w.corner_radius = CornerRadius::ZERO;
        w.fg_stroke = Stroke::new(1.0, fg);
        w.expansion = 0.0;
    }
    v
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&Tokens::DARK));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&Tokens::LIGHT));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(22.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(13.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(11.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.5, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}

/// Material You symbols, outlined and monochrome.
pub mod icon {
    pub const OVERVIEW: &str = "\u{e871}";
    pub const GAMEPADS: &str = "\u{ea28}";
    pub const SCREEN: &str = "\u{ef5b}";
    pub const NOTIFICATIONS: &str = "\u{e7f5}";
    pub const HOME_ASSISTANT: &str = "\u{e283}";
    pub const SECURITY: &str = "\u{e32a}";
    pub const INSTALL: &str = "\u{f090}";
    pub const SETTINGS: &str = "\u{e8b8}";
    pub const LOGS: &str = "\u{eb8e}";
    pub const CHECK: &str = "\u{e668}";
    pub const CLOSE: &str = "\u{e5cd}";
    pub const STAR: &str = "\u{f09a}";
    pub const ARROW_LEFT: &str = "\u{e5c4}";
    pub const ARROW_RIGHT: &str = "\u{e5c8}";
    pub const DRAG: &str = "\u{e25d}";
    pub const CHECK_BOX: &str = "\u{e86c}";
    pub const ADDON: &str = "\u{e87b}";
    pub const AUDIO: &str = "\u{e32d}";
}

/// One Material symbol, sized and coloured like body text.
pub fn icon(text: &str, size: f32, color: Color32) -> RichText {
    RichText::new(text)
        .family(icon_family())
        .size(size)
        .color(color)
}

pub fn icon_family() -> FontFamily {
    FontFamily::Name("material".into())
}

/// Icon + label as one laid-out run, so the two fonts can coexist.
pub fn nav_text(icon: &str, label: &str, size: f32, color: Color32) -> egui::WidgetText {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    job.append(
        icon,
        0.0,
        TextFormat {
            font_id: FontId::new(size + 3.0, icon_family()),
            color,
            ..Default::default()
        },
    );
    job.append(
        label,
        10.0,
        TextFormat {
            font_id: FontId::new(size, FontFamily::Proportional),
            color,
            ..Default::default()
        },
    );
    job.into()
}

pub fn window_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/icons/sd-widgets-256.png"
    ));
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "archivo".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/Archivo.ttf"
        )))),
    );
    fonts.font_data.insert(
        "material".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/MaterialSymbolsOutlined.ttf"
        )))),
    );
    fonts.font_data.insert(
        "jetbrains".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/JetBrainsMono-Regular.ttf"
        )))),
    );
    if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
        f.insert(0, "archivo".to_owned());
    }
    if let Some(f) = fonts.families.get_mut(&FontFamily::Monospace) {
        f.insert(0, "jetbrains".to_owned());
    }
    fonts.families.insert(
        FontFamily::Name("material".into()),
        vec!["material".to_owned(), "archivo".to_owned()],
    );
    ctx.set_fonts(fonts);
}

pub fn hud(text: &str, color: Color32) -> RichText {
    RichText::new(text.to_lowercase())
        .family(FontFamily::Monospace)
        .size(10.5)
        .extra_letter_spacing(1.4)
        .color(color)
}

pub fn tag(ui: &mut Ui, text: &str, color: Color32) -> Response {
    egui::Frame::new()
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.7)))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| ui.label(hud(text, color)))
        .response
}

pub fn primary(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    ui.add(
        egui::Button::new(
            RichText::new(text.to_lowercase())
                .size(13.0)
                .strong()
                .color(t.inv_fg),
        )
        .fill(t.inv_bg)
        .stroke(Stroke::new(1.0, t.inv_bg))
        .corner_radius(0)
        .min_size(Vec2::new(0.0, 30.0)),
    )
}

pub fn ghost(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text.to_lowercase()).size(13.0).color(t.ink))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, t.line_strong))
            .corner_radius(0)
            .min_size(Vec2::new(0.0, 30.0)),
    )
}

pub fn card<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .stroke(t.hairline())
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn switch(ui: &mut Ui, on: &mut bool, t: &Tokens) -> Response {
    let size = egui::vec2(34.0, 18.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let k = ui.ctx().animate_bool_responsive(resp.id, *on);
    let p = ui.painter();
    p.rect_filled(rect, 9.0, if *on { t.accent } else { t.well });
    p.rect_stroke(
        rect,
        9.0,
        Stroke::new(1.0, if *on { t.accent } else { t.line_strong }),
        egui::StrokeKind::Inside,
    );
    let x = egui::lerp((rect.left() + 9.0)..=(rect.right() - 9.0), k);
    p.circle_filled(
        egui::pos2(x, rect.center().y),
        6.5,
        if *on { Color32::WHITE } else { t.muted },
    );
    resp
}

pub fn dot(ui: &mut Ui, color: Color32, filled: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 16.0), egui::Sense::hover());
    let c = rect.center();
    if filled {
        ui.painter().circle_filled(c, 4.5, color);
    } else {
        ui.painter().circle_stroke(c, 4.0, Stroke::new(1.2, color));
    }
}

pub fn row(ui: &mut Ui, t: &Tokens, title: &str, hint: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() - 170.0).max(160.0));
            ui.label(RichText::new(title).size(14.0).color(t.strong));
            if !hint.is_empty() {
                ui.add(egui::Label::new(RichText::new(hint).size(12.0).color(t.muted)).wrap());
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

pub fn small(ui: &mut Ui, text: &str, color: Color32) {
    ui.add(egui::Label::new(RichText::new(text).size(12.0).color(color)).wrap());
}

pub fn mono(ui: &mut Ui, text: &str, color: Color32) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .family(FontFamily::Monospace)
                .size(12.0)
                .color(color),
        )
        .wrap(),
    );
}

pub fn divider(ui: &mut Ui) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);
}

pub fn copy_button(ui: &mut Ui, text: &str, t: &Tokens) {
    if ui
        .add(
            egui::Button::new(RichText::new("copy").size(11.0).color(t.muted))
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(0),
        )
        .on_hover_text(text)
        .clicked()
    {
        ui.ctx().copy_text(text.to_string());
    }
}
