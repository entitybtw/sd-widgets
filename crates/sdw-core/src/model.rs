//! SD Widgets shared types. Everything that crosses the network lives here.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROTOCOL: u32 = 1;
pub const API_PORT: u16 = 47830;
pub const DISCOVERY_PORT: u16 = 47832;
pub const DISCOVERY_MAGIC: &str = "SDWIDGETS1?";

fn default_true() -> bool {
    true
}
fn default_fps() -> u32 {
    15
}
fn default_quality() -> u8 {
    75
}

// ---------------------------------------------------------------- pairing

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairRequest {
    pub password: String,
    pub client_name: String,
    #[serde(default)]
    pub client_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairResponse {
    pub token: String,
    pub daemon_name: String,
    pub daemon_id: String,
    pub version: String,
    pub api_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ping {
    pub name: String,
    pub version: String,
    pub protocol: u32,
    pub daemon_id: String,
    pub setup_done: bool,
    pub auth_required: bool,
    pub platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResponse {
    pub magic: String,
    pub name: String,
    pub daemon_id: String,
    pub version: String,
    pub port: u16,
    pub setup_done: bool,
    /// Source address of the responder, filled in by the scanner.
    #[serde(default)]
    pub address: String,
}

// ---------------------------------------------------------------- power

/// A power action on the computer. These are declared variants, not strings:
/// the deck can ask for one of them and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerAction {
    /// Lock the session, or blank the lock screen on Windows.
    Lock,
    /// Suspend to RAM.
    Sleep,
    /// Restart.
    Reboot,
    /// Power off.
    Shutdown,
    /// Turn the display off without locking.
    ScreenOff,
}

impl PowerAction {
    pub const ALL: [PowerAction; 5] = [
        PowerAction::Lock,
        PowerAction::Sleep,
        PowerAction::Reboot,
        PowerAction::Shutdown,
        PowerAction::ScreenOff,
    ];

    /// The command string used by `POST /api/v1/power` when a deck without
    /// the enum (an older bind, an addon action) sends a plain name.
    pub fn key(self) -> &'static str {
        match self {
            Self::Lock => "lock",
            Self::Sleep => "sleep",
            Self::Reboot => "reboot",
            Self::Shutdown => "shutdown",
            Self::ScreenOff => "screen_off",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "lock" => Some(Self::Lock),
            "sleep" | "suspend" => Some(Self::Sleep),
            "reboot" | "restart" => Some(Self::Reboot),
            "shutdown" | "poweroff" | "power_off" => Some(Self::Shutdown),
            "screen_off" | "monitor_off" | "display_off" => Some(Self::ScreenOff),
            _ => None,
        }
    }

    /// True when the action ends the session; the deck asks for a second press.
    pub fn destructive(self) -> bool {
        matches!(self, Self::Reboot | Self::Shutdown)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    Gui,
    Terminal,
}

/// One application the computer can launch, listed by the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppEntry {
    /// Stable id: a `.desktop` file stem on Linux, an `AUMID` or a path stem
    /// on Windows. This is what the deck sends back.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub comment: String,
    /// `gui` or `terminal`, so the deck can show a different glyph.
    #[serde(default = "app_kind_default")]
    pub kind: AppKind,
    /// Desktop categories or the install location; shown as a subtitle.
    #[serde(default)]
    pub source: String,
    /// The computer found a picture for this application. The deck then asks
    /// for it at `/api/v1/apps/{id}/icon`; the path itself never leaves the
    /// computer.
    #[serde(default)]
    pub has_icon: bool,
}

fn app_kind_default() -> AppKind {
    AppKind::Gui
}

/// Bluetooth adapter and the devices known to the computer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BluetoothStatus {
    pub available: bool,
    pub powered: bool,
    pub discovering: bool,
    /// Adapter address, or an empty string when there is no adapter.
    pub adapter: String,
    pub devices: Vec<BluetoothDevice>,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BluetoothDevice {
    pub address: String,
    pub name: String,
    /// BlueZ icon name, e.g. `audio-headphones` or `input-keyboard`.
    pub icon: String,
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
    pub blocked: bool,
    /// Percent, when BlueZ or UPower knows it.
    pub battery: Option<u8>,
    pub rssi: Option<i32>,
}

impl BluetoothDevice {
    /// Headphones, a headset or earbuds: the devices whose battery matters.
    pub fn is_audio(&self) -> bool {
        self.icon.starts_with("audio-")
    }
}

/// One running window or process the computer can focus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowEntry {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub focused: bool,
}

// ---------------------------------------------------------------- status

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub name: String,
    pub hostname: String,
    pub version: String,
    pub daemon_id: String,
    pub platform: String,
    pub uptime_secs: u64,
    pub api_port: u16,
    pub addresses: Vec<String>,
    pub setup_done: bool,
    pub connected_clients: u32,
    pub viiper: ViiperStatus,
    pub monitor: MonitorStatus,
    pub notifications: NotificationStatus,
    pub ha: HaStatus,
    pub pads: Vec<PadInfo>,
    #[serde(default)]
    pub audio: AudioStatus,
    pub addons: Vec<crate::addons::AddonInfo>,
    pub config_dir: String,
    pub log_tail: Vec<LogEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogEntry {
    pub ts: u64,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ViiperStatus {
    pub installed: bool,
    pub running: bool,
    pub version: String,
    pub api_addr: String,
    pub usb_addr: String,
    pub password_set: bool,
    pub key_path: String,
    pub usbip_installed: bool,
    pub auto_start: bool,
    pub error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PadKind {
    #[default]
    Xbox360,
    DualShock4,
    DualSense,
    SwitchPro,
}

impl PadKind {
    pub const ALL: [PadKind; 4] = [
        Self::Xbox360,
        Self::DualShock4,
        Self::DualSense,
        Self::SwitchPro,
    ];
    pub fn viiper_type(self) -> &'static str {
        match self {
            Self::Xbox360 => "xbox360",
            Self::DualShock4 => "dualshock4",
            Self::DualSense => "dualsense",
            Self::SwitchPro => "ns2pro",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Xbox360 => "Xbox 360",
            Self::DualShock4 => "DualShock 4",
            Self::DualSense => "DualSense",
            Self::SwitchPro => "Switch 2 Pro",
        }
    }
    /// The names are product names, so both languages are the same.
    pub fn label_lang(self, _lang: crate::i18n::Lang) -> &'static str {
        self.label()
    }
    pub fn hint(self) -> &'static str {
        self.hint_lang(crate::i18n::Lang::Ru)
    }
    pub fn hint_lang(self, lang: crate::i18n::Lang) -> &'static str {
        let (ru, en) = match self {
            Self::Xbox360 => (
                "Самый совместимый: XInput, Steam, почти все игры.",
                "The most compatible: XInput, Steam, nearly every game.",
            ),
            Self::DualShock4 => ("PS4-пад, гироскоп и тачпад.", "PS4 pad, gyro and touchpad."),
            Self::DualSense => ("PS5-пад, гироскоп и тачпад.", "PS5 pad, gyro and touchpad."),
            Self::SwitchPro => (
                "Nintendo Switch 2 Pro, гироскоп.",
                "Nintendo Switch 2 Pro, gyro.",
            ),
        };
        crate::i18n::tr(lang, ru, en)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PadInfo {
    pub id: String,
    pub name: String,
    pub kind: PadKind,
    pub bus_id: u32,
    pub dev_id: String,
    pub created_at: u64,
    pub connected: bool,
    pub last_input_ms: u64,
    pub rumble: [u8; 2],
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PadCreate {
    pub name: String,
    pub kind: PadKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PadCreated {
    pub pad: PadInfo,
    /// Secret for the low-latency UDP input stream. Never shown again in full.
    pub udp_secret: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PadFeedback {
    pub rumble: [u8; 2],
    pub last_input_ms: u64,
    pub connected: bool,
}

// ---------------------------------------------------------------- monitor

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MonitorMode {
    #[default]
    Extend,
    Mirror,
}

impl MonitorMode {
    pub fn label(self) -> &'static str {
        self.label_lang(crate::i18n::Lang::Ru)
    }

    pub fn label_lang(self, lang: crate::i18n::Lang) -> &'static str {
        let (ru, en) = match self {
            Self::Extend => ("Дополнительный монитор", "Extra monitor"),
            Self::Mirror => ("Зеркало экрана", "Screen mirror"),
        };
        crate::i18n::tr(lang, ru, en)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MonitorBackend {
    #[default]
    Auto,
    Hyprland,
    /// A real output created inside niri itself (biri or the upstream
    /// virtual-output patch). Unlike VKMS it is rendered by the same GPU, so
    /// it works on NVIDIA where a second DRM device cannot be scanned out.
    NiriVirtual,
    Vkms,
    Krfb,
    Mirror,
    X11,
    Gdigrab,
}

impl MonitorBackend {
    pub fn label(self) -> &'static str {
        self.label_lang(crate::i18n::Lang::Ru)
    }

    pub fn label_lang(self, lang: crate::i18n::Lang) -> &'static str {
        let (ru, en) = match self {
            Self::Auto => ("Авто", "Auto"),
            Self::Hyprland => ("Hyprland headless", "Hyprland headless"),
            Self::NiriVirtual => ("niri виртуальный выход", "niri virtual output"),
            Self::Vkms => ("VKMS (виртуальный DRM)", "VKMS (virtual DRM)"),
            Self::Krfb => ("KDE Krfb", "KDE Krfb"),
            Self::Mirror => ("Зеркало (grim)", "Mirror (grim)"),
            Self::X11 => ("X11 (ffmpeg)", "X11 (ffmpeg)"),
            Self::Gdigrab => ("Windows (gdigrab)", "Windows (gdigrab)"),
        };
        crate::i18n::tr(lang, ru, en)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DisplayOutput {
    pub name: String,
    pub description: String,
    pub width: u32,
    pub height: u32,
    pub refresh: f32,
    pub x: i32,
    pub y: i32,
    pub scale: f32,
    pub primary: bool,
    pub virtual_output: bool,
    /// The compositor currently drives this output. A connected but disabled
    /// output has no logical size and must not be used as the layout anchor.
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MonitorPosition {
    /// Pick the side automatically: usually right of the primary display.
    #[default]
    Auto,
    Right,
    Left,
    Above,
    Below,
}

impl MonitorPosition {
    pub const ALL: [MonitorPosition; 5] = [
        Self::Auto,
        Self::Right,
        Self::Left,
        Self::Above,
        Self::Below,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Авто",
            Self::Right => "Справа",
            Self::Left => "Слева",
            Self::Above => "Сверху",
            Self::Below => "Снизу",
        }
    }

    pub fn label_lang(self, lang: crate::i18n::Lang) -> &'static str {
        let (ru, en) = match self {
            Self::Auto => ("Авто", "Auto"),
            Self::Right => ("Справа", "Right"),
            Self::Left => ("Слева", "Left"),
            Self::Above => ("Сверху", "Above"),
            Self::Below => ("Снизу", "Below"),
        };
        crate::i18n::tr(lang, ru, en)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonitorStatus {
    pub running: bool,
    pub mode: MonitorMode,
    #[serde(default)]
    pub position: MonitorPosition,
    pub backend: MonitorBackend,
    pub backend_label: String,
    pub compositor: String,
    pub output: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub quality: u8,
    pub frame_age_ms: u64,
    pub frames: u64,
    pub input_enabled: bool,
    pub outputs: Vec<DisplayOutput>,
    pub tools: Vec<ToolStatus>,
    pub config_hint: String,
    pub config_file: String,
    pub last_error: String,
    /// Non-fatal message, e.g. an automatic fallback to mirroring.
    #[serde(default)]
    pub note: String,
    /// How the computer is virtualised ("kvm", "vmware", …), empty on metal.
    #[serde(default)]
    pub vm: String,
    /// The kernel has a `vkms` module that can provide a virtual output.
    #[serde(default)]
    pub vkms: bool,
    /// The running niri was built with virtual outputs (`niri msg
    /// create-virtual-output`). Such an output is rendered by the compositor's
    /// own GPU and works where VKMS cannot be scanned out.
    #[serde(default)]
    pub niri_virtual: bool,
    /// `vkms` is loaded right now.
    #[serde(default)]
    pub vkms_loaded: bool,
    pub started_at: u64,
}

/// A running or idle PC-audio transfer to the Deck.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioStatus {
    /// PulseAudio/PipeWire and `parec` are present, so streaming can work.
    pub available: bool,
    pub backend: String,
    pub running: bool,
    /// `system` pours the whole computer into the stream, `apps` only the picked ones.
    pub source: String,
    pub apps: Vec<String>,
    pub rate: u32,
    pub channels: u8,
    pub bytes: u64,
    pub seconds: u64,
    pub error: String,
}

/// One application that is currently producing audio on the computer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioApp {
    pub name: String,
    pub binary: String,
    pub index: u32,
    pub media: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioStart {
    /// `system` or `apps`.
    pub source: String,
    /// Application names from `GET /api/v1/audio/sources`.
    pub apps: Vec<String>,
    pub rate: u32,
    pub channels: u8,
}

impl Default for AudioStart {
    fn default() -> Self {
        Self {
            source: "system".into(),
            apps: Vec::new(),
            rate: 48000,
            channels: 2,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolStatus {
    pub name: String,
    pub installed: bool,
    pub path: String,
    pub purpose: String,
    pub install_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorStart {
    #[serde(default)]
    pub mode: MonitorMode,
    #[serde(default)]
    pub backend: MonitorBackend,
    /// Where the new output is placed relative to the primary display.
    #[serde(default)]
    pub position: MonitorPosition,
    #[serde(default)]
    pub output: String,
    #[serde(default = "deck_width")]
    pub width: u32,
    #[serde(default = "deck_height")]
    pub height: u32,
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default = "default_quality")]
    pub quality: u8,
    #[serde(default)]
    pub input_enabled: bool,
    /// Optional sudo password for backends that need root (VKMS).
    #[serde(default)]
    pub sudo_password: String,
}

fn deck_width() -> u32 {
    1280
}
fn deck_height() -> u32 {
    800
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInput {
    /// Normalized 0..1 coordinates inside the streamed output.
    pub x: f32,
    pub y: f32,
    /// move | down | up | click | right_click | scroll_up | scroll_down
    pub action: String,
    #[serde(default)]
    pub amount: i32,
}

// ---------------------------------------------------------------- notifications

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Notification {
    pub id: u64,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub icon: String,
    /// Optional picture carried with the notification, PNG in base64.
    #[serde(default)]
    pub image: String,
    pub urgency: u8,
    pub ts: u64,
    pub has_actions: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationStatus {
    pub enabled: bool,
    pub listening: bool,
    pub count: u32,
    pub unread: u32,
    pub backend: String,
    pub error: String,
    pub recent: Vec<Notification>,
}

// ---------------------------------------------------------------- deck mirror

/// Frames per second a deck sends while somebody on the computer is watching
/// its screen. The frame is halved and JPEG-packed, so this is roughly
/// 100 KB/s on a 1280×800 deck — fine for Wi-Fi, enough to follow the ui.
pub const DECK_SHARE_FPS: u32 = 3;

/// One deck sending its screen to the daemon: who it is, what it shows and
/// how much it has sent. Filled by `POST /api/v1/deck/frame`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeckMirrorClient {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// The page the deck was on when it sent the last frame.
    #[serde(default)]
    pub page: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// Unix milliseconds of the last frame.
    #[serde(default)]
    pub last_ms: u64,
    /// Sending right now (a frame arrived within the last few seconds).
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub frames: u64,
    #[serde(default)]
    pub bytes: u64,
}

/// The live deck picture plus everyone sending one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeckMirrorState {
    /// A frame arrived recently enough to be shown as live.
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub frame_ms: u64,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub page: String,
    /// Bumped with every stored frame, so a viewer decodes each one once.
    #[serde(default)]
    pub seq: u64,
    /// Somebody on the computer is showing the picture right now.
    #[serde(default)]
    pub watched: bool,
    #[serde(default)]
    pub clients: Vec<DeckMirrorClient>,
}

// ---------------------------------------------------------------- media

/// What is playing on the computer right now, plus the system output volume.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MediaState {
    /// A player answered and has something loaded (or at least exists).
    pub available: bool,
    pub player: String,
    pub players: Vec<String>,
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// `mpris:artUrl` as reported by the player (file://, http(s):// or data:).
    pub art_url: String,
    /// Stable key for the current cover; changes when the track changes.
    pub art_id: String,
    pub position: f64,
    pub length: f64,
    pub player_volume: Option<f64>,
    /// System output volume, 0..=100.
    pub volume: u8,
    pub muted: bool,
    pub sink: String,
    pub sinks: Vec<String>,
    pub error: String,
    pub updated_ms: u64,
}

impl MediaState {
    pub fn playing(&self) -> bool {
        self.status.eq_ignore_ascii_case("playing")
    }
}

// ---------------------------------------------------------------- home assistant

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HaStatus {
    pub configured: bool,
    pub connected: bool,
    pub url: String,
    pub entities: usize,
    pub last_sync_ms: u64,
    pub error: String,
    pub favorites: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaEntity {
    pub entity_id: String,
    pub state: String,
    #[serde(default)]
    pub attributes: serde_json::Value,
    #[serde(default)]
    pub last_changed: String,
    #[serde(default)]
    pub last_updated: String,
    /// Home Assistant area (room) name, filled by the daemon.
    #[serde(default)]
    pub area: String,
}

impl HaEntity {
    pub fn domain(&self) -> &str {
        self.entity_id.split('.').next().unwrap_or("")
    }
    pub fn object_id(&self) -> &str {
        self.entity_id.split_once('.').map(|(_, b)| b).unwrap_or("")
    }
    pub fn name(&self) -> String {
        self.attributes
            .get("friendly_name")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| self.object_id())
            .to_string()
    }
    pub fn is_on(&self) -> bool {
        matches!(
            self.state.as_str(),
            "on" | "home" | "playing" | "open" | "unlocked"
        )
    }
    pub fn brightness(&self) -> Option<u8> {
        self.attributes
            .get("brightness")
            .and_then(|v| v.as_u64())
            .map(|v| (v.min(255)) as u8)
    }
    pub fn color_temp_kelvin(&self) -> Option<u32> {
        self.attributes
            .get("color_temp_kelvin")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
    }
    pub fn temperature(&self) -> Option<f32> {
        self.attributes
            .get("temperature")
            .or_else(|| self.attributes.get("current_temperature"))
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
    }
    pub fn current_temperature(&self) -> Option<f32> {
        self.attributes
            .get("current_temperature")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
    }
    pub fn unit(&self) -> String {
        self.attributes
            .get("unit_of_measurement")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }
    pub fn media_title(&self) -> Option<String> {
        self.attributes
            .get("media_title")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
    pub fn media_artist(&self) -> Option<String> {
        self.attributes
            .get("media_artist")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
    pub fn battery(&self) -> Option<u8> {
        self.attributes
            .get("battery_level")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(100) as u8)
    }
    pub fn target_temperature(&self) -> Option<f32> {
        self.attributes
            .get("temperature")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
    }
    pub fn cover_position(&self) -> Option<u8> {
        self.attributes
            .get("current_position")
            .and_then(|v| v.as_u64())
            .map(|v| v.min(100) as u8)
    }
    pub fn is_open(&self) -> bool {
        matches!(self.state.as_str(), "open" | "opening")
    }
    pub fn supports_brightness(&self) -> bool {
        self.attributes.get("brightness").is_some()
            || self
                .attributes
                .get("supported_color_modes")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().any(|m| m.as_str() != Some("onoff")))
                .unwrap_or(false)
    }
    /// Brightness as a percentage, so sliders do not have to know about 0..255.
    pub fn brightness_pct(&self) -> Option<u8> {
        self.brightness()
            .map(|b| ((b as f32 / 255.0) * 100.0).round().clamp(0.0, 100.0) as u8)
    }
    pub fn min_color_temp_kelvin(&self) -> u32 {
        self.attributes
            .get("min_color_temp_kelvin")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(2000)
    }
    pub fn max_color_temp_kelvin(&self) -> u32 {
        self.attributes
            .get("max_color_temp_kelvin")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(6500)
    }
    pub fn supports_color_temp(&self) -> bool {
        // Only a lamp that can actually take a temperature: an RGB-only bulb
        // still reports a stale `color_temp_kelvin`, and presets must not be
        // sent to it as kelvin.
        match self
            .attributes
            .get("supported_color_modes")
            .and_then(|v| v.as_array())
        {
            Some(modes) => modes.iter().any(|m| m.as_str() == Some("color_temp")),
            None => self.attributes.get("color_temp_kelvin").is_some(),
        }
    }
    pub fn supports_color(&self) -> bool {
        self.attributes.get("rgb_color").is_some()
            || self
                .attributes
                .get("supported_color_modes")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter().any(|m| {
                        matches!(
                            m.as_str(),
                            Some("hs") | Some("xy") | Some("rgb") | Some("rgbw") | Some("rgbww")
                        )
                    })
                })
                .unwrap_or(false)
    }
    pub fn rgb_color(&self) -> Option<[u8; 3]> {
        let arr = self.attributes.get("rgb_color")?.as_array()?;
        if arr.len() < 3 {
            return None;
        }
        let c = |i: usize| arr[i].as_u64().map(|v| v.min(255) as u8).unwrap_or(0);
        Some([c(0), c(1), c(2)])
    }
    pub fn media_app(&self) -> String {
        self.attributes
            .get("app_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }
    pub fn icon(&self) -> &str {
        self.attributes
            .get("icon")
            .and_then(|v| v.as_str())
            .unwrap_or("")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaConfigRequest {
    pub url: String,
    pub token: String,
    #[serde(default)]
    pub favorites: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaCall {
    pub domain: String,
    pub service: String,
    #[serde(default)]
    pub target: serde_json::Value,
    #[serde(default)]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HaStates {
    pub entities: Vec<HaEntity>,
    /// Wall-clock time of the last successful sync, `0` when there was none.
    pub fetched_ms: u64,
    /// Age of the data when the daemon answered, in milliseconds. The deck
    /// uses it instead of comparing clocks with another machine.
    #[serde(default)]
    pub age_ms: u64,
    /// Whether the last attempt to reach Home Assistant succeeded.
    #[serde(default)]
    pub connected: bool,
    /// Last connection error, empty when everything is fine.
    #[serde(default)]
    pub error: String,
    /// Area names in Home Assistant order; used by the deck for room paging.
    #[serde(default)]
    pub areas: Vec<String>,
    /// Tabs of the dashboard (Lovelace views), in dashboard order.
    #[serde(default)]
    pub views: Vec<HaView>,
    /// User-made categories, ordered, shared by the daemon and every deck.
    #[serde(default)]
    pub categories: Vec<HaCategory>,
    /// User-made light/scene presets, ordered, shared by everyone.
    #[serde(default)]
    pub presets: Vec<HaPreset>,
}

/// One tab of a Home Assistant dashboard. `entities` keeps the order the
/// dashboard itself uses, so the deck reads like the browser page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HaView {
    pub title: String,
    pub path: String,
    pub icon: String,
    pub entities: Vec<String>,
}

/// A custom group of Home Assistant entities. The order of `entities` is the
/// order shown on the deck; the order of categories is the order of the tabs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaCategory {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub entities: Vec<String>,
}

impl HaCategory {
    pub fn new(name: &str) -> Self {
        Self {
            id: random_id(8),
            name: name.trim().to_string(),
            entities: Vec::new(),
        }
    }
}

/// One entity inside a preset: the service to call and its data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaPresetAction {
    pub entity_id: String,
    #[serde(default = "turn_on_service")]
    pub service: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

fn turn_on_service() -> String {
    "turn_on".into()
}

/// A named set of light/scene states. Applying it calls every action, so a
/// single tap can set five lamps to "кино" without any server-side scenes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaPreset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub actions: Vec<HaPresetAction>,
}

impl HaPreset {
    pub fn new(name: &str) -> Self {
        Self {
            id: random_id(8),
            name: name.trim().to_string(),
            icon: String::new(),
            actions: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------- binds

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwipeDir {
    Left,
    Right,
    Up,
    Down,
}

impl SwipeDir {
    pub fn label(self) -> &'static str {
        self.label_lang(crate::i18n::Lang::Ru)
    }

    pub fn label_lang(self, lang: crate::i18n::Lang) -> &'static str {
        let (ru, en) = match self {
            Self::Left => ("влево", "left"),
            Self::Right => ("вправо", "right"),
            Self::Up => ("вверх", "up"),
            Self::Down => ("вниз", "down"),
        };
        crate::i18n::tr(lang, ru, en)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GestureKind {
    Swipe,
    Tap,
    LongPress,
    PinchIn,
    PinchOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GestureSpec {
    pub kind: GestureKind,
    #[serde(default = "one")]
    pub fingers: u8,
    #[serde(default)]
    pub dir: Option<SwipeDir>,
}

fn one() -> u8 {
    1
}

impl Default for GestureSpec {
    fn default() -> Self {
        Self {
            kind: GestureKind::Tap,
            fingers: 1,
            dir: None,
        }
    }
}

impl GestureSpec {
    pub fn id(&self) -> String {
        match self.kind {
            GestureKind::Swipe => format!(
                "{}f-swipe-{}",
                self.fingers,
                match self.dir.unwrap_or(SwipeDir::Left) {
                    SwipeDir::Left => "left",
                    SwipeDir::Right => "right",
                    SwipeDir::Up => "up",
                    SwipeDir::Down => "down",
                }
            ),
            GestureKind::Tap => format!("{}f-tap", self.fingers),
            GestureKind::LongPress => format!("{}f-long", self.fingers),
            GestureKind::PinchIn => format!("{}f-pinch-in", self.fingers),
            GestureKind::PinchOut => format!("{}f-pinch-out", self.fingers),
        }
    }
    pub fn label(&self) -> String {
        self.label_lang(crate::i18n::Lang::Ru)
    }

    pub fn label_lang(&self, lang: crate::i18n::Lang) -> String {
        let dir = self.dir.unwrap_or(SwipeDir::Left).label_lang(lang);
        // "1 палец", "2 пальца", "5 пальцев" — the count picks the word.
        let fingers = match self.fingers {
            1 => format!("1 {}", crate::t!(lang, "палец", "finger")),
            2..=4 => format!("{} {}", self.fingers, crate::t!(lang, "пальца", "fingers")),
            n => format!("{n} {}", crate::t!(lang, "пальцев", "fingers")),
        };
        let what = match self.kind {
            GestureKind::Swipe => crate::t!(lang, "свайп", "swipe"),
            GestureKind::Tap => crate::t!(lang, "тап", "tap"),
            GestureKind::LongPress => crate::t!(lang, "удержание", "long press"),
            GestureKind::PinchIn => crate::t!(lang, "сжать", "pinch in"),
            GestureKind::PinchOut => crate::t!(lang, "разжать", "pinch out"),
        };
        if matches!(self.kind, GestureKind::Swipe) {
            format!("{fingers}: {what} {dir}")
        } else {
            format!("{fingers}: {what}")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    Gesture {
        spec: GestureSpec,
    },
    Button {
        device: String,
        code: String,
        label: String,
    },
    Combo {
        codes: Vec<String>,
        label: String,
    },
}

impl Trigger {
    pub fn label(&self) -> String {
        self.label_lang(crate::i18n::Lang::Ru)
    }

    pub fn label_lang(&self, lang: crate::i18n::Lang) -> String {
        match self {
            Self::Gesture { spec } => spec.label_lang(lang),
            Self::Button { label, .. } => {
                crate::t!(lang, "Кнопка: {label}", "Button: {label}").replace("{label}", label)
            }
            Self::Combo { label, .. } => {
                crate::t!(lang, "Комбо: {label}", "Combo: {label}").replace("{label}", label)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    None,
    Shell {
        command: String,
    },
    Launch {
        target: String,
    },
    Volume {
        delta: i32,
    },
    VolumeSet {
        percent: u8,
    },
    Mute,
    Brightness {
        delta: i32,
    },
    BrightnessSet {
        percent: u8,
    },
    Media {
        command: String,
    },
    Key {
        combo: String,
    },
    Page {
        page: String,
    },
    Remote {
        host: Option<String>,
        command: String,
    },
    Ha {
        host: Option<String>,
        domain: String,
        service: String,
        entity: String,
        #[serde(default)]
        data: serde_json::Value,
    },
    Url {
        url: String,
    },
    /// Call an API. Values in `{name}` braces are filled from the addon's
    /// settings before the request is sent, so one action can talk to many
    /// services without a line of code.
    Http {
        #[serde(default = "default_method")]
        method: String,
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        body: String,
    },
}

fn default_method() -> String {
    "GET".into()
}

impl Default for Action {
    fn default() -> Self {
        Self::None
    }
}

impl Action {
    pub fn label(&self) -> String {
        self.label_lang(crate::i18n::Lang::Ru)
    }

    pub fn label_lang(&self, lang: crate::i18n::Lang) -> String {
        match self {
            Self::None => crate::t!(lang, "Ничего", "Nothing").into(),
            Self::Shell { command } => crate::t!(lang, "Команда: {command}", "Command: {command}")
                .replace("{command}", command),
            Self::Launch { target } => crate::t!(lang, "Запустить: {target}", "Launch: {target}")
                .replace("{target}", target),
            Self::Volume { delta } => crate::t!(lang, "Громкость {delta:+}%", "Volume {delta:+}%")
                .replace("{delta:+}", &format!("{delta:+}")),
            Self::VolumeSet { percent } => {
                crate::t!(lang, "Громкость {percent}%", "Volume {percent}%")
                    .replace("{percent}", &percent.to_string())
            }
            Self::Mute => crate::t!(lang, "Выключить звук", "Mute").into(),
            Self::Brightness { delta } => {
                crate::t!(lang, "Яркость {delta:+}%", "Brightness {delta:+}%")
                    .replace("{delta:+}", &format!("{delta:+}"))
            }
            Self::BrightnessSet { percent } => {
                crate::t!(lang, "Яркость {percent}%", "Brightness {percent}%")
                    .replace("{percent}", &percent.to_string())
            }
            Self::Media { command } => crate::t!(lang, "Медиа: {command}", "Media: {command}")
                .replace("{command}", command),
            Self::Key { combo } => {
                crate::t!(lang, "Клавиши: {combo}", "Keys: {combo}").replace("{combo}", combo)
            }
            Self::Page { page } => {
                crate::t!(lang, "Открыть: {page}", "Open: {page}").replace("{page}", page)
            }
            Self::Remote { command, .. } => {
                crate::t!(lang, "ПК: {command}", "PC: {command}").replace("{command}", command)
            }
            Self::Ha {
                domain,
                service,
                entity,
                ..
            } => {
                if entity.is_empty() {
                    format!("HA: {domain}.{service}")
                } else {
                    format!("HA: {domain}.{service} · {entity}")
                }
            }
            Self::Url { url } => {
                crate::t!(lang, "Открыть URL: {url}", "Open URL: {url}").replace("{url}", url)
            }
            Self::Http { method, url, .. } => format!("{method} {url}"),
        }
    }

    pub fn kind_label(&self) -> &'static str {
        self.kind_label_lang(crate::i18n::Lang::Ru)
    }

    pub fn kind_label_lang(&self, lang: crate::i18n::Lang) -> &'static str {
        let (ru, en) = match self {
            Self::None => ("Ничего", "Nothing"),
            Self::Shell { .. } => ("Команда", "Command"),
            Self::Launch { .. } => ("Приложение", "App"),
            Self::Volume { .. } | Self::VolumeSet { .. } | Self::Mute => ("Звук", "Sound"),
            Self::Brightness { .. } | Self::BrightnessSet { .. } => ("Яркость", "Brightness"),
            Self::Media { .. } => ("Медиа", "Media"),
            Self::Key { .. } => ("Клавиши", "Keys"),
            Self::Page { .. } => ("Страница", "Page"),
            Self::Remote { .. } => ("ПК", "PC"),
            Self::Ha { .. } => ("Home Assistant", "Home Assistant"),
            Self::Url { .. } => ("URL", "URL"),
            Self::Http { .. } => ("HTTP", "HTTP"),
        };
        crate::i18n::tr(lang, ru, en)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bind {
    pub id: String,
    pub name: String,
    pub trigger: Trigger,
    pub action: Action,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Bind {
    pub fn new(name: impl Into<String>, trigger: Trigger, action: Action) -> Self {
        Self {
            id: random_id(8),
            name: name.into(),
            trigger,
            action,
            enabled: true,
        }
    }
}

// ---------------------------------------------------------------- deck config

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub last_seen_ms: u64,
    #[serde(default = "default_true")]
    pub auto_connect: bool,
    /// The virtual pad this deck created on the host, kept so it survives restarts.
    #[serde(default)]
    pub pad: Option<DeckPad>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckPad {
    pub id: String,
    pub name: String,
    pub kind: PadKind,
    pub secret: String,
    pub created_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckViewCfg {
    #[serde(default = "dv_scale")]
    pub scale: f32,
    #[serde(default)]
    pub labels: bool,
    #[serde(default = "default_true")]
    pub glow: bool,
    #[serde(default = "default_true")]
    pub sticks: bool,
}

fn dv_scale() -> f32 {
    1.0
}

impl Default for DeckViewCfg {
    fn default() -> Self {
        Self {
            scale: 1.0,
            labels: false,
            glow: true,
            sticks: true,
        }
    }
}

/// Everything the deck client lets you restyle. The CSS block is the source of
/// truth for colours; these fields control layout, motion and chrome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeCfg {
    #[serde(default = "default_css")]
    pub css: String,
    /// 0 = hard grid edges, 24 = soft pills. Applies to every surface.
    #[serde(default)]
    pub radius: f32,
    #[serde(default = "default_true")]
    pub grid: bool,
    #[serde(default = "default_grid_step")]
    pub grid_step: f32,
    #[serde(default = "default_grid_alpha")]
    pub grid_alpha: f32,
    #[serde(default = "default_one")]
    pub density: f32,
    #[serde(default = "default_one")]
    pub font_scale: f32,
    #[serde(default = "default_tile_height")]
    pub tile_height: f32,
    #[serde(default = "default_tile_columns")]
    pub tile_columns: u32,
    #[serde(default = "default_true")]
    pub arrows: bool,
    /// Page arrow square size in points.
    #[serde(default = "default_arrow_size")]
    pub arrow_size: f32,
    #[serde(default = "default_true")]
    pub dock: bool,
    /// Dock glyph size in points.
    #[serde(default = "default_dock_icon")]
    pub dock_icon: f32,
    /// Put the dock at the top edge instead of the bottom one.
    #[serde(default)]
    pub dock_top: bool,
    #[serde(default)]
    pub labels: bool,
    #[serde(default = "default_true")]
    pub glyphs: bool,
    #[serde(default = "default_true")]
    pub animations: bool,
    #[serde(default)]
    pub compact: bool,
    /// pages shown in the dock and the arrow walk. Empty = all.
    #[serde(default)]
    pub pages: Vec<String>,
    /// home tiles, in order. Empty = all.
    #[serde(default)]
    pub home_tiles: Vec<String>,
}

fn default_css() -> String {
    crate::model::PRESET_GRID.to_string()
}
fn default_grid_step() -> f32 {
    32.0
}
fn default_grid_alpha() -> f32 {
    9.0
}
fn default_one() -> f32 {
    1.0
}
fn default_arrow_size() -> f32 {
    48.0
}
fn default_dock_icon() -> f32 {
    17.0
}
fn default_tile_height() -> f32 {
    176.0
}
fn default_tile_columns() -> u32 {
    3
}

impl Default for ThemeCfg {
    fn default() -> Self {
        Self {
            css: default_css(),
            radius: 0.0,
            grid: true,
            grid_step: default_grid_step(),
            grid_alpha: default_grid_alpha(),
            density: 1.0,
            font_scale: 1.0,
            tile_height: default_tile_height(),
            tile_columns: default_tile_columns(),
            arrows: true,
            arrow_size: default_arrow_size(),
            dock: true,
            dock_icon: default_dock_icon(),
            dock_top: false,
            labels: false,
            glyphs: true,
            animations: true,
            compact: false,
            pages: Vec::new(),
            home_tiles: Vec::new(),
        }
    }
}

/// UI sounds: a built-in pack plus optional files for any single event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundCfg {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_volume")]
    pub volume: u8,
    /// built-in character: grid, soft or chip
    #[serde(default = "default_sound_pack")]
    pub pack: String,
    #[serde(default)]
    pub click: String,
    #[serde(default)]
    pub navigate: String,
    #[serde(default)]
    pub open: String,
    #[serde(default)]
    pub back: String,
    #[serde(default)]
    pub notify: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub startup: String,
}

fn default_volume() -> u8 {
    70
}
fn default_sound_pack() -> String {
    "grid".into()
}

impl Default for SoundCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            volume: default_volume(),
            pack: default_sound_pack(),
            click: String::new(),
            navigate: String::new(),
            open: String::new(),
            back: String::new(),
            notify: String::new(),
            error: String::new(),
            startup: String::new(),
        }
    }
}

/// Built-in CSS themes. Kept here so both the model defaults and the editor use them.
pub const PRESET_GRID: &str = "\
/* sd-widgets · Grid */
:root {
  --bg: #10141b;
  --bg2: #121820;
  --card: #18212c;
  --well: #18212c;
  --ink: #edf2f8;
  --strong: #ffffff;
  --muted: #acb9ca;
  --faint: #8493a7;
  --line: #263343;
  --line-strong: #344458;
  --accent: #a6c8ff;
  --accent-soft: #9cc3ff;
  --grid-step: 32;
  --grid-alpha: 9;
  --radius: 0;

  --light-bg: #eef1f6;
  --light-bg2: #e7ebf2;
  --light-card: #ffffff;
  --light-well: #f5f7fa;
  --light-ink: #10141b;
  --light-strong: #000000;
  --light-muted: #52627a;
  --light-faint: #6c7c94;
  --light-line: #d3dae5;
  --light-line-strong: #b6c2d2;
  --light-accent: #3b6ea5;
}
";

pub const PRESET_MONO: &str = "\
/* sd-widgets · Monochrome */
:root {
  --bg: #0b0b0b;
  --bg2: #111111;
  --card: #171717;
  --well: #1c1c1c;
  --ink: #f0f0f0;
  --strong: #ffffff;
  --muted: #bdbdbd;
  --faint: #7a7a7a;
  --line: #262626;
  --line-strong: #3a3a3a;
  --accent: #f0f0f0;
  --accent-soft: #cfcfcf;
  --radius: 0;
  --light-bg: #f3f3f3;
  --light-bg2: #ebebeb;
  --light-card: #ffffff;
  --light-well: #f7f7f7;
  --light-ink: #101010;
  --light-strong: #000000;
  --light-muted: #5c5c5c;
  --light-faint: #7a7a7a;
  --light-line: #d6d6d6;
  --light-line-strong: #b8b8b8;
  --light-accent: #101010;
}
";

pub const PRESET_NORD: &str = "\
/* sd-widgets · Nord */
:root {
  --bg: #242933;
  --bg2: #2e3440;
  --card: #2f3742;
  --well: #3b4252;
  --ink: #eceff4;
  --strong: #ffffff;
  --muted: #d8dee9;
  --faint: #8f9bb0;
  --line: #3b4252;
  --line-strong: #4c566a;
  --accent: #88c0d0;
  --accent-soft: #8fbcbb;
  --radius: 2;
  --light-bg: #e5e9f0;
  --light-bg2: #eceff4;
  --light-card: #ffffff;
  --light-well: #e5e9f0;
  --light-ink: #2e3440;
  --light-strong: #000000;
  --light-muted: #4c566a;
  --light-faint: #6d7a8c;
  --light-line: #d0d6e0;
  --light-line-strong: #b4bdcc;
  --light-accent: #3b7d8c;
}
";

pub const PRESET_GRUVBOX: &str = "\
/* sd-widgets · Gruvbox */
:root {
  --bg: #1d2021;
  --bg2: #282828;
  --card: #32302f;
  --well: #3c3836;
  --ink: #ebdbb2;
  --strong: #fbf1c7;
  --muted: #d5c4a1;
  --faint: #a89984;
  --line: #3c3836;
  --line-strong: #504945;
  --accent: #fabd2f;
  --accent-soft: #d79921;
  --radius: 1;
  --light-bg: #fbf1c7;
  --light-bg2: #f2e5bc;
  --light-card: #fffbef;
  --light-well: #f2e5bc;
  --light-ink: #282828;
  --light-strong: #1d2021;
  --light-muted: #665c54;
  --light-faint: #7c6f64;
  --light-line: #e0d3aa;
  --light-line-strong: #c9b98f;
  --light-accent: #b57614;
}
";

pub const PRESET_TERMINAL: &str = "\
/* sd-widgets · Terminal */
:root {
  --bg: #050807;
  --bg2: #0a100e;
  --card: #0d1512;
  --well: #101a16;
  --ink: #c8f7dc;
  --strong: #eafff3;
  --muted: #7fbfa0;
  --faint: #4f7f67;
  --line: #143025;
  --line-strong: #1f4636;
  --accent: #5cff9d;
  --accent-soft: #3be08a;
  --radius: 0;
  --light-bg: #eefaf3;
  --light-bg2: #e2f4ea;
  --light-card: #ffffff;
  --light-well: #e8f6ee;
  --light-ink: #062015;
  --light-strong: #000000;
  --light-muted: #335746;
  --light-faint: #527565;
  --light-line: #c8e4d6;
  --light-line-strong: #a9cfbc;
  --light-accent: #0b6b43;
}
";

pub const PRESET_CATPPUCCIN_MOCHA: &str = "\
/* sd-widgets · Catppuccin Mocha */
:root {
  --bg: #1e1e2e;
  --bg2: #181825;
  --card: #313244;
  --well: #292c3c;
  --ink: #cdd6f4;
  --strong: #f5e0dc;
  --muted: #a6adc8;
  --faint: #7f849c;
  --line: #313244;
  --line-strong: #45475a;
  --accent: #89b4fa;
  --accent-soft: #74c7ec;
  --radius: 0;
  --light-bg: #eff1f5;
  --light-bg2: #e6e9ef;
  --light-card: #ffffff;
  --light-well: #e6e9ef;
  --light-ink: #4c4f69;
  --light-strong: #11111b;
  --light-muted: #6c6f85;
  --light-faint: #9ca0b0;
  --light-line: #ccd0da;
  --light-line-strong: #bcc0cc;
  --light-accent: #1e66f5;
}
";

pub const PRESET_CATPPUCCIN_MACCHIATO: &str = "\
/* sd-widgets · Catppuccin Macchiato */
:root {
  --bg: #24273a;
  --bg2: #1e2030;
  --card: #363a4f;
  --well: #2e3247;
  --ink: #cad3f5;
  --strong: #f4dbd6;
  --muted: #a5adcb;
  --faint: #8087a2;
  --line: #363a4f;
  --line-strong: #494d64;
  --accent: #8aadf4;
  --accent-soft: #7dc4e4;
  --radius: 0;
  --light-bg: #eff1f5;
  --light-bg2: #e6e9ef;
  --light-card: #ffffff;
  --light-well: #e6e9ef;
  --light-ink: #4c4f69;
  --light-strong: #11111b;
  --light-muted: #6c6f85;
  --light-faint: #9ca0b0;
  --light-line: #ccd0da;
  --light-line-strong: #bcc0cc;
  --light-accent: #1e66f5;
}
";

pub const PRESET_CATPPUCCIN_FRAPPE: &str = "\
/* sd-widgets · Catppuccin Frappé */
:root {
  --bg: #303446;
  --bg2: #292c3c;
  --card: #414559;
  --well: #383c4f;
  --ink: #c6d0f5;
  --strong: #f2d5cf;
  --muted: #a5adce;
  --faint: #838ba7;
  --line: #414559;
  --line-strong: #51576d;
  --accent: #8caaee;
  --accent-soft: #85c1dc;
  --radius: 0;
  --light-bg: #eff1f5;
  --light-bg2: #e6e9ef;
  --light-card: #ffffff;
  --light-well: #e6e9ef;
  --light-ink: #4c4f69;
  --light-strong: #11111b;
  --light-muted: #6c6f85;
  --light-faint: #9ca0b0;
  --light-line: #ccd0da;
  --light-line-strong: #bcc0cc;
  --light-accent: #1e66f5;
}
";

pub const PRESET_CATPPUCCIN_LATTE: &str = "\
/* sd-widgets · Catppuccin Latte */
:root {
  --bg: #eff1f5;
  --bg2: #e6e9ef;
  --card: #ffffff;
  --well: #e6e9ef;
  --ink: #4c4f69;
  --strong: #11111b;
  --muted: #6c6f85;
  --faint: #9ca0b0;
  --line: #ccd0da;
  --line-strong: #bcc0cc;
  --accent: #1e66f5;
  --accent-soft: #209fb5;
  --radius: 0;
  --light-bg: #eff1f5;
  --light-bg2: #e6e9ef;
  --light-card: #ffffff;
  --light-well: #e6e9ef;
  --light-ink: #4c4f69;
  --light-strong: #11111b;
  --light-muted: #6c6f85;
  --light-faint: #9ca0b0;
  --light-line: #ccd0da;
  --light-line-strong: #bcc0cc;
  --light-accent: #1e66f5;
}
";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClockCfg {
    /// grid, digital, analog or minimal
    pub style: String,
    pub seconds: bool,
    pub date: bool,
    pub weekday: bool,
    pub battery: bool,
    pub animate: bool,
    /// drift, pulse or off
    pub anim: String,
    /// Animation speed multiplier.
    pub speed: f32,
    /// Paint the clock in the theme ink only.
    pub mono: bool,
    /// 24-hour time; off = 12-hour with an am/pm mark.
    pub h24: bool,
    /// Multiplies every clock font size.
    pub font_scale: f32,
    /// Accent colour, `#rrggbb`. Empty means "take the theme accent".
    pub accent: String,
    /// Background gradient ends, `#rrggbb`. Empty means "take the theme".
    pub bg_top: String,
    pub bg_bottom: String,
    /// A short line under the time, for example a name or a city.
    pub caption: String,
    /// Headset battery and the latest notification, as chips on the face.
    pub show_headset: bool,
    pub show_notifications: bool,
    /// left, center or right: where the big time sits.
    pub position: String,
    /// auto, wide or mono. `auto` follows the style's own face.
    pub font: String,
    /// Take the colours from the active theme instead of the three below.
    /// This is the default: every theme, preset and CSS file repaints the
    /// clock, and only an explicit colour makes it independent.
    pub follow_theme: bool,
    /// One Material symbol shown in the clock's corner, or empty for none.
    pub icon: String,
}

impl Default for ClockCfg {
    fn default() -> Self {
        Self {
            style: "grid".into(),
            seconds: true,
            date: true,
            weekday: true,
            battery: true,
            animate: true,
            anim: "drift".into(),
            speed: 1.0,
            mono: false,
            h24: true,
            font_scale: 1.0,
            accent: String::new(),
            bg_top: String::new(),
            bg_bottom: String::new(),
            caption: String::new(),
            show_headset: true,
            show_notifications: true,
            position: "left".into(),
            font: "auto".into(),
            follow_theme: true,
            icon: String::new(),
        }
    }
}

/// How the Deck receives PC audio. The confirmation count is a deck-side
/// safety net: each step is one more explicit "yes" before the stream starts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioDeckCfg {
    pub volume: u8,
    /// Extra confirmations before audio leaves the computer (0..=10).
    pub confirmations: u8,
    pub source: String,
    pub apps: Vec<String>,
    pub rate: u32,
}

impl Default for AudioDeckCfg {
    fn default() -> Self {
        Self {
            volume: 80,
            confirmations: 1,
            source: "system".into(),
            apps: Vec::new(),
            rate: 48000,
        }
    }
}

/// The extra-monitor settings the screen tab edits. They are saved with the
/// rest of the deck config, so the next launch starts where the last one
/// stopped instead of back at the defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamCfg {
    pub mode: MonitorMode,
    pub backend: MonitorBackend,
    pub position: MonitorPosition,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub quality: u8,
    pub input: bool,
}

impl Default for StreamCfg {
    fn default() -> Self {
        Self {
            mode: MonitorMode::Extend,
            backend: MonitorBackend::Auto,
            position: MonitorPosition::Auto,
            width: 1280,
            height: 800,
            fps: 15,
            quality: 75,
            input: false,
        }
    }
}

/// The little audio visualizer in the top bar and the control centre. Every
/// part can be turned off, and the bars themselves are adjustable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VizCfg {
    pub enabled: bool,
    /// Show the bars in the top bar.
    pub nav: bool,
    /// Show the bars in the pull-down control centre.
    pub shade: bool,
    /// Animate when the computer reports media even if its sound is not on
    /// the deck: the visualizer then follows the track, not the cable.
    pub media: bool,
    /// How many bars, 6..=24.
    pub bars: u8,
    /// Height of the strip in points, 10..=48.
    pub height: u8,
}

impl Default for VizCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            nav: true,
            shade: true,
            media: true,
            bars: 14,
            height: 18,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DeckConfig {
    pub device_name: String,
    #[serde(default = "client_id_default")]
    pub client_id: String,
    #[serde(default)]
    pub hosts: Vec<HostConfig>,
    #[serde(default)]
    pub active_host: Option<String>,
    #[serde(default)]
    pub binds: Vec<Bind>,
    #[serde(default = "default_true")]
    pub global_gestures: bool,
    #[serde(default)]
    pub gesture_device: Option<String>,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_columns")]
    pub columns: u32,
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    #[serde(default)]
    pub last_page: String,
    #[serde(default = "default_true")]
    pub show_clock: bool,
    #[serde(default)]
    pub onboarding_done: bool,
    #[serde(default = "default_lang")]
    pub lang: String,
    #[serde(default = "default_true")]
    pub dark: bool,
    /// Show a smooth in-app popup when a notification arrives from the PC.
    #[serde(default = "default_true")]
    pub notify_popup: bool,
    /// Keep the same popups on top of the fullscreen screen stream, so a
    /// notification is not missed while the extra monitor fills the deck.
    #[serde(default = "default_true")]
    pub notify_fullscreen: bool,
    /// Send this deck's screen to the daemon, so its window can show what is
    /// on the deck. On by default: the frame only goes to the paired computer.
    #[serde(default = "default_true")]
    pub mirror_deck: bool,
    /// Share rate in frames per second. The frame is a full screen and the
    /// link is Wi-Fi, so this stays modest by default.
    #[serde(default = "default_mirror_fps")]
    pub mirror_fps: u32,
    /// JPEG quality of a shared frame (30..=95).
    #[serde(default = "default_mirror_quality")]
    pub mirror_quality: u8,
    #[serde(default = "default_true")]
    pub grid: bool,
    #[serde(default)]
    pub style: ThemeCfg,
    #[serde(default)]
    pub sound: SoundCfg,
    #[serde(default)]
    pub deck_view: DeckViewCfg,
    #[serde(default)]
    pub clock: ClockCfg,
    /// The screen tab's settings, remembered across restarts.
    #[serde(default)]
    pub stream: StreamCfg,
    /// Home Assistant reached straight from the deck, without the computer:
    /// the address and a long-lived token. Empty means "use the daemon".
    #[serde(default)]
    pub ha_url: String,
    #[serde(default)]
    pub ha_token: String,
    #[serde(default)]
    pub audio: AudioDeckCfg,
    #[serde(default)]
    pub viz: VizCfg,
    #[serde(default)]
    pub addons: crate::addons::AddonsCfg,
    /// Lamp colours the user saved on the Deck, whole RGB triples. They sit
    /// next to the built-in palette and survive a restart.
    #[serde(default)]
    pub light_colors: Vec<[u8; 3]>,
}

fn default_lang() -> String {
    "ru".into()
}

fn client_id_default() -> String {
    random_id(8)
}

fn default_theme() -> String {
    "grid".into()
}
fn default_columns() -> u32 {
    3
}
fn default_mirror_fps() -> u32 {
    3
}
fn default_mirror_quality() -> u8 {
    62
}
fn default_ui_scale() -> f32 {
    1.0
}

impl Default for DeckConfig {
    fn default() -> Self {
        Self {
            device_name: "Steam Deck".into(),
            client_id: client_id_default(),
            hosts: Vec::new(),
            active_host: None,
            binds: Vec::new(),
            global_gestures: true,
            gesture_device: None,
            theme: default_theme(),
            columns: default_columns(),
            ui_scale: default_ui_scale(),
            last_page: "home".into(),
            show_clock: true,
            onboarding_done: false,
            lang: default_lang(),
            dark: true,
            notify_popup: true,
            notify_fullscreen: true,
            mirror_deck: true,
            mirror_fps: 3,
            mirror_quality: 62,
            grid: true,
            style: ThemeCfg::default(),
            sound: SoundCfg::default(),
            deck_view: DeckViewCfg::default(),
            clock: ClockCfg::default(),
            stream: StreamCfg::default(),
            ha_url: String::new(),
            ha_token: String::new(),
            audio: AudioDeckCfg::default(),
            viz: VizCfg::default(),
            addons: crate::addons::AddonsCfg::default(),
            light_colors: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------- helpers

pub fn random_id(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    let _ = getrandom::fill(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}
