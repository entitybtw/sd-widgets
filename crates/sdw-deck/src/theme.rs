//! grid-inspired look: big rounded tiles, soft gradients, one accent per tile.
//! the deck is touched, so everything is at least 40 points tall.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Response,
    RichText, Sense, Stroke, TextStyle, Ui, Vec2, Visuals,
};
use sdw_core::model::{
    ThemeCfg, PRESET_CATPPUCCIN_FRAPPE, PRESET_CATPPUCCIN_LATTE, PRESET_CATPPUCCIN_MACCHIATO,
    PRESET_CATPPUCCIN_MOCHA, PRESET_GRID, PRESET_GRUVBOX, PRESET_MONO, PRESET_NORD,
    PRESET_TERMINAL,
};
use std::path::PathBuf;
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

pub const PRESETS: [(&str, &str); 9] = [
    ("grid", "Grid"),
    ("mono", "Monochrome"),
    ("nord", "Nord"),
    ("gruvbox", "Gruvbox"),
    ("terminal", "Terminal"),
    ("mocha", "Catppuccin Mocha"),
    ("macchiato", "Catppuccin Macchiato"),
    ("frappe", "Catppuccin Frappé"),
    ("latte", "Catppuccin Latte"),
];

pub fn preset_css(name: &str) -> &'static str {
    match name {
        "mono" => PRESET_MONO,
        "nord" => PRESET_NORD,
        "gruvbox" => PRESET_GRUVBOX,
        "terminal" => PRESET_TERMINAL,
        "mocha" => PRESET_CATPPUCCIN_MOCHA,
        "macchiato" => PRESET_CATPPUCCIN_MACCHIATO,
        "frappe" => PRESET_CATPPUCCIN_FRAPPE,
        "latte" => PRESET_CATPPUCCIN_LATTE,
        _ => PRESET_GRID,
    }
}

/// Where user theme packs live: `~/.config/sd-widgets/themes/*.css`.
pub fn custom_dir() -> PathBuf {
    sdw_core::util::config_dir().join("themes")
}

/// Every `*.css` in the themes folder, as `(key, css)` pairs. The key is
/// `custom:<file stem>` so it never collides with a built-in preset.
pub fn custom_presets() -> Vec<(String, String)> {
    let dir = custom_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("css") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Ok(css) = std::fs::read_to_string(&path) {
            out.push((format!("custom:{stem}"), css));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub fn all_presets() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = PRESETS
        .iter()
        .map(|(key, name)| (key.to_string(), name.to_string()))
        .collect();
    for (key, _) in custom_presets() {
        let name = key.strip_prefix("custom:").unwrap_or(&key).to_string();
        out.push((key, name));
    }
    // Enabled addon themes apply exactly like a preset.
    for addon in sdw_core::addons::list() {
        if addon.manifest.deck.theme.trim().is_empty() {
            continue;
        }
        out.push((format!("addon:{}", addon.id()), addon.manifest.name.clone()));
    }
    out
}

/// Save the current CSS as a named user pack. Returns the written path.
pub fn save_custom_pack(name: &str, css: &str) -> std::io::Result<PathBuf> {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let stem = stem.trim_matches('-').to_lowercase();
    let stem = if stem.is_empty() {
        "my-theme".to_string()
    } else {
        stem
    };
    let dir = custom_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{stem}.css"));
    std::fs::write(&path, css)?;
    Ok(path)
}

/// Read the CSS for any preset key (built-in or `custom:name`).
pub fn preset_css_owned(name: &str) -> Option<String> {
    if let Some(id) = name.strip_prefix("addon:") {
        return sdw_core::addons::find(id).and_then(|a| a.theme_css());
    }
    if let Some(stem) = name.strip_prefix("custom:") {
        return std::fs::read_to_string(custom_dir().join(format!("{stem}.css"))).ok();
    }
    if PRESETS.iter().any(|(key, _)| *key == name) {
        return Some(preset_css(name).to_string());
    }
    None
}

/// Switch the whole look to a preset. Layout knobs stay as they are.
pub fn apply_preset(cfg: &mut ThemeCfg, name: &str) {
    if let Some(css) = preset_css_owned(name) {
        cfg.css = css;
    }
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
    let pick =
        |name: &str| css_var(&cfg.css, &format!("{prefix}{name}")).and_then(|v| parse_color(&v));
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
    let raw = raw
        .trim()
        .trim_end_matches("px")
        .trim_end_matches('%')
        .trim();
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
        (
            &mut v.widgets.noninteractive,
            Color32::TRANSPARENT,
            t.line,
            t.muted,
        ),
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
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&d));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&l));
    let st = style();
    let density = st.density.clamp(0.65, 1.9) * if st.compact { 0.82 } else { 1.0 };
    let fs = st.font_scale.clamp(0.75, 1.8);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = Vec2::new(12.0, 10.0) * density;
        s.spacing.button_padding = Vec2::new(18.0, 10.0) * density;
        s.spacing.interact_size.y = 42.0 * density;
        // A sensible default for a slider that shares its row; the full-width
        // ones set `slider_len` themselves.
        s.spacing.slider_width = 220.0;
        s.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(30.0 * fs, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(16.0 * fs, FontFamily::Proportional),
            ),
            (
                TextStyle::Button,
                FontId::new(15.0 * fs, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(13.0 * fs, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(13.5 * fs, FontFamily::Monospace),
            ),
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
    pub const LIGHT: &str = "\u{e518}";
    pub const DARK: &str = "\u{e51c}";
    pub const CHECK: &str = "\u{e668}";
    pub const STAR: &str = "\u{f09a}";
    pub const ARROW_UP: &str = "\u{e5d8}";
    pub const ARROW_DOWN: &str = "\u{e5db}";
    pub const ARROW_LEFT: &str = "\u{e5c4}";
    pub const ARROW_RIGHT: &str = "\u{e5c8}";
    pub const BULB: &str = "\u{e90f}";
    pub const POWER: &str = "\u{f8c7}";
    pub const SENSORS: &str = "\u{e51e}";
    pub const THERMOSTAT: &str = "\u{f076}";
    pub const PLAY: &str = "\u{e037}";
    pub const BLINDS: &str = "\u{e286}";
    pub const SCENE: &str = "\u{e65f}";
    pub const CODE: &str = "\u{e86f}";
    pub const CAMERA: &str = "\u{e412}";
    pub const PERSON: &str = "\u{f0d3}";
    pub const WIDGETS: &str = "\u{e1bd}";
    pub const MUSIC: &str = "\u{e405}";
    pub const ALBUM: &str = "\u{e019}";
    pub const PLAYLIST: &str = "\u{e03d}";
    pub const VOLUME_UP: &str = "\u{e050}";
    pub const VOLUME_DOWN: &str = "\u{e04d}";
    pub const VOLUME_OFF: &str = "\u{e04f}";
    pub const SPEAKER: &str = "\u{e32d}";
    pub const SHUFFLE: &str = "\u{e043}";
    pub const REPEAT: &str = "\u{e040}";
    pub const DRAG: &str = "\u{e25d}";
    pub const CLOCK: &str = "\u{e8b5}";
    pub const TIMER: &str = "\u{e425}";
    pub const BATTERY: &str = "\u{e1a5}";
    pub const MOON: &str = "\u{ef5e}";
    pub const SUN: &str = "\u{e81a}";
    pub const TUNE: &str = "\u{e429}";
    pub const FULLSCREEN: &str = "\u{e5d0}";
    pub const HEADPHONES: &str = "\u{e310}";
    pub const ADDON: &str = "\u{e87b}";
    pub const STORE: &str = "\u{e8d1}";
    pub const RESTART_ALT: &str = "\u{f053}";
    pub const BEDTIME: &str = "\u{ef44}";
    pub const TERMINAL: &str = "\u{eb8e}";
    pub const BLUETOOTH: &str = "\u{e1a7}";
    pub const KEYBOARD: &str = "\u{e312}";
    pub const MOUSE: &str = "\u{e323}";
    pub const SIGNAL: &str = "\u{e202}";
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
    RichText::new(text)
        .family(icon_family())
        .size(size)
        .color(color)
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
    // Mirrored desktop notifications can carry any emoji. egui's bundled
    // fallback only covers a small subset, so keep a full monochrome set
    // behind the interface fonts.
    fonts.font_data.insert(
        "noto-emoji".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/NotoEmoji.ttf"
        )))),
    );
    // A mirrored notification can carry any glyph: arrows, ticks, currency
    // signs, CJK. The two shipped faces cover Latin, Cyrillic and emoji, so a
    // system face is added behind them when one is installed. Nothing is
    // required: without it the interface is exactly what it was.
    let fallbacks = system_fonts(&mut fonts);
    if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
        f.insert(0, "archivo".to_owned());
        f.insert(1, "noto-emoji".to_owned());
        for name in &fallbacks {
            f.push(name.clone());
        }
    }
    if let Some(f) = fonts.families.get_mut(&FontFamily::Monospace) {
        f.insert(0, "jetbrains".to_owned());
        f.insert(1, "noto-emoji".to_owned());
        for name in &fallbacks {
            f.push(name.clone());
        }
    }
    fonts.families.insert(
        FontFamily::Name("material".into()),
        vec!["material".to_owned(), "archivo".to_owned()],
    );
    ctx.set_fonts(fonts);
}

/// Read the wide system faces, if they are there, and register them.
///
/// A desktop notification is written by somebody else and can hold almost any
/// character, including ones the shipped subset fonts have no picture for.
/// One or two system files cover the rest on every normal Linux install; the
/// list stays short so a frame is still cheap to shape.
fn system_fonts(fonts: &mut FontDefinitions) -> Vec<String> {
    const CANDIDATES: [(&str, &[&str]); 2] = [
        (
            "system-sans",
            &[
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
                "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            ],
        ),
        (
            "system-mono",
            &[
                "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
                "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
                "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
            ],
        ),
    ];
    let mut names = Vec::new();
    for (name, paths) in CANDIDATES {
        let Some(bytes) = paths.iter().find_map(|p| std::fs::read(p).ok()) else {
            continue;
        };
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
        names.push(name.to_owned());
    }
    names
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

/// The grid backdrop: a faint square lattice with two soft ambient glows.
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
    // the grid: 32 px cells, 1 px lines of rgba(128,158,197,0.09).
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

/// A floating page arrow: one square, blue outline, blue glyph.
pub fn nav_arrow(ui: &mut Ui, t: &Tokens, glyph: &str, enabled: bool) -> Response {
    let side = style().arrow_size.clamp(28.0, 96.0);
    let size = Vec2::splat(side);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_responsive(resp.id.with("hover"), resp.hovered());
    let fill = if t.dark {
        Color32::from_rgba_premultiplied(0x10, 0x14, 0x1b, 220)
    } else {
        Color32::from_rgba_premultiplied(0xff, 0xff, 0xff, 235)
    };
    let p = ui.painter();
    p.rect_filled(rect, t.r(0.0), fill);
    let line = if enabled {
        t.accent.gamma_multiply(0.55 + hover * 0.45)
    } else {
        t.line_strong
    };
    p.rect_stroke(
        rect,
        0.0,
        Stroke::new(if hover > 0.5 { 1.6 } else { 1.0 }, line),
        egui::StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::new(side * 0.54, glyph_family(glyph)),
        if !enabled {
            t.faint
        } else {
            t.accent.gamma_multiply(0.85 + hover * 0.15)
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
    let r = egui::Frame::new()
        .fill(t.card)
        .stroke(t.hairline())
        .corner_radius(t.corner(0))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    r.inner
}

/// A row that ends at the card edge: the slider takes the given width, the
/// trailing group is a child laid out right-to-left, and the row grows only to
/// that child's right edge.
///
/// A plain `Slider` inside `ui.horizontal` reports itself as a layout row, and
/// egui then charges item spacing twice; the extra dozen pixels land outside
/// the card and widen it. Adding the trailing label as a child keeps the
/// spacing at the right edge, where it fits.
pub fn slider_row(
    ui: &mut Ui,
    _width: f32,
    height: f32,
    add_slider: impl FnOnce(&mut Ui),
    add_right: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().interact_size.y = height;
        add_slider(ui);
        let mut child = ui.new_child(
            egui::UiBuilder::new().layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        add_right(&mut child);
        let rect = child.min_rect();
        ui.expand_to_include_rect(rect);
    });
}

/// A row with a group on the left and a group pushed to the right edge.
///
/// A plain `with_layout(right_to_left)` inside a row leaves egui charging item
/// spacing after the right group; that trailing bit lands outside the card and
/// widens it by a dozen pixels. The right group is therefore added as a child
/// placed by hand: it reports its rect without moving the row cursor.
pub fn row_ends<R>(ui: &mut Ui, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        left(ui);
        let mut child = ui.new_child(
            egui::UiBuilder::new().layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        let inner = right(&mut child);
        let rect = child.min_rect();
        ui.expand_to_include_rect(rect);
        inner
    })
    .inner
}

pub fn primary(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    ui.add(
        egui::Button::new(
            RichText::new(text)
                .size(15.5)
                .strong()
                .color(Color32::WHITE),
        )
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

/// What a running download says about itself, read on the frame that draws it.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProgressView {
    pub done: u64,
    /// 0 when the size is unknown: no honest percentage then.
    pub total: u64,
    /// Bytes per second, 0 until two readings are far enough apart.
    pub rate: f64,
    /// Seconds left, `None` while either number is unknown.
    pub eta: Option<f64>,
}

impl ProgressView {
    /// Fold in the newest reading of the byte counter.
    ///
    /// The speed is measured over the reading span rather than per frame: a
    /// download reports in bursts of 64 KB, and a per-frame rate would jump
    /// between 0 and hundreds of MB/s.
    pub fn sample(
        &mut self,
        done: u64,
        total: u64,
        at: std::time::Instant,
        last: &mut Option<(u64, std::time::Instant)>,
    ) {
        self.done = done;
        self.total = total;
        let Some((was, when)) = *last else {
            *last = Some((done, at));
            return;
        };
        let secs = at.duration_since(when).as_secs_f64();
        if done < was {
            // A new download started: forget the old curve.
            self.rate = 0.0;
            self.eta = None;
            *last = Some((done, at));
            return;
        }
        if secs < 0.4 {
            return;
        }
        let step = (done - was) as f64 / secs;
        // Smoothed so the number does not flicker; the first span counts as is.
        self.rate = if self.rate > 0.0 {
            self.rate * 0.6 + step * 0.4
        } else {
            step
        };
        self.eta = (total > done && self.rate > 1024.0).then(|| (total - done) as f64 / self.rate);
        *last = Some((done, at));
    }
}

/// A hairline progress bar in the Grid language: hard edges, a well, a filled
/// accent run on top and a label that always says the real state.
///
/// `total == 0` means the server did not say the size (no `Content-Length`),
/// so there is no honest percentage: a short runner crosses the well instead,
/// and the label counts what arrived. A full bar is drawn as full.
pub fn progress(ui: &mut Ui, t: &Tokens, done: u64, total: u64, label: &str) -> Response {
    let width = ui.available_width().clamp(140.0, 460.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 10.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, t.r(0.0), t.well);
    if total == 0 {
        // Indeterminate: the runner is timed by the frame clock, so it moves
        // without keeping any extra state.
        let phase = (ui.input(|i| i.time) * 0.55).fract() as f32;
        let run = rect.width() * 0.28;
        let x = egui::lerp((rect.left() - run)..=(rect.right()), phase);
        let left = x.max(rect.left());
        let bar = egui::Rect::from_min_size(
            egui::pos2(left, rect.top()),
            Vec2::new(run.min(rect.right() - left), rect.height()),
        );
        p.rect_filled(bar, t.r(0.0), t.accent.gamma_multiply(0.7));
        ui.ctx().request_repaint();
    } else {
        let k = (done as f64 / total as f64).clamp(0.0, 1.0) as f32;
        let bar = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width() * k, rect.height()));
        p.rect_filled(bar, t.r(0.0), t.accent);
    }
    p.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, t.line_strong),
        egui::StrokeKind::Inside,
    );
    if !label.is_empty() {
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                RichText::new(label)
                    .family(FontFamily::Monospace)
                    .size(10.5)
                    .color(t.muted),
            )
            .wrap(),
        );
    }
    resp
}

/// The line under a download bar: `34% · 12.4 MB из 36.8 MB · 8.2 MB/s · 1:20`,
/// and only the parts that are known.
pub fn progress_label(lang: sdw_core::i18n::Lang, v: &ProgressView) -> String {
    let bytes = |n: u64| sdw_core::util::human_bytes(n);
    let mut out = String::new();
    if v.total > 0 {
        let done = v.done.min(v.total);
        out.push_str(&format!("{}% · ", done * 100 / v.total.max(1)));
        out.push_str(&format!(
            "{} {} {}",
            bytes(done),
            sdw_core::t!(lang, "из", "of"),
            bytes(v.total)
        ));
    } else {
        out.push_str(&bytes(v.done));
    }
    if v.rate >= 1024.0 {
        out.push_str(&format!(" · {}/s", bytes(v.rate as u64)));
    }
    if let Some(eta) = v.eta {
        out.push_str(&format!(
            " · {} {}",
            sdw_core::t!(lang, "ещё", "about"),
            format_eta(eta)
        ));
    }
    out
}

/// Seconds left as `0:07` / `1:20`, so a long download does not read `80 с`.
fn format_eta(secs: f64) -> String {
    let s = secs.ceil().max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Big touch tile with a soft gradient and a hover lift.
#[derive(Clone)]
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

    // Flat grid panel: hard edges, a card fill and a hairline border.
    p.rect_filled(rect, t.r(0.0), t.card);
    p.rect_stroke(
        rect,
        0.0,
        Stroke::new(
            if hover > 0.5 { 1.0 } else { 1.0 },
            if hover > 0.5 { t.accent } else { t.line },
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
    let now = local_now();
    format!("{:02}:{:02}", now.hour, now.minute)
}

/// Broken-down local time, straight from libc: the clock page needs seconds,
/// the weekday and the date, and spawning `date` every frame is wasteful.
#[derive(Clone, Copy, Debug, Default)]
pub struct NowParts {
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 = Sunday … 6 = Saturday
    pub weekday: u32,
    pub day: u32,
    pub month: u32,
    pub year: i32,
}

pub fn local_now() -> NowParts {
    #[cfg(unix)]
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return NowParts::default();
        }
        NowParts {
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
            weekday: tm.tm_wday as u32,
            day: tm.tm_mday as u32,
            month: tm.tm_mon as u32 + 1,
            year: tm.tm_year + 1900,
        }
    }
    #[cfg(not(unix))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        NowParts {
            hour: (secs / 3600 % 24) as u32,
            minute: (secs / 60 % 60) as u32,
            second: (secs % 60) as u32,
            weekday: (secs / 86400 + 4) % 7,
            day: 1,
            month: 1,
            year: 1970,
        }
    }
}

// ---------------------------------------------------------------- controls

/// A Grid slider laid out as a settings row: the name on the left, a square
/// track in the middle with hairline ticks, the value on the right. It is sized
/// for a finger and works with the virtual cursor.
pub fn slider(
    ui: &mut Ui,
    t: &Tokens,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    fmt: impl Fn(f32) -> String,
) -> bool {
    slider_ex(ui, t, label, value, range, fmt).0
}

/// egui's own `Slider` takes its length from `spacing.slider_width`, not from
/// `add_sized`, so a wide row would leave a stubby 180 px track in the middle.
/// Set the spacing before adding one.
pub fn slider_len(ui: &mut Ui, width: f32) {
    ui.spacing_mut().slider_width = width.max(90.0);
    // The rail is painted with `widgets.inactive.bg_fill`, which is the card
    // colour: on a card the unfilled part of the track disappears and a slider
    // looks cut off at its handle. The trough takes the hairline colour, so the
    // whole track reads as one bar with the accent fill on top of it.
    let t = Tokens::get(ui.ctx());
    ui.visuals_mut().widgets.inactive.bg_fill = t.line;
}

/// The slider with a second flag: `true` when the finger or mouse was let go,
/// which is the moment to send a value to a remote device.
pub fn slider_ex(
    ui: &mut Ui,
    t: &Tokens,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    fmt: impl Fn(f32) -> String,
) -> (bool, bool) {
    let width = ui.available_width();
    slider_ex_width(ui, t, width, label, value, range, fmt)
}

/// A slider that occupies a given width. Use it when another widget shares the
/// row: a full-width slider would widen the whole card.
pub fn slider_with_width(
    ui: &mut Ui,
    t: &Tokens,
    width: f32,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    fmt: impl Fn(f32) -> String,
) -> bool {
    slider_ex_width(ui, t, width, label, value, range, fmt).0
}

/// The smallest step that still gives the slider a comfortable number of
/// positions: a percent-scale slider moves by one, a 0…10 value by a tenth, a
/// speed or a UI scale by a hundredth. Nothing is ever rounded to a whole
/// number just because its range is short — that is what turned the UI scale
/// into 100 % / 200 % and left most of its range unreachable.
pub fn auto_step(lo: f32, hi: f32) -> f32 {
    let span = (hi - lo).abs();
    if span >= 20.0 {
        1.0
    } else if span >= 3.0 {
        0.1
    } else if span >= 0.3 {
        0.01
    } else {
        0.001
    }
}

/// A slider with an explicit value step: `1.0` for whole numbers, `0.1` for a
/// tenth, and so on. Use it when the stored value must stay on a grid.
pub fn slider_step(
    ui: &mut Ui,
    t: &Tokens,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    fmt: impl Fn(f32) -> String,
) -> bool {
    let width = ui.available_width();
    slider_ex_step_width(ui, t, width, label, value, range, step, fmt).0
}

/// [`slider_ex`] at an explicit width.
pub fn slider_ex_width(
    ui: &mut Ui,
    t: &Tokens,
    width: f32,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    fmt: impl Fn(f32) -> String,
) -> (bool, bool) {
    let step = auto_step(*range.start(), *range.end());
    slider_ex_step_width(ui, t, width, label, value, range, step, fmt)
}

/// [`slider_ex_width`] with the step chosen by the caller.
pub fn slider_ex_step_width(
    ui: &mut Ui,
    t: &Tokens,
    width: f32,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    fmt: impl Fn(f32) -> String,
) -> (bool, bool) {
    let (lo, hi) = (*range.start(), *range.end());
    let span = (hi - lo).max(f32::EPSILON);
    let step = step.abs().max(f32::EPSILON);
    let height = 40.0;
    let width = width.max(160.0);
    let (row, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let label_w = (row.width() * 0.34).clamp(64.0, 200.0);
    let value_w = 66.0;
    let track = egui::Rect::from_min_max(
        egui::pos2(row.left() + label_w, row.center().y - 5.0),
        egui::pos2(row.right() - value_w, row.center().y + 5.0),
    );
    let resp = ui.interact(
        egui::Rect::from_min_max(
            egui::pos2(track.left() - 6.0, row.top()),
            egui::pos2(row.right(), row.bottom()),
        ),
        ui.id().with(("slider", label)),
        Sense::click_and_drag(),
    );
    let mut changed = false;
    if (resp.dragged() || resp.clicked()) {
        if let Some(pos) = resp.interact_pointer_pos() {
            let nk = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
            let raw = lo + nk * span;
            // Snap to the slider's own step, counted from the low end, so the
            // value is exactly on the grid and shows the same number the user
            // sees. A hundredth of a step prevents float noise at the edges.
            let next = lo + ((raw - lo) / step).round() * step;
            let next = (next * 10000.0).round() / 10000.0;
            let next = next.clamp(lo, hi);
            if (next - *value).abs() > f32::EPSILON {
                *value = next;
                changed = true;
            }
        }
    }
    let k = ((*value - lo) / span).clamp(0.0, 1.0);
    let kx = track.left() + track.width() * k;
    let p = ui.painter();
    p.text(
        egui::pos2(row.left(), row.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::new(12.5, FontFamily::Proportional),
        t.muted,
    );
    p.rect_filled(track, t.r(0.0), t.well);
    p.rect_stroke(
        track,
        t.r(0.0),
        Stroke::new(
            1.0,
            if resp.hovered() || resp.dragged() {
                t.line_strong
            } else {
                t.line
            },
        ),
        egui::StrokeKind::Inside,
    );
    for step in 1..4 {
        let x = track.left() + track.width() * step as f32 / 4.0;
        p.line_segment(
            [
                egui::pos2(x, track.top() + 1.0),
                egui::pos2(x, track.bottom() - 1.0),
            ],
            Stroke::new(1.0, t.line),
        );
    }
    if kx > track.left() + 1.0 {
        p.rect_filled(
            egui::Rect::from_min_max(track.min, egui::pos2(kx, track.bottom())),
            t.r(0.0),
            t.accent.gamma_multiply(0.55),
        );
    }
    let knob = egui::Rect::from_center_size(egui::pos2(kx, row.center().y), Vec2::new(12.0, 26.0));
    p.rect_filled(knob, t.r(0.0), t.accent);
    p.rect_stroke(
        knob,
        t.r(0.0),
        Stroke::new(
            1.5,
            if resp.dragged() {
                t.strong
            } else {
                t.line_strong
            },
        ),
        egui::StrokeKind::Inside,
    );
    p.text(
        egui::pos2(row.right(), row.center().y),
        egui::Align2::RIGHT_CENTER,
        fmt(*value),
        FontId::new(12.5, FontFamily::Monospace),
        if resp.hovered() || resp.dragged() {
            t.strong
        } else {
            t.muted
        },
    );
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let released = resp.drag_stopped() || (changed && !resp.dragged());
    (changed, released)
}

/// The same slider for whole numbers: the step is exactly one, so a drag can
/// never park the value between two ticks.
pub fn slider_u32(
    ui: &mut Ui,
    t: &Tokens,
    label: &str,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
) -> bool {
    let mut current = *value as f32;
    let changed = slider_step(
        ui,
        t,
        label,
        &mut current,
        *range.start() as f32..=*range.end() as f32,
        1.0,
        |v| format!("{:.0}", v),
    );
    if changed {
        *value = current.round() as u32;
    }
    changed
}

/// A palette swatch with its name underneath. Clicking opens the picker right
/// under the chip, so a 15-token palette stays readable on a small screen.
pub fn color_chip(ui: &mut Ui, t: &Tokens, color: &mut Color32, label: &str) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width().min(96.0), 40.0),
            Sense::click(),
        );
        let p = ui.painter();
        p.rect_filled(rect, t.r(0.0), *color);
        p.rect_stroke(
            rect,
            t.r(0.0),
            Stroke::new(1.0, t.line_strong),
            egui::StrokeKind::Inside,
        );
        // A 2 px accent cap reads as "this chip is a control".
        p.rect_filled(
            egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), 2.0)),
            t.r(0.0),
            t.accent,
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        egui::Popup::from_toggle_button_response(&resp)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_max_width(240.0);
                if egui::color_picker::color_picker_color32(
                    ui,
                    color,
                    egui::color_picker::Alpha::Opaque,
                ) {
                    changed = true;
                }
            });
        ui.label(RichText::new(label).size(10.5).color(t.muted));
    });
    changed
}

/// A square icon button used across the pages: hairline border, no rounding.
pub fn icon_button(ui: &mut Ui, glyph: &str, t: &Tokens, size: f32, active: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let fill = if resp.hovered() {
        t.well
    } else if active {
        t.accent.gamma_multiply(0.22)
    } else {
        t.card
    };
    ui.painter().rect_filled(rect, t.r(0.0), fill);
    ui.painter().rect_stroke(
        rect,
        t.r(0.0),
        Stroke::new(1.0, if active { t.accent } else { t.line }),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::new(size * 0.5, glyph_family(glyph)),
        if active { t.accent } else { t.muted },
    );
    resp
}

#[cfg(test)]
mod progress_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn speed_and_eta_come_from_reading_spans() {
        let mut v = ProgressView::default();
        let mut last = None;
        let t0 = Instant::now();
        v.sample(0, 1_000_000, t0, &mut last);
        v.sample(100_000, 1_000_000, t0 + Duration::from_secs(1), &mut last);
        assert!(v.rate > 90_000.0 && v.rate < 110_000.0, "rate {}", v.rate);
        assert!(v.eta.unwrap() > 8.0 && v.eta.unwrap() < 10.0);
        // The label always carries the state a person can check.
        let line = progress_label(sdw_core::i18n::Lang::Ru, &v);
        assert!(line.starts_with("10% · "), "{line}");
        assert!(line.contains('/'), "{line}");
    }

    #[test]
    fn unknown_size_has_no_percentage_and_no_eta() {
        let mut v = ProgressView::default();
        let mut last = None;
        let t0 = Instant::now();
        v.sample(0, 0, t0, &mut last);
        v.sample(400_000, 0, t0 + Duration::from_secs(2), &mut last);
        assert!(v.eta.is_none());
        let line = progress_label(sdw_core::i18n::Lang::En, &v);
        assert!(!line.contains('%'), "{line}");
        assert!(line.starts_with("390.6 KB"), "{line}");
    }
}
