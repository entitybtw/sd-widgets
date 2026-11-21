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

/// The deck's own screen going to the computer. One frame is in the air at a
/// time; the last result names what the computer is showing.
pub struct ShareState {
    pub in_flight: bool,
    /// What the computer said last time it was asked; a viewer on the PC
    /// makes the deck share.
    pub want: bool,
    pub want_at: Instant,
    /// Nobody is watching or the link is down: back off to one frame a while.
    pub quiet: Instant,
    pub last_request: Instant,
    pub sent: u64,
    pub error: String,
    /// Size of the frame the computer last accepted, for the settings page.
    pub size: (u32, u32),
}

impl Drop for StreamFeed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A colour the user just asked for, held until the lamp reports it back. A
/// temperature tap also has to hide the old kelvin reading, otherwise the tick
/// stays on the previous preset.
#[derive(Clone, Copy, Debug)]
pub struct OptimisticColor {
    pub rgb: [u8; 3],
    pub kelvin: bool,
    pub at: Instant,
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
    /// Colour right after a swatch tap, so the tick lands on the pressed
    /// swatch immediately instead of after a Home Assistant round-trip.
    pub optimistic_rgb: HashMap<String, OptimisticColor>,
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
    /// Speed and time left, worked out while the bar is drawn. The byte
    /// counter alone cannot say either, and the page needs both to look
    /// alive.
    pub view: crate::theme::ProgressView,
    /// The previous byte reading and when it was taken, for the speed.
    pub sampled: Option<(u64, Instant)>,
    /// The new file is in place and only a restart is left.
    pub installed: bool,
}

pub struct BindsState {
    pub editor: Option<BindEditor>,
    pub capture: bool,
    /// When capture was switched on. The first ~300 ms are ignored so the tap
    /// that opened the editor cannot become the recorded control.
    pub capture_since: Option<Instant>,
    pub filter: String,
    /// Search text of the application picker in a Launch action. Kept apart
    /// from `filter` so the bind list keeps its own search.
    pub launch_filter: String,
    /// Set by a headless check (`SDW_BIND`): a bind id and an action kind to
    /// open in the editor right away.
    pub open: Option<(String, crate::binds::ActionKind)>,
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
    NotifLog(Result<Vec<Notification>, String>),
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
    /// One application's picture from the computer.
    AppIcon {
        id: String,
        result: Result<Vec<u8>, String>,
    },
    /// Bluetooth adapter and devices.
    Bluetooth(Result<sdw_core::model::BluetoothStatus, String>),
    /// Result of a Bluetooth action; the page reloads afterwards.
    BtAction(Result<String, String>),
    /// The Deck sink was muted or unmuted.
    QuickMute(Result<bool, String>),
    /// The Deck's own volume (level, muted) and panel brightness.
    QuickValues(Option<(u8, bool)>, Option<u8>),
    /// A notification was activated on the computer (or refused).
    NotifActivated {
        id: u64,
        result: Result<(), String>,
    },
    /// One frame of this screen went to the computer: its size, or the reason
    /// it did not.
    Shared(Result<(u32, u32), String>),
    /// Whether the computer is watching this deck's screen right now.
    DeckWant(Result<bool, String>),
    /// The computer's own sound level just now (0..=100).
    PcVolume(u8),
}

/// Multi-finger touch state from `egui::Event::Touch`.
///
/// The compositor may deliver the touchscreen either as native touch events or
/// as a plain mouse. This tracks the native case; the mouse fallback lives in
/// [`App::swipe_raw`]. A single finger is still a tap or a vertical scroll for
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
    /// It makes the page follow the finger instead of jumping on release, and
    /// it keeps carrying the page through the turn after the finger is gone.
    pub page_slide: f32,
    /// The turn that follows the release: the offset the finger left, the one
    /// the page must reach and when the turn started. `page_turn_to_page` is
    /// the page waiting at the end (`None` when the page goes back).
    pub page_turn_from: f32,
    pub page_turn_to: f32,
    pub page_turn_at: Instant,
    /// How long the turn runs and the speed it starts with: the release keeps
    /// the speed the finger gave the page and loses it smoothly.
    pub page_turn_dur: f32,
    pub page_turn_vel: f32,
    pub page_turn_to_page: Option<Page>,
    pub page_turn_active: bool,
    /// Where the finger grabbed the page. The preview is measured from this
    /// point, so the first frame of the gesture does not jump by the
    /// recognition threshold.
    pub page_anchor: Option<f32>,
    /// The page a tap or an arrow just left, its scroll epoch and when: it is
    /// drawn sliding out while the new page comes in, so nothing vanishes.
    pub page_from: Option<(Page, u64, f32, Instant)>,
    /// The page was turned by a swipe, so it needs no fade-in of its own: the
    /// finger already showed where it comes from.
    pub page_instant: bool,
    /// The swipe already played its own cue; `set_page` must not play a second.
    pub page_silent: bool,
    pub page_at: Instant,
    pub status: Option<DaemonStatus>,
    pub status_err: String,
    pub status_pending: bool,
    pub last_status: Instant,
    /// When the last status really arrived. A computer that stops answering is
    /// shown offline after a short grace instead of staying "active" forever.
    pub status_ok_at: Instant,
    /// Debounced config write: a slider drag saves once, not on every frame.
    pub save_at: Option<Instant>,
    /// The clock is drawn over the whole window, without chrome.
    pub clock_full: bool,
    pub notifications: Vec<Notification>,
    /// Notifications the computer still remembers, newest first. The live list
    /// above is only what the daemon saw since the deck woke up; this one is
    /// the history that survives a closed lid or a restart on either side.
    pub notif_log: Vec<Notification>,
    pub notif_log_pending: bool,
    pub notif_log_loaded: bool,
    pub notif_log_err: String,
    /// Text of the search field over the history.
    pub notif_query: String,
    /// The page shows the history instead of what is live now.
    pub notif_hist: bool,
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
    /// Id of the monitor preview while it drives the computer. A drag there is
    /// the computer's, except when it starts at the screen edge: a page swipe
    /// must still work over the picture.
    pub stream_drag_id: Option<egui::Id>,
    /// How far the current page is scrolled down, in points. Pulling the
    /// control centre open is only allowed at the very top, so an ordinary
    /// downward scroll can never open it by accident.
    pub page_scroll_offset: f32,
    /// Raw multi-touch gestures from egui's touch events.
    pub touch: TouchTrack,

    pub stream: StreamState,
    pub share: ShareState,
    pub ha: HaState,
    /// The temporary web page that takes the Home Assistant token from a
    /// phone, while it is open.
    pub ha_link: Option<crate::ha_link::HaLink>,
    pub media: MediaUiState,
    /// The audio visualizer's bands, shared with the PCM player thread.
    pub viz: crate::viz::Shared,
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
    /// Application pictures, decoded once and kept while the session lives.
    pub app_icons: HashMap<String, egui::TextureHandle>,
    /// Pictures that arrived and wait for the UI thread to decode them.
    pub app_icon_bytes: HashMap<String, Vec<u8>>,
    /// Ids already asked for, so a missing picture is not requested forever.
    pub app_icon_wanted: HashSet<String>,
    pub app_icon_failed: HashSet<String>,
    /// Power action waiting for the second press.
    pub power_confirm: Option<sdw_core::model::PowerAction>,

    pub bluetooth: Option<sdw_core::model::BluetoothStatus>,
    pub bluetooth_pending: bool,
    pub bluetooth_at: Option<Instant>,
    pub bluetooth_err: String,
    /// Address waiting for a second press before it is removed.
    pub bluetooth_remove: Option<String>,
    /// The action in flight and the device it belongs to, so its row spins
    /// while BlueZ pairs or connects.
    pub bluetooth_busy: Option<(String, String)>,

    /// Pull-down control panel: the Deck's own volume and brightness plus the
    /// newest notifications.
    pub quick_open: bool,
    pub quick_volume: Option<u8>,
    pub quick_muted: bool,
    pub quick_brightness: Option<u8>,
    /// How far the panel sits above its open place, in points: 0 is open,
    /// `quick_height` is fully off screen. A pull from the top, a push up
    /// while it is open and the settle after a release all drive this one
    /// number, so the panel follows the finger instead of snapping.
    pub quick_off: f32,
    /// The panel as it was last drawn: how far it has to rise to be out.
    pub quick_height: f32,
    /// The settle after the finger left: from, to, and when it started.
    /// `quick_settle_open` says whether the panel is left open at the end.
    pub quick_off_from: f32,
    pub quick_off_to: f32,
    pub quick_off_at: Instant,
    pub quick_off_active: bool,
    pub quick_settle_open: bool,
    /// A read of volume and brightness is on its way; the panel skips the
    /// second one until it lands.
    pub quick_pending: bool,
    /// The panel opened on its own and still owes the state a read.
    pub quick_values_deferred: bool,
    pub quick_at: Option<Instant>,
    /// Finger offset while a top-edge pull-down is in progress.
    pub quick_pull: f32,
    /// A click landed on the pull handle or inside the panel this frame, so
    /// the full-screen clock must not treat it as "leave the clock".
    pub quick_clicked: bool,

    pub osk_open: bool,
    pub osk_shift: bool,
    pub osk_symbols: bool,

    pub toast: Option<Toast>,
    pub popups: Vec<NotifPopup>,
    /// Decoded notification pictures, keyed by notification id.
    pub notif_art: HashMap<u64, Option<egui::TextureHandle>>,
    /// How many pictures may still be decoded in this pass. Decoding a whole
    /// list of JPEGs at once is what made the notifications page hitch.
    pub notif_art_budget: u8,
    /// The pass the budget above belongs to.
    pub notif_art_pass: u64,
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
    /// The computer's system volume, as last read, and when. The deck's own
    /// slider follows it while the PC sound is playing, so the two never
    /// disagree.
    pub pc_volume: Option<(u8, Instant)>,
    /// How many more "yes" presses are needed before audio starts.
    pub audio_confirm_left: u8,
    /// The confirmation count as typed, kept between frames so the field can
    /// be emptied and filled again without the value resetting under the
    /// cursor.
    pub confirm_text: String,
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
            page_turn_from: 0.0,
            page_turn_to: 0.0,
            page_turn_at: Instant::now(),
            page_turn_dur: 0.3,
            page_turn_vel: 0.0,
            page_turn_to_page: None,
            page_turn_active: false,
            page_anchor: None,
            page_from: None,
            page_instant: false,
            page_silent: false,
            page_at: Instant::now(),
            status: None,
            status_err: String::new(),
            status_pending: false,
            last_status: Instant::now() - Duration::from_secs(5),
            status_ok_at: Instant::now() - Duration::from_secs(60),
            save_at: None,
            clock_full: false,
            notifications: Vec::new(),
            notif_log: Vec::new(),
            notif_log_pending: false,
            notif_log_loaded: false,
            notif_log_err: String::new(),
            notif_query: String::new(),
            notif_hist: false,
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
            stream_drag_id: None,
            page_scroll_offset: 0.0,
            touch: TouchTrack::default(),
            stream: StreamState {
                feed: None,
                texture: None,
                error: String::new(),
                frames: 0,
                last_frame_at: Instant::now() - Duration::from_secs(10),
                fullscreen: false,
                mode: cfg.stream.mode,
                position: cfg.stream.position,
                backend: cfg.stream.backend,
                width: cfg.stream.width.clamp(320, 7680),
                height: cfg.stream.height.clamp(200, 4320),
                fps: cfg.stream.fps.clamp(1, 120),
                quality: cfg.stream.quality.clamp(1, 100),
                input: cfg.stream.input,
                last_pointer: Instant::now() - Duration::from_secs(1),
                pointer_down: false,
            },
            share: ShareState {
                in_flight: false,
                want: false,
                want_at: Instant::now() - Duration::from_secs(10),
                quiet: Instant::now() - Duration::from_secs(10),
                last_request: Instant::now() - Duration::from_secs(10),
                sent: 0,
                error: String::new(),
                size: (0, 0),
            },
            ha: HaState {
                entities: Vec::new(),
                areas: Vec::new(),
                views: Vec::new(),
                categories: Vec::new(),
                presets: Vec::new(),
                optimistic: HashMap::new(),
                optimistic_pct: HashMap::new(),
                optimistic_rgb: HashMap::new(),
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
            ha_link: None,
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
            viz: crate::viz::shared(),
            binds: BindsState {
                editor: None,
                capture: false,
                capture_since: None,
                filter: String::new(),
                launch_filter: String::new(),
                open: std::env::var("SDW_BIND").ok().map(|v| {
                    let (id, kind) = v.split_once(':').unwrap_or((v.as_str(), "launch"));
                    let kind = match kind {
                        "shell" => crate::binds::ActionKind::Shell,
                        _ => crate::binds::ActionKind::Launch,
                    };
                    (id.to_string(), kind)
                }),
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
                view: crate::theme::ProgressView::default(),
                sampled: None,
                installed: false,
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
            notif_art_budget: 2,
            notif_art_pass: 0,
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
            app_icons: HashMap::new(),
            app_icon_bytes: HashMap::new(),
            app_icon_wanted: HashSet::new(),
            app_icon_failed: HashSet::new(),
            power_confirm: None,
            bluetooth: None,
            bluetooth_pending: false,
            bluetooth_at: None,
            bluetooth_err: String::new(),
            bluetooth_remove: None,
            bluetooth_busy: None,
            quick_open: false,
            quick_volume: None,
            quick_muted: false,
            quick_brightness: None,
            // Where the panel rests when closed, before it was ever drawn:
            // tall enough to be off screen on the first slide.
            quick_off: 560.0,
            quick_height: 560.0,
            quick_off_from: 0.0,
            quick_off_to: 0.0,
            quick_off_at: Instant::now(),
            quick_off_active: false,
            quick_settle_open: false,
            quick_pending: false,
            quick_values_deferred: false,
            quick_at: None,
            quick_pull: 0.0,
            quick_clicked: false,
            audio_player: None,
            pc_volume: None,
            audio_confirm_left: 0,
            confirm_text: cfg.audio.confirmations.to_string(),
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
    /// a plain mouse, [`Self::swipe_raw`] covers the single-finger case.
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
                            self.page_anchor = None;
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
                if self.drag_blocks_page(id) {
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
                // The gesture is recognised at 24 points; the page starts
                // moving from the point where it was recognised, not from
                // zero, so the first preview frame is not a jump.
                if self.page_anchor.is_none()
                    && delta.x.abs() > 24.0
                    && delta.x.abs() > delta.y.abs() * 1.6
                {
                    self.page_anchor = Some(delta.x);
                }
                if let Some(anchor) = self.page_anchor {
                    if delta.x.abs() > delta.y.abs() * 1.2 {
                        let w = rect.width().max(1.0);
                        self.page_slide = (delta.x - anchor).clamp(-w, w);
                    }
                }
            }
        }
        if let Some(spec) = finished {
            if spec.fingers == 1 && spec.kind == GestureKind::Swipe && !self.touch_blocked {
                // egui-winit also turns the touch into a mouse drag, and the
                // raw swipe below would turn the page a second time. Remember
                // the gesture so that fallback stands down for this release.
                self.last_gesture = Some((spec.clone(), Instant::now()));
                // The same smooth turn as a mouse swipe: the preview stays
                // where the finger left it and the release finishes it.
                let w = rect.width().max(1.0);
                let dir = if spec.dir == Some(SwipeDir::Left) {
                    -1.0
                } else {
                    1.0
                };
                sound::play(
                    &self.cfg.sound,
                    if dir < 0.0 { Cue::Navigate } else { Cue::Back },
                );
                self.page_silent = true;
                self.start_page_turn(
                    dir * w,
                    self.step_page(if dir < 0.0 { 1 } else { -1 }),
                    w,
                    0.0,
                );
                self.page_silent = false;
            } else {
                self.on_input(DeckEvent::Gesture(spec));
            }
            self.page_anchor = None;
        }
    }

    /// Horizontal swipe anywhere on the page, with a lower threshold when it
    /// starts at an edge. This is the fallback for a touchscreen that the
    /// compositor (or Steam Input) delivers as a mouse: there are no touch
    /// events to count fingers with, but a horizontal drag still works.
    ///
    /// Runs on the raw input, before egui turns the release into a click. A
    /// real drag is announced with a `PointerMoved` event first, so the tile
    /// under the finger can never activate after a swipe.
    /// Whether a drag claimed by `id` must keep the page from turning. The
    /// page ScrollArea never blocks, and neither does the monitor preview:
    /// a horizontal swipe over the picture is still a page turn, while
    /// clicks and vertical drags reach the computer.
    fn drag_blocks_page(&self, id: egui::Id) -> bool {
        Some(id) != self.page_scroll_id && Some(id) != self.stream_drag_id
    }

    /// A plain mouse drag (the Deck's touchscreen often arrives this way).
    fn swipe_raw(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if !deck_mode()
            || self.gamepad_on
            || self.binds.capture
            || self.stream.fullscreen
            || self.binds.editor.is_some()
        {
            self.edge_swipe = None;
            return;
        }
        let rect = ctx.content_rect();
        let events = std::mem::take(&mut raw.events);
        let mut out = Vec::with_capacity(events.len() + 4);
        for event in events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                } => {
                    self.swipe_blocked = false;
                    // A tap during a turn lands it at once: the page must not
                    // keep moving under a finger that already wants it.
                    self.finish_page_turn();
                    self.page_anchor = None;
                    self.edge_swipe = Some((pos, pos, Instant::now()));
                    out.push(egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers,
                    });
                }
                egui::Event::PointerMoved(pos) => {
                    if let Some(id) = ctx.dragged_id() {
                        // The page ScrollArea claims every drag so it can
                        // scroll. A slider or a tile drag is a different id
                        // and must keep the page swipe away. The monitor
                        // preview is not: a swipe over it still turns.
                        if self.drag_blocks_page(id) {
                            self.swipe_blocked = true;
                        }
                    }
                    if let Some((start, _, at)) = self.edge_swipe {
                        if !self.swipe_blocked {
                            let dx = pos.x - start.x;
                            let dy = pos.y - start.y;
                            // Opening the panel is a deliberate pull from the
                            // very top edge while the page is already at its
                            // top. An ordinary scroll starts anywhere and
                            // scrolls the page, never the panel.
                            let from_top = start.y <= rect.top() + 52.0;
                            let at_top = self.clock_full || self.page_scroll_offset <= 1.0;
                            if self.quick_open {
                                // The page underneath never moves while the
                                // panel is up: an upward drag pulls the panel
                                // out under the finger, and the panel's own
                                // sliders keep their drags to themselves.
                                self.page_slide = 0.0;
                                if dy < -6.0 && dy.abs() > dx.abs() * 1.2 {
                                    self.quick_off = (-dy).clamp(0.0, self.quick_height.max(1.0));
                                }
                            } else if from_top && at_top && dy > 10.0 && dy > dx.abs() * 2.0 {
                                self.quick_pull = dy;
                                // The panel is revealed as it is pulled, so
                                // the finger sees the edge coming down.
                                self.quick_off =
                                    (self.quick_height - dy).clamp(0.0, self.quick_height);
                                self.page_slide = 0.0;
                            } else if theme::style().animations {
                                // Only a clearly horizontal drag previews the
                                // next page; vertical scrolling stays intact.
                                // The preview starts where the gesture was
                                // recognised, so it never jumps by 24 points.
                                if self.page_anchor.is_none()
                                    && dx.abs() > 24.0
                                    && dx.abs() > dy.abs() * 1.6
                                {
                                    self.page_anchor = Some(dx);
                                }
                                if let Some(anchor) = self.page_anchor {
                                    if dx.abs() > dy.abs() * 1.2 {
                                        let w = rect.width().max(1.0);
                                        self.page_slide = (dx - anchor).clamp(-w, w);
                                    }
                                } else if self.page_slide != 0.0 {
                                    self.page_slide = 0.0;
                                }
                            } else if self.page_slide != 0.0 {
                                self.page_slide = 0.0;
                            }
                        }
                        self.edge_swipe = Some((start, pos, at));
                    }
                    out.push(egui::Event::PointerMoved(pos));
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers,
                } => {
                    if let Some((start, _, at)) = self.edge_swipe.take() {
                        let dx = pos.x - start.x;
                        let dy = pos.y - start.y;
                        // A drag is not a click: announce movement before the
                        // release so egui refuses to synthesise one.
                        if dx.abs() > 6.0 || dy.abs() > 6.0 {
                            out.push(egui::Event::PointerMoved(pos));
                        }
                        // A stationary long press is the touch equivalent of a
                        // right click. The Deck's compositor often reports its
                        // touchscreen as a mouse, where egui never flags
                        // `long_touched`; synthesising one button makes "hold
                        // to remove" work there too. The primary release is
                        // already too late to count as a click, so nothing
                        // fires twice.
                        if !self.swipe_blocked
                            && at.elapsed() > Duration::from_millis(850)
                            && dx.abs() < 12.0
                            && dy.abs() < 12.0
                        {
                            out.push(egui::Event::PointerMoved(pos));
                            out.push(egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Secondary,
                                pressed: true,
                                modifiers,
                            });
                            out.push(egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Secondary,
                                pressed: false,
                                modifiers,
                            });
                        }
                        let pull = self.quick_pull;
                        if self.quick_open {
                            self.quick_pull = 0.0;
                            // An open panel is dismissed by a push up, never
                            // by turning the page underneath it. A third of
                            // the panel or a flick is enough; anything less
                            // slides back to where it was.
                            self.page_slide = 0.0;
                            let h = self.quick_height.max(1.0);
                            let flick = at.elapsed() < Duration::from_millis(320) && dy < -56.0;
                            let close = !self.swipe_blocked
                                && (dy < -70.0 || self.quick_off > h * 0.3 || flick);
                            if close {
                                self.start_quick_settle(h, false);
                            } else if self.quick_off > 0.5 {
                                self.start_quick_settle(0.0, true);
                            }
                        } else if !self.swipe_blocked
                            && at.elapsed() < Duration::from_millis(900)
                            && pull > 90.0
                            && (self.clock_full || self.page_scroll_offset <= 4.0)
                            && dy > dx.abs() * 2.0
                        {
                            self.page_slide = 0.0;
                            self.set_quick(true);
                        } else {
                            // A pull that stopped short slides back up.
                            if self.quick_off < self.quick_height - 0.5 {
                                self.start_quick_settle(self.quick_height.max(1.0), false);
                            }
                            self.quick_pull = 0.0;
                            let gesture_recent =
                                self.last_gesture.as_ref().is_some_and(|(_, when)| {
                                    when.elapsed() < Duration::from_millis(450)
                                });
                            // A native touch gesture owns this release; the
                            // mouse events it also produced must not cancel
                            // the turn it just started.
                            if !gesture_recent {
                                let width = rect.width().max(1.0);
                                let slide = self.page_slide;
                                let held = at.elapsed().as_secs_f32().max(0.05);
                                // The finger's own speed, so the turn starts
                                // with it instead of yanking the page.
                                let vel = dx / held;
                                // A page turns when it has been dragged a
                                // fifth of the way, or on a quick flick: the
                                // rest of the move is carried by the turn.
                                let flick = held < 0.35
                                    && vel.abs() > 420.0
                                    && slide.abs() > 20.0
                                    && slide.signum() == vel.signum();
                                let commit = !self.swipe_blocked
                                    && self.page_anchor.is_some()
                                    && (slide.abs() > width * 0.2 || flick);
                                if commit {
                                    // The cue belongs to the flick itself; the
                                    // page lands a fraction of a second later.
                                    sound::play(
                                        &self.cfg.sound,
                                        if slide < 0.0 {
                                            Cue::Navigate
                                        } else {
                                            Cue::Back
                                        },
                                    );
                                    self.page_silent = true;
                                    let to = if slide < 0.0 { -width } else { width };
                                    self.start_page_turn(
                                        to,
                                        self.step_page(if slide < 0.0 { 1 } else { -1 }),
                                        width,
                                        vel,
                                    );
                                    self.page_silent = false;
                                } else if slide != 0.0 {
                                    self.start_page_turn(0.0, None, width, vel);
                                }
                            }
                            self.page_anchor = None;
                        }
                    }
                    // Keep the release either way: it ends the pointer state.
                    out.push(egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers,
                    });
                }
                egui::Event::PointerGone => {
                    // The finger or mouse left the window mid-gesture: forget
                    // the gesture instead of committing it on the next press.
                    self.edge_swipe = None;
                    self.quick_pull = 0.0;
                    out.push(egui::Event::PointerGone);
                }
                other => out.push(other),
            }
        }
        raw.events = out;
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
        self.cfg.stream = sdw_core::model::StreamCfg {
            mode: self.stream.mode,
            backend: self.stream.backend,
            position: self.stream.position,
            width: self.stream.width,
            height: self.stream.height,
            fps: self.stream.fps,
            quality: self.stream.quality,
            input: self.stream.input,
        };
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
        // Any explicit navigation leaves the full-screen clock behind.
        self.clock_full = false;
        // A swipe that just landed already animated its own entrance.
        self.page_instant = false;
        if self.page != page {
            if let (Some(a), Some(b)) = (
                Page::ORDER.iter().position(|p| *p == self.page),
                Page::ORDER.iter().position(|p| *p == page),
            ) {
                self.page_dir = if b > a { 1.0 } else { -1.0 };
            }
            // A tap or an arrow leaves the old page behind for a moment, so
            // it slides out instead of disappearing under the new one. A
            // swipe already has it under the finger and needs no ghost.
            if theme::style().animations && !self.page_silent && self.binds.editor.is_none() {
                self.page_from = Some((self.page, self.page_epoch, self.page_dir, Instant::now()));
            }
            self.page_at = Instant::now();
            self.page = page;
            self.page_epoch += 1;
            self.cfg.last_page = page.key().to_string();
            self.save();
            if !self.page_silent {
                sound::play(
                    &self.cfg.sound,
                    if self.page_dir >= 0.0 {
                        Cue::Navigate
                    } else {
                        Cue::Back
                    },
                );
            }
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
        self.status_ok_at = Instant::now() - Duration::from_secs(60);
        self.last_status = Instant::now() - Duration::from_secs(5);
        self.notifications.clear();
        self.notif_art.clear();
        self.notif_last_id = 0;
        self.notif_unread = 0;
        self.notif_pending = false;
        self.last_notif = Instant::now() - Duration::from_secs(5);
        self.stream.feed = None;
        self.stream.texture = None;
        // The pictures and the list belong to the computer that just went
        // away; the next one starts clean.
        self.control_apps.clear();
        self.app_icons.clear();
        self.app_icon_bytes.clear();
        self.app_icon_wanted.clear();
        self.app_icon_failed.clear();
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
            self.status_ok_at = Instant::now() - Duration::from_secs(60);
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
        // A capture that pauses for a mode switch (a fullscreen app, a new
        // resolution) must not take the picture away: the feed stays while the
        // page is open, and the last frame remains until a new one arrives.
        let want = self.page == Page::Screen;
        if want && self.stream.feed.is_none() {
            self.start_feed();
        }
        if !want && self.stream.feed.is_some() {
            self.stream.feed = None;
        }
        if !want {
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

    /// Whether the active computer answered recently. A single missed poll
    /// does not flip the badge: the grace is a few seconds, so a hiccup does
    /// not flicker, but a computer that is off goes offline at once.
    pub fn online(&self) -> bool {
        self.status.is_some() && self.status_ok_at.elapsed() < Duration::from_secs(3)
    }

    /// Ask the computer whether it is showing this deck, so an idle deck does
    /// not pay for screenshots nobody asked for.
    pub fn poll_deck_want(&mut self) {
        if !self.cfg.mirror_deck || self.share.want_at.elapsed() < Duration::from_millis(1500) {
            return;
        }
        let Some(c) = self.client() else {
            self.share.want = false;
            return;
        };
        self.share.want_at = Instant::now();
        self.spawn(move || Task::DeckWant(c.deck_want().map_err(|e| e.to_string())));
        // The same tick reads the computer's own level: while its sound plays
        // on the deck, this is the only volume there is.
        if self.audio_player.is_some() {
            if let Some(c) = self.client() {
                self.spawn(move || {
                    Task::PcVolume(
                        c.media()
                            .map(|m| m.volume.min(100) as u8)
                            .unwrap_or_else(|_| 0),
                    )
                });
            }
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

    /// A Home Assistant connection kept by the deck itself. When an address
    /// and a token are set, the deck talks to Home Assistant directly and does
    /// not need the computer at all.
    pub fn ha_direct(&self) -> Option<sdw_core::ha::HaClient> {
        if self.cfg.ha_url.trim().is_empty() || self.cfg.ha_token.trim().is_empty() {
            return None;
        }
        Some(sdw_core::ha::HaClient::new(
            &self.cfg.ha_url,
            &self.cfg.ha_token,
        ))
    }

    /// What the Home Assistant page shows: the deck's own connection when it
    /// has one, otherwise the computer's.
    pub fn ha_status(&self) -> sdw_core::model::HaStatus {
        if let Some(direct) = self.ha_direct() {
            let error = if self.ha.error.is_empty() {
                self.ha.stale_error.clone()
            } else {
                self.ha.error.clone()
            };
            return sdw_core::model::HaStatus {
                configured: true,
                connected: self.ha.connected,
                url: direct.base().to_string(),
                entities: self.ha.entities.len(),
                last_sync_ms: 0,
                error,
                favorites: Vec::new(),
            };
        }
        self.status
            .as_ref()
            .map(|s| s.ha.clone())
            .unwrap_or_default()
    }

    pub fn load_ha(&mut self) {
        if self.ha.pending {
            return;
        }
        self.ha.pending = true;
        self.ha.last_fetch = Instant::now();
        if let Some(direct) = self.ha_direct() {
            self.spawn(move || {
                let result = direct.snapshot().map(|mut states| {
                    states.connected = true;
                    states.fetched_ms = sdw_core::model::now_ms();
                    states
                });
                Task::Ha(result)
            });
            return;
        }
        match self.client() {
            Some(c) => {
                self.spawn(move || Task::Ha(c.ha_states().map_err(|e| e.to_string())));
            }
            None => self.ha.pending = false,
        }
    }

    /// Open the temporary page that takes the address and token from a phone.
    pub fn start_ha_link(&mut self) {
        if self.ha_link.is_some() {
            return;
        }
        match crate::ha_link::HaLink::start(self.lang) {
            Ok(link) => {
                self.ha_link = Some(link);
            }
            Err(e) => self.toast(format!("{}: {e}", t!(self.lang, "страница", "page")), false),
        }
    }

    pub fn cancel_ha_link(&mut self) {
        if let Some(mut link) = self.ha_link.take() {
            link.stop();
        }
    }

    /// Forget the deck's own Home Assistant connection, so the form and the
    /// phone page are reachable again.
    pub fn forget_ha(&mut self) {
        self.cfg.ha_url.clear();
        self.cfg.ha_token.clear();
        self.save();
        self.ha.entities.clear();
        self.ha.areas.clear();
        self.ha.views.clear();
        self.ha.connected = false;
        self.ha.loaded = false;
        self.ha.error.clear();
        self.ha.stale_error.clear();
        self.ha.last_fetch = Instant::now() - Duration::from_secs(60);
    }

    /// Take the answer from the phone page, if it is there.
    pub fn poll_ha_link(&mut self) {
        if self.ha_link.is_none() {
            return;
        }
        if self.ha_link.as_ref().is_some_and(|link| link.expired()) {
            if let Some(mut link) = self.ha_link.take() {
                link.stop();
            }
            self.toast(
                t!(
                    self.lang,
                    "страница ввода токена истекла",
                    "the token page has expired"
                ),
                false,
            );
            return;
        }
        let Some((url, token)) = self.ha_link.as_mut().and_then(|link| link.take()) else {
            return;
        };
        if let Some(mut link) = self.ha_link.take() {
            link.stop();
        }
        self.cfg.ha_url = url;
        self.cfg.ha_token = token;
        self.save();
        self.ha.entities.clear();
        self.ha.areas.clear();
        self.ha.views.clear();
        self.ha.connected = false;
        self.ha.loaded = false;
        self.ha.error.clear();
        self.ha.stale_error.clear();
        self.ha.last_fetch = Instant::now() - Duration::from_secs(60);
        self.toast(
            t!(
                self.lang,
                "home assistant подключён",
                "home assistant connected"
            ),
            true,
        );
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
        // Answer the press before the computer does: the chip flips and the row
        // starts spinning at once, then the reply brings the real state.
        self.bluetooth_optimistic(&action, &address);
        self.bluetooth_busy = Some((action.clone(), address.clone()));
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

    /// Move the Bluetooth state the moment a button is pressed; the reload
    /// after the reply corrects it against what BlueZ actually did.
    fn bluetooth_optimistic(&mut self, action: &str, address: &str) {
        let Some(bt) = self.bluetooth.as_mut() else {
            return;
        };
        match action {
            "power_on" => bt.powered = true,
            "power_off" => {
                bt.powered = false;
                bt.discovering = false;
            }
            "scan" => {
                bt.powered = true;
                bt.discovering = true;
            }
            "scan_off" => bt.discovering = false,
            _ => {}
        }
        if address.is_empty() {
            return;
        }
        if action == "remove" {
            bt.devices.retain(|d| d.address != address);
            return;
        }
        if let Some(d) = bt.devices.iter_mut().find(|d| d.address == address) {
            match action {
                "pair" => d.paired = true,
                "pair_connect" => {
                    d.paired = true;
                    d.trusted = true;
                    d.connected = true;
                }
                "connect" => d.connected = true,
                "disconnect" => d.connected = false,
                "trust" => d.trusted = true,
                "untrust" => d.trusted = false,
                _ => {}
            }
        }
    }

    /// Drop a notification locally and on the computer.
    pub fn dismiss_notif(&mut self, id: u64) {
        self.notifications.retain(|n| n.id != id);
        self.notif_log.retain(|n| n.id != id);
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

    /// Read the history once per visit to the notifications page. The list is
    /// a few dozen small records, so one read on arrival is enough; closing a
    /// row updates it in place instead of refetching.
    pub fn load_notif_log(&mut self) {
        let Some(c) = self.client() else {
            return;
        };
        if self.notif_log_pending {
            return;
        }
        self.notif_log_pending = true;
        self.notif_log_err.clear();
        self.spawn(move || Task::NotifLog(c.notification_log().map_err(|e| e.to_string())));
    }

    /// Run the notification's action on the computer: what a click on the
    /// desktop notification would do. The row leaves the deck only when the
    /// computer confirms, so a failed tap does not eat the notification.
    pub fn activate_notif(&mut self, id: u64) {
        let Some(c) = self.client() else {
            self.toast(
                t!(self.lang, "нет активного компьютера", "no active computer"),
                false,
            );
            return;
        };
        self.spawn(move || Task::NotifActivated {
            id,
            result: c
                .activate_notification(id)
                .map(|_| ())
                .map_err(|e| e.to_string()),
        });
    }

    // ------------------------------------------------------- screen sharing

    /// Name of the page a share frame shows, in the daemon's language.
    fn share_page(&self) -> &'static str {
        if self.clock_full {
            return "clock";
        }
        if self.stream.fullscreen {
            return "screen";
        }
        if self.binds.editor.is_some() {
            return "binds";
        }
        self.page.key()
    }

    /// Send one frame of this screen to the computer.
    ///
    /// The picture comes from egui itself (`request_screenshot`), is packed as
    /// JPEG at its own size on a worker, and goes to the daemon as a plain
    /// POST. Only one request is in the air at a time: a slow link lowers the
    /// rate instead of piling frames up.
    pub fn pump_share(&mut self, ctx: &egui::Context) {
        if !self.cfg.mirror_deck {
            return;
        }
        let Some(c) = self.client() else {
            return;
        };
        if self.share.in_flight {
            if self.share.last_request.elapsed() < Duration::from_secs(6) {
                return;
            }
            // A viewport that cannot produce a screenshot never calls back;
            // the lane must not stay blocked forever.
            self.share.in_flight = false;
        }
        // Nobody is looking: one frame every few seconds keeps the computer's
        // client list honest without spending the Wi-Fi on a picture nobody
        // sees. A viewer flips `want` within the poll interval.
        if !self.share.want && self.share.quiet.elapsed() < Duration::from_secs(5) {
            return;
        }
        // At full speed the deck sends as fast as one round-trip allows: a
        // frame is only built when the previous one has landed, so a slow
        // link lowers the rate instead of piling pictures up.
        let fps = if self.share.want {
            self.cfg.mirror_fps.clamp(1, 60) as u64
        } else {
            1
        };
        if self.share.last_request.elapsed() < Duration::from_millis(1000 / fps) {
            return;
        }
        self.share.last_request = Instant::now();
        self.share.in_flight = true;
        if !self.share.want {
            self.share.quiet = Instant::now();
        }
        let quality = self.cfg.mirror_quality.clamp(30, 95);
        let tx = self.task_tx.clone();
        let id = self.cfg.client_id.clone();
        let name = self.cfg.device_name.clone();
        let page = self.share_page().to_string();
        ctx.request_screenshot(move |image| {
            std::thread::Builder::new()
                .name("sdw-share".into())
                .spawn(move || {
                    let (w, h) = (image.size[0], image.size[1]);
                    // The screen goes at its own size: the viewer on the
                    // computer shows native pixels, not a softened half.
                    let jpeg = jpeg_full(&image, quality);
                    let result = if jpeg.is_empty() || w < 4 || h < 4 {
                        Err("не удалось снять кадр".to_string())
                    } else {
                        let (dw, dh) = (w as u32, h as u32);
                        c.push_deck_frame(&id, &name, &page, dw, dh, jpeg)
                            .map(|_| (dw, dh))
                            .map_err(|e| e.to_string())
                    };
                    let _ = tx.send(Task::Shared(result));
                })
                .ok();
        });
    }

    /// Open or close the pull-down panel. Opening reads the current values at
    /// once, so the sliders never show a stale number. The movement itself is
    /// a settle, so a tap slides the panel in and a close slides it out
    /// instead of making it appear and vanish.
    pub fn set_quick(&mut self, open: bool) {
        if open {
            self.quick_open = true;
            self.quick_pull = 0.0;
            // The Deck's sound and light are read on a thread, so the panel
            // appears with what it knew last time and fills the numbers in a
            // frame later.
            self.quick_values_deferred = true;
            self.quick_at = None;
            self.start_quick_settle(0.0, true);
        } else {
            self.quick_pull = 0.0;
            if self.quick_open {
                self.start_quick_settle(self.quick_height.max(1.0), false);
            }
        }
    }

    /// Carry the panel from where the finger left it to `to`, over a time that
    /// depends on the distance, and open or close it at the end.
    fn start_quick_settle(&mut self, to: f32, open_at_end: bool) {
        if !theme::style().animations {
            self.quick_off = to;
            self.quick_off_active = false;
            self.quick_settle_open = open_at_end;
            self.finish_quick_settle();
            return;
        }
        self.quick_off_from = self.quick_off;
        self.quick_off_to = to;
        self.quick_off_at = Instant::now();
        self.quick_off_active = true;
        self.quick_settle_open = open_at_end;
    }

    /// Land the settle: leave the panel open or out of sight.
    fn finish_quick_settle(&mut self) {
        self.quick_off_active = false;
        if self.quick_settle_open {
            self.quick_off = 0.0;
            self.quick_open = true;
        } else {
            self.quick_off = self.quick_height.max(1.0);
            self.quick_open = false;
            self.quick_values_deferred = false;
        }
    }

    /// Read the Deck's own volume and brightness. The read reaches the sound
    /// server and sysfs, so it runs on a thread and the panel is drawn from
    /// what it already knew; the answer arrives as [`Task::QuickValues`].
    fn quick_values(&mut self) {
        if self.quick_pending {
            return;
        }
        self.quick_pending = true;
        self.quick_at = Some(Instant::now());
        self.spawn(|| Task::QuickValues(crate::sys::volume(), crate::sys::brightness()));
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

    /// Ask the computer for the pictures of the applications on screen. Only
    /// the entries that claim a picture are requested, at most a couple of
    /// dozen at a time, and a failed one is not asked for again this session.
    pub fn want_app_icons(&mut self, list: &[sdw_core::model::AppEntry]) {
        let Some(client) = self.client() else {
            return;
        };
        let mut asked = 0;
        for entry in list {
            if !entry.has_icon || asked >= 24 {
                continue;
            }
            if self.app_icons.contains_key(&entry.id)
                || self.app_icon_bytes.contains_key(&entry.id)
                || self.app_icon_wanted.contains(&entry.id)
                || self.app_icon_failed.contains(&entry.id)
            {
                continue;
            }
            asked += 1;
            let id = entry.id.clone();
            self.app_icon_wanted.insert(id.clone());
            let c = client.clone();
            let want = id.clone();
            self.spawn(move || Task::AppIcon {
                id,
                result: c.app_icon(&want).map_err(|e| e.to_string()),
            });
        }
    }

    /// Decode the pictures that arrived. They are small theme PNGs, so this is
    /// a few milliseconds for a whole page.
    pub fn ensure_app_icons(&mut self, ctx: &egui::Context) {
        if self.app_icon_bytes.is_empty() {
            return;
        }
        let pending: Vec<(String, Vec<u8>)> = self.app_icon_bytes.drain().collect();
        for (id, bytes) in pending {
            let Ok(img) = image::load_from_memory(&bytes) else {
                self.app_icon_failed.insert(id);
                continue;
            };
            let img = img.to_rgba8();
            let (w, h) = img.dimensions();
            // A theme icon can be 512 px; 128 is already sharp on the deck.
            let img = if w.max(h) > 128 {
                let scale = 128.0 / w.max(h) as f32;
                let nw = (w as f32 * scale).max(1.0) as u32;
                let nh = (h as f32 * scale).max(1.0) as u32;
                image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle)
            } else {
                img
            };
            let (w, h) = img.dimensions();
            let color =
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
            // Keep the cache bounded: a long session should not hold every
            // icon ever shown.
            if self.app_icons.len() >= 256 {
                self.app_icons.clear();
            }
            let tex = ctx.load_texture(
                format!("app-icon-{id}"),
                color,
                egui::TextureOptions::LINEAR,
            );
            self.app_icons.insert(id, tex);
        }
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
        // Two pictures per pass is invisible to the eye and keeps the page
        // responsive while a list of them fills in from the top.
        let pass = ctx.cumulative_pass_nr();
        if self.notif_art_pass != pass {
            self.notif_art_pass = pass;
            self.notif_art_budget = 2;
        }
        if self.notif_art_budget == 0 {
            // The placeholder is drawn for this frame; a repaint right after
            // lets the next picture decode.
            ctx.request_repaint();
            return None;
        }
        self.notif_art_budget -= 1;
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

    /// Start the freshly installed file and leave. Inside an AppImage the
    /// runtime exposes the real file in `$APPIMAGE`; anywhere else the running
    /// executable is the one that was replaced.
    pub fn restart_app(&mut self) {
        self.save();
        let exe = std::env::var_os("APPIMAGE")
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_file())
            .or_else(|| std::env::current_exe().ok());
        let Some(exe) = exe else {
            self.toast(
                t!(
                    self.lang,
                    "не нашёл файл приложения — запустите его вручную",
                    "could not find the app file — start it by hand"
                ),
                false,
            );
            return;
        };
        let name = exe.to_string_lossy().to_string();
        match sdw_core::util::spawn_detached(&name, &[]) {
            Ok(_) => std::process::exit(0),
            Err(e) => self.toast(
                format!("{}: {e}", t!(self.lang, "перезапуск", "restart")),
                false,
            ),
        }
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
        self.update.view = crate::theme::ProgressView::default();
        self.update.sampled = None;
        self.update.installed = false;
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
        if let Some(direct) = self.ha_direct() {
            self.spawn(move || {
                Task::Msg(
                    direct
                        .call(&call)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string()),
                )
            });
            return;
        }
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
            other => {
                // Local actions spawn processes and a shell command can run
                // for seconds; doing that on the UI thread froze the whole
                // deck, so a bind looked like the app had hung. Tell the user
                // it started, then report the result when the thread is done.
                if matches!(other, Action::Shell { .. } | Action::Launch { .. }) {
                    self.toast(t!(self.lang, "выполняется…", "running…").to_string(), true);
                }
                let action = other.clone();
                let lang = self.lang;
                self.spawn(move || {
                    Task::Msg(crate::actions::run(&action, lang).map_err(|e| e.to_string()))
                });
            }
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
        if let Some(page) = self.step_page(delta) {
            self.set_page(page);
        }
    }

    /// The page `delta` steps away in the dock order, if there is one.
    fn step_page(&self, delta: i32) -> Option<Page> {
        let pages = self.dock_pages();
        if pages.is_empty() {
            return None;
        }
        let idx = pages.iter().position(|p| *p == self.page)? as i32;
        Some(pages[(idx + delta).rem_euclid(pages.len() as i32) as usize])
    }

    // ------------------------------------------------------------ frame

    fn tick(&mut self, ctx: &egui::Context) {
        if self.dark != self.applied_dark {
            theme::apply(ctx, self.dark);
            self.applied_dark = self.dark;
        }
        self.apply_scale(ctx);
        // The turn after the finger left: the offset eases from where it was
        // released to where the page belongs, so a swipe keeps moving in its
        // own direction instead of snapping to a stop under the finger.
        let dragging = self.edge_swipe.is_some() || !self.touch.points.is_empty();
        if !dragging {
            if self.page_turn_active {
                let dur = self.page_turn_dur.max(0.01);
                let k = (self.page_turn_at.elapsed().as_secs_f32() / dur).min(1.0);
                // Cubic Hermite: starts with the speed the finger had and
                // arrives with none, so the release is neither a yank nor a
                // stop. The start speed is clamped to keep the curve monotone.
                let k2 = k * k;
                let k3 = k2 * k;
                let h01 = 3.0 * k2 - 2.0 * k3;
                let h10 = k3 - 2.0 * k2 + k;
                self.page_slide = self.page_turn_from
                    + (self.page_turn_to - self.page_turn_from) * h01
                    + dur * self.page_turn_vel * h10;
                if k >= 1.0 {
                    self.finish_page_turn();
                } else {
                    ctx.request_repaint();
                }
            } else {
                self.page_slide = 0.0;
            }
        }
        // The page a tap or an arrow left slides out for a third of a second.
        if let Some((_, _, _, at)) = self.page_from {
            if at.elapsed() > Duration::from_millis(340) {
                self.page_from = None;
            } else {
                ctx.request_repaint();
            }
        }
        // The control panel is moved by the same rule as a page: the finger
        // places it, the release finishes the move. A short settle keeps the
        // slide smooth without a fixed 0.16 s snap.
        if self.quick_off_active {
            let h = self.quick_height.max(1.0);
            let dist = (self.quick_off_to - self.quick_off_from).abs();
            let dur = 0.18 + 0.12 * (dist / h).min(1.0);
            let k = (self.quick_off_at.elapsed().as_secs_f32() / dur).min(1.0);
            let ease = (k * std::f32::consts::FRAC_PI_2).sin();
            self.quick_off = self.quick_off_from + (self.quick_off_to - self.quick_off_from) * ease;
            if k >= 1.0 {
                self.finish_quick_settle();
            } else {
                ctx.request_repaint();
            }
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

        // A request that never answers must not lock a page forever: the
        // pending flag is cleared after a generous timeout and the next poll
        // tries again.
        if self.status_pending && self.last_status.elapsed() > Duration::from_secs(8) {
            self.status_pending = false;
        }
        if self.bluetooth_pending
            && self
                .bluetooth_at
                .is_some_and(|at| at.elapsed() > Duration::from_secs(8))
        {
            self.bluetooth_pending = false;
        }
        if self.control_apps_pending
            && self
                .control_apps_at
                .is_some_and(|at| at.elapsed() > Duration::from_secs(8))
        {
            self.control_apps_pending = false;
        }

        self.poll_status();
        self.poll_deck_want();
        self.poll_notifications();
        if self.page == Page::HomeAssistant && self.ha.last_fetch.elapsed() > Duration::from_secs(4)
        {
            self.load_ha();
        }
        if self.page == Page::HomeAssistant {
            self.poll_ha_link();
        } else {
            // The page belongs to the card that opened it; leaving closes it.
            self.cancel_ha_link();
        }
        if self.page == Page::Media {
            self.poll_media();
            self.ensure_media_art(ctx);
        } else if self.cfg.viz.enabled && self.cfg.viz.media {
            // The visualizer follows the computer's track even when its sound
            // stays there, so the media state is read from any page.
            self.poll_media();
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

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.go_back(ctx);
        }
    }

    fn on_task(&mut self, task: Task) {
        match task {
            Task::Status(Ok(status)) => {
                self.status_pending = false;
                self.status_err.clear();
                self.status_ok_at = Instant::now();
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
                    self.ha.optimistic_rgb.clear();
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
                self.update.view = crate::theme::ProgressView::default();
                self.update.sampled = None;
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
            Task::Installed(Ok(msg)) => {
                // The file is in place; the only thing left is a restart, and
                // the page now offers it instead of telling the user to do it
                // by hand.
                self.update.installed = true;
                self.toast(msg, true);
            }
            Task::Installed(Err(e)) => self.toast(e, false),
            Task::NotifLog(Ok(list)) => {
                self.notif_log_pending = false;
                self.notif_log_loaded = true;
                self.notif_log = list;
                // The live list keeps only the ids that are still stored.
                let have: HashSet<u64> = self.notif_log.iter().map(|n| n.id).collect();
                self.notifications.retain(|n| have.contains(&n.id));
                let live: HashSet<u64> = self.notif_log.iter().map(|n| n.id).collect();
                self.notif_art.retain(|id, _| live.contains(id));
            }
            Task::NotifLog(Err(e)) => {
                self.notif_log_pending = false;
                self.notif_log_err = e;
            }
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
                        self.notif_log.push(n.clone());
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
                // A tapped colour shows on the swatch right away. Keep it for
                // a few seconds in case the lamp is slow to answer, then let
                // the real value win again.
                self.ha
                    .optimistic_rgb
                    .retain(|_, c| c.at.elapsed() < Duration::from_secs(8));
                for e in &mut self.ha.entities {
                    if let Some(c) = self.ha.optimistic_rgb.get(&e.entity_id) {
                        if let Some(map) = e.attributes.as_object_mut() {
                            map.insert("rgb_color".into(), serde_json::json!(c.rgb));
                            map.insert("color_mode".into(), serde_json::json!("hs"));
                            if c.kelvin {
                                map.remove("color_temp_kelvin");
                            }
                        }
                    }
                }
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
            Task::AppIcon { id, result } => {
                self.app_icon_wanted.remove(&id);
                match result {
                    Ok(bytes) => {
                        self.app_icon_bytes.insert(id, bytes);
                    }
                    Err(_) => {
                        self.app_icon_failed.insert(id);
                    }
                }
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
                self.bluetooth_busy = None;
                self.bluetooth_at = None;
            }
            Task::BtAction(Err(e)) => {
                self.bluetooth_busy = None;
                self.bluetooth_at = None;
                self.toast(e, false);
            }
            Task::QuickMute(Ok(muted)) => {
                self.quick_muted = muted;
                if self.quick_volume == Some(0) && !muted {
                    self.quick_volume = Some(30);
                }
            }
            Task::QuickMute(Err(e)) => self.toast(e, false),
            Task::DeckWant(result) => {
                self.share.want_at = Instant::now();
                match result {
                    Ok(want) => {
                        self.share.want = want;
                        // Someone opened the deck on the computer: send a
                        // frame right away instead of waiting for the poll.
                        if want {
                            self.share.quiet = Instant::now() - Duration::from_secs(10);
                        }
                    }
                    Err(_) => self.share.want = false,
                }
            }
            Task::PcVolume(v) => self.pc_volume = Some((v, Instant::now())),
            Task::Shared(result) => {
                self.share.in_flight = false;
                match result {
                    Ok(size) => {
                        self.share.sent = self.share.sent.saturating_add(1);
                        self.share.size = size;
                        self.share.error.clear();
                    }
                    // A closed deck or a sleeping pc is the normal state, not
                    // an error worth showing raw: the page says it softly.
                    Err(e) => self.share.error = share_reason(self.lang, &e),
                }
            }
            Task::NotifActivated { id, result } => match result {
                Ok(()) => {
                    self.notifications.retain(|n| n.id != id);
                    self.popups.retain(|p| p.id != id);
                }
                Err(e) => self.toast(e, false),
            },
            Task::QuickValues(volume, brightness) => {
                self.quick_pending = false;
                if let Some((level, muted)) = volume {
                    self.quick_volume = Some(level);
                    self.quick_muted = muted;
                }
                self.quick_brightness = brightness;
            }
        }
    }

    // ------------------------------------------------------------ chrome

    /// Start the turn that follows a released swipe. `to` is the offset the
    /// page must reach, `width` the page width and `vel` the finger's speed in
    /// points per second. With animations off there is nothing to carry, so
    /// the page switches at once.
    fn start_page_turn(&mut self, to: f32, page: Option<Page>, width: f32, vel: f32) {
        if !theme::style().animations {
            self.page_slide = 0.0;
            if let Some(p) = page {
                self.set_page(p);
                self.page_instant = true;
            }
            return;
        }
        let from = self.page_slide;
        let dist = to - from;
        let ratio = (dist.abs() / width.max(1.0)).min(1.0);
        let dur = if page.is_some() {
            0.26 + 0.22 * ratio
        } else {
            0.18 + 0.16 * ratio
        };
        // A Hermite that starts faster than this would overshoot the target
        // and come back; a finger that moved away from the target may carry
        // the page a little further before it returns.
        let limit = 3.0 * dist.abs() / dur;
        let vel = if dist >= 0.0 {
            vel.clamp(-0.35 * limit, limit)
        } else {
            vel.clamp(-limit, 0.35 * limit)
        };
        self.page_turn_from = from;
        self.page_turn_to = to;
        self.page_turn_at = Instant::now();
        self.page_turn_dur = dur;
        self.page_turn_vel = vel;
        self.page_turn_to_page = page;
        self.page_turn_active = true;
    }

    /// Land the turn at once, without waiting for the ease to run out.
    fn finish_page_turn(&mut self) {
        if !self.page_turn_active {
            return;
        }
        self.page_turn_active = false;
        self.page_slide = 0.0;
        self.page_turn_from = 0.0;
        self.page_turn_to = 0.0;
        self.page_turn_vel = 0.0;
        if let Some(p) = self.page_turn_to_page.take() {
            // The release already played the cue and the finger already
            // showed where the page comes from: no fade, no second sound.
            self.page_silent = true;
            self.page_instant = true;
            self.set_page(p);
            self.page_silent = false;
        }
    }

    /// Draw one page into `ui`, scrolled on its own. `track` keeps the live
    /// scroll offset of the page under the finger; the neighbour in a drag is
    /// drawn without touching it.
    fn page_scroll(&mut self, ui: &mut Ui, t: &Tokens, salt: egui::Id, track: bool) {
        // The ScrollArea claims its background drags under `id.with("area")`;
        // that is the one drag which must not block the page swipe, so the
        // exact id is remembered rather than a similar-looking one.
        // `ScrollArea::id_salt` wraps what it is given in an `IdSalt`, so the
        // same wrapping is needed here to name the drag it claims.
        let area_id = ui.make_persistent_id(egui::IdSalt::new(salt)).with("area");
        if track {
            self.page_scroll_id = Some(area_id);
        }
        let area = ScrollArea::vertical()
            .id_salt(salt)
            .auto_shrink([false, false])
            .scroll_source(egui::scroll_area::ScrollSource {
                scroll_bar: false,
                // gamescope often reports the deck touchscreen as a mouse, so
                // egui's OnTouch default never turns on. While the page is
                // being swiped aside the scroll area lets go of the drag: a
                // horizontal turn must never scroll the page vertically.
                drag: if self.page_slide.abs() > 1.0 {
                    egui::scroll_area::DragScroll::Never
                } else {
                    egui::scroll_area::DragScroll::Always
                },
                mouse_wheel: true,
            })
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.binds.editor.is_some() {
                    binds::editor_ui(self, ui, t);
                } else {
                    self.page_ui(ui, t);
                }
                ui.add_space(104.0);
            });
        if track {
            self.page_scroll_offset = area.state.offset().y;
        }
    }

    fn chrome_ui(&mut self, root: &mut Ui, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        // A page turned by a swipe was already dragged into view, so it gets no
        // fade-in of its own: the turn below finishes the movement the finger
        // started. Taps and arrows keep the fade and the slight rise.
        let anim = if !theme::style().animations || self.page_instant || self.page_turn_active {
            1.0
        } else {
            // Called even while a ghost runs: it keeps egui's animation state
            // moving, so the page does not fade a second time after the ghost.
            ctx.animate_bool_with_time(Id::new(("page", self.page_epoch)), true, 0.28)
        };
        let width = ctx.content_rect().width().max(1.0);
        // A tap or an arrow: the old page slides out while the new one comes
        // in. A swipe is already carried by `page_slide`.
        let ghost = self
            .page_from
            .filter(|(_, _, _, at)| at.elapsed() < Duration::from_millis(340));
        let (ease, travel) = if let Some((_, _, _, at)) = ghost {
            let k = (at.elapsed().as_secs_f32() / 0.34).min(1.0);
            (k * k * (3.0 - 2.0 * k), width)
        } else {
            (
                1.0 - (1.0 - anim).powi(3),
                (width * 0.12).clamp(36.0, 220.0),
            )
        };
        let slide = self.page_slide + (1.0 - ease) * travel * self.page_dir;

        // The floating arrows sit over the window edge. The page is inset by
        // their width so that no card, slider or button ever ends under one.
        let arrow_inset = if theme::style().arrows {
            (theme::style().arrow_size.clamp(28.0, 96.0) + 16.0).max(34.0)
        } else {
            34.0
        };

        // While the finger drags the page aside, the neighbour is drawn where
        // it will land: the page turn follows the finger instead of being a
        // fade that happens after the release.
        let pages = self.dock_pages();
        let neighbour =
            if self.page_slide.abs() > 1.0 && pages.len() > 1 && self.binds.editor.is_none() {
                let dir = if self.page_slide < 0.0 { 1.0 } else { -1.0 };
                let idx = pages.iter().position(|p| *p == self.page).unwrap_or(0) as i32;
                let next = (idx + dir as i32).rem_euclid(pages.len() as i32) as usize;
                Some((pages[next], dir))
            } else {
                None
            };

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
                left: arrow_inset as i8,
                right: arrow_inset as i8,
                // The page rises a touch as it fades in. It is a translation,
                // not a spacer inside the scroll area: a spacer would change
                // the content height on every frame of the swap and make the
                // list re-lay out under the finger.
                top: 4,
                bottom: 4,
            }))
            .show(root, |ui| {
                theme::backdrop(ui, &t);
                let content = ui.max_rect();
                let clip = ui.clip_rect();
                if let Some((from, from_epoch, dir, _)) = ghost {
                    let mut ghost_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt(("deck-ghost-ui", from_epoch))
                            .max_rect(content.translate(Vec2::new(-dir * width * 0.22 * ease, 0.0)))
                            .layout(*ui.layout()),
                    );
                    ghost_ui.set_clip_rect(clip);
                    let saved = std::mem::replace(&mut self.page, from);
                    self.page_scroll(
                        &mut ghost_ui,
                        &t,
                        egui::Id::new(("deck-page", from_epoch)),
                        false,
                    );
                    self.page = saved;
                }
                if let Some((next, dir)) = neighbour {
                    let mut peek = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt(("deck-peek-ui", next.key()))
                            .max_rect(
                                content.translate(Vec2::new(slide + dir * content.width(), 0.0)),
                            )
                            .layout(*ui.layout()),
                    );
                    peek.set_clip_rect(clip);
                    let saved = std::mem::replace(&mut self.page, next);
                    self.page_scroll(
                        &mut peek,
                        &t,
                        egui::Id::new(("deck-peek", next.key())),
                        false,
                    );
                    self.page = saved;
                }
                let mut page_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("deck-page-ui", self.page_epoch))
                        .max_rect(content.translate(Vec2::new(slide, (1.0 - ease) * 14.0)))
                        .layout(*ui.layout()),
                );
                page_ui.set_clip_rect(clip);
                // A page held by the finger stays solid: it is being turned,
                // not fading.
                page_ui.set_opacity(if self.page_slide != 0.0 || ghost.is_some() {
                    1.0
                } else {
                    ease
                });
                self.page_scroll(
                    &mut page_ui,
                    &t,
                    egui::Id::new(("deck-page", self.page_epoch)),
                    true,
                );
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
            let online = self.online();
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
            if self.cfg.viz.enabled && self.cfg.viz.nav {
                ui.add_space(12.0);
                crate::viz::ui(
                    ui,
                    self,
                    t,
                    egui::vec2(124.0, self.cfg.viz.height.clamp(10, 48) as f32),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Notifications are one press away on every page, not only in
                // the pull-down: the bell carries the unread marker.
                let unread = self.notif_unread;
                let bell = ui
                    .add(
                        egui::Button::new(theme::icon(
                            theme::icon::NOTIFICATIONS,
                            16.0,
                            if unread > 0 { t.accent } else { t.faint },
                        ))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::ZERO),
                    )
                    .on_hover_text(t!(self.lang, "уведомления", "notifications"));
                if unread > 0 {
                    let corner = bell.rect.right_top() + egui::vec2(-3.0, 3.0);
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(corner, egui::vec2(6.0, 6.0)),
                        CornerRadius::ZERO,
                        t.bad,
                    );
                }
                if bell.clicked() {
                    self.set_page(Page::Notifications);
                }
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
        let aspect = if tex.size_vec2().x > 1.0 && tex.size_vec2().y > 1.0 {
            tex.size_vec2().x / tex.size_vec2().y
        } else {
            monitor.width as f32 / monitor.height.max(1) as f32
        };
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
        // A preview that does not drive the computer must not claim the
        // pointer: the page swipe has to work even when the finger lands on
        // the picture. In input mode the surface takes the drag, but a
        // horizontal swipe is still the deck's own page turn.
        if !monitor.input_enabled {
            self.stream_drag_id = None;
            return;
        }
        let resp = ui.interact(
            rect,
            ui.id().with("stream-surface"),
            Sense::click_and_drag(),
        );
        self.stream_drag_id = Some(resp.id);
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                self.send_pointer("click", pos);
            }
        }
        if resp.dragged() && self.page_anchor.is_none() {
            if !self.stream.pointer_down {
                self.stream.pointer_down = true;
                let origin = ui.input(|i| i.pointer.press_origin());
                self.send_pointer("down", origin.unwrap_or(rect.center()));
            }
            if let Some(pos) = resp.interact_pointer_pos() {
                self.send_pointer("move", pos);
            }
        }
        // The drag turned out to be a page swipe after the remote pointer was
        // already pressed: let it go before the page leaves.
        if self.page_anchor.is_some() && self.stream.pointer_down {
            self.stream.pointer_down = false;
            if let Some(pos) = resp.interact_pointer_pos() {
                self.send_pointer("up", pos);
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
        // Over the fullscreen screen stream the cards stay, unless the user
        // asked for a clean picture. They sit below the close button so a tap
        // on the card never lands on it.
        let full = self.stream.fullscreen;
        if full && !self.cfg.notify_fullscreen {
            return;
        }
        let top = if full { 76.0 } else { 64.0 };
        let t = Tokens::get(ctx);
        let now = Instant::now();
        self.popups
            .retain(|p| now.duration_since(p.at) < Duration::from_millis(5400));
        let mut dismiss: Option<u64> = None;
        let mut open: Option<u64> = None;
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
            let offset = Vec2::new(-18.0, top + i as f32 * 108.0 - (1.0 - ease) * 14.0);
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
                                let text = ui.vertical(|ui| {
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
                                let hit = ui.interact(
                                    text.response.rect,
                                    Id::new(("notif-open", p.id)),
                                    Sense::click(),
                                );
                                if hit.clicked() {
                                    open = Some(p.id);
                                }
                                if hit.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                hit.on_hover_text(t!(
                                    self.lang,
                                    "нажмите, чтобы открыть на компьютере",
                                    "tap to open it on the computer"
                                ));
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
        if let Some(id) = open {
            self.popups.retain(|p| p.id != id);
            self.activate_notif(id);
        }
        // Keep the fade smooth while at least one popup is alive.
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    /// The pull-down panel and the small handle that always opens it.
    fn quick_ui(&mut self, ctx: &egui::Context) {
        let t = Tokens::get(ctx);
        // The panel belongs to every screen, including the full-screen clock:
        // notifications and the Deck's own sound/light stay one pull away.
        let blocked = self.stream.fullscreen || self.binds.capture;
        if blocked {
            if self.quick_open {
                self.set_quick(false);
            }
            return;
        }
        if self.quick_open
            && (self.quick_values_deferred
                || self
                    .quick_at
                    .map(|at| at.elapsed() > Duration::from_secs(2))
                    .unwrap_or(true))
        {
            self.quick_values_deferred = false;
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
                        self.quick_clicked = true;
                    }
                });
            if toggle {
                self.set_quick(true);
            }
        }

        crate::pages::quick_panel(self, ctx, &t);
        // The Deck's values are read every two seconds and the ages move a
        // minute at a time, so four frames a second are enough while the panel
        // just sits there; a settle or a finger asks for every frame by itself.
        if self.quick_off_active {
            ctx.request_repaint();
        } else if self.quick_open || self.quick_pull > 0.0 {
            ctx.request_repaint_after(Duration::from_millis(250));
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
        self.swipe_raw(ctx, raw);
    }

    fn ui(&mut self, root: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.quick_clicked = false;
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
        self.pump_share(&ctx);
    }
}

/// A share failure as a sentence a person reads. The transport error stays
/// out of the ui: `io: Connection refused (os error 111)` says nothing about
/// what to do, and the answer is almost always "the pc is asleep".
fn share_reason(lang: Lang, error: &str) -> String {
    let low = error.to_ascii_lowercase();
    if low.contains("refused")
        || low.contains("timed out")
        || low.contains("unreachable")
        || low.contains("dns")
        || low.contains("connect")
    {
        return t!(
            lang,
            "компьютер не отвечает",
            "the computer is not answering"
        )
        .to_string();
    }
    t!(
        lang,
        "трансляция не идёт: {error}",
        "sharing failed: {error}"
    )
    .replace("{error}", error)
}

/// Pack a screenshot as JPEG at its own size, so the viewer on the computer
/// shows native pixels. The rate is capped separately and only one frame is in
/// flight, which keeps the Wi-Fi bill honest without touching the picture.
fn jpeg_full(image: &egui::ColorImage, quality: u8) -> Vec<u8> {
    let (w, h) = (image.size[0], image.size[1]);
    if w < 2 || h < 2 {
        return Vec::new();
    }
    let (dw, dh) = (w, h);
    let mut rgb = Vec::with_capacity(dw * dh * 3);
    for y in 0..dh {
        let row = y * w;
        for x in 0..dw {
            let p = image.pixels[row + x];
            // The frame is opaque; the alpha is folded in so a future
            // translucent chrome cannot turn into black boxes.
            let a = p.a() as u16;
            rgb.push((p.r() as u16 * a / 255) as u8);
            rgb.push((p.g() as u16 * a / 255) as u8);
            rgb.push((p.b() as u16 * a / 255) as u8);
        }
    }
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    if enc
        .encode(&rgb, dw as u32, dh as u32, image::ExtendedColorType::Rgb8)
        .is_err()
    {
        return Vec::new();
    }
    out
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
