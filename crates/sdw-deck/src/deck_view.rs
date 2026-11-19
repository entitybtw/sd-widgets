//! a steam deck you can touch.
//!
//! the whole handheld is drawn from primitives: body, grips, screen, sticks, trackpads,
//! triggers and every button. pressed controls light up with a short animated glow, the
//! sticks follow the real sticks, and tapping any control selects it so it can be bound.

use crate::input::{self, BTN_LEFT_CLICK, BTN_RIGHT_CLICK, BTN_TOUCH};
use crate::theme::{Tokens, hud};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Response, RichText, Sense,
    Stroke, Ui, Vec2,
};
use sdw_core::model::{DeckViewCfg, GestureKind, SwipeDir, Trigger};
use sdw_core::udp::{self, GamepadState};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeckControl {
    A,
    B,
    X,
    Y,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    Lb,
    Rb,
    Lt,
    Rt,
    View,
    Menu,
    Steam,
    L3,
    R3,
    L4,
    L5,
    R4,
    R5,
    LStick,
    RStick,
    LTrackpad,
    RTrackpad,
    Screen,
}

impl DeckControl {
    pub const ALL: [DeckControl; 26] = [
        Self::A,
        Self::B,
        Self::X,
        Self::Y,
        Self::DpadUp,
        Self::DpadDown,
        Self::DpadLeft,
        Self::DpadRight,
        Self::Lb,
        Self::Rb,
        Self::Lt,
        Self::Rt,
        Self::View,
        Self::Menu,
        Self::Steam,
        Self::L3,
        Self::R3,
        Self::L4,
        Self::L5,
        Self::R4,
        Self::R5,
        Self::LStick,
        Self::RStick,
        Self::LTrackpad,
        Self::RTrackpad,
        Self::Screen,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
            Self::Y => "Y",
            Self::DpadUp => "d-pad up",
            Self::DpadDown => "d-pad down",
            Self::DpadLeft => "d-pad left",
            Self::DpadRight => "d-pad right",
            Self::Lb => "LB",
            Self::Rb => "RB",
            Self::Lt => "LT",
            Self::Rt => "RT",
            Self::View => "View",
            Self::Menu => "Menu",
            Self::Steam => "Steam",
            Self::L3 => "L3",
            Self::R3 => "R3",
            Self::L4 => "L4",
            Self::L5 => "L5",
            Self::R4 => "R4",
            Self::R5 => "R5",
            Self::LStick => "left stick",
            Self::RStick => "right stick",
            Self::LTrackpad => "left trackpad",
            Self::RTrackpad => "right trackpad",
            Self::Screen => "touchscreen",
        }
    }

    /// Every control is a button at heart, so it can be bound like one.
    pub fn code(self) -> u16 {
        match self {
            Self::A => input::BTN_SOUTH,
            Self::B => input::BTN_EAST,
            Self::X => input::BTN_WEST,
            Self::Y => input::BTN_NORTH,
            Self::DpadUp => input::BTN_DPAD_UP,
            Self::DpadDown => input::BTN_DPAD_DOWN,
            Self::DpadLeft => input::BTN_DPAD_LEFT,
            Self::DpadRight => input::BTN_DPAD_RIGHT,
            Self::Lb => input::BTN_TL,
            Self::Rb => input::BTN_TR,
            Self::Lt => input::BTN_TL2,
            Self::Rt => input::BTN_TR2,
            Self::View => input::BTN_SELECT,
            Self::Menu => input::BTN_START,
            Self::Steam => input::BTN_MODE,
            Self::L3 | Self::LStick => input::BTN_THUMBL,
            Self::R3 | Self::RStick => input::BTN_THUMBR,
            Self::L4 => input::BTN_L4,
            Self::L5 => input::BTN_L5,
            Self::R4 => input::BTN_R4,
            Self::R5 => input::BTN_R5,
            Self::LTrackpad => BTN_LEFT_CLICK,
            Self::RTrackpad => BTN_RIGHT_CLICK,
            Self::Screen => BTN_TOUCH,
        }
    }

    pub fn trigger(self) -> Trigger {
        Trigger::Button {
            device: "steam deck".into(),
            code: format!("{}", self.code()),
            label: self.name().to_string(),
        }
    }
}

pub struct DeckInput<'a> {
    pub state: &'a GamepadState,
    pub pressed: &'a HashSet<u16>,
    pub host: &'a str,
    pub pad_label: &'a str,
    pub online: bool,
    pub enabled: bool,
    pub devices: usize,
}

pub struct DeckViewState {
    pub cfg: DeckViewCfg,
    pub selected: Option<DeckControl>,
}

impl DeckViewState {
    pub fn new(cfg: DeckViewCfg) -> Self {
        Self {
            cfg,
            selected: None,
        }
    }
}

const A_COLOR: Color32 = Color32::from_rgb(0x54, 0xd6, 0x8f);
const B_COLOR: Color32 = Color32::from_rgb(0xff, 0x76, 0x76);
const X_COLOR: Color32 = Color32::from_rgb(0x62, 0xa9, 0xff);
const Y_COLOR: Color32 = Color32::from_rgb(0xff, 0xc4, 0x5c);

pub fn show(
    ui: &mut Ui,
    t: &Tokens,
    view: &mut DeckViewState,
    input: DeckInput<'_>,
) -> Option<DeckControl> {
    let avail = ui.available_width();
    let width = (avail * view.cfg.scale.clamp(0.6, 1.4)).min(1120.0);
    let size = Vec2::new(width, width / 2.28);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter().clone();
    let w = rect.width();
    let h = rect.height();
    let at = |fx: f32, fy: f32| Pos2::new(rect.left() + w * fx, rect.top() + h * fy);
    let body = Rect::from_min_max(at(0.012, 0.135), at(0.988, 0.975));
    let body_fill = if t.dark {
        Color32::from_rgb(0x1a, 0x1a, 0x23)
    } else {
        Color32::from_rgb(0xdb, 0xd9, 0xe4)
    };
    let body_line = if t.dark {
        Color32::from_rgb(0x33, 0x33, 0x41)
    } else {
        Color32::from_rgb(0xb6, 0xb3, 0xc4)
    };
    let control_fill = if t.dark {
        Color32::from_rgb(0x2a, 0x2a, 0x36)
    } else {
        Color32::from_rgb(0xf1, 0xf0, 0xf6)
    };

    // shadow
    p.rect_filled(
        body.translate(Vec2::new(0.0, h * 0.03)),
        CornerRadius::ZERO,
        Color32::from_black_alpha(if t.dark { 120 } else { 36 }),
    );
    // grips
    let grip_r = h * 0.165;
    p.circle_filled(
        Pos2::new(body.left() + w * 0.055, body.bottom() - h * 0.03),
        grip_r,
        body_fill,
    );
    p.circle_filled(
        Pos2::new(body.right() - w * 0.055, body.bottom() - h * 0.03),
        grip_r,
        body_fill,
    );
    p.rect_filled(body, CornerRadius::ZERO, body_fill);
    p.rect_stroke(
        body,
        CornerRadius::ZERO,
        Stroke::new(1.5, body_line),
        egui::StrokeKind::Inside,
    );
    // soft top highlight
    p.rect_filled(
        Rect::from_min_max(body.min, at(0.5, 0.165)),
        CornerRadius { nw: 58, ne: 2, sw: 0, se: 0 },
        Color32::from_white_alpha(if t.dark { 6 } else { 40 }),
    );

    // screen
    let screen = Rect::from_center_size(at(0.5, 0.49), Vec2::new(w * 0.455, h * 0.40));
    p.rect_filled(screen.expand(3.0), CornerRadius::ZERO, body_line);
    p.rect_filled(screen, CornerRadius::ZERO, Color32::from_rgb(0x04, 0x04, 0x0a));
    screen_content(ui, t, screen, &input, view);

    let mut clicked = None;
    let mut hit = |ui: &mut Ui, control: DeckControl, rect: Rect| -> Response {
        let resp = ui.interact(
            rect,
            ui.id().with(("deck-control", control.name())),
            Sense::click(),
        );
        if resp.clicked() {
            clicked = Some(control);
        }
        resp.on_hover_text(format!("{} · нажмите, чтобы назначить бинд", control.name()))
    };

    // triggers
    let lt = Rect::from_min_max(at(0.145, 0.015), at(0.315, 0.085));
    let rt = Rect::from_min_max(at(0.685, 0.015), at(0.855, 0.085));
    trigger_bar(ui, t, lt, "LT", input.state.lt, t.accent, "lt", view.cfg.glow);
    trigger_bar(ui, t, rt, "RT", input.state.rt, t.accent, "rt", view.cfg.glow);
    hit(ui, DeckControl::Lt, lt);
    hit(ui, DeckControl::Rt, rt);

    // bumpers
    let lb = Rect::from_min_max(at(0.145, 0.095), at(0.315, 0.148));
    let rb = Rect::from_min_max(at(0.685, 0.095), at(0.855, 0.148));
    bumper(ui, t, lb, "LB", input.state.buttons & udp::BTN_LB != 0, control_fill, t, "lb");
    bumper(ui, t, rb, "RB", input.state.buttons & udp::BTN_RB != 0, control_fill, t, "rb");
    hit(ui, DeckControl::Lb, lb);
    hit(ui, DeckControl::Rb, rb);

    // sticks
    let lstick = at(0.155, 0.42);
    let rstick = at(0.845, 0.62);
    stick(
        ui,
        t,
        lstick,
        h * 0.088,
        input.state.lx,
        input.state.ly,
        input.state.buttons & udp::BTN_L3 != 0,
        control_fill,
        "lstick",
        view.cfg.glow,
    );
    stick(
        ui,
        t,
        rstick,
        h * 0.088,
        input.state.rx,
        input.state.ry,
        input.state.buttons & udp::BTN_R3 != 0,
        control_fill,
        "rstick",
        view.cfg.glow,
    );
    hit(ui, DeckControl::LStick, Rect::from_center_size(lstick, Vec2::splat(h * 0.176)));
    hit(ui, DeckControl::RStick, Rect::from_center_size(rstick, Vec2::splat(h * 0.176)));

    // dpad
    let dpad = at(0.205, 0.72);
    let arm = h * 0.052;
    let pad = h * 0.052;
    for (control, offset, glyph) in [
        (DeckControl::DpadUp, Vec2::new(0.0, -arm), crate::theme::icon::ARROW_UP),
        (DeckControl::DpadDown, Vec2::new(0.0, arm), crate::theme::icon::ARROW_DOWN),
        (DeckControl::DpadLeft, Vec2::new(-arm, 0.0), crate::theme::icon::ARROW_LEFT),
        (DeckControl::DpadRight, Vec2::new(arm, 0.0), crate::theme::icon::ARROW_RIGHT),
    ] {
        let active = match control {
            DeckControl::DpadUp => input.state.buttons & udp::BTN_DPAD_UP != 0,
            DeckControl::DpadDown => input.state.buttons & udp::BTN_DPAD_DOWN != 0,
            DeckControl::DpadLeft => input.state.buttons & udp::BTN_DPAD_LEFT != 0,
            _ => input.state.buttons & udp::BTN_DPAD_RIGHT != 0,
        };
        let c = dpad + offset;
        let rect = Rect::from_center_size(c, Vec2::splat(pad));
        let r = dpad_key(ui, t, rect, glyph, active, control_fill, t.accent, view.cfg.glow, control.name());
        hit(ui, control, r.rect);
    }
    // center of the dpad
    p.circle_filled(dpad, pad * 0.28, control_fill);

    // abxy
    let abxy = at(0.845, 0.38);
    let off = h * 0.088;
    for (control, delta, label, color) in [
        (DeckControl::Y, Vec2::new(0.0, -off), "Y", Y_COLOR),
        (DeckControl::A, Vec2::new(0.0, off), "A", A_COLOR),
        (DeckControl::X, Vec2::new(-off, 0.0), "X", X_COLOR),
        (DeckControl::B, Vec2::new(off, 0.0), "B", B_COLOR),
    ] {
        let bit = match control {
            DeckControl::A => udp::BTN_A,
            DeckControl::B => udp::BTN_B,
            DeckControl::X => udp::BTN_X,
            _ => udp::BTN_Y,
        };
        let active = input.state.buttons & bit != 0;
        let c = abxy + delta;
        let r = circle_key(
            ui,
            t,
            c,
            h * 0.038,
            label,
            active,
            color,
            control_fill,
            view.cfg.glow,
            control.name(),
        );
        hit(ui, control, r);
    }

    // trackpads
    let ltp = Rect::from_center_size(at(0.345, 0.765), Vec2::splat(h * 0.155));
    let rtp = Rect::from_center_size(at(0.655, 0.765), Vec2::splat(h * 0.155));
    trackpad(
        ui,
        t,
        ltp,
        "L",
        input.pressed.contains(&BTN_LEFT_CLICK),
        control_fill,
        "ltp",
        view.cfg.glow,
    );
    trackpad(
        ui,
        t,
        rtp,
        "R",
        input.pressed.contains(&BTN_RIGHT_CLICK) || input.pressed.contains(&BTN_LEFT_CLICK),
        control_fill,
        "rtp",
        view.cfg.glow,
    );
    hit(ui, DeckControl::LTrackpad, ltp);
    hit(ui, DeckControl::RTrackpad, rtp);

    // view / menu
    let view_c = at(0.055, 0.735);
    let menu_c = at(0.945, 0.735);
    let r = h * 0.026;
    let vr = circle_key(
        ui,
        t,
        view_c,
        r,
        crate::theme::icon::VIEW,
        input.state.buttons & udp::BTN_BACK != 0,
        t.accent,
        control_fill,
        view.cfg.glow,
        "view",
    );
    let mr = circle_key(
        ui,
        t,
        menu_c,
        r,
        crate::theme::icon::MENU,
        input.state.buttons & udp::BTN_START != 0,
        t.accent,
        control_fill,
        view.cfg.glow,
        "menu",
    );
    hit(ui, DeckControl::View, vr);
    hit(ui, DeckControl::Menu, mr);

    // steam button
    let steam = Rect::from_center_size(at(0.5, 0.905), Vec2::new(w * 0.075, h * 0.055));
    let active = input.state.buttons & udp::BTN_GUIDE != 0;
    let k = ui
        .ctx()
        .animate_bool_with_time(ui.id().with("steam"), active, 0.07);
    p.rect_filled(
        steam,
        CornerRadius::ZERO,
        control_fill.lerp_to_gamma(t.accent, k),
    );
    p.rect_stroke(
        steam,
        CornerRadius::ZERO,
        Stroke::new(1.0, if active { t.accent } else { body_line }),
        egui::StrokeKind::Inside,
    );
    p.text(
        steam.center(),
        Align2::CENTER_CENTER,
        "steam",
        FontId::new(h * 0.027, FontFamily::Proportional),
        if active { Color32::WHITE } else { t.muted },
    );
    hit(ui, DeckControl::Steam, steam);

    // back buttons
    for (control, x, y) in [
        (DeckControl::L4, 0.075, 0.885),
        (DeckControl::L5, 0.115, 0.935),
        (DeckControl::R4, 0.925, 0.885),
        (DeckControl::R5, 0.885, 0.935),
    ] {
        let c = at(x, y);
        let rect = Rect::from_center_size(c, Vec2::new(w * 0.036, h * 0.045));
        let active = input.pressed.contains(&control.code());
        let k = ui
            .ctx()
            .animate_bool_with_time(ui.id().with(control.name()), active, 0.07);
        p.rect_filled(
            rect,
            CornerRadius::ZERO,
            control_fill.lerp_to_gamma(t.accent, k),
        );
        p.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, if active { t.accent } else { body_line }),
            egui::StrokeKind::Inside,
        );
        if view.cfg.labels {
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                control.name(),
                FontId::new(h * 0.022, FontFamily::Monospace),
                if active { Color32::WHITE } else { t.faint },
            );
        }
        hit(ui, control, rect);
    }

    // labels for the big pieces
    if view.cfg.labels {
        for (pos, text) in [
            (at(0.155, 0.535), "L3"),
            (at(0.845, 0.535), "R3"),
            (at(0.345, 0.865), "trackpad"),
            (at(0.655, 0.865), "trackpad"),
        ] {
            p.text(
                pos,
                Align2::CENTER_CENTER,
                text,
                FontId::new(h * 0.023, FontFamily::Monospace),
                t.faint,
            );
        }
    }

    // selected outline
    if let Some(selected) = view.selected {
        if let Some(rect) = control_rect(selected, rect, w, h) {
            p.rect_stroke(
                rect.expand(4.0),
                CornerRadius::ZERO,
                Stroke::new(2.0, t.accent),
                egui::StrokeKind::Outside,
            );
        }
    }

    // touchscreen is clickable as a whole, above the screen content
    let screen_resp = hit(ui, DeckControl::Screen, screen);
    if screen_resp.clicked() {
        clicked = Some(DeckControl::Screen);
    }

    clicked
}

fn screen_content(
    ui: &mut Ui,
    t: &Tokens,
    screen: Rect,
    input: &DeckInput<'_>,
    view: &DeckViewState,
) {
    let p = ui.painter().clone();
    let h = screen.height();
    // ambient glow
    p.circle_filled(
        Pos2::new(screen.right() - screen.width() * 0.18, screen.top() + h * 0.10),
        screen.width() * 0.32,
        t.accent.gamma_multiply(0.10),
    );
    p.circle_filled(
        Pos2::new(screen.left() + screen.width() * 0.12, screen.bottom() - h * 0.10),
        screen.width() * 0.26,
        t.accent.gamma_multiply(0.06),
    );
    let pad = screen.shrink2(Vec2::new(h * 0.10, h * 0.09));
    // status row
    p.circle_filled(
        Pos2::new(pad.left() + 5.0, pad.top() + 5.0),
        4.0,
        if input.online { t.good } else { t.bad },
    );
    p.text(
        Pos2::new(pad.left() + 16.0, pad.top() - 2.0),
        Align2::LEFT_TOP,
        sdw_core::util::truncate(input.host, 26),
        FontId::new(h * 0.045, FontFamily::Monospace),
        t.muted,
    );
    p.text(
        Pos2::new(pad.right(), pad.top() - 2.0),
        Align2::RIGHT_TOP,
        if input.enabled { "live" } else { "monitor" },
        FontId::new(h * 0.040, FontFamily::Monospace),
        if input.enabled { t.good } else { t.faint },
    );
    // big title
    p.text(
        Pos2::new(pad.center().x, pad.center().y - h * 0.09),
        Align2::CENTER_CENTER,
        "sd-widgets",
        FontId::new(h * 0.13, FontFamily::Proportional),
        t.strong,
    );
    p.text(
        Pos2::new(pad.center().x, pad.center().y + h * 0.045),
        Align2::CENTER_CENTER,
        input.pad_label,
        FontId::new(h * 0.052, FontFamily::Monospace),
        t.accent,
    );
    // pressed buttons
    let mut labels = udp::pressed_labels(input.state.buttons);
    if input.state.lt > 0.08 {
        labels.push("LT");
    }
    if input.state.rt > 0.08 {
        labels.push("RT");
    }
    if view.selected.is_some() {
        // selection hint replaces the button row while customising
        p.text(
            Pos2::new(pad.center().x, pad.bottom() - h * 0.02),
            Align2::CENTER_BOTTOM,
            "нажмите «назначить бинд»",
            FontId::new(h * 0.042, FontFamily::Proportional),
            t.warn,
        );
    } else if labels.is_empty() {
        p.text(
            Pos2::new(pad.center().x, pad.bottom() - h * 0.02),
            Align2::CENTER_BOTTOM,
            if input.devices == 0 {
                "нет доступа к /dev/input"
            } else {
                "нажмите кнопку"
            },
            FontId::new(h * 0.042, FontFamily::Proportional),
            t.faint,
        );
    } else {
        let text = labels.drain(..).take(7).collect::<Vec<_>>().join("  ");
        p.text(
            Pos2::new(pad.center().x, pad.bottom() - h * 0.02),
            Align2::CENTER_BOTTOM,
            text,
            FontId::new(h * 0.055, FontFamily::Monospace),
            t.ink,
        );
    }
    p.rect_stroke(
        screen,
        CornerRadius::ZERO,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
}

fn trigger_bar(
    ui: &mut Ui,
    t: &Tokens,
    rect: Rect,
    label: &str,
    value: f32,
    color: Color32,
    id: &str,
    glow: bool,
) {
    let k = ui
        .ctx()
        .animate_value_with_time(ui.id().with(("trigger", id)), value.clamp(0.0, 1.0), 0.06);
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::ZERO, t.well);
    let fill = Rect::from_min_size(
        rect.min,
        Vec2::new(rect.width() * k.max(0.02), rect.height()),
    );
    p.rect_filled(fill, CornerRadius::ZERO, color.gamma_multiply(0.75));
    if glow && k > 0.05 {
        p.rect_stroke(
            rect.expand(2.0),
            CornerRadius::ZERO,
            Stroke::new(1.0, color.gamma_multiply(0.5 * k)),
            egui::StrokeKind::Outside,
        );
    }
    p.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        format!("{label} {}", (value * 100.0).round() as i32),
        FontId::new(rect.height() * 0.38, FontFamily::Monospace),
        t.ink,
    );
}

fn bumper(
    ui: &mut Ui,
    t: &Tokens,
    rect: Rect,
    label: &str,
    active: bool,
    fill: Color32,
    t2: &Tokens,
    id: &str,
) -> Response {
    let k = ui
        .ctx()
        .animate_bool_with_time(ui.id().with(("bumper", id)), active, 0.07);
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::ZERO, fill.lerp_to_gamma(t.accent, k));
    p.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, if active { t.accent } else { t.line }),
        egui::StrokeKind::Inside,
    );
    if active {
        p.rect_stroke(
            rect.expand(3.0),
            CornerRadius::ZERO,
            Stroke::new(1.0, t.accent.gamma_multiply(0.4)),
            egui::StrokeKind::Outside,
        );
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::new(rect.height() * 0.42, FontFamily::Monospace),
        if active { Color32::WHITE } else { t2.muted },
    );
    ui.interact(rect, ui.id().with(("bumper-hit", id)), Sense::hover())
}

fn stick(
    ui: &mut Ui,
    t: &Tokens,
    center: Pos2,
    radius: f32,
    x: f32,
    y: f32,
    pressed: bool,
    fill: Color32,
    id: &str,
    glow: bool,
) {
    let ctx = ui.ctx();
    let ax = ctx.animate_value_with_time(ui.id().with(("stick-x", id)), x.clamp(-1.0, 1.0), 0.08);
    let ay = ctx.animate_value_with_time(ui.id().with(("stick-y", id)), y.clamp(-1.0, 1.0), 0.08);
    let k = ctx.animate_bool_with_time(ui.id().with(("stick-press", id)), pressed, 0.07);
    let p = ui.painter().clone();
    p.circle_filled(center, radius + 6.0, Color32::from_black_alpha(if t.dark { 70 } else { 18 }));
    p.circle_filled(center, radius, fill);
    p.circle_stroke(center, radius, Stroke::new(1.0, t.line));
    p.circle_stroke(center, radius * 0.55, Stroke::new(1.0, t.line.gamma_multiply(0.7)));
    let dot = center + Vec2::new(ax * radius * 0.55, ay * radius * 0.55);
    p.circle_filled(dot, radius * 0.38, if t.dark { Color32::from_rgb(0x3a, 0x3a, 0x4a) } else { Color32::from_rgb(0xc9, 0xc6, 0xd6) });
    p.circle_stroke(dot, radius * 0.38, Stroke::new(1.0, t.line_strong));
    p.circle_filled(dot, radius * 0.10, t.accent.gamma_multiply(0.9));
    if pressed {
        p.circle_stroke(center, radius + 2.0, Stroke::new(2.0, t.accent));
    }
    if glow && (ax.abs() > 0.2 || ay.abs() > 0.2) {
        p.circle_stroke(
            center,
            radius + 4.0,
            Stroke::new(1.0, t.accent.gamma_multiply(0.25)),
        );
    }
    let _ = k;
}

fn circle_key(
    ui: &mut Ui,
    t: &Tokens,
    center: Pos2,
    radius: f32,
    label: &str,
    active: bool,
    color: Color32,
    fill: Color32,
    glow: bool,
    id: &str,
) -> Rect {
    let k = ui
        .ctx()
        .animate_bool_with_time(ui.id().with(("key", id)), active, 0.07);
    let r = radius * (1.0 - 0.06 * k);
    let p = ui.painter().clone();
    p.circle_filled(center + Vec2::new(0.0, 2.0), r, Color32::from_black_alpha(if t.dark { 80 } else { 20 }));
    p.circle_filled(center, r, fill.lerp_to_gamma(color, k));
    p.circle_stroke(
        center,
        r,
        Stroke::new(1.2, if active { color } else { t.line_strong }),
    );
    if glow && active {
        for i in 0..3 {
            p.circle_stroke(
                center,
                r + 3.0 + i as f32 * 3.0,
                Stroke::new(1.0, color.gamma_multiply(0.22 - i as f32 * 0.06)),
            );
        }
    }
    p.text(
        center,
        Align2::CENTER_CENTER,
        label,
        FontId::new(r * 0.95, crate::theme::glyph_family(label)),
        if active { Color32::WHITE } else { t.ink },
    );
    Rect::from_center_size(center, Vec2::splat(r * 2.0))
}

fn dpad_key(
    ui: &mut Ui,
    t: &Tokens,
    rect: Rect,
    glyph: &str,
    active: bool,
    fill: Color32,
    color: Color32,
    glow: bool,
    id: &str,
) -> Response {
    let k = ui
        .ctx()
        .animate_bool_with_time(ui.id().with(("dpad", id)), active, 0.07);
    let p = ui.painter().clone();
    p.rect_filled(
        rect,
        CornerRadius::ZERO,
        fill.lerp_to_gamma(color, k),
    );
    p.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, if active { color } else { t.line }),
        egui::StrokeKind::Inside,
    );
    if glow && active {
        p.rect_stroke(
            rect.expand(3.0),
            CornerRadius::ZERO,
            Stroke::new(1.0, color.gamma_multiply(0.35)),
            egui::StrokeKind::Outside,
        );
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::new(rect.height() * 0.52, crate::theme::glyph_family(glyph)),
        if active { Color32::WHITE } else { t.muted },
    );
    ui.interact(rect, ui.id().with(("dpad-hit", id)), Sense::hover())
}

fn trackpad(
    ui: &mut Ui,
    t: &Tokens,
    rect: Rect,
    label: &str,
    active: bool,
    fill: Color32,
    id: &str,
    glow: bool,
) {
    let k = ui
        .ctx()
        .animate_bool_with_time(ui.id().with(("pad", id)), active, 0.07);
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::ZERO, fill.lerp_to_gamma(t.accent, k * 0.7));
    p.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, if active { t.accent } else { t.line }),
        egui::StrokeKind::Inside,
    );
    // hairlines so it reads as a trackpad, not a button
    for i in 1..4 {
        let x = rect.left() + rect.width() * i as f32 / 4.0;
        p.line_segment(
            [Pos2::new(x, rect.top() + 8.0), Pos2::new(x, rect.bottom() - 8.0)],
            Stroke::new(1.0, t.line.gamma_multiply(0.55)),
        );
        let y = rect.top() + rect.height() * i as f32 / 4.0;
        p.line_segment(
            [Pos2::new(rect.left() + 8.0, y), Pos2::new(rect.right() - 8.0, y)],
            Stroke::new(1.0, t.line.gamma_multiply(0.55)),
        );
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::new(rect.height() * 0.22, FontFamily::Monospace),
        if active { Color32::WHITE } else { t.faint },
    );
    if glow && active {
        p.rect_stroke(
            rect.expand(4.0),
            CornerRadius::ZERO,
            Stroke::new(1.0, t.accent.gamma_multiply(0.35)),
            egui::StrokeKind::Outside,
        );
    }
}

fn control_rect(control: DeckControl, rect: Rect, w: f32, h: f32) -> Option<Rect> {
    let at = |fx: f32, fy: f32| Pos2::new(rect.left() + w * fx, rect.top() + h * fy);
    let size = Vec2::splat(h * 0.09);
    Some(match control {
        DeckControl::A => Rect::from_center_size(at(0.845, 0.38 + 0.088), size),
        DeckControl::B => Rect::from_center_size(at(0.845 + 0.088 * h / w, 0.38), size),
        DeckControl::X => Rect::from_center_size(at(0.845 - 0.088 * h / w, 0.38), size),
        DeckControl::Y => Rect::from_center_size(at(0.845, 0.38 - 0.088), size),
        DeckControl::DpadUp => Rect::from_center_size(at(0.205, 0.72 - 0.052), size),
        DeckControl::DpadDown => Rect::from_center_size(at(0.205, 0.72 + 0.052), size),
        DeckControl::DpadLeft => Rect::from_center_size(at(0.205 - 0.052 * h / w, 0.72), size),
        DeckControl::DpadRight => Rect::from_center_size(at(0.205 + 0.052 * h / w, 0.72), size),
        DeckControl::Lb => Rect::from_min_max(at(0.145, 0.095), at(0.315, 0.148)),
        DeckControl::Rb => Rect::from_min_max(at(0.685, 0.095), at(0.855, 0.148)),
        DeckControl::Lt => Rect::from_min_max(at(0.145, 0.015), at(0.315, 0.085)),
        DeckControl::Rt => Rect::from_min_max(at(0.685, 0.015), at(0.855, 0.085)),
        DeckControl::View => Rect::from_center_size(at(0.055, 0.735), Vec2::splat(h * 0.06)),
        DeckControl::Menu => Rect::from_center_size(at(0.945, 0.735), Vec2::splat(h * 0.06)),
        DeckControl::Steam => Rect::from_center_size(at(0.5, 0.905), Vec2::new(w * 0.08, h * 0.06)),
        DeckControl::L3 | DeckControl::LStick => {
            Rect::from_center_size(at(0.155, 0.42), Vec2::splat(h * 0.18))
        }
        DeckControl::R3 | DeckControl::RStick => {
            Rect::from_center_size(at(0.845, 0.62), Vec2::splat(h * 0.18))
        }
        DeckControl::L4 => Rect::from_center_size(at(0.075, 0.885), Vec2::new(w * 0.04, h * 0.05)),
        DeckControl::L5 => Rect::from_center_size(at(0.115, 0.935), Vec2::new(w * 0.04, h * 0.05)),
        DeckControl::R4 => Rect::from_center_size(at(0.925, 0.885), Vec2::new(w * 0.04, h * 0.05)),
        DeckControl::R5 => Rect::from_center_size(at(0.885, 0.935), Vec2::new(w * 0.04, h * 0.05)),
        DeckControl::LTrackpad => Rect::from_center_size(at(0.345, 0.765), Vec2::splat(h * 0.16)),
        DeckControl::RTrackpad => Rect::from_center_size(at(0.655, 0.765), Vec2::splat(h * 0.16)),
        DeckControl::Screen => Rect::from_center_size(at(0.5, 0.49), Vec2::new(w * 0.455, h * 0.40)),
    })
}

/// Small helper used by the bind editor when it wants to show the chosen gesture.
pub fn gesture_hint(kind: GestureKind, fingers: u8, dir: SwipeDir) -> String {
    let fingers = match fingers {
        1 => "1 палец".to_string(),
        2 => "2 пальца".to_string(),
        3 => "3 пальца".to_string(),
        n => format!("{n} пальца"),
    };
    match kind {
        GestureKind::Swipe => format!("{fingers}: свайп {}", dir.label()),
        GestureKind::Tap => format!("{fingers}: тап"),
        GestureKind::LongPress => format!("{fingers}: удержание"),
        GestureKind::PinchIn => format!("{fingers}: сжать"),
        GestureKind::PinchOut => format!("{fingers}: разжать"),
    }
}

/// Renders a compact live input strip, used under the deck when space is tight.
pub fn pressed_strip(ui: &mut Ui, t: &Tokens, state: &GamepadState) {
    let labels = udp::pressed_labels(state.buttons);
    ui.horizontal_wrapped(|ui| {
        if labels.is_empty() {
            ui.label(hud("—", t.faint));
        }
        for label in labels {
            ui.label(
                RichText::new(label)
                    .family(FontFamily::Monospace)
                    .size(12.0)
                    .color(t.accent),
            );
        }
    });
}
