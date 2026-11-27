//! a steam deck you can touch.
//!
//! the whole handheld is drawn from primitives: body, grips, screen, sticks, trackpads,
//! triggers and every button. pressed controls light up with a short animated glow, the
//! sticks follow the real sticks, and tapping any control selects it so it can be bound.

use crate::input::{self, BTN_THUMB, BTN_THUMB2, BTN_TOUCH};
use crate::theme::{hud, Tokens};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Response, RichText, Sense,
    Stroke, Ui, Vec2,
};
use sdw_core::model::{DeckViewCfg, GestureKind, GestureSpec, SwipeDir, Trigger};
use sdw_core::udp::{self, GamepadState};
use sdw_core::{t, Lang};
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
            Self::X => input::BTN_NORTH,
            Self::Y => input::BTN_WEST,
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
            // The Deck's touchpads click as BTN_THUMB / BTN_THUMB2; the
            // mouse buttons never arrive on a deck.
            Self::LTrackpad => BTN_THUMB,
            Self::RTrackpad => BTN_THUMB2,
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
    /// Latest raw touch position per pad, -1..1 around the centre. `None`
    /// means the finger is not on that pad.
    pub pad_touch: [Option<(f32, f32)>; 2],
    pub host: &'a str,
    pub pad_label: &'a str,
    pub online: bool,
    pub enabled: bool,
    pub devices: usize,
    pub lang: Lang,
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
    // Never ask for more width than the page has: at a large interface scale
    // the Deck art must shrink with the column instead of overflowing it.
    let width = (avail * view.cfg.scale.clamp(0.6, 1.4))
        .min(1120.0)
        .min(avail)
        .max(1.0);
    // The Deck is 298 x 117 mm, so the front is about 2.55:1. A little margin
    // above the body keeps room for the triggers.
    let size = Vec2::new(width, width / 2.62);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter().clone();
    let w = rect.width();
    let h = rect.height();
    let l = deck_layout(rect);
    let body = l.body;
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
        body.translate(Vec2::new(0.0, h * 0.025)),
        CornerRadius::ZERO,
        Color32::from_black_alpha(if t.dark { 120 } else { 36 }),
    );
    // grips bulge out of the lower corners
    let grip_r = h * 0.20;
    p.circle_filled(
        Pos2::new(body.left() + w * 0.035, body.bottom() - h * 0.02),
        grip_r,
        body_fill,
    );
    p.circle_filled(
        Pos2::new(body.right() - w * 0.035, body.bottom() - h * 0.02),
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
        Rect::from_min_max(body.min, Pos2::new(body.center().x, body.top() + h * 0.02)),
        CornerRadius {
            nw: 58,
            ne: 2,
            sw: 0,
            se: 0,
        },
        Color32::from_white_alpha(if t.dark { 6 } else { 40 }),
    );

    // screen: nearly the whole height, controls live in the side bands
    let screen = l.screen;
    p.rect_filled(screen.expand(3.0), CornerRadius::ZERO, body_line);
    p.rect_filled(
        screen,
        CornerRadius::ZERO,
        Color32::from_rgb(0x04, 0x04, 0x0a),
    );
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
        resp.on_hover_text(format!(
            "{} · нажмите, чтобы назначить бинд",
            control.name()
        ))
    };

    // triggers, above the body
    trigger_bar(
        ui,
        t,
        l.lt,
        "LT",
        input.state.lt,
        t.accent,
        "lt",
        view.cfg.glow,
    );
    trigger_bar(
        ui,
        t,
        l.rt,
        "RT",
        input.state.rt,
        t.accent,
        "rt",
        view.cfg.glow,
    );
    hit(ui, DeckControl::Lt, l.lt);
    hit(ui, DeckControl::Rt, l.rt);

    // bumpers on the top edge
    bumper(
        ui,
        t,
        l.lb,
        "LB",
        input.state.buttons & udp::BTN_LB != 0,
        control_fill,
        t,
        "lb",
    );
    bumper(
        ui,
        t,
        l.rb,
        "RB",
        input.state.buttons & udp::BTN_RB != 0,
        control_fill,
        t,
        "rb",
    );
    hit(ui, DeckControl::Lb, l.lb);
    hit(ui, DeckControl::Rb, l.rb);

    // sticks: left above the left trackpad, right above the right trackpad
    stick(
        ui,
        t,
        l.lstick,
        l.stick_r,
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
        l.rstick,
        l.stick_r,
        input.state.rx,
        input.state.ry,
        input.state.buttons & udp::BTN_R3 != 0,
        control_fill,
        "rstick",
        view.cfg.glow,
    );
    hit(
        ui,
        DeckControl::LStick,
        Rect::from_center_size(l.lstick, Vec2::splat(l.stick_r * 2.0)),
    );
    hit(
        ui,
        DeckControl::RStick,
        Rect::from_center_size(l.rstick, Vec2::splat(l.stick_r * 2.0)),
    );

    // d-pad, outer top-left
    for (control, offset, glyph) in [
        (
            DeckControl::DpadUp,
            Vec2::new(0.0, -l.dpad_arm_y),
            crate::theme::icon::ARROW_UP,
        ),
        (
            DeckControl::DpadDown,
            Vec2::new(0.0, l.dpad_arm_y),
            crate::theme::icon::ARROW_DOWN,
        ),
        (
            DeckControl::DpadLeft,
            Vec2::new(-l.dpad_arm_x, 0.0),
            crate::theme::icon::ARROW_LEFT,
        ),
        (
            DeckControl::DpadRight,
            Vec2::new(l.dpad_arm_x, 0.0),
            crate::theme::icon::ARROW_RIGHT,
        ),
    ] {
        let active = match control {
            DeckControl::DpadUp => input.state.buttons & udp::BTN_DPAD_UP != 0,
            DeckControl::DpadDown => input.state.buttons & udp::BTN_DPAD_DOWN != 0,
            DeckControl::DpadLeft => input.state.buttons & udp::BTN_DPAD_LEFT != 0,
            _ => input.state.buttons & udp::BTN_DPAD_RIGHT != 0,
        };
        let rect = Rect::from_center_size(l.dpad + offset, l.dpad_key);
        let r = dpad_key(
            ui,
            t,
            rect,
            glyph,
            active,
            control_fill,
            t.accent,
            view.cfg.glow,
            control.name(),
        );
        hit(ui, control, r.rect);
    }
    p.circle_filled(l.dpad, h * 0.012, control_fill);

    // abxy, outer top-right
    for (control, delta, label, color) in [
        (DeckControl::Y, Vec2::new(0.0, -l.abxy_off_y), "Y", Y_COLOR),
        (DeckControl::A, Vec2::new(0.0, l.abxy_off_y), "A", A_COLOR),
        (DeckControl::X, Vec2::new(-l.abxy_off_x, 0.0), "X", X_COLOR),
        (DeckControl::B, Vec2::new(l.abxy_off_x, 0.0), "B", B_COLOR),
    ] {
        let bit = match control {
            DeckControl::A => udp::BTN_A,
            DeckControl::B => udp::BTN_B,
            DeckControl::X => udp::BTN_X,
            _ => udp::BTN_Y,
        };
        let active = input.state.buttons & bit != 0;
        let r = circle_key(
            ui,
            t,
            l.abxy + delta,
            l.abxy_r,
            label,
            active,
            color,
            control_fill,
            view.cfg.glow,
            control.name(),
        );
        hit(ui, control, r);
    }

    // trackpads, under each stick
    trackpad(
        ui,
        t,
        l.ltp,
        "L",
        input.pressed.contains(&BTN_THUMB),
        control_fill,
        "ltp",
        view.cfg.glow,
        input.pad_touch[0],
    );
    trackpad(
        ui,
        t,
        l.rtp,
        "R",
        input.pressed.contains(&BTN_THUMB2),
        control_fill,
        "rtp",
        view.cfg.glow,
        input.pad_touch[1],
    );
    hit(ui, DeckControl::LTrackpad, l.ltp);
    hit(ui, DeckControl::RTrackpad, l.rtp);

    // view / menu, top edge beside the screen
    let vr = circle_key(
        ui,
        t,
        l.view_c,
        l.small_r,
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
        l.menu_c,
        l.small_r,
        crate::theme::icon::MENU,
        input.state.buttons & udp::BTN_START != 0,
        t.accent,
        control_fill,
        view.cfg.glow,
        "menu",
    );
    hit(ui, DeckControl::View, vr);
    hit(ui, DeckControl::Menu, mr);

    // steam button, bottom of the left grip
    let active = input.state.buttons & udp::BTN_GUIDE != 0;
    let k = ui
        .ctx()
        .animate_bool_with_time(ui.id().with("steam"), active, 0.07);
    p.rect_filled(
        l.steam,
        CornerRadius::ZERO,
        control_fill.lerp_to_gamma(t.accent, k),
    );
    p.rect_stroke(
        l.steam,
        CornerRadius::ZERO,
        Stroke::new(1.0, if active { t.accent } else { body_line }),
        egui::StrokeKind::Inside,
    );
    p.text(
        l.steam.center(),
        Align2::CENTER_CENTER,
        "steam",
        FontId::new(h * 0.023, FontFamily::Proportional),
        if active { Color32::WHITE } else { t.muted },
    );
    hit(ui, DeckControl::Steam, l.steam);

    // quick access "..." button, mirroring steam on the right grip
    p.rect_filled(l.quick, CornerRadius::ZERO, control_fill);
    p.rect_stroke(
        l.quick,
        CornerRadius::ZERO,
        Stroke::new(1.0, body_line),
        egui::StrokeKind::Inside,
    );
    p.text(
        l.quick.center(),
        Align2::CENTER_CENTER,
        "•••",
        FontId::new(h * 0.020, FontFamily::Monospace),
        t.muted,
    );

    // back grips, shown as small tabs on the outer bottom edge
    for (control, rect) in l.back {
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
                FontId::new(h * 0.020, FontFamily::Monospace),
                if active { Color32::WHITE } else { t.faint },
            );
        }
        hit(ui, control, rect);
    }

    // labels for the big pieces
    if view.cfg.labels {
        for (pos, text) in [
            (Pos2::new(l.lstick.x, l.lstick.y + l.stick_r * 1.5), "L3"),
            (Pos2::new(l.rstick.x, l.rstick.y + l.stick_r * 1.5), "R3"),
        ] {
            p.text(
                pos,
                Align2::CENTER_CENTER,
                text,
                FontId::new(h * 0.021, FontFamily::Monospace),
                t.faint,
            );
        }
    }

    // selected outline
    if let Some(selected) = view.selected {
        if let Some(rect) = control_rect(selected, rect) {
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

struct DeckLayout {
    body: Rect,
    screen: Rect,
    lt: Rect,
    rt: Rect,
    lb: Rect,
    rb: Rect,
    lstick: Pos2,
    rstick: Pos2,
    stick_r: f32,
    dpad: Pos2,
    dpad_arm_x: f32,
    dpad_arm_y: f32,
    dpad_key: Vec2,
    abxy: Pos2,
    abxy_off_x: f32,
    abxy_off_y: f32,
    abxy_r: f32,
    ltp: Rect,
    rtp: Rect,
    view_c: Pos2,
    menu_c: Pos2,
    small_r: f32,
    steam: Rect,
    quick: Rect,
    back: [(DeckControl, Rect); 4],
}

/// Front-view geometry taken from the real Deck: a full-height screen in the
/// middle, d-pad and left stick at the top-left, ABXY and right stick at the
/// top-right, a trackpad under each stick, and Steam/Quick Access at the
/// bottom of the grips.
fn deck_layout(rect: Rect) -> DeckLayout {
    let w = rect.width();
    let h = rect.height();
    let at = |x: f32, y: f32| Pos2::new(rect.left() + w * x, rect.top() + h * y);
    let body = Rect::from_min_max(at(0.010, 0.085), at(0.990, 0.985));
    let screen = Rect::from_center_size(at(0.5, 0.535), Vec2::new(w * 0.55, w * 0.344));
    let lt = Rect::from_min_max(at(0.105, 0.008), at(0.185, 0.058));
    let rt = Rect::from_min_max(at(0.815, 0.008), at(0.895, 0.058));
    let lb = Rect::from_min_max(at(0.100, 0.060), at(0.190, 0.084));
    let rb = Rect::from_min_max(at(0.810, 0.060), at(0.900, 0.084));
    let tp = w * 0.11;
    let ltp = Rect::from_center_size(at(0.125, 0.575), Vec2::splat(tp));
    let rtp = Rect::from_center_size(at(0.875, 0.575), Vec2::splat(tp));
    let back = |x: f32, y: f32| Rect::from_center_size(at(x, y), Vec2::new(w * 0.028, h * 0.038));
    DeckLayout {
        body,
        screen,
        lt,
        rt,
        lb,
        rb,
        lstick: at(0.165, 0.275),
        rstick: at(0.835, 0.275),
        stick_r: w * 0.0295,
        dpad: at(0.078, 0.235),
        dpad_arm_x: w * 0.0245,
        dpad_arm_y: h * 0.062,
        dpad_key: Vec2::new(w * 0.0265, h * 0.066),
        abxy: at(0.935, 0.235),
        abxy_off_x: w * 0.031,
        abxy_off_y: h * 0.066,
        abxy_r: w * 0.0215,
        ltp,
        rtp,
        view_c: at(0.135, 0.120),
        menu_c: at(0.865, 0.120),
        small_r: h * 0.024,
        steam: Rect::from_center_size(at(0.150, 0.790), Vec2::new(w * 0.095, h * 0.050)),
        quick: Rect::from_center_size(at(0.850, 0.790), Vec2::new(w * 0.095, h * 0.050)),
        back: [
            (DeckControl::L4, back(0.045, 0.900)),
            (DeckControl::L5, back(0.088, 0.905)),
            (DeckControl::R4, back(0.955, 0.900)),
            (DeckControl::R5, back(0.912, 0.905)),
        ],
    }
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
        Pos2::new(
            screen.right() - screen.width() * 0.18,
            screen.top() + h * 0.10,
        ),
        screen.width() * 0.32,
        t.accent.gamma_multiply(0.10),
    );
    p.circle_filled(
        Pos2::new(
            screen.left() + screen.width() * 0.12,
            screen.bottom() - h * 0.10,
        ),
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
            t!(
                input.lang,
                "нажмите «назначить бинд»",
                "press “assign bind”"
            ),
            FontId::new(h * 0.042, FontFamily::Proportional),
            t.warn,
        );
    } else if labels.is_empty() {
        p.text(
            Pos2::new(pad.center().x, pad.bottom() - h * 0.02),
            Align2::CENTER_BOTTOM,
            if input.devices == 0 {
                t!(
                    input.lang,
                    "нет доступа к /dev/input",
                    "no access to /dev/input"
                )
            } else {
                t!(input.lang, "нажмите кнопку", "press a button")
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
    let k = ui.ctx().animate_value_with_time(
        ui.id().with(("trigger", id)),
        value.clamp(0.0, 1.0),
        0.06,
    );
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
    p.circle_filled(
        center,
        radius + 6.0,
        Color32::from_black_alpha(if t.dark { 70 } else { 18 }),
    );
    p.circle_filled(center, radius, fill);
    p.circle_stroke(center, radius, Stroke::new(1.0, t.line));
    p.circle_stroke(
        center,
        radius * 0.55,
        Stroke::new(1.0, t.line.gamma_multiply(0.7)),
    );
    let dot = center + Vec2::new(ax * radius * 0.55, ay * radius * 0.55);
    p.circle_filled(
        dot,
        radius * 0.38,
        if t.dark {
            Color32::from_rgb(0x3a, 0x3a, 0x4a)
        } else {
            Color32::from_rgb(0xc9, 0xc6, 0xd6)
        },
    );
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
    p.circle_filled(
        center + Vec2::new(0.0, 2.0),
        r,
        Color32::from_black_alpha(if t.dark { 80 } else { 20 }),
    );
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
    p.rect_filled(rect, CornerRadius::ZERO, fill.lerp_to_gamma(color, k));
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
    touch: Option<(f32, f32)>,
) {
    let k = ui
        .ctx()
        .animate_bool_with_time(ui.id().with(("pad", id)), active, 0.07);
    let p = ui.painter().clone();
    p.rect_filled(
        rect,
        CornerRadius::ZERO,
        fill.lerp_to_gamma(t.accent, k * 0.7),
    );
    p.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, if active { t.accent } else { t.line }),
        egui::StrokeKind::Inside,
    );
    // The 8×8 grid the pad is bound by: a hairline per cell, the lines on the
    // quarters a touch stronger so the surface still reads as four areas at a
    // glance, and the cell under a finger filled in. The lines and the live dot
    // share one inset area, so the filled cell is the one under the dot.
    let zone_alpha = if touch.is_some() { 0.75 } else { 0.28 };
    let grid = f32::from(sdw_core::model::PadZone::GRID);
    let inset = rect.width() * 0.12;
    let area = rect.shrink2(Vec2::new(inset, inset));
    for i in 1..sdw_core::model::PadZone::GRID {
        let strong = i % sdw_core::model::PadZone::QUARTER == 0;
        let stroke = Stroke::new(
            1.0,
            t.line
                .gamma_multiply(if strong { zone_alpha } else { 0.12 }),
        );
        let x = area.left() + area.width() * f32::from(i) / grid;
        p.line_segment(
            [Pos2::new(x, area.top()), Pos2::new(x, area.bottom())],
            stroke,
        );
        let y = area.top() + area.height() * f32::from(i) / grid;
        p.line_segment(
            [Pos2::new(area.left(), y), Pos2::new(area.right(), y)],
            stroke,
        );
    }
    if let Some((x, y)) = touch {
        let zone = sdw_core::model::PadZone::from_point((x + 1.0) * 0.5, (y + 1.0) * 0.5);
        // Same band the live dot is drawn in, so the filled cell is the one
        // under that dot.
        let cw = area.width() / grid;
        let ch = area.height() / grid;
        let zr = Rect::from_min_size(
            Pos2::new(
                area.left() + cw * f32::from(zone.col),
                area.top() + ch * f32::from(zone.row),
            ),
            Vec2::new(cw, ch),
        );
        p.rect_filled(zr, CornerRadius::ZERO, t.accent.gamma_multiply(0.55));
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::new(rect.height() * 0.22, FontFamily::Monospace),
        if active { Color32::WHITE } else { t.faint },
    );
    // The live touch point: a small hard square with a hairline crosshair, so
    // a finger on the exact centre is visible as "here", not as "nothing".
    if let Some((x, y)) = touch {
        let pos = Pos2::new(
            area.left() + (x.clamp(-1.0, 1.0) + 1.0) * 0.5 * area.width(),
            area.top() + (y.clamp(-1.0, 1.0) + 1.0) * 0.5 * area.height(),
        );
        let dot = rect.width() * 0.045;
        p.line_segment(
            [
                Pos2::new(pos.x, rect.top() + 6.0),
                Pos2::new(pos.x, rect.bottom() - 6.0),
            ],
            Stroke::new(1.0, t.accent.gamma_multiply(0.45)),
        );
        p.line_segment(
            [
                Pos2::new(rect.left() + 6.0, pos.y),
                Pos2::new(rect.right() - 6.0, pos.y),
            ],
            Stroke::new(1.0, t.accent.gamma_multiply(0.45)),
        );
        p.rect_filled(
            Rect::from_center_size(pos, Vec2::splat(dot)),
            CornerRadius::ZERO,
            t.accent,
        );
        p.rect_stroke(
            Rect::from_center_size(pos, Vec2::splat(dot * 2.2)),
            CornerRadius::ZERO,
            Stroke::new(1.0, t.accent.gamma_multiply(0.5)),
            egui::StrokeKind::Outside,
        );
    }
    if glow && (active || touch.is_some()) {
        p.rect_stroke(
            rect.expand(4.0),
            CornerRadius::ZERO,
            Stroke::new(1.0, t.accent.gamma_multiply(0.35)),
            egui::StrokeKind::Outside,
        );
    }
}

fn control_rect(control: DeckControl, rect: Rect) -> Option<Rect> {
    let l = deck_layout(rect);
    Some(match control {
        DeckControl::A => Rect::from_center_size(
            l.abxy + Vec2::new(0.0, l.abxy_off_y),
            Vec2::splat(l.abxy_r * 2.0),
        ),
        DeckControl::B => Rect::from_center_size(
            l.abxy + Vec2::new(l.abxy_off_x, 0.0),
            Vec2::splat(l.abxy_r * 2.0),
        ),
        DeckControl::X => Rect::from_center_size(
            l.abxy + Vec2::new(-l.abxy_off_x, 0.0),
            Vec2::splat(l.abxy_r * 2.0),
        ),
        DeckControl::Y => Rect::from_center_size(
            l.abxy + Vec2::new(0.0, -l.abxy_off_y),
            Vec2::splat(l.abxy_r * 2.0),
        ),
        DeckControl::DpadUp => {
            Rect::from_center_size(l.dpad + Vec2::new(0.0, -l.dpad_arm_y), l.dpad_key)
        }
        DeckControl::DpadDown => {
            Rect::from_center_size(l.dpad + Vec2::new(0.0, l.dpad_arm_y), l.dpad_key)
        }
        DeckControl::DpadLeft => {
            Rect::from_center_size(l.dpad + Vec2::new(-l.dpad_arm_x, 0.0), l.dpad_key)
        }
        DeckControl::DpadRight => {
            Rect::from_center_size(l.dpad + Vec2::new(l.dpad_arm_x, 0.0), l.dpad_key)
        }
        DeckControl::Lb => l.lb,
        DeckControl::Rb => l.rb,
        DeckControl::Lt => l.lt,
        DeckControl::Rt => l.rt,
        DeckControl::View => Rect::from_center_size(l.view_c, Vec2::splat(l.small_r * 2.0)),
        DeckControl::Menu => Rect::from_center_size(l.menu_c, Vec2::splat(l.small_r * 2.0)),
        DeckControl::Steam => l.steam,
        DeckControl::L3 | DeckControl::LStick => {
            Rect::from_center_size(l.lstick, Vec2::splat(l.stick_r * 2.0))
        }
        DeckControl::R3 | DeckControl::RStick => {
            Rect::from_center_size(l.rstick, Vec2::splat(l.stick_r * 2.0))
        }
        DeckControl::L4 | DeckControl::L5 | DeckControl::R4 | DeckControl::R5 => l
            .back
            .iter()
            .find(|(c, _)| *c == control)
            .map(|(_, r)| *r)?,
        DeckControl::LTrackpad => l.ltp,
        DeckControl::RTrackpad => l.rtp,
        DeckControl::Screen => l.screen,
    })
}

/// Small helper used by the bind editor when it wants to show the chosen gesture.
pub fn gesture_hint(kind: GestureKind, fingers: u8, dir: SwipeDir, lang: Lang) -> String {
    GestureSpec {
        kind,
        fingers,
        dir: Some(dir),
        pad: None,
        zone: None,
    }
    .label_lang(lang)
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
