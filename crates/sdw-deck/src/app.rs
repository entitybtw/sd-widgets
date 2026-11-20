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
    Clock,
    Computers,
    Control,
    Bluetooth,
    Gamepad,
    Media,
    Screen,
    Audio,
    Notifications,
    HomeAssistant,
    Binds,
    Addons,
    Settings,
    Theme,
}

impl Page {
    pub const NAV: [Page; 8] = [
        Page::Home,
        Page::Clock,
        Page::Computers,
        Page::Gamepad,
        Page::Media,
        Page::Screen,
        Page::Notifications,
        Page::Binds,
    ];
    /// Every page, in the order the arrows and the floating dock walk through them.
    pub const ORDER: [Page; 15] = [
        Page::Home,
        Page::Clock,
        Page::Computers,
        Page::Control,
        Page::Bluetooth,
        Page::Gamepad,
        Page::Media,
        Page::Screen,
        Page::Audio,
        Page::Notifications,
        Page::HomeAssistant,
        Page::Binds,
        Page::Addons,
        Page::Theme,
        Page::Settings,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Page::Home => "home",
            Page::Clock => "clock",
            Page::Computers => "computers",
            Page::Control => "control",
            Page::Bluetooth => "bluetooth",
            Page::Gamepad => "gamepad",
            Page::Media => "media",
            Page::Screen => "screen",
            Page::Audio => "audio",
            Page::Notifications => "notifications",
            Page::HomeAssistant => "ha",
            Page::Binds => "binds",
            Page::Addons => "addons",
            Page::Settings => "settings",
            Page::Theme => "theme",
        }
    }
    pub fn from_key(key: &str) -> Page {
        match key.trim().to_ascii_lowercase().as_str() {
            "clock" | "watch" | "time" => Page::Clock,
            "computers" | "pc" => Page::Computers,
            "control" | "pult" | "power" => Page::Control,
            "bluetooth" | "bt" | "headphones" => Page::Bluetooth,
            "gamepad" => Page::Gamepad,
            "media" | "music" => Page::Media,
            "screen" | "stream" => Page::Screen,
            "audio" | "sound" => Page::Audio,
            "notifications" => Page::Notifications,
            "ha" | "homeassistant" => Page::HomeAssistant,
            "binds" => Page::Binds,
            "addons" | "addon" | "plugins" => Page::Addons,
            "settings" => Page::Settings,
            "theme" | "look" | "appearance" => Page::Theme,
            _ => Page::Home,
        }
    }
    pub fn label(self, l: Lang) -> &'static str {
        match self {
            Page::Home => t!(l, "домой", "home"),
            Page::Clock => t!(l, "часы", "clock"),
            Page::Computers => t!(l, "компьютеры", "computers"),
            Page::Control => t!(l, "пульт", "control"),
            Page::Bluetooth => t!(l, "bluetooth", "bluetooth"),
            Page::Gamepad => t!(l, "геймпад", "gamepad"),
            Page::Media => t!(l, "медиа", "media"),
            Page::Screen => t!(l, "экран", "screen"),
            Page::Audio => t!(l, "звук", "audio"),
            Page::Notifications => t!(l, "уведомления", "alerts"),
            Page::HomeAssistant => t!(l, "home assistant", "home assistant"),
            Page::Binds => t!(l, "бинды", "binds"),
            Page::Addons => t!(l, "аддоны", "addons"),
            Page::Settings => t!(l, "настройки", "settings"),
            Page::Theme => t!(l, "вид", "look"),
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Page::Home => theme::icon::HOME,
            Page::Clock => theme::icon::CLOCK,
            Page::Computers => theme::icon::COMPUTERS,
            Page::Control => theme::icon::POWER,
            Page::Bluetooth => theme::icon::BLUETOOTH,
            Page::Gamepad => theme::icon::GAMEPAD,
            Page::Media => theme::icon::MUSIC,
            Page::Screen => theme::icon::SCREEN,
            Page::Audio => theme::icon::SPEAKER,
            Page::Notifications => theme::icon::NOTIFICATIONS,
            Page::HomeAssistant => theme::icon::HOME_ASSISTANT,
            Page::Binds => theme::icon::BINDS,
            Page::Addons => theme::icon::ADDON,
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
    /// Tabs of the Lovelace dashboard, in dashboard order.
    pub views: Vec<HaView>,
    pub categories: Vec<HaCategory>,
    /// Named presets shared with the daemon and every other deck.
    pub presets: Vec<HaPreset>,
    /// Local state right after a tap, until Home Assistant confirms it.
    pub optimistic: HashMap<String, (String, Instant)>,
    /// Brightness right after a slider release, until the next real state is
    /// different from it. Without this the slider snaps back for a second.
    pub optimistic_pct: HashMap<String, (u8, Instant)>,
    pub room: usize,
    /// Page of the current room; a room can hold hundreds of entities.
    pub page: usize,
    /// The room and search the current page belongs to, so switching resets it.
    pub page_room: usize,
    pub page_needle: String,
    /// Show unavailable/unknown tiles and integration noise.
    pub show_offline: bool,
    /// Last successful answer: connection flag, data age and error text.
    pub connected: bool,
    pub age_ms: u64,
    pub stale_error: String,
    pub search: String,
    pub url: String,
    pub token: String,
    pub token_show: bool,
    pub loaded: bool,
    pub pending: bool,
    pub error: String,
    pub last_fetch: Instant,
    /// Category editor open.
    pub manage: bool,
    pub new_category: String,
    /// Preset bar open and its new-name field.
    pub presets_open: bool,
    pub preset_name: String,
}

pub struct MediaUiState {
    pub state: MediaState,
    pub pending: bool,
    pub last_poll: Instant,
    pub art: Option<egui::TextureHandle>,
    pub art_id: String,
    pub art_bytes: Option<(String, Vec<u8>)>,
    pub art_error: String,
    pub art_requested: String,
    /// While the user drags the position slider we do not overwrite it.
    pub seeking: Option<f64>,
    pub error: String,
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
    /// When capture was switched on. The first ~300 ms are ignored so the tap
    /// that opened the editor cannot become the recorded control.
    pub capture_since: Option<Instant>,
    pub filter: String,
}

pub struct Toast {
    pub text: String,
    pub ok: bool,
    pub at: Instant,
}

/// A notification mirrored as a smooth in-app popup. It lives for a few
/// seconds, fades in from the edge and can be dismissed with a tap.
#[derive(Clone)]
pub struct NotifPopup {
    pub id: u64,
    pub app: String,
    pub summary: String,
    pub body: String,
    /// PNG in base64, empty when the notification carries no picture.
    pub image: String,
    pub urgency: u8,
    pub at: Instant,
}

/// Battery of a Bluetooth or USB headset, as UPower reports it.
#[derive(Debug, Clone)]
pub struct Headset {
    pub name: String,
    pub percent: u8,
}

pub enum Task {
    Status(Result<DaemonStatus, String>),
    Paired {
        base: String,
        result: Result<PairResponse, String>,
    },
    Discovery(Result<Vec<DiscoveryResponse>, String>),
    Pad(Result<PadCreated, String>),
    Monitor(Result<MonitorStatus, String>),
    MonitorStopped(Result<MonitorStatus, String>),
    Notifications(Result<Vec<Notification>, String>),
    Ha(Result<HaStates, String>),
    Media(Result<MediaState, String>),
    MediaArt {
        id: String,
        result: Result<Vec<u8>, String>,
    },
    Categories(Result<Vec<HaCategory>, String>),
    Presets(Result<Vec<HaPreset>, String>),
    Update(Result<UpdateInfo, String>),
    Updated(Result<(String, u64), String>),
    Installed(Result<String, String>),
    Audio(Result<AudioStatus, String>),
    AudioApps(Result<Vec<AudioApp>, String>),
    Msg(Result<String, String>),
    Headset(Option<Headset>),
    /// Catalogue entries from every store base, plus the errors that happened.
    Catalog(Vec<sdw_core::addons::CatalogEntry>, String),
    /// A catalogue package installed on the deck (and on the computer when
    /// one is connected), with a note when the computer part failed.
    AddonInstalled(Result<(sdw_core::addons::Addon, String), String>),
    /// Applications listed by the active computer.
    Apps(Result<Vec<sdw_core::model::AppEntry>, String>),
    /// Bluetooth adapter and devices.
    Bluetooth(Result<sdw_core::model::BluetoothStatus, String>),
    /// Result of a Bluetooth action; the page reloads afterwards.
    BtAction(Result<String, String>),
    /// The Deck sink was muted or unmuted.
    QuickMute(Result<bool, String>),
}

/// Multi-finger touch state from `egui::Event::Touch`.
///
/// The compositor may deliver the touchscreen either as native touch events or
/// as a plain mouse. This tracks the native case; the mouse fallback lives in
/// [`App::swipe_tick`]. A single finger is still a tap or a vertical scroll for
/// egui, but a clearly horizontal flick becomes "previous/next page"; two or
/// more fingers keep their own gestures.
#[derive(Default)]
pub struct TouchTrack {
    points: HashMap<u64, egui::Pos2>,
    last: HashMap<u64, egui::Pos2>,
    start: Option<Instant>,
    anchor: Option<(f32, f32)>,
    max: u8,
    pinch: Option<(u64, u64, f32)>,
}

impl TouchTrack {
    fn norm(pos: egui::Pos2, rect: Rect) -> (f32, f32) {
        (
            ((pos.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0),
            ((pos.y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0),
        )
    }

    fn centroid(&self, map: &HashMap<u64, egui::Pos2>, rect: Rect) -> (f32, f32) {
        if map.is_empty() {
            return (0.0, 0.0);
        }
        let (mut x, mut y) = (0.0, 0.0);
        for p in map.values() {
            let (nx, ny) = Self::norm(*p, rect);
            x += nx;
            y += ny;
        }
        (x / map.len() as f32, y / map.len() as f32)
    }

    fn begin(&mut self, id: u64, pos: egui::Pos2, rect: Rect, now: Instant) {
        self.points.insert(id, pos);
        self.last.insert(id, pos);
        if self.start.is_none() {
            self.start = Some(now);
        }
        self.max = self.max.max(self.points.len().min(4) as u8);
        // Re-anchor whenever the finger count changes: adding a second finger
        // must not look like the first one jumped sideways.
        self.anchor = Some(self.centroid(&self.points, rect));
        if self.pinch.is_none() && self.points.len() >= 2 {
            let mut ids = self.points.keys().copied();
            if let (Some(a), Some(b)) = (ids.next(), ids.next()) {
                let pa = self.points[&a];
                let pb = self.points[&b];
                let dx = (pb.x - pa.x) / rect.width().max(1.0);
                let dy = (pb.y - pa.y) / rect.height().max(1.0);
                self.pinch = Some((a, b, (dx * dx + dy * dy).sqrt()));
            }
        }
    }

    fn moved(&mut self, id: u64, pos: egui::Pos2) {
        if self.points.contains_key(&id) {
            self.points.insert(id, pos);
        }
        self.last.insert(id, pos);
    }

    /// Horizontal travel of the fingers since the gesture started, in points.
    fn drag_px(&self, rect: Rect) -> Option<Vec2> {
        let anchor = self.anchor?;
        let now = self.centroid(&self.points, rect);
        Some(Vec2::new(
            (now.0 - anchor.0) * rect.width(),
            (now.1 - anchor.1) * rect.height(),
        ))
    }

    /// Returns a finished gesture once the last finger lifts.
    fn end(&mut self, id: u64, pos: egui::Pos2, rect: Rect, now: Instant) -> Option<GestureSpec> {
        self.last.insert(id, pos);
        self.points.remove(&id);
        if !self.points.is_empty() {
            return None;
        }
        let spec = self.finish(rect, now);
        self.reset();
        spec
    }

    fn finish(&mut self, rect: Rect, now: Instant) -> Option<GestureSpec> {
        let started = self.start?;
        let start = self.anchor?;
        let end = self.centroid(&self.last, rect);
        if self.max < 2 {
            // One finger: only a quick, clearly horizontal flick counts. A tap
            // and a vertical scroll stay with egui.
            let dx = end.0 - start.0;
            let dy = end.1 - start.1;
            if now.saturating_duration_since(started) < Duration::from_millis(700)
                && dx.abs() > 0.14
                && dx.abs() > dy.abs() * 1.6
            {
                return Some(GestureSpec {
                    kind: GestureKind::Swipe,
                    fingers: 1,
                    dir: Some(if dx < 0.0 {
                        SwipeDir::Left
                    } else {
                        SwipeDir::Right
                    }),
                });
            }
            return None;
        }
        let pinch = self.pinch.and_then(|(a, b, d0)| {
            let pa = *self.last.get(&a)?;
            let pb = *self.last.get(&b)?;
            let dx = (pb.x - pa.x) / rect.width().max(1.0);
            let dy = (pb.y - pa.y) / rect.height().max(1.0);
            Some((d0, (dx * dx + dy * dy).sqrt()))
        });
        crate::input::gesture_from(
            self.max.clamp(1, 4),
            now.saturating_duration_since(started),
            start,
            end,
            pinch,
        )
    }

    fn reset(&mut self) {
        self.points.clear();
        self.last.clear();
        self.start = None;
        self.anchor = None;
        self.max = 0;
        self.pinch = None;
    }
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
    /// Live horizontal offset while a finger swipe is in progress, in points.
    /// It makes the page follow the finger instead of jumping on release.
    pub page_slide: f32,
    pub page_at: Instant,
    pub status: Option<DaemonStatus>,
    pub status_err: String,
    pub status_pending: bool,
    pub last_status: Instant,
    /// Debounced config write: a slider drag saves once, not on every frame.
    pub save_at: Option<Instant>,
    /// The clock is drawn over the whole window, without chrome.
    pub clock_full: bool,
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
    /// A gamepad-capable event node exists, so the UI must poll fast.
    pub has_gamepad: bool,
    /// Presses collected between frames; fast taps would otherwise be lost.
    pub pad_press: Vec<u16>,
    pub last_pad: Instant,
    pub pressed: HashSet<u16>,
    pub last_button: Option<(u16, String, Instant)>,
    pub last_gesture: Option<(GestureSpec, Instant)>,

    /// Virtual pointer used when the deck's own gamepad drives the interface.
    pub pad_cursor: Option<egui::Pos2>,
    pub pad_cursor_at: Instant,
    pub pad_buttons: u32,
    pub pad_release: bool,
    pub pad_seen: bool,
    /// Finger swipe used when the compositor sends touch as mouse.
    pub edge_swipe: Option<(egui::Pos2, egui::Pos2, Instant)>,
    /// The current pointer drag belongs to a widget (slider, splitter, …), so
    /// it must not also turn into a page swipe.
    pub swipe_blocked: bool,
    /// Same for a native single-finger touch: a slider owns the finger.
    pub touch_blocked: bool,
    /// Id of the page ScrollArea in the last frame. It always claims drags to
    /// scroll; that must not be mistaken for a control that owns the pointer.
    pub page_scroll_id: Option<egui::Id>,
    /// Raw multi-touch gestures from egui's touch events.
    pub touch: TouchTrack,

    pub stream: StreamState,
    pub ha: HaState,
    pub media: MediaUiState,
    pub binds: BindsState,
    pub update: UpdateState,
    pub settings_name: String,
    /// Path typed in the appearance page to import a theme pack.
    pub theme_import: String,
    /// Name used when saving the current CSS as a user pack.
    pub theme_pack_name: String,
    /// Path typed in the addons page to import a package.
    pub addon_import: String,
    /// Store base typed in the addons page.
    pub addon_repo_input: String,
    /// Catalogue of every configured store, flattened.
    pub addon_catalog: Vec<sdw_core::addons::CatalogEntry>,
    pub addon_catalog_pending: bool,
    pub addon_catalog_err: String,
    /// When the catalogue was last fetched; keeps a page visit from firing a
    /// request on every frame.
    pub addon_catalog_at: Option<Instant>,
    /// Id of the package currently downloading.
    pub addon_installing: Option<String>,

    /// Applications on the active computer, newest list first.
    pub control_apps: Vec<sdw_core::model::AppEntry>,
    pub control_apps_pending: bool,
    pub control_apps_err: String,
    pub control_apps_at: Option<Instant>,
    pub control_filter: String,
    /// Power action waiting for the second press.
    pub power_confirm: Option<sdw_core::model::PowerAction>,

    pub bluetooth: Option<sdw_core::model::BluetoothStatus>,
    pub bluetooth_pending: bool,
    pub bluetooth_at: Option<Instant>,
    pub bluetooth_err: String,
    /// Address waiting for a second press before it is removed.
    pub bluetooth_remove: Option<String>,

    /// Pull-down control panel: the Deck's own volume and brightness plus the
    /// newest notifications.
    pub quick_open: bool,
    pub quick_volume: Option<u8>,
    pub quick_muted: bool,
    pub quick_brightness: Option<u8>,
    pub quick_at: Option<Instant>,
    /// Finger offset while a top-edge pull-down is in progress.
    pub quick_pull: f32,

    pub osk_open: bool,
    pub osk_shift: bool,
    pub osk_symbols: bool,

    pub toast: Option<Toast>,
    pub popups: Vec<NotifPopup>,
    /// Decoded notification pictures, keyed by notification id.
    pub notif_art: HashMap<u64, Option<egui::TextureHandle>>,
    /// The system on-screen keyboard is open for the focused field.
    pub kb_open: bool,
    pub task_tx: Sender<Task>,
    pub task_rx: Receiver<Task>,
    pub clock: String,
    pub last_clock: Instant,
    pub battery: Option<u8>,
    pub last_battery: Instant,
    /// Bluetooth/USB headset battery, read from UPower now and then.
    pub headset: Option<Headset>,
    pub last_headset: Instant,
    pub applied_scale: f32,
    /// Working copy of the theme CSS while the appearance page is open.
    pub style_draft: String,
    /// The player process receiving the PC audio stream.
    pub audio_player: Option<std::process::Child>,
    /// How many more "yes" presses are needed before audio starts.
    pub audio_confirm_left: u8,
    pub audio_error: String,
    pub audio_apps: Vec<AudioApp>,
    pub audio_apps_at: Option<Instant>,
    pub audio_starting: bool,
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
        let app = Self {
            lang: Lang::from_code(&cfg.lang),
            dark: cfg.dark,
            applied_dark: cfg.dark,
            page,
            page_epoch: 1,
            page_dir: 1.0,
            page_slide: 0.0,
            page_at: Instant::now(),
            status: None,
            status_err: String::new(),
            status_pending: false,
            last_status: Instant::now() - Duration::from_secs(5),
            save_at: None,
            clock_full: false,
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
            has_gamepad: false,
            pad_press: Vec::new(),
            last_pad: Instant::now() - Duration::from_secs(10),
            pressed: HashSet::new(),
            last_button: None,
            last_gesture: None,
            pad_cursor: None,
            pad_cursor_at: Instant::now() - Duration::from_secs(10),
            pad_buttons: 0,
            pad_release: false,
            pad_seen: false,
            edge_swipe: None,
            swipe_blocked: false,
            touch_blocked: false,
            page_scroll_id: None,
            touch: TouchTrack::default(),
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
                views: Vec::new(),
                categories: Vec::new(),
                presets: Vec::new(),
                optimistic: HashMap::new(),
                optimistic_pct: HashMap::new(),
                room: 0,
                page: 0,
                page_room: 0,
                page_needle: String::new(),
                show_offline: false,
                connected: false,
                age_ms: 0,
                stale_error: String::new(),
                search: String::new(),
                url: String::new(),
                token: String::new(),
                token_show: false,
                loaded: false,
                pending: false,
                error: String::new(),
                last_fetch: Instant::now() - Duration::from_secs(60),
                manage: false,
                new_category: String::new(),
                presets_open: false,
                preset_name: String::new(),
            },
            media: MediaUiState {
                state: MediaState::default(),
                pending: false,
                last_poll: Instant::now() - Duration::from_secs(60),
                art: None,
                art_id: String::new(),
                art_bytes: None,
                art_error: String::new(),
                art_requested: String::new(),
                seeking: None,
                error: String::new(),
            },
            binds: BindsState {
                editor: None,
                capture: false,
                capture_since: None,
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
            theme_import: String::new(),
            theme_pack_name: String::new(),
            osk_open: false,
            osk_shift: false,
            osk_symbols: false,
            toast: None,
            popups: Vec::new(),
            notif_art: HashMap::new(),
            kb_open: false,
            task_tx,
            task_rx,
            clock: theme::clock_text(),
            last_clock: Instant::now(),
            battery: None,
            last_battery: Instant::now() - Duration::from_secs(60),
            headset: None,
            last_headset: Instant::now() - Duration::from_secs(60),
            applied_scale: 0.0,
            style_draft: cfg.style.css.clone(),
            addon_import: String::new(),
            addon_repo_input: String::new(),
            addon_catalog: Vec::new(),
            addon_catalog_pending: false,
            addon_catalog_err: String::new(),
            addon_catalog_at: None,
            addon_installing: None,
            control_apps: Vec::new(),
            control_apps_pending: false,
            control_apps_err: String::new(),
            control_apps_at: None,
            control_filter: String::new(),
            power_confirm: None,
            bluetooth: None,
            bluetooth_pending: false,
            bluetooth_at: None,
            bluetooth_err: String::new(),
            bluetooth_remove: None,
            quick_open: false,
            quick_volume: None,
            quick_muted: false,
            quick_brightness: None,
            quick_at: None,
            quick_pull: 0.0,
            audio_player: None,
            audio_confirm_left: 0,
            audio_error: String::new(),
            audio_apps: Vec::new(),
            audio_apps_at: None,
            audio_starting: false,
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

        // While the pad is forwarded to a computer (or a bind is being
        // captured) the same buttons must not also drive this interface.
        if self.gamepad_on || self.binds.capture {
            self.pad_buttons = buttons;
            self.pad_release = false;
            self.pad_press.clear();
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

        // Discrete presses come from a queue filled by the input thread, not
        // from the current button bit: a tap that starts and ends between two
        // repaints would otherwise never be seen.
        let presses = std::mem::take(&mut self.pad_press);
        let mut want_click = false;
        for code in presses {
            match code {
                crate::input::BTN_SOUTH => want_click = true,
                crate::input::BTN_EAST => self.go_back(ctx),
                crate::input::BTN_TL => self.nav_step(-1),
                crate::input::BTN_TR => self.nav_step(1),
                crate::input::BTN_NORTH => self.set_page(Page::Home),
                crate::input::BTN_SELECT => self.set_page(Page::Settings),
                crate::input::BTN_START => self.set_page(Page::Gamepad),
                crate::input::BTN_MODE => self.set_page(Page::Home),
                _ => {}
            }
            self.pad_seen = true;
            moved = true;
        }

        // A clicks at the virtual pointer, with the release one frame later so
        // widgets see a real press/release pair.
        if want_click {
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

        if moved {
            ctx.request_repaint();
        }
        self.pad_buttons = buttons;
    }

    /// Touch gestures on the page surface.
    ///
    /// Native touch events are turned into `GestureSpec`s here (multi-finger
    /// swipes, pinches). When the compositor instead reports the touchscreen as
    /// a plain mouse, [`Self::swipe_tick`] covers the single-finger case.
    fn touch_gesture_tick(&mut self, ctx: &egui::Context, raw: &egui::RawInput) {
        if self.gamepad_on || self.binds.capture {
            self.touch.reset();
            return;
        }
        let rect = raw.screen_rect.unwrap_or_else(|| ctx.content_rect());
        let now = Instant::now();
        let mut finished = None;
        for event in &raw.events {
            if let egui::Event::Touch { id, phase, pos, .. } = event {
                match phase {
                    egui::TouchPhase::Start => {
                        if self.touch.points.is_empty() {
                            self.touch_blocked = false;
                        }
                        self.touch.begin(id.0, *pos, rect, now)
                    }
                    egui::TouchPhase::Move => self.touch.moved(id.0, *pos),
                    egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                        if let Some(spec) = self.touch.end(id.0, *pos, rect, now) {
                            finished = Some(spec);
                        }
                    }
                }
            }
        }
        // The scroll area itself claims every drag it can; a slider or a tile
        // drag is a different id. Only the latter blocks the page gesture.
        if !self.touch.points.is_empty() {
            if let Some(id) = ctx.dragged_id() {
                if Some(id) != self.page_scroll_id {
                    self.touch_blocked = true;
                }
            }
        }
        if finished.is_none()
            && theme::style().animations
            && !self.gamepad_on
            && !self.binds.capture
            && !self.touch_blocked
            && self.touch.points.len() >= 1
            && self.touch.max >= 1
        {
            if let Some(delta) = self.touch.drag_px(rect) {
                if delta.x.abs() > delta.y.abs() {
                    let w = rect.width().max(1.0);
                    self.page_slide = delta.x.clamp(-w * 0.6, w * 0.6);
                }
            }
        }
        if let Some(spec) = finished {
            self.page_slide = 0.0;
            if spec.fingers == 1 && spec.kind == GestureKind::Swipe && !self.touch_blocked {
                self.nav_step(if spec.dir == Some(SwipeDir::Left) {
                    1
                } else {
                    -1
                });
            } else {
                self.on_input(DeckEvent::Gesture(spec));
            }
        }
    }

    /// Horizontal swipe anywhere on the page, with a lower threshold when it
    /// starts at an edge. This is the fallback for a touchscreen that the
    /// compositor (or Steam Input) delivers as a mouse: there are no touch
    /// events to count fingers with, but a horizontal drag still works.
    fn swipe_tick(&mut self, ctx: &egui::Context) {
        if !deck_mode()
            || self.gamepad_on
            || self.binds.capture
            || self.stream.fullscreen
            || self.binds.editor.is_some()
        {
            self.edge_swipe = None;
            return;
        }
        let (pressed, down, released, pos) = ctx.input(|i| {
            (
                i.pointer.any_pressed(),
                i.pointer.any_down(),
                i.pointer.any_released(),
                i.pointer.interact_pos().or_else(|| i.pointer.latest_pos()),
            )
        });
        let rect = ctx.content_rect();

        // An open panel is dismissed by a pull up rather than a page swipe.
        if self.quick_open {
            if pressed {
                if let Some(p) = pos {
                    self.edge_swipe = Some((p, p, Instant::now()));
                }
            }
            if let Some((start, _, at)) = self.edge_swipe {
                if down {
                    if let Some(p) = pos {
                        self.edge_swipe = Some((start, p, at));
                    }
                } else if released {
                    if let Some(p) = pos {
                        let dy = p.y - start.y;
                        let dx = p.x - start.x;
                        if dy < -70.0 && dy.abs() > dx.abs() * 1.2 {
                            self.set_quick(false);
                        }
                    }
                    self.edge_swipe = None;
                } else {
                    self.edge_swipe = None;
                }
            }
            return;
        }

        if pressed {
            if let Some(p) = pos {
                self.swipe_blocked = false;
                self.edge_swipe = Some((p, p, Instant::now()));
            }
        }
        if down {
            if let Some(id) = ctx.dragged_id() {
                // The page ScrollArea claims every drag so it can scroll. That
                // is not a widget owning the pointer; a slider or a tile drag
                // is, and must keep the swipe away.
                if Some(id) != self.page_scroll_id {
                    self.swipe_blocked = true;
                }
            }
        }
        if let Some((start, _, at)) = self.edge_swipe {
            if down {
                if let Some(p) = pos {
                    self.edge_swipe = Some((start, p, at));
                    let dx = p.x - start.x;
                    let dy = p.y - start.y;
                    let from_top = start.y <= rect.top() + 96.0;
                    if from_top && dy > 8.0 && dy > dx.abs() * 1.2 {
                        // A pull down from the top edge shows the panel; the
                        // handle grows under the finger as feedback.
                        self.quick_pull = dy;
                        self.page_slide = 0.0;
                    } else if theme::style().animations
                        && !self.swipe_blocked
                        && dx.abs() > 6.0
                        && dx.abs() > dy.abs() * 1.2
                    {
                        let w = rect.width().max(1.0);
                        self.page_slide = dx.clamp(-w * 0.6, w * 0.6);
                    } else if !self.swipe_blocked {
                        self.page_slide = 0.0;
                        self.quick_pull = 0.0;
                    }
                }
            } else if released {
                if let Some(p) = pos {
                    let dx = p.x - start.x;
                    let dy = p.y - start.y;
                    let edge = start.x <= rect.left() + 64.0 || start.x >= rect.right() - 64.0;
                    let min = if edge { 80.0 } else { 140.0 };
                    let pull = self.quick_pull;
                    self.quick_pull = 0.0;
                    if !self.swipe_blocked
                        && pull > 70.0
                        && at.elapsed() < Duration::from_millis(900)
                    {
                        self.page_slide = 0.0;
                        self.set_quick(true);
                        self.edge_swipe = None;
                        return;
                    }
                    let gesture_recent = self
                        .last_gesture
                        .as_ref()
                        .is_some_and(|(_, when)| when.elapsed() < Duration::from_millis(450));
                    if !self.swipe_blocked
                        && !gesture_recent
                        && at.elapsed() < Duration::from_millis(900)
                        && dx.abs() > min
                        && dx.abs() > dy.abs() * 1.4
                    {
                        self.page_slide = 0.0;
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
        if self.quick_open {
            self.set_quick(false);
        } else if self.clock_full {
            self.clock_full = false;
        } else if self.stream.fullscreen {
            self.set_fullscreen(ctx, false);
        } else if self.osk_open {
            self.osk_open = false;
        } else if self.binds.capture {
            self.binds.capture = false;
            self.binds.capture_since = None;
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
        let fade = (1.0 - (self.pad_cursor_at.elapsed().as_secs_f32() - 1.8).max(0.0) / 1.2)
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

    /// Save once the user stops dragging, instead of on every changed frame.
    pub fn save_soon(&mut self) {
        self.save_at = Some(Instant::now());
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
        self.active_host().map(|h| Client::new(&h.url, &h.token))
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
            if let (Some(a), Some(b)) = (
                Page::ORDER.iter().position(|p| *p == self.page),
                Page::ORDER.iter().position(|p| *p == page),
            ) {
                self.page_dir = if b > a { 1.0 } else { -1.0 };
            }
            self.page_at = Instant::now();
            self.page = page;
            self.page_epoch += 1;
            self.cfg.last_page = page.key().to_string();
            self.save();
            sound::play(
                &self.cfg.sound,
                if self.page_dir >= 0.0 {
                    Cue::Navigate
                } else {
                    Cue::Back
                },
            );
            if page == Page::Notifications {
                self.notif_unread = 0;
            }
            if page == Page::HomeAssistant {
                self.ha.last_fetch = Instant::now() - Duration::from_secs(60);
            }
            if page == Page::Media {
                self.media.last_poll = Instant::now() - Duration::from_secs(5);
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
        self.cfg.style.pages.is_empty() || self.cfg.style.pages.iter().any(|p| p == page.key())
    }

    /// Pages in dock order. The stored list is both the filter and the order;
    /// an empty list means "all pages in the default order".
    pub fn dock_pages(&self) -> Vec<Page> {
        if self.cfg.style.pages.is_empty() {
            return Page::ORDER
                .iter()
                .copied()
                .filter(|p| self.page_shown(*p))
                .collect();
        }
        let mut out: Vec<Page> = Vec::new();
        for key in &self.cfg.style.pages {
            let page = Page::from_key(key);
            if !out.contains(&page) {
                out.push(page);
            }
        }
        out
    }

    /// Whether a home tile is visible.
    pub fn tile_shown(&self, key: &str) -> bool {
        self.cfg.style.home_tiles.is_empty() || self.cfg.style.home_tiles.iter().any(|k| k == key)
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
        self.notif_art.clear();
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
            self.pair_error = t!(
                self.lang,
                "укажите адрес компьютера",
                "enter the computer address"
            )
            .into();
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
                    t!(
                        self.lang,
                        "геймпад передаётся на компьютер",
                        "gamepad is live on the computer"
                    ),
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
            self.toast(
                t!(
                    self.lang,
                    "сначала подключите компьютер",
                    "connect a computer first"
                ),
                false,
            );
            return;
        };
        self.gamepad_pending = true;
        let kind = self.gamepad_kind;
        let name = if self.gamepad_name.trim().is_empty() {
            format!("{} · {}", self.cfg.device_name, kind.label())
        } else {
            self.gamepad_name.trim().to_string()
        };
        self.toast(
            t!(
                self.lang,
                "создаю виртуальный пад…",
                "creating the virtual pad…"
            ),
            true,
        );
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
        let lang = self.lang;
        self.spawn(move || {
            Task::Msg(
                c.delete_pad(&pad.id)
                    .map(|_| t!(lang, "пад удалён", "pad removed").to_string())
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
            self.stream.error = t!(
                self.lang,
                "не удалось декодировать кадр",
                "could not decode the frame"
            )
            .into();
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
            self.spawn(move || {
                Task::Notifications(c.notifications(since).map_err(|e| e.to_string()))
            });
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

    /// Read every store catalogue in one worker. Network work never runs on
    /// the UI thread, and one broken repository does not hide the others.
    pub fn load_addon_catalog(&mut self) {
        if self.addon_catalog_pending {
            return;
        }
        let repos = self.cfg.addons.repos.clone();
        if repos.is_empty() {
            self.addon_catalog.clear();
            self.addon_catalog_err.clear();
            return;
        }
        self.addon_catalog_pending = true;
        self.addon_catalog_at = Some(Instant::now());
        self.spawn(move || {
            let mut list = Vec::new();
            let mut errors = Vec::new();
            for repo in repos {
                match sdw_core::addons::fetch_repo(&repo) {
                    Ok(catalog) => list.extend(catalog.addons),
                    Err(e) => errors.push(e),
                }
            }
            Task::Catalog(list, errors.join("; "))
        });
    }

    /// Install one catalogue package locally, then ask the computer to do the
    /// same: daemon actions only run where the package is installed.
    pub fn install_catalog_entry(&mut self, entry: sdw_core::addons::CatalogEntry) {
        if self.addon_installing.is_some() {
            return;
        }
        self.addon_installing = Some(entry.id.clone());
        let client = self.client();
        self.spawn(move || {
            let result = sdw_core::addons::install_entry(&entry).map(|addon| {
                let note = match client {
                    Some(c) => c
                        .addon_install(&entry.url)
                        .err()
                        .map(|e| e.message)
                        .unwrap_or_default(),
                    None => String::new(),
                };
                (addon, note)
            });
            Task::AddonInstalled(result)
        });
    }

    /// Read the Bluetooth adapter and devices from the active computer.
    pub fn load_bluetooth(&mut self) {
        if self.bluetooth_pending {
            return;
        }
        let Some(client) = self.client() else {
            self.bluetooth_err = t!(
                self.lang,
                "сначала выберите компьютер",
                "choose a computer first"
            )
            .to_string();
            return;
        };
        self.bluetooth_pending = true;
        self.bluetooth_at = Some(Instant::now());
        self.spawn(move || Task::Bluetooth(client.bluetooth().map_err(|e| e.to_string())));
    }

    pub fn bluetooth_action(&mut self, action: &str, address: &str) {
        let Some(client) = self.client() else {
            self.toast(
                t!(self.lang, "нет активного компьютера", "no active computer"),
                false,
            );
            return;
        };
        let (action, address) = (action.to_string(), address.to_string());
        let lang = self.lang;
        self.spawn(move || {
            Task::BtAction(
                client
                    .bluetooth_action(&action, &address)
                    .map(|v| {
                        v.get("message")
                            .and_then(|m| m.as_str())
                            .map(str::to_string)
                            .unwrap_or_else(|| t!(lang, "готово", "done").to_string())
                    })
                    .map_err(|e| e.to_string()),
            )
        });
    }

    /// Drop a notification locally and on the computer.
    pub fn dismiss_notif(&mut self, id: u64) {
        self.notifications.retain(|n| n.id != id);
        if let Some(c) = self.client() {
            self.spawn(move || {
                Task::Msg(
                    c.dismiss_notification(id)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string()),
                )
            });
        }
    }

    /// Open or close the pull-down panel. Opening reads the current values at
    /// once, so the sliders never show a stale number.
    pub fn set_quick(&mut self, open: bool) {
        self.quick_open = open;
        self.quick_pull = 0.0;
        if open {
            self.quick_at = None;
            self.quick_values();
        }
    }

    /// Read the Deck's own volume and brightness. Called when the panel opens
    /// and every couple of seconds while it is visible.
    pub fn quick_values(&mut self) {
        if let Some((level, muted)) = crate::sys::volume() {
            self.quick_volume = Some(level);
            self.quick_muted = muted;
        }
        self.quick_brightness = crate::sys::brightness();
        self.quick_at = Some(Instant::now());
    }

    /// Set the Deck volume from the panel. The value is applied when the finger
    /// leaves the slider, not on every pixel of the drag.
    pub fn quick_set_volume(&mut self, percent: u8) {
        self.quick_volume = Some(percent.min(100));
        self.quick_muted = false;
        if let Err(e) = crate::sys::set_volume(percent, self.lang) {
            self.toast(e, false);
        }
    }

    pub fn quick_set_brightness(&mut self, percent: u8) {
        self.quick_brightness = Some(percent.clamp(1, 100));
        if let Err(e) = crate::sys::set_brightness(percent, self.lang) {
            self.toast(e, false);
        }
    }

    pub fn quick_toggle_mute(&mut self) {
        let lang = self.lang;
        self.spawn(move || Task::QuickMute(crate::sys::toggle_mute(lang)));
    }

    /// Read the application list from the active computer. One request at a
    /// time; a failed scan is shown in the page, not as a toast storm.
    pub fn load_control_apps(&mut self) {
        if self.control_apps_pending {
            return;
        }
        self.control_apps_pending = true;
        self.control_apps_at = Some(Instant::now());
        let Some(client) = self.client() else {
            self.control_apps_pending = false;
            self.control_apps_err = t!(
                self.lang,
                "сначала выберите компьютер",
                "choose a computer first"
            )
            .to_string();
            return;
        };
        self.spawn(move || Task::Apps(client.apps().map_err(|e| e.to_string())));
    }

    /// Ask the computer for a declared power action. Reboot and shutdown never
    /// reach this method without the confirmation on the page.
    pub fn remote_power(&mut self, action: sdw_core::model::PowerAction) {
        let Some(client) = self.client() else {
            self.toast(
                t!(self.lang, "нет активного компьютера", "no active computer"),
                false,
            );
            return;
        };
        let lang = self.lang;
        self.spawn(move || {
            Task::Msg(
                client
                    .power(action)
                    .map(|v| {
                        v.get("message")
                            .and_then(|m| m.as_str())
                            .map(str::to_string)
                            .unwrap_or_else(|| t!(lang, "готово", "done").to_string())
                    })
                    .map_err(|e| e.to_string()),
            )
        });
    }

    pub fn launch_app(&mut self, id: String, name: String) {
        let Some(client) = self.client() else {
            self.toast(
                t!(self.lang, "нет активного компьютера", "no active computer"),
                false,
            );
            return;
        };
        let lang = self.lang;
        self.spawn(move || {
            Task::Msg(
                client
                    .app_launch(&id)
                    .map(|_| format!("{name}: {}", t!(lang, "запущено", "started")))
                    .map_err(|e| e.to_string()),
            )
        });
    }

    pub fn save_ha_categories(&mut self) {
        let list = self.ha.categories.clone();
        if let Some(c) = self.client() {
            self.spawn(move || {
                Task::Categories(c.set_ha_categories(&list).map_err(|e| e.to_string()))
            });
        }
    }

    pub fn save_ha_presets(&mut self) {
        let list = self.ha.presets.clone();
        if let Some(c) = self.client() {
            self.spawn(move || Task::Presets(c.set_ha_presets(&list).map_err(|e| e.to_string())));
        }
    }

    /// Capture the current state of every visible light into a new preset.
    pub fn save_preset_from_lights(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let mut actions = sdw_core::ha::capture_light_preset(&self.ha.entities);
        if actions.is_empty() {
            self.toast(
                t!(
                    self.lang,
                    "нет включённых ламп — пресет пустой",
                    "no lights are on — the preset would be empty"
                ),
                false,
            );
            return;
        }
        let mut preset = HaPreset::new(name);
        preset.actions = std::mem::take(&mut actions);
        self.ha.presets.push(preset);
        self.ha.preset_name.clear();
        self.save_ha_presets();
        self.toast(
            t!(self.lang, "пресет сохранён", "preset saved").to_string(),
            true,
        );
    }

    /// Apply every action of a preset and show the guessed state right away.
    pub fn apply_preset(&mut self, preset: &HaPreset) {
        for a in &preset.actions {
            let call = HaCall {
                domain: "light".into(),
                service: a.service.clone(),
                target: serde_json::json!({ "entity_id": a.entity_id }),
                data: a.data.clone(),
            };
            let state = match a.service.as_str() {
                "turn_off" => Some("off".into()),
                "turn_on" => Some("on".into()),
                _ => None,
            };
            if let Some(state) = state {
                self.ha
                    .optimistic
                    .insert(a.entity_id.clone(), (state, Instant::now()));
            }
            self.ha_call(call);
        }
        self.ha.last_fetch = Instant::now() - Duration::from_secs(4);
        self.toast(
            format!(
                "{} · {}",
                t!(self.lang, "пресет", "preset"),
                sdw_core::util::truncate(&preset.name, 30)
            ),
            true,
        );
    }

    pub fn poll_media(&mut self) {
        if self.media.pending || self.client().is_none() {
            return;
        }
        if self.media.last_poll.elapsed() < Duration::from_millis(900) {
            return;
        }
        self.media.last_poll = Instant::now();
        self.media.pending = true;
        if let Some(c) = self.client() {
            self.spawn(move || Task::Media(c.media().map_err(|e| e.to_string())));
        }
    }

    /// Send one media/volume action; the state updates immediately so the UI
    /// never feels stuck waiting for the next poll.
    pub fn media_command(&mut self, action: &str, value: f64) {
        let player = self.media.state.player.clone();
        match action {
            "play_pause" => {
                self.media.state.status = if self.media.state.playing() {
                    "Paused".into()
                } else {
                    "Playing".into()
                };
            }
            "play" => self.media.state.status = "Playing".into(),
            "pause" => self.media.state.status = "Paused".into(),
            "stop" => self.media.state.status = "Stopped".into(),
            "volume" => {
                self.media.state.volume = value.clamp(0.0, 100.0) as u8;
                self.media.state.muted = false;
            }
            "mute" => self.media.state.muted = !self.media.state.muted,
            "seek" => {
                self.media.state.position = value.max(0.0);
                self.media.seeking = None;
            }
            _ => {}
        }
        let action = action.to_string();
        self.media.last_poll = Instant::now() - Duration::from_secs(5);
        if let Some(c) = self.client() {
            self.spawn(move || {
                Task::Msg(
                    c.media_control(&action, value, &player)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string()),
                )
            });
        }
    }

    pub fn select_media_player(&mut self, name: &str) {
        self.media.state.player = name.to_string();
        let name = name.to_string();
        self.media.last_poll = Instant::now() - Duration::from_secs(5);
        if let Some(c) = self.client() {
            self.spawn(move || {
                Task::Msg(
                    c.media_control("select", 0.0, &name)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string()),
                )
            });
        }
    }

    pub fn set_media_sink(&mut self, sink: &str) {
        let sink = sink.to_string();
        self.media.state.sink = sink.clone();
        if let Some(c) = self.client() {
            self.spawn(move || {
                Task::Msg(
                    c.media_control("sink", 0.0, &sink)
                        .map(|_| format!("выход: {sink}"))
                        .map_err(|e| e.to_string()),
                )
            });
        }
    }

    /// Decode and cache a notification picture. Returns a cheap handle clone;
    /// `None` when the notification has no picture or it cannot be decoded.
    pub fn notif_texture(
        &mut self,
        ctx: &egui::Context,
        id: u64,
        image: &str,
    ) -> Option<egui::TextureHandle> {
        if image.is_empty() {
            return None;
        }
        if let Some(slot) = self.notif_art.get(&id) {
            return slot.clone();
        }
        let tex = sdw_core::util::b64_decode(image)
            .and_then(|bytes| image::load_from_memory(&bytes).ok())
            .map(|img| {
                let img = img.to_rgba8();
                let (w, h) = img.dimensions();
                let img = if w.max(h) > 192 {
                    let scale = 192.0 / w.max(h) as f32;
                    let nw = (w as f32 * scale).max(1.0) as u32;
                    let nh = (h as f32 * scale).max(1.0) as u32;
                    image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle)
                } else {
                    img
                };
                let (w, h) = img.dimensions();
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [w as usize, h as usize],
                    img.as_raw(),
                );
                ctx.load_texture(
                    format!("notif-art-{id}"),
                    color,
                    egui::TextureOptions::LINEAR,
                )
            });
        self.notif_art.insert(id, tex.clone());
        tex
    }

    fn ensure_media_art(&mut self, ctx: &egui::Context) {
        let id = self.media.state.art_id.clone();
        if id.is_empty() {
            self.media.art = None;
            self.media.art_id.clear();
            self.media.art_bytes = None;
            return;
        }
        if self.media.art_id != id {
            self.media.art_id = id.clone();
            self.media.art = None;
            self.media.art_error.clear();
        }
        if self.media.art.is_none() {
            if let Some((bytes_id, bytes)) = self.media.art_bytes.take() {
                if bytes_id == id {
                    match image::load_from_memory(&bytes) {
                        Ok(img) => {
                            let img = img.to_rgba8();
                            let (w, h) = img.dimensions();
                            let img = if w.max(h) > 900 {
                                let scale = 900.0 / w.max(h) as f32;
                                let nw = (w as f32 * scale).max(1.0) as u32;
                                let nh = (h as f32 * scale).max(1.0) as u32;
                                image::imageops::resize(
                                    &img,
                                    nw,
                                    nh,
                                    image::imageops::FilterType::Triangle,
                                )
                            } else {
                                img
                            };
                            let (w, h) = img.dimensions();
                            let color = egui::ColorImage::from_rgba_unmultiplied(
                                [w as usize, h as usize],
                                img.as_raw(),
                            );
                            self.media.art = Some(ctx.load_texture(
                                format!("media-art-{id}"),
                                color,
                                egui::TextureOptions::LINEAR,
                            ));
                        }
                        Err(e) => {
                            self.media.art_error = format!("обложка: {e}");
                        }
                    }
                }
            }
        }
        if self.media.art.is_none() && self.media.art_requested != id {
            let url = self.media.state.art_url.clone();
            self.media.art_requested = id.clone();
            if let Some(c) = self.client() {
                self.spawn(move || Task::MediaArt {
                    id,
                    result: c.media_art(&url).map_err(|e| e.to_string()),
                });
            }
        }
    }

    /// One-tap start/stop for the extra screen, used on the home page.
    pub fn toggle_monitor_default(&mut self) {
        let Some(c) = self.client() else {
            self.toast(
                t!(
                    self.lang,
                    "сначала подключите компьютер",
                    "connect a computer first"
                ),
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
                t!(
                    self.lang,
                    "в релизе нет файла для дека",
                    "no deck file in the release"
                ),
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

    /// Start or stop listening for a control to bind. The timestamp arms a
    /// short settle window, and buttons already down (a finger on the glass)
    /// are ignored by `on_input`.
    pub fn set_bind_capture(&mut self, on: bool) {
        self.binds.capture = on;
        self.binds.capture_since = on.then(Instant::now);
    }

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
            Action::Http {
                method,
                url,
                headers,
                body,
            } => {
                // An addon may call any API. The request lives in a worker so
                // a slow or unreachable host never freezes the page.
                let (method, url, headers, body) =
                    (method.clone(), url.clone(), headers.clone(), body.clone());
                let shown = method.clone();
                self.spawn(move || {
                    Task::Msg(
                        sdw_core::util::http_call(&method, &url, &headers, &body)
                            .map(|text| {
                                if text.is_empty() {
                                    format!("{shown} ok")
                                } else {
                                    text
                                }
                            })
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
            other => match crate::actions::run(other, self.lang) {
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
            DeckEvent::Pad(state) => {
                self.gamepad = state;
                self.last_pad = Instant::now();
            }
            DeckEvent::Devices(list, pad) => {
                self.input_devices = list;
                self.has_gamepad = pad;
            }
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
                        if !self.gamepad_on && !self.binds.capture && pad_key(code) {
                            self.pad_press.push(code);
                            if self.pad_press.len() > 16 {
                                self.pad_press.remove(0);
                            }
                        }
                        if self.binds.capture {
                            let armed = self
                                .binds
                                .capture_since
                                .map(|at| at.elapsed() >= Duration::from_millis(300))
                                .unwrap_or(true);
                            // 0x14a is BTN_TOUCH: a finger on the glass is not a
                            // control the user meant to bind.
                            if fresh && armed && !input::is_touch_event(code) {
                                binds::capture_button(self, code, &label, &device);
                                return;
                            }
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
        let pages = self.dock_pages();
        if pages.is_empty() {
            return;
        }
        let idx = pages.iter().position(|p| *p == self.page).unwrap_or(0) as i32;
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
        // A swipe that did not commit eases back to zero instead of snapping.
        let dragging = self.edge_swipe.is_some() || !self.touch.points.is_empty();
        if !dragging && self.page_slide != 0.0 {
            self.page_slide = if self.page_slide.abs() < 1.0 {
                0.0
            } else {
                self.page_slide * 0.72
            };
            ctx.request_repaint();
        }
        if self.last_clock.elapsed() >= Duration::from_secs(1) {
            self.last_clock = Instant::now();
            self.clock = theme::clock_text();
        }
        if self.last_battery.elapsed() >= Duration::from_secs(30) {
            self.last_battery = Instant::now();
            self.battery = read_battery();
        }
        if self.last_headset.elapsed() >= Duration::from_secs(60) {
            self.last_headset = Instant::now();
            self.spawn(|| Task::Headset(read_headset()));
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
        if self.page == Page::HomeAssistant && self.ha.last_fetch.elapsed() > Duration::from_secs(4)
        {
            self.load_ha();
        }
        if self.page == Page::Media {
            self.poll_media();
            self.ensure_media_art(ctx);
        }
        self.ensure_stream();
        self.pump_frames(ctx);

        if self
            .toast
            .as_ref()
            .is_some_and(|t| t.at.elapsed() > Duration::from_secs(4))
        {
            self.toast = None;
        }

        // A capture that never sees a button must not leave the Deck listening
        // forever: give up after a few seconds and keep the editor open.
        if self.binds.capture
            && self
                .binds
                .capture_since
                .is_some_and(|at| at.elapsed() > Duration::from_secs(12))
        {
            self.binds.capture = false;
            self.binds.capture_since = None;
            self.toast(
                t!(
                    self.lang,
                    "запись отменена: кнопка не нажата",
                    "capture cancelled: no button pressed"
                ),
                false,
            );
        }

        self.swipe_tick(ctx);

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.go_back(ctx);
        }
    }

    fn on_task(&mut self, task: Task) {
        match task {
            Task::Status(Ok(status)) => {
                self.status_pending = false;
                self.status_err.clear();
                // A daemon that restarted has no Home Assistant state at all.
                // Dropping ours beats showing values from the previous run as
                // if they were live; the page shows "обновляю…" until the
                // first fresh answer.
                let restarted = self
                    .status
                    .as_ref()
                    .map(|old| status.uptime_secs + 2 < old.uptime_secs)
                    .unwrap_or(false);
                if restarted {
                    self.ha.entities.clear();
                    self.ha.views.clear();
                    self.ha.optimistic.clear();
                    self.ha.optimistic_pct.clear();
                    self.ha.loaded = false;
                    self.ha.age_ms = 0;
                    self.ha.connected = false;
                    self.ha.error.clear();
                    self.ha.stale_error.clear();
                    self.ha.last_fetch = Instant::now() - Duration::from_secs(60);
                }
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
                        if let Some(existing) =
                            self.cfg.hosts.iter_mut().find(|h| h.id == pair.daemon_id)
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
                        if self.cfg.notify_popup && self.page != Page::Notifications {
                            self.popups.push(NotifPopup {
                                id: n.id,
                                app: n.app.clone(),
                                summary: n.summary.clone(),
                                body: n.body.clone(),
                                image: n.image.clone(),
                                urgency: n.urgency,
                                at: Instant::now(),
                            });
                            if self.popups.len() > 3 {
                                self.popups.remove(0);
                            }
                        }
                        self.notifications.push(n);
                        added += 1;
                    }
                }
                self.notifications.sort_by_key(|n| n.id);
                if self.notifications.len() > 80 {
                    let cut = self.notifications.len() - 80;
                    self.notifications.drain(..cut);
                }
                let live: HashSet<u64> = self.notifications.iter().map(|n| n.id).collect();
                self.notif_art.retain(|id, _| live.contains(id));
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
                self.ha.connected = states.connected;
                self.ha.age_ms = states.age_ms;
                self.ha.stale_error = states.error.clone();
                self.ha.error.clear();
                // A failed fetch keeps the last entities on purpose: the page
                // then says how old they are instead of going blank.
                if !states.entities.is_empty() || states.fetched_ms > 0 {
                    self.ha.entities = states.entities;
                    self.ha.areas = states.areas;
                    self.ha.views = states.views;
                    self.ha.categories = states.categories;
                    self.ha.presets = states.presets;
                    self.ha.last_fetch = Instant::now();
                }
                self.ha.optimistic.retain(|id, (state, at)| {
                    at.elapsed() < Duration::from_secs(6)
                        && self
                            .ha
                            .entities
                            .iter()
                            .find(|e| &e.entity_id == id)
                            .map(|e| &e.state != state)
                            .unwrap_or(false)
                });
                self.ha.optimistic_pct.retain(|id, (pct, at)| {
                    at.elapsed() < Duration::from_secs(8)
                        && self
                            .ha
                            .entities
                            .iter()
                            .find(|e| &e.entity_id == id)
                            .and_then(|e| e.brightness_pct())
                            .map(|real| real.abs_diff(*pct) > 2)
                            .unwrap_or(false)
                });
            }
            Task::Ha(Err(e)) => {
                self.ha.pending = false;
                self.ha.error = e;
            }
            Task::Media(Ok(state)) => {
                self.media.pending = false;
                if state.error.is_empty() || state.available {
                    self.media.error.clear();
                } else {
                    self.media.error = state.error.clone();
                }
                if let Some(pos) = self.media.seeking {
                    let mut state = state;
                    state.position = pos;
                    self.media.state = state;
                } else {
                    self.media.state = state;
                }
            }
            Task::Media(Err(e)) => {
                self.media.pending = false;
                self.media.error = e;
            }
            Task::MediaArt { id, result } => match result {
                Ok(bytes) => {
                    if id == self.media.art_id {
                        self.media.art_error.clear();
                        self.media.art_bytes = Some((id, bytes));
                    }
                }
                Err(e) => {
                    if id == self.media.art_id {
                        self.media.art_error = e;
                    }
                }
            },
            Task::Categories(Ok(list)) => {
                self.ha.categories = list;
            }
            Task::Categories(Err(e)) => self.toast(e, false),
            Task::Presets(Ok(list)) => {
                self.ha.presets = list;
            }
            Task::Presets(Err(e)) => self.toast(e, false),
            Task::Audio(Ok(status)) => {
                self.audio_starting = false;
                if let Some(s) = self.status.as_mut() {
                    s.audio = status.clone();
                }
                if status.running {
                    self.audio_error.clear();
                    crate::audio_ui::spawn_player(self);
                }
            }
            Task::Audio(Err(e)) => {
                self.audio_starting = false;
                self.audio_error = e.clone();
                self.toast(format!("звук: {e}"), false);
            }
            Task::AudioApps(Ok(apps)) => {
                self.audio_apps = apps;
                self.audio_apps_at = Some(Instant::now());
            }
            Task::AudioApps(Err(e)) => {
                self.audio_error = e.clone();
                self.audio_apps_at = Some(Instant::now());
            }
            Task::Msg(Ok(message)) => {
                if !message.is_empty() {
                    self.toast(message, true);
                }
                self.last_status = Instant::now() - Duration::from_secs(5);
            }
            Task::Msg(Err(e)) => self.toast(e, false),
            Task::Headset(found) => self.headset = found,
            Task::Catalog(list, errors) => {
                self.addon_catalog_pending = false;
                self.addon_catalog = list;
                self.addon_catalog_err = errors;
            }
            Task::AddonInstalled(Ok((addon, note))) => {
                self.addon_installing = None;
                let id = addon.id().to_string();
                self.cfg.addons.set(&id, true);
                crate::addons_ui::defaults(self, &addon);
                self.save();
                let name = addon.manifest.name.clone();
                self.toast(
                    if note.is_empty() {
                        format!(
                            "{} {name}",
                            t!(self.lang, "аддон установлен:", "addon installed:")
                        )
                    } else {
                        format!(
                            "{} {name}; {}: {note}",
                            t!(self.lang, "аддон установлен:", "addon installed:"),
                            t!(self.lang, "компьютер", "computer")
                        )
                    },
                    note.is_empty(),
                );
            }
            Task::AddonInstalled(Err(e)) => {
                self.addon_installing = None;
                self.toast(format!("{}: {e}", t!(self.lang, "аддон", "addon")), false);
            }
            Task::Apps(Ok(list)) => {
                self.control_apps_pending = false;
                self.control_apps = list;
                self.control_apps_err.clear();
            }
            Task::Apps(Err(e)) => {
                self.control_apps_pending = false;
                self.control_apps_err = e;
            }
            Task::Bluetooth(Ok(status)) => {
                self.bluetooth_pending = false;
                self.bluetooth_err = status.error.clone();
                self.bluetooth = Some(status);
            }
            Task::Bluetooth(Err(e)) => {
                self.bluetooth_pending = false;
                self.bluetooth_err = e;
            }
            Task::BtAction(Ok(message)) => {
                self.toast(message, true);
                self.bluetooth_at = None;
            }
            Task::BtAction(Err(e)) => self.toast(e, false),
            Task::QuickMute(Ok(muted)) => {
                self.quick_muted = muted;
                if self.quick_volume == Some(0) && !muted {
                    self.quick_volume = Some(30);
                }
            }
            Task::QuickMute(Err(e)) => self.toast(e, false),
        }
    }

    // ------------------------------------------------------------ chrome

    fn chrome_ui(&mut self, root: &mut Ui, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        // One cubic ease for the whole page swap: it fades, rises a touch and
        // slides in from the side you came from. Margins move with it so the
        // content never spills outside the panel.
        let anim = if theme::style().animations {
            ctx.animate_bool_with_time(Id::new(("page", self.page_epoch)), true, 0.28)
        } else {
            1.0
        };
        let ease = 1.0 - (1.0 - anim).powi(3);
        let width = ctx.content_rect().width().max(1.0);
        // A wider travel than before: the new space clearly comes from the
        // side the swipe went, while the live finger offset is added on top.
        let travel = (width * 0.12).clamp(36.0, 220.0);
        let slide = (1.0 - ease) * travel * self.page_dir + self.page_slide;
        let left = (34.0 + slide).max(4.0) as i8;
        let right = (34.0 - slide).max(4.0) as i8;
        let drag_fade = 1.0 - (self.page_slide.abs() / width * 0.5).min(0.35);
        egui::Panel::top("deck-top")
            .exact_size(50.0)
            .frame(
                egui::Frame::new()
                    .fill(t.bg)
                    .inner_margin(Margin::symmetric(22, 4)),
            )
            .show(root, |ui| self.top_bar(ui, &t));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.bg).inner_margin(Margin {
                left,
                right,
                top: 4,
                bottom: 4,
            }))
            .show(root, |ui| {
                theme::backdrop(ui, &t);
                ui.set_opacity(ease * drag_fade);
                ui.add_space((1.0 - ease) * 14.0);
                self.page_scroll_id = Some(
                    ui.make_persistent_id(egui::IdSalt::new("deck-page"))
                        .with("area"),
                );
                ScrollArea::vertical()
                    .id_salt("deck-page")
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
                            1 => {
                                if !open_system_keyboard() {
                                    self.osk_open = !self.osk_open;
                                }
                            }
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
        let anchor = if st.dock_top {
            (egui::Align2::CENTER_TOP, egui::vec2(0.0, 8.0))
        } else {
            (egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -8.0))
        };
        egui::Area::new(Id::new("deck-dock"))
            .anchor(anchor.0, anchor.1)
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
                                for page in self.dock_pages() {
                                    let active = current == page;
                                    let resp = ui
                                        .add(
                                            egui::Button::new(theme::icon(
                                                page.icon(),
                                                if active {
                                                    st.dock_icon + 1.0
                                                } else {
                                                    st.dock_icon
                                                },
                                                if active { t.strong } else { t.muted },
                                            ))
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
                                            .min_size(Vec2::new(
                                                st.dock_icon + 19.0,
                                                st.dock_icon + 15.0,
                                            )),
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
            Page::Clock => crate::pages::clock(self, ui, t),
            Page::Computers => crate::pages::computers(self, ui, t),
            Page::Control => crate::pages::control(self, ui, t),
            Page::Bluetooth => crate::pages::bluetooth(self, ui, t),
            Page::Gamepad => crate::pages::gamepad(self, ui, t),
            Page::Media => crate::pages::media(self, ui, t),
            Page::Screen => crate::pages::screen(self, ui, t),
            Page::Audio => crate::pages::audio(self, ui, t),
            Page::Notifications => crate::pages::notifications(self, ui, t),
            Page::HomeAssistant => crate::pages::home_assistant(self, ui, t),
            Page::Binds => binds::list_ui(self, ui, t),
            Page::Addons => crate::pages::addons(self, ui, t),
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
                                egui::Button::new(theme::icon(
                                    theme::icon::CLOSE,
                                    22.0,
                                    Color32::WHITE,
                                ))
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
                    t!(
                        self.lang,
                        "ждём первый кадр…",
                        "waiting for the first frame…"
                    )
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
                            ui.label(theme::hud(t!(self.lang, "клавиатура", "keyboard"), t.faint));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if theme::ghost(ui, t!(self.lang, "готово", "done"), &t).clicked()
                                {
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
                                [
                                    "z",
                                    "x",
                                    "c",
                                    "v",
                                    "b",
                                    "n",
                                    "m",
                                    ".",
                                    "@",
                                    theme::icon::BACKSPACE,
                                ],
                            ]
                        };
                        for (ri, row) in rows.iter().enumerate() {
                            ui.horizontal(|ui| {
                                if ri == 2 {
                                    if theme::key_button(ui, theme::icon::SHIFT, &t, 58.0).clicked()
                                    {
                                        self.osk_shift = !self.osk_shift;
                                    }
                                }
                                for key in row {
                                    let label = if key.len() == 1
                                        && self.osk_shift
                                        && !self.osk_symbols
                                        && key
                                            .chars()
                                            .next()
                                            .is_some_and(|c| c.is_ascii_alphabetic())
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
                                    if theme::key_button(ui, theme::icon::RETURN, &t, 58.0)
                                        .clicked()
                                    {
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
                            if theme::key_button(ui, t!(self.lang, "пробел", "space"), &t, 300.0)
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

    /// When a text field takes focus, call the system on-screen keyboard
    /// (the Steam Deck one, via Steam) instead of drawing our own. The built-in
    /// keyboard is only a fallback for machines without Steam.
    fn keyboard_tick(&mut self, ctx: &egui::Context) {
        let wants = ctx.egui_wants_keyboard_input();
        if wants && !self.kb_open {
            self.kb_open = true;
            if !open_system_keyboard() {
                self.osk_open = true;
            }
        } else if !wants && self.kb_open {
            self.kb_open = false;
        }
    }

    /// Notification popups: a small Grid card that eases in from the right,
    /// lives for about five seconds and can be tapped away.
    fn popup_ui(&mut self, ctx: &egui::Context) {
        if self.popups.is_empty() {
            return;
        }
        let t = Tokens::get(ctx);
        let now = Instant::now();
        self.popups
            .retain(|p| now.duration_since(p.at) < Duration::from_millis(5400));
        let mut dismiss: Option<u64> = None;
        let items: Vec<NotifPopup> = self.popups.clone();
        for (i, p) in items.iter().enumerate() {
            let art = self.notif_texture(ctx, p.id, &p.image);
            let age = now.duration_since(p.at).as_secs_f32();
            let appear = (age / 0.22).clamp(0.0, 1.0);
            let ease = 1.0 - (1.0 - appear).powi(3);
            let fade = ((5.4 - age) / 0.4).clamp(0.0, 1.0);
            let alpha = ease.min(fade).clamp(0.0, 1.0);
            let color = match p.urgency {
                0 => t.accent,
                1 => t.warn,
                _ => t.bad,
            };
            let offset = Vec2::new(-18.0, 64.0 + i as f32 * 108.0 - (1.0 - ease) * 14.0);
            egui::Area::new(Id::new(("notif-popup", p.id)))
                .anchor(Align2::RIGHT_TOP, offset)
                .order(egui::Order::Tooltip)
                .show(ctx, |ui| {
                    ui.set_opacity(alpha);
                    ui.set_max_width(360.0);
                    egui::Frame::new()
                        .fill(t.bg2)
                        .stroke(Stroke::new(1.0, color.gamma_multiply(0.65)))
                        .corner_radius(CornerRadius::ZERO)
                        .inner_margin(Margin::symmetric(14, 12))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if let Some(tex) = &art {
                                    let (rect, _) = ui.allocate_exact_size(
                                        Vec2::splat(40.0),
                                        egui::Sense::hover(),
                                    );
                                    let uv = egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    );
                                    ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
                                    ui.painter().rect_stroke(
                                        rect,
                                        CornerRadius::ZERO,
                                        Stroke::new(1.0, t.line),
                                        egui::StrokeKind::Inside,
                                    );
                                    ui.add_space(10.0);
                                }
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        if art.is_none() {
                                            ui.label(theme::icon(
                                                crate::pages::notif_glyph(&p.app, ""),
                                                15.0,
                                                color,
                                            ));
                                        }
                                        ui.label(
                                            RichText::new(if p.app.is_empty() {
                                                "system"
                                            } else {
                                                &p.app
                                            })
                                            .family(FontFamily::Monospace)
                                            .size(10.5)
                                            .color(color),
                                        );
                                    });
                                    ui.label(
                                        RichText::new(&p.summary)
                                            .size(15.0)
                                            .strong()
                                            .color(t.strong),
                                    );
                                    if !p.body.is_empty() {
                                        ui.label(
                                            RichText::new(sdw_core::util::truncate(&p.body, 120))
                                                .size(12.0)
                                                .color(t.muted),
                                        );
                                    }
                                });
                                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(theme::hud("×", t.muted))
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::NONE),
                                        )
                                        .clicked()
                                    {
                                        dismiss = Some(p.id);
                                    }
                                });
                            });
                        });
                });
        }
        if let Some(id) = dismiss {
            self.popups.retain(|p| p.id != id);
        }
        // Keep the fade smooth while at least one popup is alive.
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    /// The pull-down panel and the small handle that always opens it.
    fn quick_ui(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        let blocked = self.clock_full || self.stream.fullscreen || self.binds.capture;
        if blocked {
            if self.quick_open {
                self.set_quick(false);
            }
            return;
        }
        if self.quick_open
            && self
                .quick_at
                .map(|at| at.elapsed() > Duration::from_secs(2))
                .unwrap_or(true)
        {
            self.quick_values();
        }

        if !self.quick_open {
            let pull = self.quick_pull.max(0.0);
            let mut toggle = false;
            egui::Area::new(Id::new("deck-quick-handle"))
                .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 1.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(110.0, 12.0), Sense::click());
                    let body = egui::Rect::from_center_size(
                        rect.center(),
                        Vec2::new(46.0 + pull.min(60.0) * 1.3, 3.0),
                    );
                    ui.painter().rect_filled(
                        body,
                        CornerRadius::ZERO,
                        if pull > 4.0 { t.accent } else { t.line_strong },
                    );
                    if resp.clicked() {
                        toggle = true;
                    }
                });
            if toggle {
                self.set_quick(true);
            }
        }

        crate::pages::quick_panel(self, ctx, &t);
        if self.quick_open
            || ctx.animate_bool_with_time(Id::new("deck-quick"), self.quick_open, 0.16) > 0.001
        {
            ctx.request_repaint();
        }
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
        self.touch_gesture_tick(ctx, raw);
    }

    fn ui(&mut self, root: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.maybe_check_updates();
        self.tick(&ctx);
        self.keyboard_tick(&ctx);
        if let Some(at) = self.save_at {
            if at.elapsed() >= Duration::from_millis(400) {
                self.save_at = None;
                self.save();
            }
        }
        crate::audio_ui::tick(self);
        if self.clock_full {
            crate::clock::fullscreen(self, root, &ctx);
        } else if self.stream.fullscreen {
            self.fullscreen_ui(root, &ctx);
        } else {
            self.chrome_ui(root, &ctx);
        }
        crate::audio_ui::confirm_modal(self, &ctx);
        self.osk_ui(&ctx);
        self.popup_ui(&ctx);
        self.toast_ui(&ctx);
        self.pad_cursor_ui(&ctx);
        self.quick_ui(&ctx);

        let ms = if self.stream.feed.is_some() {
            24
        } else if self.gamepad_on
            || self.binds.capture
            || deck_mode()
            || self.has_gamepad
            || self.last_pad.elapsed() < Duration::from_secs(2)
        {
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

/// Ask the system for its keyboard. On the Steam Deck this is Steam's own
/// on-screen keyboard; on a desktop with Steam installed it is the same one.
fn open_system_keyboard() -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::process::{Command, Stdio};
        let mut cmd = Command::new("steam");
        cmd.arg("steam://open/keyboard")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        return cmd.spawn().is_ok();
    }
    #[allow(unreachable_code)]
    false
}

/// Battery of a connected headset. UPower knows about Bluetooth earbuds, and a
/// few USB headsets expose a plain power supply; try both, cheapest first.
fn read_headset() -> Option<Headset> {
    #[cfg(target_os = "linux")]
    {
        if let Some(found) = headset_from_sysfs() {
            return Some(found);
        }
        if let Some(found) = headset_from_upower() {
            return Some(found);
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn headset_from_sysfs() -> Option<Headset> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !["head", "bud", "ear"].iter().any(|k| name.contains(k)) {
            continue;
        }
        let base = entry.path();
        let percent = std::fs::read_to_string(base.join("capacity"))
            .ok()
            .and_then(|text| text.trim().parse::<u8>().ok());
        let Some(percent) = percent else {
            continue;
        };
        let model = std::fs::read_to_string(base.join("model_name"))
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string());
        return Some(Headset {
            name: model,
            percent: percent.min(100),
        });
    }
    None
}

#[cfg(target_os = "linux")]
fn headset_from_upower() -> Option<Headset> {
    let out = std::process::Command::new("upower")
        .arg("-d")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut blocks: Vec<(bool, String, Option<u8>)> = Vec::new();
    let mut current: Option<(bool, String, Option<u8>)> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Device:") {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            let device = rest.trim().to_lowercase();
            let head = ["headphone", "headset"].iter().any(|k| device.contains(k));
            current = Some((head, String::new(), None));
            continue;
        }
        let Some(block) = current.as_mut() else {
            continue;
        };
        let text = line.trim();
        if text.eq_ignore_ascii_case("headphones") || text.eq_ignore_ascii_case("headset") {
            block.0 = true;
        }
        if let Some(value) = text.strip_prefix("model:") {
            block.1 = value.trim().to_string();
        }
        if let Some(value) = text.strip_prefix("percentage:") {
            block.2 = value
                .trim()
                .trim_end_matches('%')
                .parse::<u8>()
                .ok()
                .map(|p| p.min(100));
        }
    }
    if let Some(block) = current {
        blocks.push(block);
    }
    blocks
        .into_iter()
        .find(|(head, _, percent)| *head && percent.is_some())
        .map(|(_, model, percent)| Headset {
            name: if model.is_empty() {
                "headset".into()
            } else {
                model
            },
            percent: percent.unwrap_or(0),
        })
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
pub(crate) fn deck_mode() -> bool {
    std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some() || sdw_core::util::is_steam_deck()
}

/// Upper bound for the ui scale slider. A 1280x800 deck screen cannot carry
/// the same 1.4x zoom a desktop window can.
pub fn max_ui_scale() -> f32 {
    if deck_mode() {
        1.5
    } else {
        1.7
    }
}

/// Buttons that drive the deck UI itself (not the virtual pad).
fn pad_key(code: u16) -> bool {
    matches!(
        code,
        input::BTN_SOUTH
            | input::BTN_EAST
            | input::BTN_NORTH
            | input::BTN_TL
            | input::BTN_TR
            | input::BTN_SELECT
            | input::BTN_START
            | input::BTN_MODE
    )
}

/// Radial-ish deadzone: keep the centre still, then ramp to full speed.
fn deadzone(v: f32, dz: f32) -> f32 {
    if v.abs() <= dz {
        0.0
    } else {
        (v - dz.copysign(v)) / (1.0 - dz)
    }
}

#[cfg(test)]
mod touch_tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 500.0))
    }

    #[test]
    fn one_finger_horizontal_flick_is_a_swipe() {
        let mut track = TouchTrack::default();
        let now = Instant::now();
        let r = rect();
        track.begin(1, egui::pos2(700.0, 250.0), r, now);
        track.moved(1, egui::pos2(300.0, 260.0));
        let spec = track
            .end(
                1,
                egui::pos2(300.0, 260.0),
                r,
                now + Duration::from_millis(200),
            )
            .expect("a flick across the screen is a swipe");
        assert_eq!(spec.kind, GestureKind::Swipe);
        assert_eq!(spec.fingers, 1);
        assert_eq!(spec.dir, Some(SwipeDir::Left));
    }

    #[test]
    fn one_finger_vertical_drag_is_not_a_swipe() {
        let mut track = TouchTrack::default();
        let now = Instant::now();
        let r = rect();
        track.begin(1, egui::pos2(400.0, 400.0), r, now);
        track.moved(1, egui::pos2(430.0, 100.0));
        assert!(track
            .end(
                1,
                egui::pos2(430.0, 100.0),
                r,
                now + Duration::from_millis(300)
            )
            .is_none());
    }

    #[test]
    fn one_finger_tap_is_not_a_swipe() {
        let mut track = TouchTrack::default();
        let now = Instant::now();
        let r = rect();
        track.begin(1, egui::pos2(400.0, 200.0), r, now);
        assert!(track
            .end(
                1,
                egui::pos2(402.0, 201.0),
                r,
                now + Duration::from_millis(80)
            )
            .is_none());
    }
}
