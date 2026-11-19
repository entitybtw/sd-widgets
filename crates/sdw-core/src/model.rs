//! SD Widgets shared types. Everything that crosses the network lives here.

use serde::{Deserialize, Serialize};

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
    pub fn hint(self) -> &'static str {
        match self {
            Self::Xbox360 => "Самый совместимый: XInput, Steam, почти все игры.",
            Self::DualShock4 => "PS4-пад, гироскоп и тачпад.",
            Self::DualSense => "PS5-пад, гироскоп и тачпад.",
            Self::SwitchPro => "Nintendo Switch 2 Pro, гироскоп.",
        }
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
        match self {
            Self::Extend => "Дополнительный монитор",
            Self::Mirror => "Зеркало экрана",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MonitorBackend {
    #[default]
    Auto,
    Hyprland,
    Vkms,
    Krfb,
    Mirror,
    X11,
    Gdigrab,
    MacCapture,
}

impl MonitorBackend {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Авто",
            Self::Hyprland => "Hyprland headless",
            Self::Vkms => "VKMS (виртуальный DRM)",
            Self::Krfb => "KDE Krfb",
            Self::Mirror => "Зеркало (grim)",
            Self::X11 => "X11 (ffmpeg)",
            Self::Gdigrab => "Windows (gdigrab)",
            Self::MacCapture => "macOS (screencapture)",
        }
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
        match lang {
            crate::i18n::Lang::Ru => self.label(),
            crate::i18n::Lang::En => match self {
                Self::Auto => "Auto",
                Self::Right => "Right",
                Self::Left => "Left",
                Self::Above => "Above",
                Self::Below => "Below",
            },
        }
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
    pub started_at: u64,
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
        matches!(self.state.as_str(), "on" | "home" | "playing" | "open" | "unlocked")
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
    pub fetched_ms: u64,
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
        match self {
            Self::Left => "влево",
            Self::Right => "вправо",
            Self::Up => "вверх",
            Self::Down => "вниз",
        }
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
        let fingers = match self.fingers {
            1 => "1 палец".to_string(),
            2 => "2 пальца".to_string(),
            3 => "3 пальца".to_string(),
            4 => "4 пальца".to_string(),
            n => format!("{n} пальцев"),
        };
        match self.kind {
            GestureKind::Swipe => format!(
                "{fingers}: свайп {}",
                self.dir.unwrap_or(SwipeDir::Left).label()
            ),
            GestureKind::Tap => format!("{fingers}: тап"),
            GestureKind::LongPress => format!("{fingers}: удержание"),
            GestureKind::PinchIn => format!("{fingers}: сжать"),
            GestureKind::PinchOut => format!("{fingers}: разжать"),
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
        match self {
            Self::Gesture { spec } => spec.label(),
            Self::Button { label, .. } => format!("Кнопка: {label}"),
            Self::Combo { label, .. } => format!("Комбо: {label}"),
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
}

impl Default for Action {
    fn default() -> Self {
        Self::None
    }
}

impl Action {
    pub fn label(&self) -> String {
        match self {
            Self::None => "Ничего".into(),
            Self::Shell { command } => format!("Команда: {command}"),
            Self::Launch { target } => format!("Запустить: {target}"),
            Self::Volume { delta } => format!("Громкость {delta:+}%"),
            Self::VolumeSet { percent } => format!("Громкость {percent}%"),
            Self::Mute => "Выключить звук".into(),
            Self::Brightness { delta } => format!("Яркость {delta:+}%"),
            Self::BrightnessSet { percent } => format!("Яркость {percent}%"),
            Self::Media { command } => format!("Медиа: {command}"),
            Self::Key { combo } => format!("Клавиши: {combo}"),
            Self::Page { page } => format!("Открыть: {page}"),
            Self::Remote { command, .. } => format!("ПК: {command}"),
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
            Self::Url { url } => format!("Открыть URL: {url}"),
        }
    }
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::None => "Ничего",
            Self::Shell { .. } => "Команда",
            Self::Launch { .. } => "Приложение",
            Self::Volume { .. } | Self::VolumeSet { .. } | Self::Mute => "Звук",
            Self::Brightness { .. } | Self::BrightnessSet { .. } => "Яркость",
            Self::Media { .. } => "Медиа",
            Self::Key { .. } => "Клавиши",
            Self::Page { .. } => "Страница",
            Self::Remote { .. } => "ПК",
            Self::Ha { .. } => "Home Assistant",
            Self::Url { .. } => "URL",
        }
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
    #[serde(default = "default_true")]
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
            labels: true,
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
    /// 0 = hard edges, 24 = soft pills. Applies to every surface.
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
    #[serde(default = "default_true")]
    pub dock: bool,
    #[serde(default = "default_true")]
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
            dock: true,
            labels: true,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(default = "default_true")]
    pub grid: bool,
    #[serde(default)]
    pub style: ThemeCfg,
    #[serde(default)]
    pub sound: SoundCfg,
    #[serde(default)]
    pub deck_view: DeckViewCfg,
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
            grid: true,
            style: ThemeCfg::default(),
            sound: SoundCfg::default(),
            deck_view: DeckViewCfg::default(),
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
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}
