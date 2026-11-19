//! the deck app: pages, polling, pairing, gestures, the virtual gamepad and the stream.

use crate::binds::{self, BindEditor};
use crate::deck_view::DeckViewState;
use crate::input::{self, DeckEvent, InputHandle, UdpTarget};
use crate::sound::{self, Cue};
use crate::theme::{self, Tokens};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, Id, Layout, Margin, Rect, RichText,
    ScrollArea, Sense, Stroke, Ui, Vec2,
};
use sdw_core::api::{self, Client};
use sdw_core::model::*;
use sdw_core::udp::GamepadState;
use sdw_core::update::{self, UpdateInfo};
use sdw_core::{t, Lang};
use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Home,
    Computers,
    Gamepad,
    Screen,
    Notifications,
    HomeAssistant,
    Binds,
    Settings,
    Theme,
}

impl Page {
    pub const NAV: [Page; 6] = [
        Page::Home,
        Page::Computers,
        Page::Gamepad,
        Page::Screen,
        Page::Notifications,
        Page::Binds,
    ];
    /// Every page, in the order the arrows and the floating dock walk through them.
    pub const ORDER: [Page; 9] = [
        Page::Home,
        Page::Computers,
        Page::Gamepad,
        Page::Screen,
        Page::Notifications,
        Page::HomeAssistant,
        Page::Binds,
        Page::Theme,
        Page::Settings,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Page::Home => "home",
            Page::Computers => "computers",
            Page::Gamepad => "gamepad",
            Page::Screen => "screen",
            Page::Notifications => "notifications",
            Page::HomeAssistant => "ha",
            Page::Binds => "binds",
            Page::Settings => "settings",
            Page::Theme => "theme",
        }
    }
    pub fn from_key(key: &str) -> Page {
        match key.trim().to_ascii_lowercase().as_str() {
            "computers" | "pc" => Page::Computers,
            "gamepad" => Page::Gamepad,
            "screen" | "stream" => Page::Screen,
            "notifications" => Page::Notifications,
            "ha" | "homeassistant" => Page::HomeAssistant,
            "binds" => Page::Binds,
            "settings" => Page::Settings,
            "theme" | "look" | "appearance" => Page::Theme,
            _ => Page::Home,
        }
    }
    pub fn label(self, l: Lang) -> &'static str {
        match self {
            Page::Home => t!(l, "домой", "home"),
            Page::Computers => t!(l, "компьютеры", "computers"),
            Page::Gamepad => t!(l, "геймпад", "gamepad"),
            Page::Screen => t!(l, "экран", "screen"),
            Page::Notifications => t!(l, "уведомления", "alerts"),
            Page::HomeAssistant => t!(l, "home assistant", "home assistant"),
            Page::Binds => t!(l, "бинды", "binds"),
            Page::Settings => t!(l, "настройки", "settings"),
            Page::Theme => t!(l, "вид", "look"),
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Page::Home => theme::icon::HOME,
            Page::Computers => theme::icon::COMPUTERS,
            Page::Gamepad => theme::icon::GAMEPAD,
            Page::Screen => theme::icon::SCREEN,
            Page::Notifications => theme::icon::NOTIFICATIONS,
            Page::HomeAssistant => theme::icon::HOME_ASSISTANT,
            Page::Binds => theme::icon::BINDS,
            Page::Settings => theme::icon::SETTINGS,
            Page::Theme => theme::icon::LOOK,
        }
    }
}

pub struct StreamState {
    pub feed: Option<StreamFeed>,
    pub texture: Option<egui::TextureHandle>,
    pub error: String,
    pub frames: u64,
    pub last_frame_at: Instant,
    pub fullscreen: bool,
    pub mode: MonitorMode,
    pub position: MonitorPosition,
    pub backend: MonitorBackend,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub quality: u8,
    pub input: bool,
    pub last_pointer: Instant,
    pub pointer_down: bool,
}

pub struct StreamFeed {
    stop: Arc<AtomicBool>,
    rx: Receiver<Vec<u8>>,
}

impl Drop for StreamFeed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub struct HaState {
    pub entities: Vec<HaEntity>,
    pub areas: Vec<String>,
    pub room: usize,
    pub search: String,
    pub url: String,
    pub token: String,
    pub token_show: bool,
    pub loaded: bool,
    pub pending: bool,
    pub error: String,
    pub last_fetch: Instant,
}

pub struct UpdateState {
    pub info: Option<UpdateInfo>,
    pub error: String,
    pub checking: bool,
    pub downloading: bool,
    pub progress: Arc<AtomicU64>,
    pub total: u64,
    pub path: String,
    pub last_check: Instant,
    pub notified: bool,
}

pub struct BindsState {
    pub editor: Option<BindEditor>,
    pub capture: bool,
    pub filter: String,
}

pub struct Toast {
    pub text: String,
    pub ok: bool,
    pub at: Instant,
}

pub enum Task {
    Status(Result<DaemonStatus, String>),
    Paired { base: String, result: Result<PairResponse, String> },
    Discovery(Result<Vec<DiscoveryResponse>, String>),
    Pad(Result<PadCreated, String>),
    Monitor(Result<MonitorStatus, String>),
    MonitorStopped(Result<MonitorStatus, String>),
    Notifications(Result<Vec<Notification>, String>),
    Ha(Result<HaStates, String>),
    Update(Result<UpdateInfo, String>),
    Updated(Result<(String, u64), String>),
    Installed(Result<String, String>),
    Msg(Result<String, String>),
}

pub struct App {
    pub cfg: DeckConfig,
    pub lang: Lang,
    pub dark: bool,
    pub applied_dark: bool,
    pub page: Page,
    pub page_epoch: u64,
    /// +1 when walking forward through the pages, -1 when walking back.
    pub page_dir: f32,
    pub page_at: Instant,
    pub status: Option<DaemonStatus>,
    pub status_err: String,
    pub status_pending: bool,
    pub last_status: Instant,
    pub notifications: Vec<Notification>,
    pub notif_last_id: u64,
    pub notif_unread: u32,
    pub notif_pending: bool,
    pub last_notif: Instant,

    pub pair_url: String,
    pub pair_pass: String,
    pub pair_name: String,
    pub pair_show: bool,
    pub pair_error: String,
    pub pairing: bool,
    pub discovery: Vec<DiscoveryResponse>,
    pub discovering: bool,
    pub add_open: bool,

    pub gamepad: GamepadState,
    pub gamepad_pad: Option<DeckPad>,
    pub gamepad_on: bool,
    pub gamepad_kind: PadKind,
    pub gamepad_name: String,
    pub gamepad_pending: bool,
    pub deck_view: DeckViewState,
    pub input: InputHandle,
    pub input_devices: Vec<String>,
    pub input_errors: Vec<String>,
    pub pressed: HashSet<u16>,
    pub last_button: Option<(u16, String, Instant)>,
    pub last_gesture: Option<(GestureSpec, Instant)>,

    /// Virtual pointer used when the deck's own gamepad drives the interface.
    pub pad_cursor: Option<egui::Pos2>,
    pub pad_cursor_at: Instant,
    pub pad_buttons: u32,
    pub pad_release: bool,
    pub pad_seen: bool,
    /// Finger swipe from a screen edge, used when the compositor sends touch as mouse.
    pub edge_swipe: Option<(egui::Pos2, egui::Pos2)>,

    pub stream: StreamState,
    pub ha: HaState,
    pub binds: BindsState,
    pub update: UpdateState,
    pub settings_name: String,

    pub osk_open: bool,
    pub osk_shift: bool,
    pub osk_symbols: bool,

    pub toast: Option<Toast>,
    pub task_tx: Sender<Task>,
    pub task_rx: Receiver<Task>,
    pub clock: String,
    pub last_clock: Instant,
    pub battery: Option<u8>,
    pub last_battery: Instant,
    pub applied_scale: f32,
    /// Working copy of the theme CSS while the appearance page is open.
    pub style_draft: String,
    pub last_frame_rect: Option<Rect>,
    pub quit_confirm: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, mut cfg: DeckConfig) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        theme::set_style(&cfg.style);
        theme::apply(&cc.egui_ctx, cfg.dark);
        cc.egui_ctx
            .set_zoom_factor(cfg.ui_scale.clamp(0.75, max_ui_scale()));
        if cfg.active_host.is_none() {
            if let Some(first) = cfg.hosts.first() {
                cfg.active_host = Some(first.id.clone());
            }
        }
        let active_pad = cfg
            .hosts
            .iter()
            .find(|h| Some(&h.id) == cfg.active_host.as_ref())
            .and_then(|h| h.pad.clone());
        let page = if cfg.hosts.is_empty() {
            Page::Computers
        } else {
            Page::from_key(&cfg.last_page)
        };
        let (task_tx, task_rx) = channel();
        let input = input::start();
        let deck_view = DeckViewState::new(cfg.deck_view.clone());
        let mut app = Self {
            lang: Lang::from_code(&cfg.lang),
            dark: cfg.dark,
            applied_dark: cfg.dark,
            page,
            page_epoch: 1,
            page_dir: 1.0,
            page_at: Instant::now(),
            status: None,
            status_err: String::new(),
            status_pending: false,
            last_status: Instant::now() - Duration::from_secs(5),
            notifications: Vec::new(),
            notif_last_id: 0,
            notif_unread: 0,
            notif_pending: false,
            last_notif: Instant::now() - Duration::from_secs(5),
            pair_url: String::new(),
            pair_pass: String::new(),
            pair_name: cfg.device_name.clone(),
            pair_show: false,
            pair_error: String::new(),
            pairing: false,
            discovery: Vec::new(),
            discovering: false,
            add_open: cfg.hosts.is_empty(),
            gamepad: GamepadState::default(),
            gamepad_pad: active_pad,
            gamepad_on: false,
            gamepad_kind: PadKind::Xbox360,
            gamepad_name: String::new(),
            gamepad_pending: false,
            deck_view,
            input,
            input_devices: Vec::new(),
            input_errors: Vec::new(),
            pressed: HashSet::new(),
            last_button: None,
            last_gesture: None,
            pad_cursor: None,
            pad_cursor_at: Instant::now() - Duration::from_secs(10),
            pad_buttons: 0,
            pad_release: false,
            pad_seen: false,
            edge_swipe: None,
            stream: StreamState {
                feed: None,
                texture: None,
                error: String::new(),
                frames: 0,
                last_frame_at: Instant::now() - Duration::from_secs(10),
                fullscreen: false,
                mode: MonitorMode::Extend,
                position: MonitorPosition::Auto,
                backend: MonitorBackend::Auto,
                width: 1280,
                height: 800,
                fps: 15,
                quality: 75,
                input: false,
                last_pointer: Instant::now() - Duration::from_secs(1),
                pointer_down: false,
            },
            ha: HaState {
                entities: Vec::new(),
                areas: Vec::new(),
                room: 0,
                search: String::new(),
                url: String::new(),
                token: String::new(),
                token_show: false,
                loaded: false,
                pending: false,
                error: String::new(),
                last_fetch: Instant::now() - Duration::from_secs(60),
            },
            binds: BindsState {
                editor: None,
                capture: false,
                filter: String::new(),
            },
            update: UpdateState {
                info: None,
                error: String::new(),
                checking: false,
                downloading: false,
                progress: Arc::new(AtomicU64::new(0)),
                total: 0,
                path: String::new(),
                last_check: Instant::now() - Duration::from_secs(86_400),
                notified: false,
            },
            settings_name: cfg.device_name.clone(),
            osk_open: false,
            osk_shift: false,
            osk_symbols: false,
            toast: None,
            task_tx,
            task_rx,
            clock: theme::clock_text(),
            last_clock: Instant::now(),
            battery: None,
            last_battery: Instant::now() - Duration::from_secs(60),
            applied_scale: 0.0,
            style_draft: cfg.style.css.clone(),
            last_frame_rect: None,
            quit_confirm: false,
            cfg,
        };
        sound::play(&app.cfg.sound, Cue::Startup);
        app
    }

    // ------------------------------------------------------------ helpers

    /// The deck reports whatever scale factor gamescope feels like, which can
    /// make a 1280x800 screen render at 1.5x or 2x and look enormous. Pin the
    /// deck to roughly one point per pixel and keep the rest of the platforms
    /// at their native density.
    pub fn apply_scale(&mut self, ctx: &egui::Context) {
        let native = ctx.native_pixels_per_point().unwrap_or(1.0).max(0.1);
        let limit = max_ui_scale();
        let ui = self.cfg.ui_scale.clamp(0.75, limit);
        let target = if deck_mode() { ui } else { native * ui };
        let zoom = (target / native).clamp(0.5, 3.0);
        if (self.applied_scale - zoom).abs() > 0.005 {
            self.applied_scale = zoom;
            ctx.set_zoom_factor(zoom);
        }
    }

    /// Turn the deck's own controls into a virtual mouse and page shortcuts.
    ///
    /// This runs in [`eframe::App::raw_input_hook`], before egui builds its
    /// input state. Pushing events from `ui()` is too late: pointer state for
    /// the frame has already been built, so a synthetic click would land one
    /// frame behind the cursor (or not at all).
    fn pad_ui(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let buttons = self.gamepad.buttons;
        let prev = self.pad_buttons;
        let pressed = |bit: u32| buttons & bit != 0 && prev & bit == 0;

        // While the pad is forwarded to a computer (or a bind is being
        // captured) the same buttons must not also drive this interface.
        if self.gamepad_on || self.binds.capture {
            self.pad_buttons = buttons;
            self.pad_release = false;
            return;
        }

        let rect = raw.screen_rect.unwrap_or_else(|| ctx.content_rect());
        let dt = raw.predicted_dt.clamp(0.001, 0.05);
        let mut moved = false;

        // left stick and d-pad move the virtual pointer
        let mut dx = deadzone(self.gamepad.lx, 0.18) * 980.0;
        let mut dy = -deadzone(self.gamepad.ly, 0.18) * 980.0;
        if buttons & sdw_core::udp::BTN_DPAD_LEFT != 0 {
            dx -= 760.0;
        }
        if buttons & sdw_core::udp::BTN_DPAD_RIGHT != 0 {
            dx += 760.0;
        }
        if buttons & sdw_core::udp::BTN_DPAD_UP != 0 {
            dy -= 760.0;
        }
        if buttons & sdw_core::udp::BTN_DPAD_DOWN != 0 {
            dy += 760.0;
        }
        if dx != 0.0 || dy != 0.0 {
            let pos =
                self.pad_cursor.unwrap_or_else(|| rect.center()) + Vec2::new(dx * dt, dy * dt);
            let pos = egui::pos2(
                pos.x.clamp(rect.left() + 2.0, rect.right() - 2.0),
                pos.y.clamp(rect.top() + 2.0, rect.bottom() - 2.0),
            );
            self.pad_cursor = Some(pos);
            self.pad_cursor_at = Instant::now();
            self.pad_seen = true;
            raw.events.push(egui::Event::PointerMoved(pos));
            moved = true;
        }

        // right stick scrolls, so long pages work without a touchscreen
        let sx = deadzone(self.gamepad.rx, 0.20) * 1100.0;
        let sy = deadzone(self.gamepad.ry, 0.20) * 1100.0;
        if sx != 0.0 || sy != 0.0 {
            raw.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: Vec2::new(-sx * dt, sy * dt),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: egui::Modifiers::default(),
            });
            self.pad_seen = true;
            moved = true;
        }

        // A clicks at the virtual pointer, with the release one frame later so
        // widgets see a real press/release pair.
        if pressed(sdw_core::udp::BTN_A) {
            if self.pad_cursor.is_none() {
                self.pad_cursor = Some(rect.center());
                raw.events.push(egui::Event::PointerMoved(rect.center()));
            }
            self.pad_seen = true;
            self.pad_cursor_at = Instant::now();
            let pos = self.pad_cursor.unwrap_or_else(|| rect.center());
            raw.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            });
            self.pad_release = true;
            moved = true;
        }
        if self.pad_release {
            let pos = self.pad_cursor.unwrap_or_else(|| rect.center());
            raw.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            });
            self.pad_release = false;
            moved = true;
        }

        if pressed(sdw_core::udp::BTN_B) {
            self.go_back(ctx);
        }
        if pressed(sdw_core::udp::BTN_LB) {
            self.nav_step(-1);
        }
        if pressed(sdw_core::udp::BTN_RB) {
            self.nav_step(1);
        }
        if pressed(sdw_core::udp::BTN_Y) {
            self.set_page(Page::Home);
        }
        if pressed(sdw_core::udp::BTN_BACK) {
            self.set_page(Page::Settings);
        }

        if moved {
            ctx.request_repaint();
        }
        self.pad_buttons = buttons;
    }

    /// Horizontal swipe starting at a screen edge. gamescope often hands the
    /// deck touchscreen to egui as a plain mouse, so egui never treats it as a
    /// touch device and drag-to-scroll stays off. This gives pages a gesture
    /// that does not depend on touch detection at all.
    fn edge_swipe_tick(&mut self, ctx: &egui::Context) {
        if !deck_mode() || self.gamepad_on || self.binds.capture {
            self.edge_swipe = None;
            return;
        }
        let (pressed, down, released, pos) = ctx.input(|i| {
            (
                i.pointer.any_pressed(),
                i.pointer.any_down(),
                i.pointer.any_released(),
                i.pointer.interact_pos(),
            )
        });
        let rect = ctx.content_rect();
        if pressed {
            if let Some(p) = pos {
                let edge = p.x <= rect.left() + 56.0 || p.x >= rect.right() - 56.0;
                self.edge_swipe = edge.then_some((p, p));
            }
        }
        if let Some((start, _)) = self.edge_swipe {
            if down {
                if let Some(p) = pos {
                    self.edge_swipe = Some((start, p));
                }
            } else if released {
                if let Some(p) = pos {
                    let dx = p.x - start.x;
                    let dy = p.y - start.y;
                    if dx.abs() > 140.0 && dx.abs() > dy.abs() * 1.4 {
                        self.nav_step(if dx < 0.0 { 1 } else { -1 });
                    }
                }
                self.edge_swipe = None;
            } else {
                self.edge_swipe = None;
            }
        }
    }

    fn go_back(&mut self, ctx: &egui::Context) {
        if self.stream.fullscreen {
            self.set_fullscreen(ctx, false);
        } else if self.osk_open {
            self.osk_open = false;
        } else if self.binds.editor.is_some() {
            self.binds.editor = None;
            self.binds.capture = false;
        } else if self.page != Page::Home {
            self.set_page(Page::Home);
        }
    }

    fn pad_cursor_ui(&self, ctx: &egui::Context) {
        let Some(pos) = self.pad_cursor else {
            return;
        };
        if self.gamepad_on || self.binds.capture {
            return;
        }
        if self.pad_cursor_at.elapsed() > Duration::from_secs(3) {
            return;
        }
        let fade = (1.0
            - (self.pad_cursor_at.elapsed().as_secs_f32() - 1.8).max(0.0) / 1.2)
            .clamp(0.0, 1.0);
        let t = Tokens::get(ctx);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            Id::new("pad-cursor"),
        ));
        let r = Rect::from_center_size(pos, Vec2::splat(20.0));
        painter.rect_filled(
            r,
            0.0,
            Color32::from_rgba_unmultiplied(0, 0, 0, (150.0 * fade) as u8),
        );
        painter.rect_stroke(
            r,
            0.0,
            Stroke::new(2.0, t.accent.gamma_multiply(fade.max(0.35))),
            egui::StrokeKind::Inside,
        );
        let arm = 5.0;
        painter.line_segment(
            [
                egui::pos2(pos.x - arm, pos.y),
                egui::pos2(pos.x + arm, pos.y),
            ],
            Stroke::new(1.5, Color32::WHITE.gamma_multiply(fade)),
        );
        painter.line_segment(
            [
                egui::pos2(pos.x, pos.y - arm),
                egui::pos2(pos.x, pos.y + arm),
            ],
            Stroke::new(1.5, Color32::WHITE.gamma_multiply(fade)),
        );
        if fade < 1.0 {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    pub fn save(&mut self) {
        self.cfg.deck_view = self.deck_view.cfg.clone();
        self.cfg.lang = self.lang.code().to_string();
        self.cfg.dark = self.dark;
        if let Err(e) = sdw_core::config::save(&self.cfg) {
            self.toast(format!("не удалось сохранить: {e}"), false);
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, ok: bool) {
        sound::play(&self.cfg.sound, if ok { Cue::Notify } else { Cue::Error });
        self.toast = Some(Toast {
            text: text.into(),
            ok,
            at: Instant::now(),
        });
    }

    pub fn active_host(&self) -> Option<&HostConfig> {
        let id = self.cfg.active_host.as_ref()?;
        self.cfg.hosts.iter().find(|h| &h.id == id)
    }

    pub fn active_host_name(&self) -> String {
        self.active_host()
            .map(|h| h.name.clone())
            .or_else(|| self.status.as_ref().map(|s| s.name.clone()))
            .unwrap_or_else(|| t!(self.lang, "нет компьютера", "no computer").to_string())
    }

    pub fn client(&self) -> Option<Client> {
        self.active_host()
            .map(|h| Client::new(&h.url, &h.token))
    }

    pub fn client_for(&self, host: Option<&str>) -> Option<Client> {
        if let Some(needle) = host {
            if !needle.trim().is_empty() {
                if let Some(h) = self
                    .cfg
                    .hosts
                    .iter()
                    .find(|h| h.id == needle || h.name.eq_ignore_ascii_case(needle))
                {
                    return Some(Client::new(&h.url, &h.token));
                }
            }
        }
        self.client()
    }

    pub fn monitor(&self) -> MonitorStatus {
        let mut m = self
            .status
            .as_ref()
            .map(|s| s.monitor.clone())
            .unwrap_or_default();
        if m.width == 0 {
            m.width = 1280;
        }
        if m.height == 0 {
            m.height = 800;
        }
        if m.fps == 0 {
            m.fps = 15;
        }
        if m.quality == 0 {
            m.quality = 75;
        }
        m
    }

    pub fn set_page(&mut self, page: Page) {
        if self.page != page {
            if let (Some(a), Some(b)) = (Page::ORDER.iter().position(|p| *p == self.page), Page::ORDER.iter().position(|p| *p == page)) {
                self.page_dir = if b > a { 1.0 } else { -1.0 };
            }
            self.page_at = Instant::now();
            self.page = page;
            self.page_epoch += 1;
            self.cfg.last_page = page.key().to_string();
            self.save();
            sound::play(
                &self.cfg.sound,
                if self.page_dir >= 0.0 { Cue::Navigate } else { Cue::Back },
            );
            if page == Page::Notifications {
                self.notif_unread = 0;
            }
            if page == Page::HomeAssistant {
                self.ha.last_fetch = Instant::now() - Duration::from_secs(60);
            }
        }
    }

    /// Entrance progress for the n-th element of a page: a soft stagger.
    pub fn reveal(&self, index: f32) -> f32 {
        if !theme::style().animations {
            return 1.0;
        }
        let dt = self.page_at.elapsed().as_secs_f32();
        let start = index * 0.035;
        let span = 0.30;
        ((dt - start) / span).clamp(0.0, 1.0)
    }

    /// Push the current `cfg.style` into the theme engine and repaint.
    pub fn apply_style(&mut self, ctx: &egui::Context) {
        theme::set_style(&self.cfg.style);
        theme::apply(ctx, self.dark);
        self.applied_dark = self.dark;
        self.save();
        ctx.request_repaint();
    }

    /// Whether a page should show up in the dock and the arrow walk.
    pub fn page_shown(&self, page: Page) -> bool {
        self.cfg.style.pages.is_empty()
            || self.cfg.style.pages.iter().any(|p| p == page.key())
    }

    /// Whether a home tile is visible.
    pub fn tile_shown(&self, key: &str) -> bool {
        self.cfg.style.home_tiles.is_empty()
            || self.cfg.style.home_tiles.iter().any(|k| k == key)
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        self.save();
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
        self.save();
    }

    pub fn spawn<F>(&self, f: F)
    where
        F: FnOnce() -> Task + Send + 'static,
    {
        let tx = self.task_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
    }

    // ------------------------------------------------------------ host switching and pairing

    pub fn set_active(&mut self, id: &str) {
        if !self.cfg.hosts.iter().any(|h| h.id == id) {
            return;
        }
        self.cfg.active_host = Some(id.to_string());
        self.save();
        self.status = None;
        self.status_err.clear();
        self.status_pending = false;
        self.last_status = Instant::now() - Duration::from_secs(5);
        self.notifications.clear();
        self.notif_last_id = 0;
        self.notif_unread = 0;
        self.notif_pending = false;
        self.last_notif = Instant::now() - Duration::from_secs(5);
        self.stream.feed = None;
        self.stream.texture = None;
        self.gamepad_on = false;
        self.input.set_enabled(false);
        self.input.set_target(None);
        self.gamepad_pad = self
            .cfg
            .hosts
            .iter()
            .find(|h| h.id == id)
            .and_then(|h| h.pad.clone());
        self.ha.entities.clear();
        self.ha.loaded = false;
        self.apply_target();
        let name = self
            .cfg
            .hosts
            .iter()
            .find(|h| h.id == id)
            .map(|h| h.name.clone())
            .unwrap_or_default();
        self.toast(format!("компьютер: {name}"), true);
    }

    pub fn switch_host(&mut self, delta: i32) {
        if self.cfg.hosts.len() < 2 {
            return;
        }
        let cur = self
            .cfg
            .active_host
            .as_ref()
            .and_then(|id| self.cfg.hosts.iter().position(|h| &h.id == id))
            .unwrap_or(0) as i32;
        let next = (cur + delta).rem_euclid(self.cfg.hosts.len() as i32) as usize;
        let id = self.cfg.hosts[next].id.clone();
        self.set_active(&id);
    }

    pub fn forget_host(&mut self, id: &str) {
        self.cfg.hosts.retain(|h| h.id != id);
        if self.cfg.active_host.as_deref() == Some(id) {
            self.cfg.active_host = self.cfg.hosts.first().map(|h| h.id.clone());
            self.status = None;
            self.gamepad_pad = None;
            self.gamepad_on = false;
            self.input.set_enabled(false);
            self.stream.feed = None;
        }
        self.save();
        if self.cfg.hosts.is_empty() {
            self.set_page(Page::Computers);
        }
    }

    pub fn start_pair(&mut self) {
        let url = self.pair_url.trim().to_string();
        if url.is_empty() {
            self.pair_error = t!(self.lang, "укажите адрес компьютера", "enter the computer address").into();
            return;
        }
        let pass = self.pair_pass.clone();
        if pass.is_empty() {
            self.pair_error = t!(self.lang, "введите пароль", "enter the password").into();
            return;
        }
        let name = if self.pair_name.trim().is_empty() {
            self.cfg.device_name.clone()
        } else {
            self.pair_name.trim().to_string()
        };
        let client_id = self.cfg.client_id.clone();
        self.pairing = true;
        self.pair_error.clear();
        self.spawn(move || {
            let result = api::ping(&url)
                .map_err(|e| e.to_string())
                .and_then(|_| api::pair(&url, &pass, &name, &client_id).map_err(|e| e.to_string()));
            Task::Paired { base: url, result }
        });
    }

    pub fn scan_lan(&mut self) {
        if self.discovering {
            return;
        }
        self.discovering = true;
        self.discovery.clear();
        self.spawn(|| Task::Discovery(sdw_core::discovery::scan(Duration::from_millis(1200))));
    }

    // ------------------------------------------------------------ gamepad

    pub fn apply_target(&mut self) {
        let Some(host) = self.active_host() else {
            self.input.set_target(None);
            return;
        };
        let Some(pad) = &self.gamepad_pad else {
            self.input.set_target(None);
            return;
        };
        let Some(addr) = udp_addr(&host.url) else {
            self.input.set_target(None);
            return;
        };
        let Ok(pad_id) = u32::from_str_radix(&pad.id, 16) else {
            self.input.set_target(None);
            return;
        };
        self.input.set_target(Some(UdpTarget {
            addr,
            pad_id,
            secret: pad.secret.clone(),
        }));
        self.input.set_enabled(self.gamepad_on);
    }

    pub fn set_gamepad_on(&mut self, on: bool) {
        if on {
            if self.gamepad_pad.is_some() {
                self.gamepad_on = true;
                self.input.set_enabled(true);
                self.apply_target();
                self.toast(
                    t!(self.lang, "геймпад передаётся на компьютер", "gamepad is live on the computer"),
                    true,
                );
            } else {
                self.create_pad();
            }
        } else {
            self.gamepad_on = false;
            self.input.set_enabled(false);
            self.toast(t!(self.lang, "геймпад выключен", "gamepad off"), true);
        }
    }

    pub fn create_pad(&mut self) {
        if self.gamepad_pending {
            return;
        }
        let Some(c) = self.client() else {
            self.toast(t!(self.lang, "сначала подключите компьютер", "connect a computer first"), false);
            return;
        };
        self.gamepad_pending = true;
        let kind = self.gamepad_kind;
        let name = if self.gamepad_name.trim().is_empty() {
            format!("{} · {}", self.cfg.device_name, kind.label())
        } else {
            self.gamepad_name.trim().to_string()
        };
        self.toast(t!(self.lang, "создаю виртуальный пад…", "creating the virtual pad…"), true);
        self.spawn(move || Task::Pad(c.create_pad(&name, kind).map_err(|e| e.to_string())));
    }

    pub fn remove_pad(&mut self) {
        let Some(pad) = self.gamepad_pad.clone() else {
            return;
        };
        let Some(c) = self.client() else {
            return;
        };
        self.gamepad_on = false;
        self.input.set_enabled(false);
        self.input.set_target(None);
        self.gamepad_pad = None;
        if let Some(host) = self
            .cfg
            .hosts
            .iter_mut()
            .find(|h| Some(&h.id) == self.cfg.active_host.as_ref())
        {
            host.pad = None;
        }
        self.save();
        self.spawn(move || {
            Task::Msg(
                c.delete_pad(&pad.id)
                    .map(|_| "пад удалён".into())
                    .map_err(|e| e.to_string()),
            )
        });
    }

    pub fn sync_pad_after_status(&mut self) {
        let Some(status) = &self.status else {
            return;
        };
        if let Some(pad) = self.gamepad_pad.clone() {
            if !status.pads.iter().any(|p| p.id == pad.id) {
                self.gamepad_pad = None;
                self.gamepad_on = false;
                self.input.set_enabled(false);
                self.input.set_target(None);
                if let Some(host) = self
                    .cfg
                    .hosts
                    .iter_mut()
                    .find(|h| Some(&h.id) == self.cfg.active_host.as_ref())
                {
                    host.pad = None;
                }
                self.save();
                self.toast(
                    t!(
                        self.lang,
                        "виртуальный пад исчез на компьютере — создайте новый",
                        "the virtual pad is gone on the computer — create a new one"
                    ),
                    false,
                );
            }
        }
    }

    // ------------------------------------------------------------ stream

    pub fn start_feed(&mut self) {
        let Some(c) = self.client() else {
            return;
        };
        if self.stream.feed.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let (tx, rx) = sync_channel(2);
        std::thread::Builder::new()
            .name("sdw-stream".into())
            .spawn(move || {
                while !stop2.load(Ordering::Relaxed) {
                    match c.monitor_frame() {
                        Ok(bytes) => {
                            let _ = tx.try_send(bytes);
                            std::thread::sleep(Duration::from_millis(24));
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(700)),
                    }
                }
            })
            .ok();
        self.stream.feed = Some(StreamFeed { stop, rx });
        self.stream.error.clear();
        self.stream.last_frame_at = Instant::now();
    }

    pub fn ensure_stream(&mut self) {
        let running = self.monitor().running;
        let want = self.page == Page::Screen && running;
        if want && self.stream.feed.is_none() {
            self.start_feed();
        }
        if !want && self.stream.feed.is_some() {
            self.stream.feed = None;
        }
        if self.stream.feed.is_none() {
            self.stream.texture = None;
        }
    }

    pub fn pump_frames(&mut self, ctx: &egui::Context) {
        let Some(feed) = &self.stream.feed else {
            return;
        };
        let mut latest = None;
        while let Ok(bytes) = feed.rx.try_recv() {
            latest = Some(bytes);
        }
        let Some(bytes) = latest else {
            return;
        };
        let Ok(img) = image::load_from_memory(&bytes) else {
            self.stream.error = "не удалось декодировать кадр".into();
            return;
        };
        let size = [img.width() as usize, img.height() as usize];
        let rgba = img.to_rgba8();
        let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        match &mut self.stream.texture {
            Some(tex) => tex.set(color, egui::TextureOptions::LINEAR),
            None => {
                self.stream.texture =
                    Some(ctx.load_texture("screen-stream", color, egui::TextureOptions::LINEAR))
            }
        }
        self.stream.frames += 1;
        self.stream.last_frame_at = Instant::now();
        self.stream.error.clear();
    }

    pub fn send_pointer(&mut self, action: &str, pos: egui::Pos2) {
        let Some(rect) = self.last_frame_rect else {
            return;
        };
        if rect.width() <= 1.0 || rect.height() <= 1.0 {
            return;
        }
        let x = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let y = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        if action == "move" {
            if self.stream.last_pointer.elapsed() < Duration::from_millis(45) {
                return;
            }
            self.stream.last_pointer = Instant::now();
        }
        let Some(c) = self.client() else {
            return;
        };
        let input = MonitorInput {
            x,
            y,
            action: action.to_string(),
            amount: 0,
        };
        self.spawn(move || {
            Task::Msg(
                c.monitor_input(&input)
                    .map(|_| String::new())
                    .map_err(|e| e.to_string()),
            )
        });
    }

    pub fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        self.stream.fullscreen = on;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
        ctx.request_repaint();
    }

    // ------------------------------------------------------------ notifications, ha

    pub fn poll_status(&mut self) {
        if self.status_pending || self.client().is_none() {
            return;
        }
        if self.last_status.elapsed() < Duration::from_millis(1000) {
            return;
        }
        self.last_status = Instant::now();
        self.status_pending = true;
        if let Some(c) = self.client() {
            self.spawn(move || Task::Status(c.status().map_err(|e| e.to_string())));
        }
    }

    pub fn poll_notifications(&mut self) {
        if self.notif_pending || self.client().is_none() {
            return;
        }
        if self.last_notif.elapsed() < Duration::from_millis(1800) {
            return;
        }
        self.last_notif = Instant::now();
        self.notif_pending = true;
        let since = self.notif_last_id;
        if let Some(c) = self.client() {
            self.spawn(move || Task::Notifications(c.notifications(since).map_err(|e| e.to_string())));
        }
    }

    pub fn load_ha(&mut self) {
        if self.ha.pending || self.client().is_none() {
            return;
        }
        self.ha.pending = true;
        self.ha.last_fetch = Instant::now();
        if let Some(c) = self.client() {
            self.spawn(move || Task::Ha(c.ha_states().map_err(|e| e.to_string())));
        }
    }

    /// One-tap start/stop for the extra screen, used on the home page.
    pub fn toggle_monitor_default(&mut self) {
        let Some(c) = self.client() else {
            self.toast(
                t!(self.lang, "сначала подключите компьютер", "connect a computer first"),
                false,
            );
            return;
        };
        let monitor = self.monitor();
        if monitor.running {
            self.spawn(move || Task::MonitorStopped(c.stop_monitor().map_err(|e| e.to_string())));
            return;
        }
        let req = MonitorStart {
            mode: self.stream.mode,
            backend: self.stream.backend,
            position: self.stream.position,
            output: monitor.output.clone(),
            width: self.stream.width,
            height: self.stream.height,
            fps: self.stream.fps,
            quality: self.stream.quality,
            input_enabled: self.stream.input,
            sudo_password: String::new(),
        };
        self.spawn(move || Task::Monitor(c.start_monitor(&req).map_err(|e| e.to_string())));
    }

    pub fn check_updates(&mut self) {
        if self.update.checking {
            return;
        }
        self.update.checking = true;
        self.update.error.clear();
        self.spawn(|| Task::Update(update::check()));
    }

    pub fn download_update(&mut self) {
        if self.update.downloading {
            return;
        }
        let Some(info) = self.update.info.clone() else {
            return;
        };
        let Some(asset) = info.asset_for("sdw-deck").cloned() else {
            self.toast(
                t!(self.lang, "в релизе нет файла для дека", "no deck file in the release"),
                false,
            );
            return;
        };
        self.update.downloading = true;
        self.update.total = asset.size;
        self.update.progress = Arc::new(AtomicU64::new(0));
        let progress = self.update.progress.clone();
        self.spawn(move || {
            let dest = update::updates_dir().join(&asset.name);
            match update::download(&asset, &dest, progress) {
                Ok(n) => Task::Updated(Ok((dest.display().to_string(), n))),
                Err(e) => Task::Updated(Err(e)),
            }
        });
    }

    fn maybe_check_updates(&mut self) {
        if self.update.last_check.elapsed() > Duration::from_secs(6 * 3600) {
            self.update.last_check = Instant::now();
            self.check_updates();
        }
    }

    pub fn ha_rooms_count(&self) -> usize {
        let mut areas: Vec<&str> = self
            .ha
            .areas
            .iter()
            .map(String::as_str)
            .chain(
                self.ha
                    .entities
                    .iter()
                    .map(|e| e.area.as_str())
                    .filter(|a| !a.is_empty()),
            )
            .collect();
        areas.sort_unstable();
        areas.dedup();
        let mut n = areas.len();
        if self
            .status
            .as_ref()
            .map(|s| !s.ha.favorites.is_empty())
            .unwrap_or(false)
        {
            n += 1;
        }
        if self.ha.entities.iter().any(|e| e.area.is_empty()) {
            n += 1;
        }
        n.max(1)
    }

    pub fn ha_call(&mut self, call: HaCall) {
        let Some(c) = self.client() else {
            return;
        };
        self.spawn(move || {
            Task::Msg(
                c.ha_call(&call)
                    .map(|_| String::new())
                    .map_err(|e| e.to_string()),
            )
        });
    }

    // ------------------------------------------------------------ actions

    pub fn execute_bind(&mut self, bind: &Bind) {
        let action = bind.action.clone();
        self.execute_action(&action);
    }

    pub fn execute_action(&mut self, action: &Action) {
        match action {
            Action::None => {}
            Action::Page { page } => self.set_page(Page::from_key(page)),
            Action::Remote { host, command } => {
                let Some(c) = self.client_for(host.as_deref()) else {
                    self.toast(t!(self.lang, "нет компьютера", "no computer"), false);
                    return;
                };
                let command = command.clone();
                self.spawn(move || {
                    Task::Msg(
                        c.remote(&command)
                            .map(|_| format!("пк: {command}"))
                            .map_err(|e| e.to_string()),
                    )
                });
            }
            Action::Ha {
                host,
                domain,
                service,
                entity,
                data,
            } => {
                let Some(c) = self.client_for(host.as_deref()) else {
                    self.toast(t!(self.lang, "нет компьютера", "no computer"), false);
                    return;
                };
                let call = HaCall {
                    domain: domain.clone(),
                    service: service.clone(),
                    target: if entity.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::json!({ "entity_id": entity })
                    },
                    data: data.clone(),
                };
                self.spawn(move || {
                    Task::Msg(
                        c.ha_call(&call)
                            .map(|_| "home assistant".into())
                            .map_err(|e| e.to_string()),
                    )
                });
            }
            other => match crate::actions::run(other) {
                Ok(message) => {
                    if !message.is_empty() {
                        self.toast(message, true);
                    }
                }
                Err(e) => self.toast(e, false),
            },
        }
    }

    // ------------------------------------------------------------ input events

    fn on_input(&mut self, event: DeckEvent) {
        match event {
            DeckEvent::Pad(state) => self.gamepad = state,
            DeckEvent::Devices(list) => self.input_devices = list,
            DeckEvent::Error(e) => {
                if !self.input_errors.contains(&e) {
                    self.input_errors.push(e);
                    if self.input_errors.len() > 4 {
                        self.input_errors.remove(0);
                    }
                }
            }
            DeckEvent::Gesture(spec) => {
                self.last_gesture = Some((spec.clone(), Instant::now()));
                if self.binds.capture {
                    binds::capture_gesture(self, &spec);
                    return;
                }
                let id = spec.id();
                let hit = self
                    .cfg
                    .binds
                    .iter()
                    .find(|b| {
                        b.enabled
                            && matches!(&b.trigger, Trigger::Gesture { spec: s } if s.id() == id)
                    })
                    .cloned();
                if let Some(bind) = hit {
                    self.execute_bind(&bind);
                    return;
                }
                self.global_gesture(&spec);
            }
            DeckEvent::Button {
                code,
                label,
                device,
                pressed,
            } => {
                if pressed {
                    let fresh = self.pressed.insert(code);
                    self.last_button = Some((code, label.clone(), Instant::now()));
                    if fresh {
                        if self.binds.capture {
                            binds::capture_button(self, code, &label, &device);
                            return;
                        }
                        self.run_button_binds(code);
                    }
                } else {
                    self.pressed.remove(&code);
                }
            }
        }
    }

    fn run_button_binds(&mut self, code: u16) {
        let code_s = code.to_string();
        let candidates: Vec<Bind> = self
            .cfg
            .binds
            .iter()
            .filter(|b| {
                if !b.enabled {
                    return false;
                }
                match &b.trigger {
                    Trigger::Button { code: c, .. } => c == &code_s,
                    Trigger::Combo { codes, .. } => {
                        codes.len() >= 2
                            && codes.iter().all(|c| {
                                c.parse::<u16>()
                                    .map(|n| self.pressed.contains(&n))
                                    .unwrap_or(false)
                            })
                            && codes.iter().any(|c| c == &code_s)
                    }
                    _ => false,
                }
            })
            .cloned()
            .collect();
        for bind in candidates {
            self.execute_bind(&bind);
        }
    }

    fn global_gesture(&mut self, spec: &GestureSpec) {
        if !self.cfg.global_gestures || spec.kind != GestureKind::Swipe {
            return;
        }
        let dir = spec.dir.unwrap_or(SwipeDir::Left);
        match (spec.fingers, dir) {
            (3, SwipeDir::Left) => self.nav_step(1),
            (3, SwipeDir::Right) => self.nav_step(-1),
            (3, SwipeDir::Up) | (4, SwipeDir::Up) => self.set_page(Page::Home),
            (3, SwipeDir::Down) | (4, SwipeDir::Down) => self.set_page(Page::Notifications),
            (4, SwipeDir::Left) => self.switch_host(1),
            (4, SwipeDir::Right) => self.switch_host(-1),
            _ => {}
        }
    }

    fn nav_step(&mut self, delta: i32) {
        let pages: Vec<Page> = Page::ORDER
            .iter()
            .copied()
            .filter(|p| self.page_shown(*p))
            .collect();
        if pages.is_empty() {
            return;
        }
        let idx = pages
            .iter()
            .position(|p| *p == self.page)
            .unwrap_or(0) as i32;
        let next = (idx + delta).rem_euclid(pages.len() as i32) as usize;
        self.set_page(pages[next]);
    }

    // ------------------------------------------------------------ frame

    fn tick(&mut self, ctx: &egui::Context) {
        if self.dark != self.applied_dark {
            theme::apply(ctx, self.dark);
            self.applied_dark = self.dark;
        }
        self.apply_scale(ctx);
        if self.last_clock.elapsed() >= Duration::from_secs(1) {
            self.last_clock = Instant::now();
            self.clock = theme::clock_text();
        }
        if self.last_battery.elapsed() >= Duration::from_secs(30) {
            self.last_battery = Instant::now();
            self.battery = read_battery();
        }

        while let Ok(task) = self.task_rx.try_recv() {
            self.on_task(task);
        }
        let mut events = Vec::new();
        while let Ok(event) = self.input.evt_rx.try_recv() {
            events.push(event);
            if events.len() >= 512 {
                break;
            }
        }
        for event in events {
            self.on_input(event);
        }

        self.poll_status();
        self.poll_notifications();
        if self.page == Page::HomeAssistant && self.ha.last_fetch.elapsed() > Duration::from_secs(8) {
            self.load_ha();
        }
        self.ensure_stream();
        self.pump_frames(ctx);

        if self.toast.as_ref().is_some_and(|t| t.at.elapsed() > Duration::from_secs(4)) {
            self.toast = None;
        }

        self.edge_swipe_tick(ctx);

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.go_back(ctx);
        }
    }

    fn on_task(&mut self, task: Task) {
        match task {
            Task::Status(Ok(status)) => {
                self.status_pending = false;
                self.status_err.clear();
                if let Some(host) = self
                    .cfg
                    .hosts
                    .iter_mut()
                    .find(|h| Some(&h.id) == self.cfg.active_host.as_ref())
                {
                    host.last_seen_ms = now_ms();
                    host.name = status.name.clone();
                }
                self.status = Some(status);
                self.sync_pad_after_status();
            }
            Task::Status(Err(e)) => {
                self.status_pending = false;
                self.status_err = e;
            }
            Task::Paired { base, result } => {
                self.pairing = false;
                match result {
                    Ok(pair) => {
                        let host = HostConfig {
                            id: pair.daemon_id.clone(),
                            name: pair.daemon_name.clone(),
                            url: api::normalize_base(&base),
                            token: pair.token,
                            last_seen_ms: now_ms(),
                            auto_connect: true,
                            pad: None,
                        };
                        if let Some(existing) = self
                            .cfg
                            .hosts
                            .iter_mut()
                            .find(|h| h.id == pair.daemon_id)
                        {
                            let pad = existing.pad.clone();
                            *existing = HostConfig { pad, ..host };
                        } else {
                            self.cfg.hosts.push(host);
                        }
                        self.cfg.active_host = Some(pair.daemon_id.clone());
                        self.save();
                        self.pair_pass.clear();
                        self.pair_error.clear();
                        self.add_open = false;
                        self.page = Page::Home;
                        self.page_epoch += 1;
                        self.status = None;
                        self.last_status = Instant::now() - Duration::from_secs(5);
                        self.toast(
                            format!(
                                "{} {}",
                                t!(self.lang, "подключено к", "connected to"),
                                pair.daemon_name
                            ),
                            true,
                        );
                    }
                    Err(e) => {
                        self.pair_error = e.clone();
                        self.toast(e, false);
                    }
                }
            }
            Task::Discovery(Ok(list)) => {
                self.discovering = false;
                if list.is_empty() {
                    self.pair_error = t!(
                        self.lang,
                        "компьютер не найден. введите адрес вручную.",
                        "no computer found. enter the address manually."
                    )
                    .into();
                }
                self.discovery = list;
            }
            Task::Discovery(Err(e)) => {
                self.discovering = false;
                self.pair_error = e;
            }
            Task::Pad(Ok(created)) => {
                self.gamepad_pending = false;
                let pad = DeckPad {
                    id: created.pad.id.clone(),
                    name: created.pad.name.clone(),
                    kind: created.pad.kind,
                    secret: created.udp_secret.clone(),
                    created_ms: now_ms(),
                };
                if let Some(host) = self
                    .cfg
                    .hosts
                    .iter_mut()
                    .find(|h| Some(&h.id) == self.cfg.active_host.as_ref())
                {
                    host.pad = Some(pad.clone());
                }
                self.gamepad_pad = Some(pad);
                self.gamepad_on = true;
                self.save();
                self.apply_target();
                self.toast(
                    format!(
                        "{} «{}»",
                        t!(self.lang, "пад создан:", "pad created:"),
                        created.pad.name
                    ),
                    true,
                );
            }
            Task::Pad(Err(e)) => {
                self.gamepad_pending = false;
                self.gamepad_on = false;
                self.toast(e, false);
            }
            Task::Monitor(Ok(status)) => {
                if status.running && !status.note.is_empty() {
                    self.toast(status.note.clone(), true);
                } else if status.running {
                    self.toast(
                        format!(
                            "{} · {}",
                            t!(self.lang, "экран запущен", "screen started"),
                            status.backend_label
                        ),
                        true,
                    );
                    self.last_status = Instant::now() - Duration::from_secs(5);
                } else {
                    self.toast(t!(self.lang, "экран остановлен", "screen stopped"), true);
                }
                if let Some(s) = &mut self.status {
                    s.monitor = status;
                }
            }
            Task::Monitor(Err(e)) => self.toast(e, false),
            Task::MonitorStopped(Ok(status)) => {
                self.stream.feed = None;
                self.stream.texture = None;
                if let Some(s) = &mut self.status {
                    s.monitor = status;
                }
                self.toast(t!(self.lang, "экран остановлен", "screen stopped"), true);
            }
            Task::MonitorStopped(Err(e)) => self.toast(e, false),
            Task::Update(Ok(info)) => {
                self.update.checking = false;
                self.update.error.clear();
                if info.newer && !self.update.notified {
                    self.update.notified = true;
                    self.toast(
                        format!(
                            "{} {} · {} {}",
                            t!(self.lang, "доступно обновление", "update available"),
                            info.latest,
                            t!(self.lang, "сейчас", "current"),
                            info.current
                        ),
                        true,
                    );
                }
                self.update.info = Some(info);
            }
            Task::Update(Err(e)) => {
                self.update.checking = false;
                self.update.error = e;
            }
            Task::Updated(Ok((path, size))) => {
                self.update.downloading = false;
                self.update.total = size;
                self.update.path = path;
                self.toast(
                    t!(self.lang, "обновление скачано", "update downloaded"),
                    true,
                );
            }
            Task::Updated(Err(e)) => {
                self.update.downloading = false;
                self.toast(e, false);
            }
            Task::Installed(Ok(msg)) => self.toast(msg, true),
            Task::Installed(Err(e)) => self.toast(e, false),
            Task::Notifications(Ok(list)) => {
                self.notif_pending = false;
                let mut added = 0;
                for n in list {
                    if !self.notifications.iter().any(|x| x.id == n.id) {
                        self.notif_last_id = self.notif_last_id.max(n.id);
                        self.notifications.push(n);
                        added += 1;
                    }
                }
                self.notifications.sort_by_key(|n| n.id);
                if self.notifications.len() > 80 {
                    let cut = self.notifications.len() - 80;
                    self.notifications.drain(..cut);
                }
                if self.page == Page::Notifications {
                    self.notif_unread = 0;
                } else {
                    self.notif_unread = self.notif_unread.saturating_add(added);
                    if added > 0 {
                        sound::play(&self.cfg.sound, Cue::Notify);
                    }
                }
            }
            Task::Notifications(Err(_)) => {
                self.notif_pending = false;
            }
            Task::Ha(Ok(states)) => {
                self.ha.pending = false;
                self.ha.loaded = true;
                self.ha.error.clear();
                self.ha.entities = states.entities;
                self.ha.areas = states.areas;
            }
            Task::Ha(Err(e)) => {
                self.ha.pending = false;
                self.ha.error = e;
            }
            Task::Msg(Ok(message)) => {
                if !message.is_empty() {
                    self.toast(message, true);
                }
                self.last_status = Instant::now() - Duration::from_secs(5);
            }
            Task::Msg(Err(e)) => self.toast(e, false),
        }
    }

    // ------------------------------------------------------------ chrome

    fn chrome_ui(&mut self, root: &mut Ui, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        // One cubic ease for the whole page swap: it fades, rises a touch and
        // slides in from the side you came from. Margins move with it so the
        // content never spills outside the panel.
        let anim = if theme::style().animations {
            ctx.animate_bool_with_time(Id::new(("page", self.page_epoch)), true, 0.22)
        } else {
            1.0
        };
        let ease = 1.0 - (1.0 - anim).powi(3);
        let slide = (1.0 - ease) * 26.0 * self.page_dir;
        let left = (34.0 + slide).max(4.0) as i8;
        let right = (34.0 - slide).max(4.0) as i8;
        egui::Panel::top("deck-top")
            .exact_size(50.0)
            .frame(
                egui::Frame::new()
                    .fill(t.bg)
                    .inner_margin(Margin::symmetric(22, 4)),
            )
            .show(root, |ui| self.top_bar(ui, &t));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new().fill(t.bg).inner_margin(Margin {
                    left,
                    right,
                    top: 4,
                    bottom: 4,
                }),
            )
            .show(root, |ui| {
                theme::backdrop(ui, &t);
                ui.set_opacity(ease);
                ui.add_space((1.0 - ease) * 8.0);
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .scroll_source(egui::scroll_area::ScrollSource {
                        scroll_bar: false,
                        // gamescope often reports the deck touchscreen as a
                        // mouse, so egui's OnTouch default never turns on.
                        drag: egui::scroll_area::DragScroll::Always,
                        mouse_wheel: true,
                    })
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if self.binds.editor.is_some() {
                            binds::editor_ui(self, ui, &t);
                        } else {
                            self.page_ui(ui, &t);
                        }
                        ui.add_space(104.0);
                    });
                if ease < 1.0 {
                    ctx.request_repaint();
                }
            });

        // Explicit arrows on both sides: the swipes stay, but nothing is hidden.
        let show_arrows = theme::style().arrows;
        let editing = self.binds.editor.is_some();
        let mut action: Option<(i32, bool)> = None;
        if show_arrows {
            egui::Area::new(Id::new("deck-arrow-left"))
                .anchor(egui::Align2::LEFT_CENTER, egui::vec2(4.0, 0.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    if theme::nav_arrow(ui, &t, theme::icon::BACK, true)
                        .on_hover_text(t!(self.lang, "назад", "previous"))
                        .clicked()
                    {
                        action = Some((-1, true));
                    }
                });
            egui::Area::new(Id::new("deck-arrow-right"))
                .anchor(egui::Align2::RIGHT_CENTER, egui::vec2(-4.0, 0.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    if theme::nav_arrow(ui, &t, theme::icon::FORWARD, true)
                        .on_hover_text(t!(self.lang, "далее", "next"))
                        .clicked()
                    {
                        action = Some((1, false));
                    }
                });
        }
        if let Some((delta, may_close)) = action {
            if editing {
                if may_close {
                    self.binds.editor = None;
                    self.binds.capture = false;
                }
            } else {
                self.nav_step(delta);
            }
        }

        self.floating_nav(ctx, &t);
    }

    fn top_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        ui.horizontal_centered(|ui| {
            ui.label(
                RichText::new("sd-widgets")
                    .family(FontFamily::Monospace)
                    .size(13.0)
                    .extra_letter_spacing(1.4)
                    .color(t.faint),
            );
            ui.add_space(8.0);
            let online = self.status.is_some();
            theme::dot(ui, if online { t.good } else { t.faint }, online);
            let host = self.active_host_name();
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(sdw_core::util::truncate(&host, 26))
                            .size(14.0)
                            .color(if online { t.strong } else { t.muted }),
                    )
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius::ZERO),
                )
                .on_hover_text(t!(self.lang, "сменить компьютер", "switch computer"))
                .clicked()
            {
                self.set_page(Page::Computers);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for (at, glyph, hint, active) in [
                    (
                        0,
                        theme::icon::SETTINGS,
                        t!(self.lang, "настройки", "settings"),
                        self.page == Page::Settings,
                    ),
                    (
                        1,
                        theme::icon::BINDS,
                        t!(self.lang, "клавиатура", "keyboard"),
                        self.osk_open,
                    ),
                    (
                        2,
                        if self.dark {
                            theme::icon::LIGHT
                        } else {
                            theme::icon::DARK
                        },
                        t!(self.lang, "тема", "theme"),
                        false,
                    ),
                ] {
                    if ui
                        .add(
                            egui::Button::new(theme::icon(
                                glyph,
                                16.0,
                                if active { t.accent } else { t.faint },
                            ))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .corner_radius(CornerRadius::ZERO),
                        )
                        .on_hover_text(hint)
                        .clicked()
                    {
                        match at {
                            0 => self.set_page(Page::Settings),
                            1 => self.osk_open = !self.osk_open,
                            _ => self.set_dark(!self.dark),
                        }
                    }
                }
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(self.lang.code().to_uppercase())
                                .family(FontFamily::Monospace)
                                .size(11.0)
                                .color(t.faint),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::ZERO),
                    )
                    .on_hover_text(t!(self.lang, "язык", "language"))
                    .clicked()
                {
                    self.set_lang(self.lang.toggle());
                }
                if let Some(battery) = self.battery {
                    ui.label(
                        RichText::new(format!("{battery}%"))
                            .family(FontFamily::Monospace)
                            .size(11.5)
                            .color(if battery < 20 { t.warn } else { t.faint }),
                    );
                }
                if self.cfg.show_clock {
                    ui.label(
                        RichText::new(&self.clock)
                            .family(FontFamily::Monospace)
                            .size(12.5)
                            .color(t.faint),
                    );
                }
            });
        });
    }

    /// Minimal floating dock with one glyph per page.
    fn floating_nav(&mut self, ctx: &egui::Context, t: &Tokens) {
        let st = theme::style();
        if !st.dock {
            return;
        }
        let mut go: Option<Page> = None;
        let lang = self.lang;
        let current = self.page;
        let unread = self.notif_unread;
        egui::Area::new(Id::new("deck-dock"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -8.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let dock_fill = if t.dark {
                    Color32::from_rgba_premultiplied(0x12, 0x12, 0x1b, 232)
                } else {
                    Color32::from_rgba_premultiplied(0xff, 0xff, 0xff, 240)
                };
                egui::Frame::new()
                    .fill(dock_fill)
                    .stroke(Stroke::new(1.0, t.line))
                    .corner_radius(CornerRadius::ZERO)
                    .inner_margin(Margin::symmetric(14, 6))
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            if st.labels {
                                ui.label(theme::hud(current.label(lang), t.faint));
                            }
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 1.0;
                                for page in Page::ORDER.into_iter().filter(|p| self.page_shown(*p)) {
                                    let active = current == page;
                                    let resp = ui
                                        .add(
                                            egui::Button::new(
                                                theme::icon(
                                                    page.icon(),
                                                    if active { 18.0 } else { 17.0 },
                                                    if active { t.strong } else { t.muted },
                                                ),
                                            )
                                            .fill(if active {
                                                t.accent_soft
                                            } else {
                                                Color32::TRANSPARENT
                                            })
                                            .stroke(if active {
                                                Stroke::new(1.0, t.accent.gamma_multiply(0.45))
                                            } else {
                                                Stroke::NONE
                                            })
                                            .corner_radius(CornerRadius::ZERO)
                                            .min_size(Vec2::new(36.0, 32.0)),
                                        )
                                        .on_hover_text(page.label(lang));
                                    if page == Page::Notifications && unread > 0 {
                                        ui.painter().circle_filled(
                                            resp.rect.right_top() + Vec2::new(-5.0, 6.0),
                                            3.0,
                                            t.warn,
                                        );
                                    }
                                    if page == Page::Gamepad && self.gamepad_on {
                                        ui.painter().circle_filled(
                                            resp.rect.right_top() + Vec2::new(-5.0, 6.0),
                                            3.0,
                                            t.good,
                                        );
                                    }
                                    if resp.clicked() {
                                        go = Some(page);
                                    }
                                }
                            });
                        });
                    });
            });
        if let Some(page) = go {
            self.set_page(page);
        }
    }

    fn page_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        match self.page {
            Page::Home => crate::pages::home(self, ui, t),
            Page::Computers => crate::pages::computers(self, ui, t),
            Page::Gamepad => crate::pages::gamepad(self, ui, t),
            Page::Screen => crate::pages::screen(self, ui, t),
            Page::Notifications => crate::pages::notifications(self, ui, t),
            Page::HomeAssistant => crate::pages::home_assistant(self, ui, t),
            Page::Binds => binds::list_ui(self, ui, t),
            Page::Settings => crate::pages::settings(self, ui, t),
            Page::Theme => crate::pages::appearance(self, ui, t),
        }
    }

    fn fullscreen_ui(&mut self, root: &mut Ui, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::BLACK))
            .show(root, |ui| {
                let rect = ui.available_rect_before_wrap();
                ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
                self.draw_stream(ui, &t, rect, true);
                egui::Area::new(Id::new("fullscreen-close"))
                    .anchor(Align2::RIGHT_TOP, Vec2::new(-18.0, 18.0))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        if ui
                            .add(
                                egui::Button::new(theme::icon(theme::icon::CLOSE, 22.0, Color32::WHITE))
                                .fill(Color32::from_black_alpha(140))
                                .stroke(Stroke::new(1.0, Color32::from_white_alpha(60)))
                                .corner_radius(CornerRadius::ZERO),
                            )
                            .clicked()
                        {
                            self.set_fullscreen(ctx, false);
                        }
                    });
                egui::Area::new(Id::new("fullscreen-hint"))
                    .anchor(Align2::LEFT_BOTTOM, Vec2::new(18.0, -18.0))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} · {}×{} · {} fps",
                                self.active_host_name(),
                                self.monitor().width,
                                self.monitor().height,
                                self.monitor().fps
                            ))
                            .family(FontFamily::Monospace)
                            .size(12.0)
                            .color(Color32::from_white_alpha(150)),
                        );
                    });
            });
    }

    pub fn draw_stream(&mut self, ui: &mut Ui, t: &Tokens, area: Rect, fullscreen: bool) {
        let monitor = self.monitor();
        let tex = self.stream.texture.clone();
        let Some(tex) = tex else {
            ui.painter().text(
                area.center(),
                Align2::CENTER_CENTER,
                if self.stream.feed.is_some() {
                    t!(self.lang, "ждём первый кадр…", "waiting for the first frame…")
                } else {
                    t!(self.lang, "экран не запущен", "screen is not running")
                },
                egui::FontId::proportional(18.0),
                t.muted,
            );
            return;
        };
        let aspect = monitor.width as f32 / monitor.height.max(1) as f32;
        let mut size = Vec2::new(area.width(), area.width() / aspect);
        if size.y > area.height() {
            size.y = area.height();
            size.x = area.height() * aspect;
        }
        let rect = Rect::from_center_size(area.center(), size);
        self.last_frame_rect = Some(rect);
        ui.painter().image(
            tex.id(),
            rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        if !fullscreen {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, t.line),
                egui::StrokeKind::Inside,
            );
        }
        let resp = ui.interact(
            rect,
            ui.id().with("stream-surface"),
            Sense::click_and_drag(),
        );
        if monitor.input_enabled {
            if resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    self.send_pointer("click", pos);
                }
            }
            if resp.drag_started() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    self.stream.pointer_down = true;
                    self.send_pointer("down", pos);
                }
            }
            if resp.dragged() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    self.send_pointer("move", pos);
                }
            }
            if resp.drag_stopped() && self.stream.pointer_down {
                self.stream.pointer_down = false;
                if let Some(pos) = resp.interact_pointer_pos() {
                    self.send_pointer("up", pos);
                }
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
        }
    }

    fn osk_ui(&mut self, ctx: &egui::Context) {
        if !self.osk_open {
            return;
        }
        let t = Tokens::get(ctx);
        egui::Area::new(Id::new("osk"))
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -92.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(t.bg2)
                    .stroke(Stroke::new(1.0, t.line_strong))
                    .corner_radius(CornerRadius::ZERO)
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_max_width(760.0);
                        ui.horizontal(|ui| {
                            ui.label(theme::hud(
                                t!(self.lang, "клавиатура", "keyboard"),
                                t.faint,
                            ));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if theme::ghost(ui, t!(self.lang, "готово", "done"), &t).clicked() {
                                    self.osk_open = false;
                                }
                            });
                        });
                        ui.add_space(4.0);
                        let rows: [[&str; 10]; 3] = if self.osk_symbols {
                            [
                                ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
                                ["@", "#", "$", "%", "&", "*", "-", "_", "=", "+"],
                                [".", ",", ":", ";", "'", "\"", "/", "\\", "?", "!"],
                            ]
                        } else {
                            [
                                ["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"],
                                ["a", "s", "d", "f", "g", "h", "j", "k", "l", "—"],
                                ["z", "x", "c", "v", "b", "n", "m", ".", "@", theme::icon::BACKSPACE],
                            ]
                        };
                        for (ri, row) in rows.iter().enumerate() {
                            ui.horizontal(|ui| {
                                if ri == 2 {
                                    if theme::key_button(
                                        ui,
                                        theme::icon::SHIFT,
                                        &t,
                                        58.0,
                                    )
                                    .clicked()
                                    {
                                        self.osk_shift = !self.osk_shift;
                                    }
                                }
                                for key in row {
                                    let label = if key.len() == 1
                                        && self.osk_shift
                                        && !self.osk_symbols
                                        && key.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                                    {
                                        key.to_uppercase()
                                    } else {
                                        (*key).to_string()
                                    };
                                    if theme::key_button(ui, &label, &t, 54.0).clicked() {
                                        if *key == theme::icon::BACKSPACE {
                                            push_key(ctx, egui::Key::Backspace);
                                        } else if *key == "—" {
                                            push_text(ctx, " ");
                                        } else if *key == " " {
                                            push_text(ctx, " ");
                                        } else {
                                            push_text(ctx, &label);
                                        }
                                    }
                                }
                                if ri == 2 {
                                    if theme::key_button(ui, theme::icon::RETURN, &t, 58.0).clicked() {
                                        push_key(ctx, egui::Key::Enter);
                                    }
                                }
                            });
                            ui.add_space(4.0);
                        }
                        ui.horizontal(|ui| {
                            if theme::key_button(
                                ui,
                                if self.osk_symbols { "abc" } else { "123" },
                                &t,
                                78.0,
                            )
                            .clicked()
                            {
                                self.osk_symbols = !self.osk_symbols;
                            }
                            if theme::key_button(
                                ui,
                                t!(self.lang, "пробел", "space"),
                                &t,
                                300.0,
                            )
                            .clicked()
                            {
                                push_text(ctx, " ");
                            }
                            if theme::key_button(ui, "/", &t, 54.0).clicked() {
                                push_text(ctx, "/");
                            }
                            if theme::key_button(ui, "-", &t, 54.0).clicked() {
                                push_text(ctx, "-");
                            }
                            if theme::key_button(ui, ":", &t, 54.0).clicked() {
                                push_text(ctx, ":");
                            }
                        });
                        ui.add_space(2.0);
                        ui.label(theme::hud(
                            t!(
                                self.lang,
                                "сначала коснитесь поля ввода",
                                "tap a text field first"
                            ),
                            t.faint,
                        ));
                    });
            });
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        let Some(toast) = &self.toast else {
            return;
        };
        let t = Tokens::get(ctx);
        let color = if toast.ok { t.good } else { t.bad };
        egui::Area::new(Id::new("deck-toast"))
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -92.0))
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(t.bg2)
                    .stroke(Stroke::new(1.0, color.gamma_multiply(0.7)))
                    .corner_radius(CornerRadius::ZERO)
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.label(RichText::new(&toast.text).size(14.0).color(color));
                    });
            });
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.pad_ui(ctx, raw);
    }

    fn ui(&mut self, root: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.maybe_check_updates();
        self.tick(&ctx);
        if self.stream.fullscreen {
            self.fullscreen_ui(root, &ctx);
        } else {
            self.chrome_ui(root, &ctx);
        }
        self.osk_ui(&ctx);
        self.toast_ui(&ctx);
        self.pad_cursor_ui(&ctx);

        let ms = if self.stream.feed.is_some() {
            24
        } else if self.gamepad_on || self.binds.capture || deck_mode() {
            // The pad is read on a background thread; poll the frame often
            // enough that the virtual cursor does not feel laggy.
            33
        } else {
            350
        };
        ctx.request_repaint_after(Duration::from_millis(ms));
    }
}

fn push_text(ctx: &egui::Context, text: &str) {
    ctx.input_mut(|i| i.events.push(egui::Event::Text(text.to_string())));
}

fn push_key(ctx: &egui::Context, key: egui::Key) {
    ctx.input_mut(|i| {
        i.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        })
    });
}

fn read_battery() -> Option<u8> {
    for name in ["BAT1", "BAT0", "battery"] {
        let path = format!("/sys/class/power_supply/{name}/capacity");
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(v) = text.trim().parse::<u8>() {
                return Some(v);
            }
        }
    }
    None
}

fn udp_addr(url: &str) -> Option<SocketAddr> {
    let url = url.trim().trim_end_matches('/');
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let authority = rest.split('/').next()?;
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        let port = tail.strip_prefix(':').and_then(|p| p.parse::<u16>().ok());
        (host.to_string(), port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if let Ok(p) = port.parse::<u16>() {
            (host.to_string(), Some(p))
        } else {
            (authority.to_string(), None)
        }
    } else {
        (authority.to_string(), None)
    };
    let input_port = port.unwrap_or(API_PORT).saturating_add(1);
    (host.as_str(), input_port).to_socket_addrs().ok()?.next()
}

/// Notification and HA helpers shared with the pages.
pub fn ha_domain_action(domain: &str) -> &'static str {
    match domain {
        "scene" | "script" | "button" => "turn_on",
        _ => "toggle",
    }
}

pub fn notification_color(t: &Tokens, n: &Notification) -> Color32 {
    if n.urgency >= 2 {
        t.bad
    } else if n.urgency == 1 {
        t.warn
    } else {
        t.accent
    }
}

pub fn brightness_map() -> HashMap<String, u8> {
    HashMap::new()
}

/// Running on a handheld where the compositor may scale a small screen.
fn deck_mode() -> bool {
    std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some()
        || sdw_core::util::is_steam_deck()
}

/// Upper bound for the ui scale slider. A 1280x800 deck screen cannot carry
/// the same 1.4x zoom a desktop window can.
pub fn max_ui_scale() -> f32 {
    if deck_mode() { 1.15 } else { 1.4 }
}

/// Radial-ish deadzone: keep the centre still, then ramp to full speed.
fn deadzone(v: f32, dz: f32) -> f32 {
    if v.abs() <= dz {
        0.0
    } else {
        (v - dz.copysign(v)) / (1.0 - dz)
    }
}
