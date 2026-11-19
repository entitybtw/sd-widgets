//! grid-inspired look: big rounded tiles, soft gradients, one accent per tile.
//! the deck is touched, so everything is at least 40 points tall.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Response,
    RichText, Sense, Stroke, TextStyle, Ui, Vec2, Visuals,
};
use sdw_core::model::{
    ThemeCfg, PRESET_GRUVBOX, PRESET_MONO, PRESET_NORD, PRESET_GRID, PRESET_TERMINAL,
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
pub struct Tokens {
    pub dark: bool,
    pub bg: Color32,
    pub bg2: Color32,
    pub card: Color32,
    pub well: Color32,
    pub ink: Color32,
    pub strong: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
    pub shadow: Color32,
    /// -1 keeps each surface's own radius, otherwise every corner uses this.
    pub radius: f32,
}

const fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}
/// Premultiplied RGBA, computed at compile time so the token tables can stay `const`.
const fn rgba(c: u32, a: u8) -> Color32 {
    let r = (c >> 16) as u32;
    let g = ((c >> 8) & 0xff) as u32;
    let b = (c & 0xff) as u32;
    let a32 = a as u32;
    let r = ((r * a32 + 127) / 255) as u8;
    let g = ((g * a32 + 127) / 255) as u8;
    let b = ((b * a32 + 127) / 255) as u8;
    Color32::from_rgba_premultiplied(r, g, b, a)
}

/// The active customisation, shared with every page through `Tokens::get`.
static STYLE: Mutex<Option<ThemeCfg>> = Mutex::new(None);
static DARK_CACHE: Mutex<Option<Tokens>> = Mutex::new(None);
static LIGHT_CACHE: Mutex<Option<Tokens>> = Mutex::new(None);

pub const PRESETS: [(&str, &str); 5] = [
    ("grid", "Grid"),
    ("mono", "Monochrome"),
    ("nord", "Nord"),
    ("gruvbox", "Gruvbox"),
    ("terminal", "Terminal"),
];

pub fn preset_css(name: &str) -> &'static str {
    match name {
        "mono" => PRESET_MONO,
        "nord" => PRESET_NORD,
        "gruvbox" => PRESET_GRUVBOX,
        "terminal" => PRESET_TERMINAL,
        _ => PRESET_GRID,
    }
}

/// Switch the whole look to a built-in preset. Layout knobs stay as they are.
pub fn apply_preset(cfg: &mut ThemeCfg, name: &str) {
    cfg.css = preset_css(name).to_string();
    cfg.radius = match name {
        "nord" => 2.0,
        "gruvbox" => 1.0,
        _ => 0.0,
    };
    cfg.grid = true;
    cfg.grid_step = 32.0;
    cfg.grid_alpha = 9.0;
}

pub fn set_style(cfg: &ThemeCfg) {
    let dark = apply_css(Tokens::DARK, cfg, false);
    let light = apply_css(Tokens::LIGHT, cfg, true);
    if let Ok(mut slot) = DARK_CACHE.lock() {
        *slot = Some(dark);
    }
    if let Ok(mut slot) = LIGHT_CACHE.lock() {
        *slot = Some(light);
    }
    if let Ok(mut slot) = STYLE.lock() {
        *slot = Some(cfg.clone());
    }
}

pub fn style() -> ThemeCfg {
    STYLE
        .lock()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_default()
}

fn apply_css(mut t: Tokens, cfg: &ThemeCfg, light: bool) -> Tokens {
    let prefix = if light { "light-" } else { "" };
    let pick = |name: &str| {
        css_var(&cfg.css, &format!("{prefix}{name}")).and_then(|v| parse_color(&v))
    };
    if let Some(c) = pick("bg") {
        t.bg = c;
    }
    if let Some(c) = pick("bg2") {
        t.bg2 = c;
    }
    if let Some(c) = pick("card") {
        t.card = c;
    }
    if let Some(c) = pick("well") {
        t.well = c;
    }
    if let Some(c) = pick("ink") {
        t.ink = c;
    }
    if let Some(c) = pick("strong") {
        t.strong = c;
    }
    if let Some(c) = pick("muted") {
        t.muted = c;
    }
    if let Some(c) = pick("faint") {
        t.faint = c;
    }
    if let Some(c) = pick("line") {
        t.line = c;
    }
    if let Some(c) = pick("line-strong") {
        t.line_strong = c;
    }
    if let Some(c) = pick("accent") {
        t.accent = c;
        t.good = c;
    }
    if let Some(c) = pick("accent-soft") {
        t.accent_soft = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 40);
    }
    if let Some(c) = pick("good") {
        t.good = c;
    }
    if let Some(c) = pick("warn") {
        t.warn = c;
    }
    if let Some(c) = pick("bad") {
        t.bad = c;
    }
    // Grid is a hard-edge system: no rounding anywhere, by design.
    t.radius = 0.0;
    let _ = cfg.radius;
    t
}

/// Read a `--name: value;` declaration from the theme CSS.
pub fn css_var(css: &str, name: &str) -> Option<String> {
    let needle = format!("--{name}");
    let mut from = 0;
    while let Some(at) = css[from..].find(&needle) {
        let at = from + at;
        let after = &css[at + needle.len()..];
        let boundary = after
            .chars()
            .next()
            .map(|c| !(c.is_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(true);
        if !boundary {
            from = at + needle.len();
            continue;
        }
        let rest = after.trim_start();
        let Some(rest) = rest.strip_prefix(':') else {
            from = at + needle.len();
            continue;
        };
        let value = rest
            .split([';', '}', '\n'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        return Some(value);
    }
    None
}

pub fn css_number(css: &str, name: &str) -> Option<f32> {
    let raw = css_var(css, name)?;
    let raw = raw.trim().trim_end_matches("px").trim_end_matches('%').trim();
    raw.parse().ok()
}

/// Set (or append) a variable in the CSS text. Used by the colour pickers.
pub fn css_set(css: &mut String, name: &str, value: &str) {
    let needle = format!("--{name}:");
    if let Some(at) = css.find(&needle) {
        let start = at + needle.len();
        let end = css[start..]
            .find([';', '}', '\n'])
            .map(|i| start + i)
            .unwrap_or(css.len());
        css.replace_range(start..end, &format!(" {value}"));
        return;
    }
    let block = format!("  --{name}: {value};\n");
    if let Some(close) = css.rfind('}') {
        css.insert_str(close, &block);
    } else {
        css.push_str(&format!(":root {{\n{block}}}\n"));
    }
}

pub fn parse_color(raw: &str) -> Option<Color32> {
    let value = raw.trim().trim_end_matches(';').trim();
    if let Some(hex) = value.strip_prefix('#') {
        let byte = |s: &str| u8::from_str_radix(s, 16).ok();
        return match hex.len() {
            3 => {
                let r = byte(&hex[0..1])? * 17;
                let g = byte(&hex[1..2])? * 17;
                let b = byte(&hex[2..3])? * 17;
                Some(Color32::from_rgb(r, g, b))
            }
            4 => {
                let r = byte(&hex[0..1])? * 17;
                let g = byte(&hex[1..2])? * 17;
                let b = byte(&hex[2..3])? * 17;
                let a = byte(&hex[3..4])? * 17;
                Some(Color32::from_rgba_unmultiplied(r, g, b, a))
            }
            6 => Some(Color32::from_rgb(
                byte(&hex[0..2])?,
                byte(&hex[2..4])?,
                byte(&hex[4..6])?,
            )),
            8 => Some(Color32::from_rgba_unmultiplied(
                byte(&hex[0..2])?,
                byte(&hex[2..4])?,
                byte(&hex[4..6])?,
                byte(&hex[6..8])?,
            )),
            _ => None,
        };
    }
    let inside = value.split_once('(')?.1.split(')').next()?;
    let mut parts = inside
        .split([',', '/', ' '])
        .filter(|p| !p.trim().is_empty())
        .map(|p| p.trim());
    let channel = |s: &str| -> Option<u8> {
        if let Some(p) = s.strip_suffix('%') {
            let v: f32 = p.parse().ok()?;
            Some((v.clamp(0.0, 100.0) * 2.55).round() as u8)
        } else {
            let v: f32 = s.parse().ok()?;
            Some(v.clamp(0.0, 255.0).round() as u8)
        }
    };
    let r = channel(parts.next()?)?;
    let g = channel(parts.next()?)?;
    let b = channel(parts.next()?)?;
    let a = match parts.next() {
        Some(s) => {
            if let Some(p) = s.strip_suffix('%') {
                let v: f32 = p.parse().ok()?;
                (v.clamp(0.0, 100.0) * 2.55).round() as u8
            } else {
                let v: f32 = s.parse().ok()?;
                (v.clamp(0.0, 1.0) * 255.0).round() as u8
            }
        }
        None => 255,
    };
    Some(Color32::from_rgba_unmultiplied(r, g, b, a))
}

impl Tokens {
    pub const DARK: Self = Self {
        dark: true,
        // The Grid palette, straight from its grid.css:
        // #10141b window, #121820 view, #18212c dialog, #263343 line,
        // #a6c8ff accent. Hard edges, no gradients.
        bg: rgb(0x10141b),
        bg2: rgb(0x121820),
        card: rgba(0x18212c, 240),
        well: rgb(0x18212c),
        ink: rgb(0xedf2f8),
        strong: rgb(0xffffff),
        muted: rgb(0xacb9ca),
        faint: rgb(0x8493a7),
        line: rgb(0x263343),
        line_strong: rgb(0x344458),
        accent: rgb(0xa6c8ff),
        accent_soft: rgba(0x9cc3ff, 30),
        good: rgb(0xa6c8ff),
        warn: rgb(0xd7c08a),
        bad: rgb(0xf0a0a0),
        shadow: Color32::from_black_alpha(110),
        radius: 0.0,
    };
    pub const LIGHT: Self = Self {
        dark: false,
        bg: rgb(0xeef1f6),
        bg2: rgb(0xe7ebf2),
        card: rgb(0xffffff),
        well: rgb(0xf5f7fa),
        ink: rgb(0x10141b),
        strong: rgb(0x000000),
        muted: rgb(0x52627a),
        faint: rgb(0x6c7c94),
        line: rgb(0xd3dae5),
        line_strong: rgb(0xb6c2d2),
        accent: rgb(0x3b6ea5),
        accent_soft: rgba(0x3b6ea5, 26),
        good: rgb(0x2f7d5c),
        warn: rgb(0x9a6400),
        bad: rgb(0xb03a3a),
        shadow: Color32::from_black_alpha(30),
        radius: 0.0,
    };

    pub fn get(ctx: &egui::Context) -> Self {
        let dark = ctx.global_style().visuals.dark_mode;
        let cached = if dark {
            DARK_CACHE.lock().ok().and_then(|s| *s)
        } else {
            LIGHT_CACHE.lock().ok().and_then(|s| *s)
        };
        cached.unwrap_or(if dark { Self::DARK } else { Self::LIGHT })
    }

    /// Every corner in the app goes through here, so one slider reshapes it all.
    pub fn r(&self, fallback: f32) -> f32 {
        if self.radius >= 0.0 {
            self.radius
        } else {
            fallback
        }
    }

    pub fn corner(&self, fallback: u8) -> CornerRadius {
        CornerRadius::same(self.r(fallback as f32).clamp(0.0, 40.0) as u8)
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
    v.window_fill = t.bg2;
    v.extreme_bg_color = t.well;
    v.faint_bg_color = t.well;
    v.code_bg_color = t.well;
    v.window_stroke = Stroke::new(1.0, t.line_strong);
    v.window_corner_radius = t.corner(0);
    v.menu_corner_radius = t.corner(0);
    v.override_text_color = Some(t.ink);
    v.hyperlink_color = t.accent;
    v.selection.bg_fill = t.accent;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.warn_fg_color = t.warn;
    v.error_fg_color = t.bad;
    for (w, fill, stroke, fg) in [
        (&mut v.widgets.noninteractive, Color32::TRANSPARENT, t.line, t.muted),
        (&mut v.widgets.inactive, t.card, t.line, t.ink),
        (&mut v.widgets.hovered, t.well, t.line_strong, t.strong),
        (&mut v.widgets.active, t.well, t.line_strong, t.strong),
        (&mut v.widgets.open, t.well, t.line_strong, t.strong),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
        w.corner_radius = t.corner(0);
        w.fg_stroke = Stroke::new(1.0, fg);
        w.expansion = 0.0;
    }
    v
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    let d = DARK_CACHE
        .lock()
        .ok()
        .and_then(|s| *s)
        .unwrap_or(Tokens::DARK);
    let l = LIGHT_CACHE
        .lock()
        .ok()
        .and_then(|s| *s)
        .unwrap_or(Tokens::LIGHT);
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&d));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&l));
    let st = style();
    let density = st.density.clamp(0.65, 1.9) * if st.compact { 0.82 } else { 1.0 };
    let fs = st.font_scale.clamp(0.75, 1.8);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = Vec2::new(12.0, 10.0) * density;
        s.spacing.button_padding = Vec2::new(18.0, 10.0) * density;
        s.spacing.interact_size.y = 42.0 * density;
        s.spacing.slider_width = 180.0;
        s.text_styles = [
            (TextStyle::Heading, FontId::new(30.0 * fs, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(16.0 * fs, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(15.0 * fs, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(13.0 * fs, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.5 * fs, FontFamily::Monospace)),
        ]
        .into();
    });
}

/// Material You symbol codepoints. Outlined, monochrome, one shape per idea.
pub mod icon {
    pub const HOME: &str = "\u{e9b2}";
    pub const COMPUTERS: &str = "\u{e326}";
    pub const GAMEPAD: &str = "\u{ea28}";
    pub const SCREEN: &str = "\u{ef5b}";
    pub const NOTIFICATIONS: &str = "\u{e7f5}";
    pub const HOME_ASSISTANT: &str = "\u{e283}";
    pub const BINDS: &str = "\u{e312}";
    pub const LOOK: &str = "\u{e40a}";
    pub const SETTINGS: &str = "\u{e8b8}";
    pub const ADD: &str = "\u{e145}";
    pub const CLOSE: &str = "\u{e5cd}";
    pub const LOCK: &str = "\u{e899}";
    pub const OPEN: &str = "\u{e89e}";
    pub const BACK: &str = "\u{e5cb}";
    pub const FORWARD: &str = "\u{e5cc}";
    pub const PLAY_PAUSE: &str = "\u{f137}";
    pub const NEXT: &str = "\u{e044}";
    pub const PREV: &str = "\u{e045}";
    pub const STOP: &str = "\u{e047}";
    pub const BACKSPACE: &str = "\u{e14a}";
    pub const SHIFT: &str = "\u{e5f2}";
    pub const RETURN: &str = "\u{e31b}";
    pub const MENU: &str = "\u{e5d2}";
    pub const VIEW: &str = "\u{e8e9}";
}

/// The window / taskbar icon, decoded once from the packaged PNG.
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

pub fn icon_family() -> FontFamily {
    FontFamily::Name("material".into())
}

/// One Material symbol, sized and coloured like body text.
pub fn icon(text: &str, size: f32, color: Color32) -> RichText {
    RichText::new(text).family(icon_family()).size(size).color(color)
}

/// Material symbols live in the private use area; everything else is text.
pub fn glyph_family(text: &str) -> FontFamily {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if (0xe000..=0xf8ff).contains(&(c as u32)) => icon_family(),
        _ => FontFamily::Proportional,
    }
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

/// Linear blend between two colours; `k` = 0 keeps `a`, 1 keeps `b`.
pub fn mix(a: Color32, b: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    Color32::from_rgba_premultiplied(
        lerp(a.r(), b.r()),
        lerp(a.g(), b.g()),
        lerp(a.b(), b.b()),
        lerp(a.a(), b.a()),
    )
}

/// The grid grid backdrop: a faint square lattice with two soft ambient glows.
/// Painted on the panel before the content, so every page shares it.
pub fn backdrop(ui: &Ui, t: &Tokens) {
    let rect = ui.max_rect();
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    let st = style();
    if !st.grid {
        return;
    }
    let p = ui.painter();
    // Grid's grid: 32 px cells, 1 px lines of rgba(128,158,197,0.09).
    let step = st.grid_step.clamp(8.0, 160.0);
    let alpha = st.grid_alpha.clamp(0.0, 60.0).round() as u8;
    let line = if t.dark {
        Color32::from_rgba_unmultiplied(128, 158, 197, alpha)
    } else {
        Color32::from_rgba_unmultiplied(52, 68, 88, alpha)
    };
    let stroke = Stroke::new(1.0, line);
    let mut x = rect.left();
    while x < rect.right() {
        p.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            stroke,
        );
        x += step;
    }
    let mut y = rect.top();
    while y < rect.bottom() {
        p.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            stroke,
        );
        y += step;
    }
}

/// A floating page arrow. Big enough for a thumb, quiet enough to disappear.
pub fn nav_arrow(ui: &mut Ui, t: &Tokens, glyph: &str, enabled: bool) -> Response {
    let size = Vec2::new(54.0, 84.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_responsive(resp.id.with("hover"), resp.hovered());
    let fill = if t.dark {
        Color32::from_rgba_premultiplied(0x18, 0x18, 0x24, 200)
    } else {
        Color32::from_rgba_premultiplied(0xff, 0xff, 0xff, 220)
    };
    let p = ui.painter();
    p.rect_filled(rect, t.r(0.0), fill);
    p.rect_stroke(
        rect,
        0.0,
        Stroke::new(
            1.0,
            t.line_strong.gamma_multiply(0.45 + hover * 0.5),
        ),
        egui::StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::new(34.0, FontFamily::Proportional),
        if !enabled {
            t.faint
        } else if hover > 0.5 {
            t.strong
        } else {
            t.muted
        },
    );
    resp
}

pub fn hud(text: &str, color: Color32) -> RichText {
    RichText::new(text.to_lowercase())
        .family(FontFamily::Monospace)
        .size(11.0)
        .extra_letter_spacing(1.3)
        .color(color)
}

pub fn card<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(t.card)
        .stroke(t.hairline())
        .corner_radius(t.corner(0))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn primary(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).size(15.5).strong().color(Color32::WHITE))
            .fill(t.accent)
            .stroke(Stroke::new(1.0, t.accent))
            .corner_radius(t.corner(0))
            .min_size(Vec2::new(0.0, 44.0)),
    )
}

pub fn ghost(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).size(15.0).color(t.ink))
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.line_strong))
            .corner_radius(t.corner(0))
            .min_size(Vec2::new(0.0, 44.0)),
    )
}

pub fn chip(ui: &mut Ui, text: &str, color: Color32) -> Response {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .corner_radius(0.0)
        .inner_margin(Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).strong().color(color))
        })
        .response
}

pub fn dot(ui: &mut Ui, color: Color32, filled: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::hover());
    let r = egui::Rect::from_center_size(rect.center(), Vec2::splat(6.0));
    if filled {
        ui.painter().rect_filled(r, 0.0, color);
    } else {
        ui.painter()
            .rect_stroke(r, 0.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
    }
}

pub fn switch(ui: &mut Ui, on: &mut bool, t: &Tokens) -> Response {
    let size = Vec2::new(52.0, 28.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let k = ui.ctx().animate_bool_responsive(resp.id, *on);
    let p = ui.painter();
    p.rect_filled(rect, t.r(0.0), if *on { t.accent } else { t.well });
    p.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, if *on { t.accent } else { t.line_strong }),
        egui::StrokeKind::Inside,
    );
    let x = egui::lerp((rect.left() + 4.0)..=(rect.right() - 24.0), k);
    p.rect_filled(
        egui::Rect::from_min_size(egui::pos2(x, rect.top() + 4.0), Vec2::new(20.0, 20.0)),
        t.r(0.0),
        Color32::WHITE,
    );
    resp
}

pub fn row(ui: &mut Ui, t: &Tokens, title: &str, hint: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() - 120.0).max(200.0));
            ui.label(RichText::new(title).size(15.5).color(t.strong));
            if !hint.is_empty() {
                ui.add(egui::Label::new(RichText::new(hint).size(13.0).color(t.muted)).wrap());
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

pub fn small(ui: &mut Ui, text: &str, color: Color32) {
    ui.add(egui::Label::new(RichText::new(text).size(13.0).color(color)).wrap());
}

pub fn mono(ui: &mut Ui, text: &str, color: Color32) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .family(FontFamily::Monospace)
                .size(13.0)
                .color(color),
        )
        .wrap(),
    );
}

pub fn separator(ui: &mut Ui, t: &Tokens) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, t.r(0.0), t.line);
    ui.add_space(6.0);
}

/// Big touch tile with a soft gradient and a hover lift.
pub struct Tile {
    pub title: String,
    pub subtitle: String,
    pub glyph: String,
    pub accent: Color32,
    pub badge: Option<String>,
    pub badge_color: Color32,
    pub dim: bool,
    /// 0..1 entrance progress; 1 is the resting state.
    pub reveal: f32,
}

pub fn tile(ui: &mut Ui, t: &Tokens, tile: &Tile, size: Vec2) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_responsive(resp.id.with("hover"), resp.hovered());
    let press = ui
        .ctx()
        .animate_bool_responsive(resp.id.with("press"), resp.is_pointer_button_down_on());
    let lift = hover * 2.0 - press * 2.0;
    let reveal = tile.reveal.clamp(0.0, 1.0);
    let grow = 0.965 + 0.035 * reveal;
    let sized = egui::Rect::from_center_size(rect.center(), rect.size() * grow);
    let rect = sized.translate(Vec2::new(0.0, -lift + (1.0 - reveal) * 14.0));
    let p = ui.painter();
    let accent = if tile.dim { t.faint } else { t.accent };

    // Flat Grid panel: hard edges, a card fill and a hairline border.
    p.rect_filled(rect, t.r(0.0), t.card);
    p.rect_stroke(
        rect,
        0.0,
        Stroke::new(
            if hover > 0.5 { 1.0 } else { 1.0 },
            if hover > 0.5 {
                t.accent
            } else {
                t.line
            },
        ),
        egui::StrokeKind::Inside,
    );
    // The 2 px accent cap is what makes a grid cell read as selected.
    if !tile.dim || hover > 0.5 {
        p.rect_filled(
            egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 2.0)),
            t.r(0.0),
            accent,
        );
    }

    let text = if tile.dim { t.muted } else { t.strong };
    let st = style();
    if st.glyphs {
        p.text(
            egui::pos2(rect.left() + 18.0, rect.top() + 15.0),
            egui::Align2::LEFT_TOP,
            &tile.glyph,
            FontId::new(20.0, glyph_family(&tile.glyph)),
            accent,
        );
    }
    p.text(
        egui::pos2(rect.left() + 18.0, rect.bottom() - 54.0),
        egui::Align2::LEFT_BOTTOM,
        &tile.title,
        FontId::new(19.0, FontFamily::Proportional),
        text,
    );
    p.text(
        egui::pos2(rect.left() + 18.0, rect.bottom() - 20.0),
        egui::Align2::LEFT_BOTTOM,
        &tile.subtitle,
        FontId::new(12.0, FontFamily::Proportional),
        t.muted,
    );
    if let Some(badge) = &tile.badge {
        let color = if tile.badge_color == t.faint {
            t.faint
        } else {
            t.accent
        };
        let galley = p.layout_no_wrap(
            badge.clone(),
            FontId::new(10.5, FontFamily::Monospace),
            color,
        );
        let br = egui::Rect::from_min_size(
            egui::pos2(rect.right() - galley.size().x - 18.0, rect.top() + 18.0),
            galley.size(),
        );
        p.galley(br.min, galley, color);
    }
    resp
}

pub fn key_button(ui: &mut Ui, label: &str, t: &Tokens, width: f32) -> Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .family(glyph_family(label))
                .size(17.0)
                .color(t.strong),
        )
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.line))
            .corner_radius(t.corner(0))
            .min_size(Vec2::new(width, 42.0)),
    )
}

pub fn clock_text() -> String {
    let now = chrono_lite();
    now
}

fn chrono_lite() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // local time without a chrono dependency: ask the C library through `date`
    let out = std::process::Command::new("date")
        .arg("+%H:%M")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    out.unwrap_or_else(|| {
        let h = (secs / 3600 % 24) as u32;
        format!("{:02}:{:02}", h, secs / 60 % 60)
    })
}
