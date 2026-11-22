//! Daemon configuration on disk. Secrets live in one 0600 TOML file.

use sdw_core::model::{
    now_ms, random_id, HostConfig, MonitorBackend, MonitorMode, MonitorPosition, PadKind,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    pub name: String,
    pub daemon_id: String,
    pub api_port: u16,
    pub input_port: u16,
    pub api_password_hash: String,
    pub api_token: String,
    pub setup_done: bool,
    pub lang: String,
    pub dark: bool,
    pub autostart: bool,
    /// Show a StatusNotifierItem icon while the daemon runs.
    #[serde(default = "default_true")]
    pub tray: bool,
    pub allow_public: bool,
    pub started_once: bool,
    pub viiper: ViiperCfg,
    pub monitor: MonitorCfg,
    pub notifications: NotificationsCfg,
    pub audio: AudioCfg,
    pub ha: HaCfg,
    pub clients: Vec<ClientRecord>,
    pub addons: sdw_core::addons::AddonsCfg,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ViiperCfg {
    pub enabled: bool,
    pub auto_start: bool,
    pub bin: String,
    pub api_addr: String,
    pub usb_addr: String,
    pub password: String,
    pub default_pad: PadKind,
}

/// Streaming the computer's sound to the Deck. Off by default on the daemon;
/// the user enables it consciously because it can capture private audio.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioCfg {
    pub enabled: bool,
    /// Let a paired Deck start the stream. When off, only the daemon window can.
    pub allow_remote: bool,
    pub rate: u32,
    pub channels: u8,
}

impl Default for AudioCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            allow_remote: true,
            rate: 48000,
            channels: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorCfg {
    pub mode: MonitorMode,
    pub backend: MonitorBackend,
    pub position: MonitorPosition,
    pub output: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub quality: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationsCfg {
    pub enabled: bool,
    /// Become `org.freedesktop.Notifications` when no other daemon owns it.
    /// Apps then deliver notifications straight to sd-widgets.
    pub own_service: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HaCfg {
    pub url: String,
    pub token: String,
    pub favorites: Vec<String>,
    /// Custom tabs shared with every deck.
    pub categories: Vec<sdw_core::model::HaCategory>,
    /// Named light presets shared with every deck.
    pub presets: Vec<sdw_core::model::HaPreset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientRecord {
    pub id: String,
    pub name: String,
    pub paired_at: u64,
    pub last_seen_ms: u64,
}

fn default_true() -> bool {
    true
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            name: format!("{} · SD Widgets", sdw_core::util::hostname()),
            daemon_id: random_id(8),
            api_port: sdw_core::model::API_PORT,
            input_port: sdw_core::model::API_PORT + 1,
            api_password_hash: String::new(),
            api_token: random_id(24),
            setup_done: false,
            lang: "ru".into(),
            dark: true,
            autostart: true,
            tray: true,
            allow_public: false,
            started_once: false,
            viiper: ViiperCfg::default(),
            monitor: MonitorCfg::default(),
            notifications: NotificationsCfg::default(),
            audio: AudioCfg::default(),
            ha: HaCfg::default(),
            clients: Vec::new(),
            addons: sdw_core::addons::AddonsCfg::default(),
        }
    }
}

impl Default for ViiperCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_start: true,
            bin: String::new(),
            api_addr: "127.0.0.1:3242".into(),
            usb_addr: "127.0.0.1:3241".into(),
            password: String::new(),
            default_pad: PadKind::Xbox360,
        }
    }
}

impl Default for MonitorCfg {
    fn default() -> Self {
        Self {
            mode: MonitorMode::Extend,
            backend: MonitorBackend::Auto,
            position: MonitorPosition::Auto,
            output: String::new(),
            width: 1280,
            height: 800,
            fps: 15,
            quality: 75,
        }
    }
}

impl Default for NotificationsCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            own_service: true,
        }
    }
}

impl Default for HaCfg {
    fn default() -> Self {
        Self {
            url: String::new(),
            token: String::new(),
            favorites: Vec::new(),
            categories: Vec::new(),
            presets: Vec::new(),
        }
    }
}

impl DaemonConfig {
    pub fn lang(&self) -> sdw_core::Lang {
        sdw_core::Lang::from_code(&self.lang)
    }

    pub fn password_set(&self) -> bool {
        !self.api_password_hash.is_empty()
    }

    pub fn set_password(&mut self, password: &str) {
        self.api_password_hash = sdw_core::auth::hash_password(password);
        self.api_token = random_id(24);
        self.setup_done = true;
    }

    pub fn rotate_token(&mut self) -> String {
        self.api_token = random_id(24);
        self.api_token.clone()
    }

    pub fn record_client(&mut self, id: &str, name: &str) {
        let ts = now_ms();
        if let Some(c) = self.clients.iter_mut().find(|c| c.id == id) {
            c.name = name.to_string();
            c.last_seen_ms = ts;
        } else {
            self.clients.push(ClientRecord {
                id: id.to_string(),
                name: name.to_string(),
                paired_at: ts,
                last_seen_ms: ts,
            });
        }
    }

    pub fn touch_client(&mut self, id: &str) {
        if let Some(c) = self.clients.iter_mut().find(|c| c.id == id) {
            c.last_seen_ms = now_ms();
        }
    }

    pub fn host_stub(&self) -> HostConfig {
        HostConfig {
            id: self.daemon_id.clone(),
            name: self.name.clone(),
            url: format!("http://127.0.0.1:{}", self.api_port),
            token: String::new(),
            last_seen_ms: now_ms(),
            auto_connect: true,
            pad: None,
        }
    }
}

pub fn path() -> PathBuf {
    sdw_core::util::config_dir().join("daemon.toml")
}

/// A one-shot signal between the tray, a second launch and the running daemon:
/// `show` brings a hidden window back, `quit` stops the process.
pub fn request(kind: &str) {
    let dir = sdw_core::util::config_dir();
    let _ = sdw_core::util::ensure_dir(&dir);
    let _ = std::fs::write(dir.join(format!("daemon.{kind}")), b"1");
}

/// Present while the daemon runs a GUI thread, so a second launch knows whether
/// asking the running daemon for a window is useful or whether it is headless.
/// It exists even while no window is open: the daemon opens one on request.
pub fn window_marker() -> PathBuf {
    sdw_core::util::config_dir().join("daemon.window")
}

pub fn take_request(kind: &str) -> bool {
    let path = sdw_core::util::config_dir().join(format!("daemon.{kind}"));
    if path.exists() {
        let _ = std::fs::remove_file(&path);
        true
    } else {
        false
    }
}

pub fn load() -> DaemonConfig {
    let p = path();
    if let Ok(s) = std::fs::read_to_string(&p) {
        match toml::from_str::<DaemonConfig>(&s) {
            Ok(mut cfg) => {
                if cfg.api_token.is_empty() {
                    cfg.api_token = random_id(24);
                }
                if cfg.daemon_id.is_empty() {
                    cfg.daemon_id = random_id(8);
                }
                return cfg;
            }
            Err(e) => {
                let backup = p.with_extension(format!("broken.{}.toml", now_ms()));
                let _ = std::fs::copy(&p, &backup);
                eprintln!("config: {e}; сохранена копия {}", backup.display());
            }
        }
    }
    let cfg = DaemonConfig::default();
    let _ = save(&cfg);
    cfg
}

pub fn save(cfg: &DaemonConfig) -> Result<(), String> {
    let s = toml::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    sdw_core::util::atomic_write(&path(), s.as_bytes()).map_err(|e| e.to_string())
}

fn running_as_root() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self")
            .map(|m| m.uid() == 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Where VIIPER stores its API key.
///
/// VIIPER itself uses `/etc/viiper` when the server runs as root and the XDG
/// config directory otherwise, so the daemon must look in the same place.
pub fn viiper_key_path() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        if running_as_root() {
            return PathBuf::from("/etc/viiper/viiper.key.txt");
        }
        sdw_core::util::config_dir()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
            .join("github.com/Alia5/viiper/viiper.key.txt")
    }
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("VIIPER/viiper.key.txt")
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        sdw_core::util::config_dir().join("viiper.key.txt")
    }
}

/// Every key file the daemon may read: its own path first, then the paths used
/// by a root VIIPER service and by older installs.
pub fn viiper_key_paths() -> Vec<PathBuf> {
    let mut out = vec![viiper_key_path()];
    #[cfg(target_os = "linux")]
    {
        let mut add = |p: PathBuf| {
            if !out.contains(&p) {
                out.push(p);
            }
        };
        add(PathBuf::from("/etc/viiper/viiper.key.txt"));
        if let Some(home) = dirs::home_dir() {
            add(home.join(".config/github.com/Alia5/viiper/viiper.key.txt"));
        }
    }
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("AppData") {
            let p = PathBuf::from(appdata).join("VIIPER/viiper.key.txt");
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

pub fn viiper_bin_path() -> PathBuf {
    let name = if cfg!(windows) {
        "viiper.exe"
    } else {
        "viiper"
    };
    sdw_core::util::data_dir().join("bin").join(name)
}

pub fn log_path() -> PathBuf {
    sdw_core::util::data_dir().join("daemon.log")
}

/// niri and Hyprland read `$XDG_CONFIG_HOME` only when it is set; otherwise
/// their configs live under `$HOME/.config`. `dirs::config_dir` always answers
/// `$HOME/.config` on Linux, which would miss a session that sets the variable.
pub fn session_config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::config_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn niri_config_path() -> PathBuf {
    session_config_dir().join("niri/config.kdl")
}

pub fn hypr_config_path() -> PathBuf {
    session_config_dir().join("hypr/hyprland.conf")
}
