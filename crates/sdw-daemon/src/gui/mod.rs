//! the daemon window: setup, overview, gamepads, screen, notifications, home assistant,
//! security, install and logs. one column of hairline cards, nothing rounded.

mod theme;

use crate::autostart;
use crate::config::{self, DaemonConfig};
use crate::install;
use crate::state::AppState;
use eframe::egui::{
    self, Align, Color32, FontFamily, Layout, Margin, RichText, ScrollArea, Stroke, TextEdit, Ui,
    Vec2,
};
use sdw_core::model::*;
use sdw_core::update::{self, UpdateInfo};
use sdw_core::{t, Lang};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use theme::{card, divider, ghost, hud, mono, primary, row, small, switch, tag, Tokens};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Gamepads,
    Screen,
    Audio,
    Notifications,
    HomeAssistant,
    Control,
    Bluetooth,
    Addons,
    Security,
    Install,
    Settings,
    Logs,
}

impl Page {
    const ALL: [Page; 13] = [
        Page::Overview,
        Page::Gamepads,
        Page::Screen,
        Page::Audio,
        Page::Notifications,
        Page::HomeAssistant,
        Page::Control,
        Page::Bluetooth,
        Page::Addons,
        Page::Security,
        Page::Install,
        Page::Settings,
        Page::Logs,
    ];
    fn label(self, l: Lang) -> &'static str {
        match self {
            Page::Overview => t!(l, "обзор", "overview"),
            Page::Gamepads => t!(l, "геймпады", "gamepads"),
            Page::Screen => t!(l, "экран", "screen"),
            Page::Audio => t!(l, "звук", "audio"),
            Page::Notifications => t!(l, "уведомления", "notifications"),
            Page::HomeAssistant => t!(l, "home assistant", "home assistant"),
            Page::Control => t!(l, "пульт", "control"),
            Page::Bluetooth => t!(l, "bluetooth", "bluetooth"),
            Page::Addons => t!(l, "аддоны", "addons"),
            Page::Security => t!(l, "безопасность", "security"),
            Page::Install => t!(l, "установка", "install"),
            Page::Settings => t!(l, "настройки", "settings"),
            Page::Logs => t!(l, "журнал", "log"),
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Page::Overview => theme::icon::OVERVIEW,
            Page::Gamepads => theme::icon::GAMEPADS,
            Page::Screen => theme::icon::SCREEN,
            Page::Audio => theme::icon::AUDIO,
            Page::Notifications => theme::icon::NOTIFICATIONS,
            Page::HomeAssistant => theme::icon::HOME_ASSISTANT,
            Page::Control => theme::icon::POWER,
            Page::Bluetooth => theme::icon::BLUETOOTH,
            Page::Addons => theme::icon::ADDON,
            Page::Security => theme::icon::SECURITY,
            Page::Install => theme::icon::INSTALL,
            Page::Settings => theme::icon::SETTINGS,
            Page::Logs => theme::icon::LOGS,
        }
    }
}

enum TaskResult {
    Msg(Result<String, String>),
    Pad(Result<PadCreated, String>),
    Monitor(Result<MonitorStatus, String>),
    Update(Result<UpdateInfo, String>),
    Updated(Result<(String, u64), String>),
    Installed(Result<String, String>),
    Catalog(Vec<sdw_core::addons::CatalogEntry>, String),
    AddonInstalled(Result<sdw_core::addons::Addon, String>),
    Apps(Vec<AppEntry>),
    Bluetooth(Result<sdw_core::model::BluetoothStatus, String>),
    BtAction(Result<String, String>),
}

#[derive(Clone)]
struct ScreenForm {
    mode: MonitorMode,
    backend: MonitorBackend,
    position: MonitorPosition,
    output: String,
    width: u32,
    height: u32,
    fps: u32,
    quality: u8,
    input: bool,
    sudo: String,
    step: usize,
    preset: usize,
}

impl Default for ScreenForm {
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
            input: false,
            sudo: String::new(),
            step: 0,
            preset: 0,
        }
    }
}

struct Toast {
    text: String,
    ok: bool,
    at: Instant,
}

pub struct App {
    state: Arc<AppState>,
    lang: Lang,
    dark: bool,
    applied_dark: bool,
    page: Page,
    status: DaemonStatus,
    last_poll: Instant,
    toast: Option<Toast>,
    task_tx: Sender<TaskResult>,
    task_rx: Receiver<TaskResult>,

    // setup
    setup_step: usize,
    setup_name: String,
    setup_pass: String,
    setup_pass2: String,
    setup_show: bool,
    setup_viiper_pass: String,
    setup_start_screen: bool,
    setup_screen_mode: MonitorMode,
    setup_preset: usize,
    setup_sudo: String,

    // gamepads
    pad_name: String,
    pad_kind: PadKind,
    created_pad: Option<PadCreated>,
    show_pad_secret: bool,
    viiper_pass: String,
    viiper_show: bool,

    // screen
    screen: ScreenForm,
    screen_loaded: bool,
    frame_tex: Option<egui::TextureHandle>,
    last_frames: u64,
    show_hint: bool,

    // audio
    audio_source: String,
    audio_apps: Vec<AudioApp>,
    audio_app_sel: HashSet<String>,
    audio_apps_at: Instant,

    // notifications
    notif_enabled: bool,

    // home assistant
    ha_url: String,
    ha_token: String,
    ha_show_token: bool,
    ha_search: String,
    ha_entities: Vec<HaEntity>,
    ha_categories: Vec<HaCategory>,
    ha_new_category: String,
    ha_pending_category: Option<(String, Option<String>)>,
    ha_presets: Vec<HaPreset>,
    ha_new_preset: String,
    last_ha_fetch: Instant,

    // security
    new_pass: String,
    new_pass2: String,
    show_pass: bool,
    new_token: Option<String>,

    // settings
    name_edit: String,
    addon_import: String,
    addon_repo_input: String,
    addon_catalog: Vec<sdw_core::addons::CatalogEntry>,
    addon_catalog_pending: bool,
    addon_catalog_err: String,
    addon_catalog_at: Option<Instant>,
    addon_installing: Option<String>,
    control_apps: Vec<AppEntry>,
    control_filter: String,
    control_pending: bool,
    control_scanned: bool,
    bluetooth: Option<sdw_core::model::BluetoothStatus>,
    bluetooth_pending: bool,
    bluetooth_loaded: bool,
    bluetooth_err: String,
    bluetooth_remove: Option<String>,

    // updates
    update_info: Option<UpdateInfo>,
    update_error: String,
    update_checking: bool,
    update_downloading: bool,
    update_progress: Arc<AtomicU64>,
    update_total: u64,
    update_path: String,
    last_update_check: Instant,
    update_auto_notified: bool,

    // install
    install_sel: HashSet<String>,
    install_sudo: String,
    install_components: Vec<install::ComponentStatus>,
    last_job: u64,

    // logs
    logs_follow: bool,
    /// The "it keeps running in the background" notice is written once.
    hidden_notice: bool,
    /// When the close button was last pressed; a second press quits.
    close_at: Option<Instant>,
    /// Started by the session (`--hidden`): hide on the first frame.
    start_hidden: bool,
}

/// Set once the user really wants the process to end (the tray, `--quit` or the
/// quit button). Until then closing the window only hides it.
static ALLOW_CLOSE: AtomicBool = AtomicBool::new(false);

pub fn run(state: Arc<AppState>, hidden: bool) -> eframe::Result<()> {
    let marker = config::window_marker();
    let _ = std::fs::write(&marker, b"1");
    let options = eframe::NativeOptions {
        viewport: {
            let mut vp = egui::ViewportBuilder::default()
                .with_inner_size([1180.0, 790.0])
                .with_min_inner_size([920.0, 620.0])
                .with_title("sd-widgets");
            if let Some(icon) = theme::window_icon() {
                vp = vp.with_icon(std::sync::Arc::new(icon));
            }
            vp
        },
        ..Default::default()
    };
    let result = eframe::run_native(
        "sd-widgets",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            let app = App::new(state.clone(), hidden);
            theme::apply(&cc.egui_ctx, app.dark);
            watch_requests(state.clone(), cc.egui_ctx.clone());
            Ok(Box::new(app))
        }),
    );
    let _ = std::fs::remove_file(&marker);
    result
}

/// The window is hidden, not gone: the tray, `--show` and a second launch all
/// drop a small request file that this thread turns back into a visible window.
/// The HTTP server and the Deck connections never stop meanwhile.
fn watch_requests(state: Arc<AppState>, ctx: egui::Context) {
    std::thread::Builder::new()
        .name("sdw-daemon-watch".into())
        .spawn(move || loop {
            if config::take_request("quit") {
                ALLOW_CLOSE.store(true, Ordering::Relaxed);
                state.shutdown();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            if config::take_request("show") {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.request_repaint();
            }
            std::thread::sleep(Duration::from_millis(300));
        })
        .ok();
}

impl App {
    fn new(state: Arc<AppState>, hidden: bool) -> Self {
        let cfg = state.cfg();
        let (task_tx, task_rx) = channel();
        let components = install::component_status();
        let mut install_sel = HashSet::new();
        for c in &components {
            if !c.installed {
                install_sel.insert(c.id.clone());
            }
        }
        let status = state.status();
        let notif_enabled = cfg.notifications.enabled;
        Self {
            lang: Lang::from_code(&cfg.lang),
            dark: cfg.dark,
            applied_dark: cfg.dark,
            page: Page::Overview,
            status,
            last_poll: Instant::now(),
            toast: None,
            task_tx,
            task_rx,
            setup_step: 0,
            setup_name: cfg.name.clone(),
            setup_pass: String::new(),
            setup_pass2: String::new(),
            setup_show: false,
            setup_viiper_pass: cfg.viiper.password.clone(),
            setup_start_screen: false,
            setup_screen_mode: MonitorMode::Extend,
            setup_preset: 0,
            setup_sudo: String::new(),
            pad_name: String::new(),
            pad_kind: cfg.viiper.default_pad,
            created_pad: None,
            show_pad_secret: false,
            viiper_pass: cfg.viiper.password.clone(),
            viiper_show: false,
            screen: ScreenForm::default(),
            screen_loaded: false,
            frame_tex: None,
            last_frames: 0,
            show_hint: false,
            audio_source: "system".into(),
            audio_apps: Vec::new(),
            audio_app_sel: HashSet::new(),
            audio_apps_at: Instant::now() - Duration::from_secs(60),
            notif_enabled,
            ha_url: cfg.ha.url.clone(),
            ha_token: cfg.ha.token.clone(),
            ha_show_token: false,
            ha_search: String::new(),
            ha_entities: Vec::new(),
            ha_categories: cfg.ha.categories.clone(),
            ha_new_category: String::new(),
            ha_pending_category: None,
            ha_presets: cfg.ha.presets.clone(),
            ha_new_preset: String::new(),
            last_ha_fetch: Instant::now() - Duration::from_secs(60),
            new_pass: String::new(),
            new_pass2: String::new(),
            show_pass: false,
            new_token: None,
            name_edit: cfg.name.clone(),
            addon_import: String::new(),
            addon_repo_input: String::new(),
            addon_catalog: Vec::new(),
            addon_catalog_pending: false,
            addon_catalog_err: String::new(),
            addon_catalog_at: None,
            addon_installing: None,
            control_apps: Vec::new(),
            control_filter: String::new(),
            control_pending: false,
            control_scanned: false,
            bluetooth: None,
            bluetooth_pending: false,
            bluetooth_loaded: false,
            bluetooth_err: String::new(),
            bluetooth_remove: None,
            update_info: None,
            update_error: String::new(),
            update_checking: false,
            update_downloading: false,
            update_progress: Arc::new(AtomicU64::new(0)),
            update_total: 0,
            update_path: String::new(),
            last_update_check: Instant::now() - Duration::from_secs(86_400),
            update_auto_notified: false,
            install_sel,
            install_sudo: String::new(),
            install_components: components,
            last_job: 0,
            logs_follow: true,
            hidden_notice: false,
            start_hidden: hidden,
            close_at: None,
            state,
        }
    }

    fn toast(&mut self, text: impl Into<String>, ok: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            ok,
            at: Instant::now(),
        });
    }

    fn save(&mut self, f: impl FnOnce(&mut DaemonConfig)) {
        if let Err(e) = self.state.update_cfg(f) {
            self.toast(format!("не удалось сохранить: {e}"), false);
        }
    }

    fn spawn<F>(&self, f: F)
    where
        F: FnOnce() -> TaskResult + Send + 'static,
    {
        let tx = self.task_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
    }

    /// Read every store catalogue off the UI thread.
    fn load_addon_catalog(&mut self) {
        if self.addon_catalog_pending {
            return;
        }
        let repos = self.state.cfg().addons.repos;
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
            TaskResult::Catalog(list, errors.join("; "))
        });
    }

    fn poll(&mut self) {
        if self.last_poll.elapsed() >= Duration::from_millis(900) {
            self.last_poll = Instant::now();
            self.status = self.state.status();
            if self
                .toast
                .as_ref()
                .is_some_and(|t| t.at.elapsed() > Duration::from_secs(6))
            {
                self.toast = None;
            }
        }
        if !self.update_checking && self.last_update_check.elapsed() > Duration::from_secs(6 * 3600)
        {
            self.last_update_check = Instant::now();
            self.check_updates();
        }
        while let Ok(task) = self.task_rx.try_recv() {
            match task {
                TaskResult::Msg(Ok(m)) => self.toast(m, true),
                TaskResult::Msg(Err(e)) => self.toast(e, false),
                TaskResult::Pad(Ok(p)) => {
                    self.toast(format!("пад «{}» создан", p.pad.name), true);
                    self.created_pad = Some(p);
                    self.show_pad_secret = true;
                    self.pad_name.clear();
                }
                TaskResult::Pad(Err(e)) => self.toast(e, false),
                TaskResult::Monitor(Ok(m)) => {
                    self.toast(
                        if m.running {
                            format!("экран запущен · {}", m.backend_label)
                        } else {
                            "экран остановлен".into()
                        },
                        true,
                    );
                    self.screen.step = 3;
                }
                TaskResult::Monitor(Err(e)) => self.toast(e, false),
                TaskResult::Update(Ok(info)) => {
                    self.update_checking = false;
                    self.update_error.clear();
                    if info.newer && !self.update_auto_notified {
                        self.update_auto_notified = true;
                        self.toast(
                            format!(
                                "доступно обновление {} (сейчас {})",
                                info.latest, info.current
                            ),
                            true,
                        );
                    }
                    self.update_info = Some(info);
                }
                TaskResult::Update(Err(e)) => {
                    self.update_checking = false;
                    self.update_error = e;
                }
                TaskResult::Updated(Ok((path, size))) => {
                    self.update_downloading = false;
                    self.update_path = path.clone();
                    self.update_total = size;
                    self.toast("обновление скачано", true);
                }
                TaskResult::Updated(Err(e)) => {
                    self.update_downloading = false;
                    self.toast(format!("скачивание: {e}"), false);
                }
                TaskResult::Catalog(list, errors) => {
                    self.addon_catalog_pending = false;
                    self.addon_catalog = list;
                    self.addon_catalog_err = errors;
                }
                TaskResult::AddonInstalled(Ok(addon)) => {
                    self.addon_installing = None;
                    let id = addon.id().to_string();
                    self.save(|c| {
                        c.addons.set(&id, true);
                    });
                    for field in &addon.manifest.settings {
                        let key = field.key.clone();
                        let value = field.default.clone();
                        self.save(|c| {
                            if c.addons.value(&id, &key).is_null() {
                                c.addons.set_value(&id, &key, value);
                            }
                        });
                    }
                    self.toast(
                        format!(
                            "{}: {}",
                            t!(self.lang, "аддон установлен", "addon installed"),
                            addon.manifest.name
                        ),
                        true,
                    );
                    self.status = self.state.status();
                }
                TaskResult::AddonInstalled(Err(e)) => {
                    self.addon_installing = None;
                    self.toast(e, false);
                }
                TaskResult::Apps(list) => {
                    self.control_pending = false;
                    self.control_apps = list;
                }
                TaskResult::Bluetooth(Ok(status)) => {
                    self.bluetooth_pending = false;
                    self.bluetooth_loaded = true;
                    self.bluetooth_err = status.error.clone();
                    self.bluetooth = Some(status);
                }
                TaskResult::Bluetooth(Err(e)) => {
                    self.bluetooth_pending = false;
                    self.bluetooth_loaded = true;
                    self.bluetooth_err = e;
                }
                TaskResult::BtAction(Ok(message)) => {
                    self.toast(message, true);
                    self.bluetooth_loaded = false;
                    self.bluetooth_remove = None;
                }
                TaskResult::BtAction(Err(e)) => self.toast(e, false),
                TaskResult::Installed(Ok(msg)) => self.toast(msg, true),
                TaskResult::Installed(Err(e)) => self.toast(format!("установка: {e}"), false),
            }
        }
    }

    fn check_updates(&mut self) {
        if self.update_checking {
            return;
        }
        self.update_checking = true;
        self.update_error.clear();
        self.spawn(|| TaskResult::Update(update::check()));
    }

    fn download_update(&mut self) {
        let Some(info) = self.update_info.clone() else {
            return;
        };
        let Some(asset) = info.asset_for("sdw-daemon").cloned() else {
            self.toast("в релизе нет файла для этого компьютера", false);
            return;
        };
        if self.update_downloading {
            return;
        }
        self.update_downloading = true;
        self.update_total = asset.size;
        self.update_progress = Arc::new(AtomicU64::new(0));
        let progress = self.update_progress.clone();
        self.spawn(move || {
            let dest = update::updates_dir().join(&asset.name);
            match update::download(&asset, &dest, progress) {
                Ok(n) => TaskResult::Updated(Ok((dest.display().to_string(), n))),
                Err(e) => TaskResult::Updated(Err(e)),
            }
        });
    }

    fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        self.save(|c| c.lang = lang.code().to_string());
    }

    fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
        self.save(|c| c.dark = dark);
    }

    fn start_viiper(&mut self, restart: bool) {
        let state = self.state.clone();
        self.toast(
            if restart {
                "перезапускаю viiper…"
            } else {
                "запускаю viiper…"
            },
            true,
        );
        self.spawn(move || {
            let cfg = state.cfg();
            let r = if restart {
                state.viiper.restart(&cfg)
            } else {
                state.viiper.start(&cfg)
            };
            TaskResult::Msg(r.map(|_| "viiper запущен".to_string()))
        });
    }

    fn stop_viiper(&mut self) {
        self.state.viiper.stop();
        self.toast("viiper остановлен", true);
        self.last_poll = Instant::now() - Duration::from_secs(2);
    }

    fn start_screen(&mut self) {
        let req = MonitorStart {
            mode: self.screen.mode,
            backend: self.screen.backend,
            position: self.screen.position,
            output: self.screen.output.clone(),
            width: self.screen.width,
            height: self.screen.height,
            fps: self.screen.fps,
            quality: self.screen.quality,
            input_enabled: self.screen.input,
            sudo_password: self.screen.sudo.clone(),
        };
        let state = self.state.clone();
        self.toast("запускаю экран…", true);
        self.spawn(move || {
            let cfg = state.cfg();
            let r = state.monitor.start(&req, &cfg);
            if let Ok(st) = &r {
                let _ = state.update_cfg(|c| {
                    c.monitor.mode = st.mode;
                    c.monitor.backend = st.backend;
                    c.monitor.position = st.position;
                    c.monitor.output = st.output.clone();
                    c.monitor.width = st.width;
                    c.monitor.height = st.height;
                    c.monitor.fps = st.fps;
                    c.monitor.quality = st.quality;
                    c.monitor.input_enabled = st.input_enabled;
                });
            }
            TaskResult::Monitor(r)
        });
    }

    fn update_preview(&mut self, ctx: &egui::Context) {
        if !self.status.monitor.running {
            self.frame_tex = None;
            return;
        }
        if self.status.monitor.frames == self.last_frames {
            return;
        }
        self.last_frames = self.status.monitor.frames;
        let Some(bytes) = self.state.monitor.latest_frame() else {
            return;
        };
        let Ok(img) = image::load_from_memory(&bytes) else {
            return;
        };
        let size = [img.width() as usize, img.height() as usize];
        let rgba = img.to_rgba8();
        let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        match &mut self.frame_tex {
            Some(tex) => tex.set(color, egui::TextureOptions::LINEAR),
            None => {
                self.frame_tex =
                    Some(ctx.load_texture("screen-preview", color, egui::TextureOptions::LINEAR))
            }
        }
    }

    fn refresh_ha(&mut self) {
        if let Ok(mut ha) = self.state.ha.lock() {
            if ha.last_sync.elapsed() > Duration::from_secs(8) {
                let _ = ha.refresh();
            }
            self.ha_entities = ha.entities.clone();
            self.ha_categories = ha.categories.clone();
            self.ha_presets = ha.presets.clone();
        }
        self.last_ha_fetch = Instant::now();
    }

    /// Persist the category list to the shared config and to the running
    /// Home Assistant runtime, so the deck sees it on its next poll.
    fn save_ha_categories(&mut self) {
        let list = self.ha_categories.clone();
        let clean = self
            .state
            .ha
            .lock()
            .map(|mut h| h.set_categories(list))
            .unwrap_or_default();
        let saved = clean.clone();
        let _ = self.state.update_cfg(|c| c.ha.categories = saved);
        self.ha_categories = clean;
    }

    fn assign_category(&mut self, entity: &str, category: Option<&str>) {
        for c in &mut self.ha_categories {
            c.entities.retain(|x| x != entity);
        }
        if let Some(id) = category {
            if let Some(c) = self.ha_categories.iter_mut().find(|c| c.id == id) {
                c.entities.push(entity.to_string());
            }
        }
        self.save_ha_categories();
    }

    fn save_ha_presets(&mut self) {
        let list = self.ha_presets.clone();
        let clean = self
            .state
            .ha
            .lock()
            .map(|mut h| h.set_presets(list))
            .unwrap_or_default();
        let saved = clean.clone();
        let _ = self.state.update_cfg(|c| c.ha.presets = saved);
        self.ha_presets = clean;
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        // Closing the window puts the daemon in the background instead of
        // ending it: the Deck keeps its connection. A second press within a
        // second and a half, the tray or `--quit` end it for real.
        if ctx.input(|i| i.viewport().close_requested()) && !ALLOW_CLOSE.load(Ordering::Relaxed) {
            let again = self
                .close_at
                .is_some_and(|at| at.elapsed() < Duration::from_millis(1500));
            if again {
                ALLOW_CLOSE.store(true, Ordering::Relaxed);
                self.state.shutdown();
            } else {
                self.close_at = Some(Instant::now());
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                // Wayland has no "hide this toplevel" request, so the window is
                // minimized there; X11 and the rest really hide it.
                if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                if !self.hidden_notice {
                    self.hidden_notice = true;
                    self.state.logger.info(
                        "окно свёрнуто: демон работает в фоне; трей, --show или повторный \
                         запуск вернут окно, --quit остановит",
                    );
                }
            }
        }
        if self.start_hidden {
            // At login the daemon belongs in the tray, not over the desktop.
            // Wayland has no hide request, so minimize; elsewhere hide.
            self.start_hidden = false;
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
        self.poll();
        if self.dark != self.applied_dark {
            theme::apply(ctx, self.dark);
            self.applied_dark = self.dark;
        }
        let t = Tokens::get(ctx);
        self.update_preview(ctx);

        if self.status.monitor.running {
            ctx.request_repaint_after(Duration::from_millis(120));
        } else {
            ctx.request_repaint_after(Duration::from_millis(700));
        }

        egui::Panel::top("top")
            .exact_size(54.0)
            .frame(
                egui::Frame::new()
                    .fill(t.bg)
                    .stroke(Stroke::new(1.0, t.line))
                    .inner_margin(Margin::symmetric(16, 10)),
            )
            .show(root, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        RichText::new("sd-widgets")
                            .family(FontFamily::Monospace)
                            .size(18.0)
                            .strong()
                            .color(t.strong),
                    );
                    ui.add_space(6.0);
                    ui.label(hud("daemon", t.faint));
                    ui.add_space(14.0);
                    theme::dot(
                        ui,
                        if self.status.viiper.running {
                            t.good
                        } else {
                            t.warn
                        },
                        true,
                    );
                    ui.label(
                        RichText::new(if self.status.viiper.running {
                            t!(self.lang, "viiper работает", "viiper running")
                        } else {
                            t!(self.lang, "viiper выключен", "viiper off")
                        })
                        .size(11.5)
                        .color(t.muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(hud(
                                if self.dark { "light" } else { "dark" },
                                t.muted,
                            )))
                            .clicked()
                        {
                            self.set_dark(!self.dark);
                        }
                        if ui
                            .add(egui::Button::new(hud(self.lang.short(), t.muted)))
                            .on_hover_text(if self.lang == Lang::Ru {
                                "switch to english"
                            } else {
                                "переключить на русский"
                            })
                            .clicked()
                        {
                            self.set_lang(self.lang.toggle());
                        }
                        ui.add_space(8.0);
                        let addr = self.status.addresses.first().cloned().unwrap_or_default();
                        if !addr.is_empty() {
                            if ui
                                .add(egui::Button::new(
                                    RichText::new(&addr)
                                        .family(FontFamily::Monospace)
                                        .size(11.0)
                                        .color(t.faint),
                                ))
                                .on_hover_text(t!(self.lang, "скопировать адрес", "copy address"))
                                .clicked()
                            {
                                ui.ctx().copy_text(addr);
                                self.toast(
                                    t!(self.lang, "адрес скопирован", "address copied"),
                                    true,
                                );
                            }
                        }
                    });
                });
            });

        if !self.status.setup_done {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(t.bg).inner_margin(Margin::same(24)))
                .show(root, |ui| self.setup_ui(ui, &t));
        } else {
            egui::Panel::left("nav")
                .exact_size(214.0)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(t.bg)
                        .stroke(Stroke::new(1.0, t.line))
                        .inner_margin(Margin::symmetric(12, 12)),
                )
                .show(root, |ui| self.nav_ui(ui, &t));
            egui::Panel::bottom("statusbar")
                .exact_size(30.0)
                .frame(
                    egui::Frame::new()
                        .fill(t.bg)
                        .stroke(Stroke::new(1.0, t.line))
                        .inner_margin(Margin::symmetric(14, 6)),
                )
                .show(root, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(hud(
                            &format!(
                                "{} · {} · {}",
                                self.status.platform,
                                sdw_core::util::format_duration(self.status.uptime_secs),
                                VERSION
                            ),
                            t.faint,
                        ));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(hud(
                                &format!(
                                    "{} {}",
                                    self.status.pads.len(),
                                    t!(self.lang, "падов", "pads")
                                ),
                                t.faint,
                            ));
                            ui.add_space(10.0);
                            ui.label(hud(
                                &format!(
                                    "{} {}",
                                    self.status.connected_clients,
                                    t!(self.lang, "клиентов", "clients")
                                ),
                                t.faint,
                            ));
                        });
                    });
                });
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(t.bg)
                        .inner_margin(Margin::symmetric(22, 16)),
                )
                .show(root, |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.page_ui(ui, &t);
                            ui.add_space(30.0);
                        });
                });
        }

        if let Some(toast) = &self.toast {
            let color = if toast.ok { t.good } else { t.bad };
            egui::Area::new(egui::Id::new("toast"))
                .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -46.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(t.well)
                        .stroke(Stroke::new(1.0, color))
                        .inner_margin(Margin::symmetric(14, 8))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&toast.text).size(12.5).color(color));
                        });
                });
        }
    }
}

impl App {
    fn nav_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        ui.add_space(2.0);
        ui.label(hud("sd-widgets", t.faint));
        ui.label(
            RichText::new(sdw_core::util::truncate(&self.status.name, 24))
                .size(14.5)
                .strong()
                .color(t.strong),
        );
        ui.add_space(12.0);
        divider(ui);
        ui.add_space(6.0);
        for page in Page::ALL {
            let selected = self.page == page;
            let text = theme::nav_text(
                page.icon(),
                page.label(self.lang),
                13.0,
                if selected { t.strong } else { t.muted },
            );
            let resp = ui.add_sized(
                [ui.available_width(), 30.0],
                egui::Button::new(text)
                    .fill(if selected {
                        t.well
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(Stroke::new(
                        1.0,
                        if selected {
                            t.line_strong
                        } else {
                            Color32::TRANSPARENT
                        },
                    ))
                    .corner_radius(0),
            );
            if resp.clicked() {
                self.page = page;
                if page == Page::HomeAssistant {
                    self.last_ha_fetch = Instant::now() - Duration::from_secs(60);
                }
            }
            if page == Page::Gamepads && !self.status.pads.is_empty() {
                ui.painter().circle_filled(
                    egui::pos2(ui.max_rect().right() - 16.0, resp.rect.center().y),
                    3.0,
                    t.good,
                );
            }
            if page == Page::Notifications && self.status.notifications.unread > 0 {
                ui.painter().circle_filled(
                    egui::pos2(ui.max_rect().right() - 16.0, resp.rect.center().y),
                    3.0,
                    t.warn,
                );
            }
        }
        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.label(hud("mit · v0.1", t.faint));
            ui.add_space(6.0);
            if ui
                .add_sized(
                    [ui.available_width(), 28.0],
                    egui::Button::new(hud(t!(self.lang, "открыть папку", "open folder"), t.muted))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::new(1.0, t.line))
                        .corner_radius(0),
                )
                .clicked()
            {
                let _ =
                    sdw_core::util::open_url(&sdw_core::util::config_dir().display().to_string());
            }
            ui.add_space(4.0);
            if ui
                .add_sized(
                    [ui.available_width(), 28.0],
                    egui::Button::new(hud(t!(self.lang, "сайт проекта", "project page"), t.muted))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::new(1.0, t.line))
                        .corner_radius(0),
                )
                .clicked()
            {
                let _ = sdw_core::util::open_url("https://git.entitybtw.ru/entitybtw/sd-widgets");
            }
        });
    }

    fn page_header(&self, ui: &mut Ui, t: &Tokens, title: &str, subtitle: &str) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(title.to_lowercase())
                        .size(25.0)
                        .strong()
                        .color(t.strong),
                );
                if !subtitle.is_empty() {
                    ui.label(RichText::new(subtitle).size(12.5).color(t.muted));
                }
            });
        });
        ui.add_space(10.0);
    }

    fn page_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        match self.page {
            Page::Overview => self.overview_ui(ui, t),
            Page::Gamepads => self.gamepads_ui(ui, t),
            Page::Screen => self.screen_ui(ui, t),
            Page::Audio => self.audio_ui(ui, t),
            Page::Notifications => self.notifications_ui(ui, t),
            Page::HomeAssistant => self.ha_ui(ui, t),
            Page::Control => self.control_ui(ui, t),
            Page::Bluetooth => self.bluetooth_ui(ui, t),
            Page::Addons => self.addons_ui(ui, t),
            Page::Security => self.security_ui(ui, t),
            Page::Install => self.install_ui(ui, t),
            Page::Settings => self.settings_ui(ui, t),
            Page::Logs => self.logs_ui(ui, t),
        }
    }
}

// ---------------------------------------------------------------- setup

impl App {
    fn setup_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("sd-widgets")
                    .family(FontFamily::Monospace)
                    .size(22.0)
                    .strong()
                    .color(t.strong),
            );
            ui.label(hud(t!(self.lang, "первый запуск", "first run"), t.faint));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(hud(self.lang.short(), t.muted)))
                    .clicked()
                {
                    self.set_lang(self.lang.toggle());
                }
                if ui
                    .add(egui::Button::new(hud(
                        if self.dark { "light" } else { "dark" },
                        t.muted,
                    )))
                    .clicked()
                {
                    self.set_dark(!self.dark);
                }
            });
        });
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(10.0);

        // step indicator
        ui.horizontal(|ui| {
            for (i, name) in [
                t!(self.lang, "язык", "language"),
                t!(self.lang, "пароль", "password"),
                "viiper",
                t!(self.lang, "экран", "screen"),
                t!(self.lang, "готово", "done"),
            ]
            .iter()
            .enumerate()
            {
                let active = i == self.setup_step;
                let done = i < self.setup_step;
                let color = if active {
                    t.accent_ink
                } else if done {
                    t.good
                } else {
                    t.faint
                };
                ui.label(theme::nav_text(
                    if done { theme::icon::CHECK } else { "" },
                    name,
                    11.5,
                    color,
                ));
                if i < 4 {
                    ui.label(RichText::new("/").size(11.0).color(t.line_strong));
                }
            }
        });
        ui.add_space(16.0);

        match self.setup_step {
            0 => self.setup_welcome(ui, t),
            1 => self.setup_password(ui, t),
            2 => self.setup_viiper(ui, t),
            3 => self.setup_screen(ui, t),
            _ => self.setup_done(ui, t),
        }
    }

    fn setup_welcome(&mut self, ui: &mut Ui, t: &Tokens) {
        card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(self.lang, "компьютерная часть", "the computer side"))
                    .size(19.0)
                    .strong(),
            );
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    RichText::new(t!(
                        self.lang,
                        "sd-widgets превращает этот пк в приставку для steam deck: виртуальные геймпады, дополнительный экран, уведомления и home assistant. настройка займёт пару минут.",
                        "sd-widgets turns this pc into a companion for the steam deck: virtual gamepads, an extra screen, notifications and home assistant. setup takes a couple of minutes."
                    ))
                    .size(13.5)
                    .color(t.muted),
                )
                .wrap(),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(hud(t!(self.lang, "язык", "language"), t.faint));
                if ui
                    .selectable_label(self.lang == Lang::Ru, "русский")
                    .clicked()
                {
                    self.set_lang(Lang::Ru);
                }
                if ui
                    .selectable_label(self.lang == Lang::En, "english")
                    .clicked()
                {
                    self.set_lang(Lang::En);
                }
                ui.add_space(16.0);
                ui.label(hud(t!(self.lang, "тема", "theme"), t.faint));
                if ui
                    .selectable_label(self.dark, t!(self.lang, "тёмная", "dark"))
                    .clicked()
                {
                    self.set_dark(true);
                }
                if ui
                    .selectable_label(!self.dark, t!(self.lang, "светлая", "light"))
                    .clicked()
                {
                    self.set_dark(false);
                }
            });
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if primary(ui, t!(self.lang, "начать", "start"), t).clicked() {
                self.setup_step = 1;
            }
            small(
                ui,
                t!(
                    self.lang,
                    "всё можно изменить позже в настройках",
                    "everything can be changed later in settings"
                ),
                t.faint,
            );
        });
    }

    fn setup_password(&mut self, ui: &mut Ui, t: &Tokens) {
        card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(self.lang, "имя и пароль", "name and password"))
                    .size(18.0)
                    .strong(),
            );
            ui.add_space(4.0);
            small(
                ui,
                t!(
                    self.lang,
                    "этим паролем steam deck подключается к компьютеру. он хранится только в виде хеша.",
                    "the steam deck uses this password to pair with the computer. only a hash is stored."
                ),
                t.muted,
            );
            ui.add_space(10.0);
            ui.label(hud(
                t!(self.lang, "имя компьютера", "computer name"),
                t.faint,
            ));
            ui.add(TextEdit::singleline(&mut self.setup_name).desired_width(f32::INFINITY));
            ui.add_space(8.0);
            ui.label(hud(t!(self.lang, "пароль", "password"), t.faint));
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.setup_pass)
                        .password(!self.setup_show)
                        .desired_width(320.0),
                );
                if ui
                    .add(egui::Button::new(hud(
                        if self.setup_show {
                            t!(self.lang, "скрыть", "hide")
                        } else {
                            t!(self.lang, "показать", "show")
                        },
                        t.muted,
                    )))
                    .clicked()
                {
                    self.setup_show = !self.setup_show;
                }
            });
            ui.add_space(8.0);
            ui.label(hud(t!(self.lang, "ещё раз", "repeat"), t.faint));
            ui.add(
                TextEdit::singleline(&mut self.setup_pass2)
                    .password(!self.setup_show)
                    .desired_width(f32::INFINITY),
            );
            if !self.setup_pass.is_empty() && self.setup_pass.len() < 4 {
                small(
                    ui,
                    t!(self.lang, "минимум 4 символа", "at least 4 characters"),
                    t.bad,
                );
            }
            if !self.setup_pass2.is_empty() && self.setup_pass != self.setup_pass2 {
                small(
                    ui,
                    t!(self.lang, "пароли не совпадают", "passwords do not match"),
                    t.bad,
                );
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ghost(ui, t!(self.lang, "назад", "back"), t).clicked() {
                self.setup_step = 0;
            }
            let valid = self.setup_pass.chars().count() >= 4 && self.setup_pass == self.setup_pass2;
            if ui
                .add_enabled(
                    valid,
                    egui::Button::new(
                        RichText::new(t!(self.lang, "далее", "next"))
                            .size(13.0)
                            .strong()
                            .color(t.inv_fg),
                    )
                    .fill(t.inv_bg)
                    .stroke(Stroke::new(1.0, t.inv_bg))
                    .corner_radius(0)
                    .min_size(Vec2::new(0.0, 30.0)),
                )
                .clicked()
            {
                let name = self.setup_name.trim().to_string();
                let pass = self.setup_pass.clone();
                self.save(|c| {
                    if !name.is_empty() {
                        c.name = name.clone();
                    }
                    c.set_password(&pass);
                });
                self.status = self.state.status();
                self.setup_step = 2;
            }
        });
    }

    fn setup_viiper(&mut self, ui: &mut Ui, t: &Tokens) {
        let v = self.status.viiper.clone();
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("viiper").size(18.0).strong());
                tag(
                    ui,
                    if v.running {
                        t!(self.lang, "работает", "running")
                    } else if v.installed {
                        t!(self.lang, "установлен", "installed")
                    } else {
                        t!(self.lang, "не установлен", "not installed")
                    },
                    if v.running {
                        t.good
                    } else if v.installed {
                        t.warn
                    } else {
                        t.bad
                    },
                );
            });
            ui.add_space(4.0);
            small(
                ui,
                t!(
                    self.lang,
                    "viiper создаёт usb-геймпады, которые видит система и игры. это отдельный сервер, sd-widgets запускает его сам.",
                    "viiper creates usb gamepads that the system and games can see. sd-widgets starts it automatically."
                ),
                t.muted,
            );
            ui.add_space(10.0);
            if !v.installed {
                if primary(ui, t!(self.lang, "установить viiper", "install viiper"), t).clicked()
                {
                    self.install_sel.insert("viiper".into());
                    self.page = Page::Install;
                }
            } else if !v.running {
                if primary(ui, t!(self.lang, "запустить", "start"), t).clicked() {
                    self.start_viiper(false);
                }
            } else {
                small(ui, &format!("{} · {}", v.version, v.api_addr), t.good);
            }
            ui.add_space(8.0);
            ui.label(hud(
                t!(
                    self.lang,
                    "пароль viiper (необязательно)",
                    "viiper password (optional)"
                ),
                t.faint,
            ));
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.setup_viiper_pass)
                        .password(!self.viiper_show)
                        .hint_text(t!(
                            self.lang,
                            "оставьте пустым, если без пароля",
                            "leave empty for no password"
                        ))
                        .desired_width(320.0),
                );
                if ui
                    .add(egui::Button::new(hud(
                        if self.viiper_show {
                            t!(self.lang, "скрыть", "hide")
                        } else {
                            t!(self.lang, "показать", "show")
                        },
                        t.muted,
                    )))
                    .clicked()
                {
                    self.viiper_show = !self.viiper_show;
                }
                if ui
                    .add(egui::Button::new(hud(
                        t!(self.lang, "сохранить", "save"),
                        t.muted,
                    )))
                    .clicked()
                {
                    let pass = self.setup_viiper_pass.trim().to_string();
                    let _ = self.state.viiper.write_key(&pass);
                    self.save(|c| c.viiper.password = pass);
                    self.toast(
                        t!(self.lang, "пароль viiper сохранён", "viiper password saved"),
                        true,
                    );
                }
            });
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ghost(ui, t!(self.lang, "назад", "back"), t).clicked() {
                self.setup_step = 1;
            }
            if primary(ui, t!(self.lang, "далее", "next"), t).clicked() {
                self.setup_step = 3;
            }
            if ui
                .add(egui::Button::new(hud(
                    t!(self.lang, "пропустить", "skip"),
                    t.faint,
                )))
                .clicked()
            {
                self.setup_step = 3;
            }
        });
    }

    fn setup_screen(&mut self, ui: &mut Ui, t: &Tokens) {
        card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(self.lang, "дополнительный экран", "extra screen"))
                    .size(18.0)
                    .strong(),
            );
            ui.add_space(4.0);
            small(
                ui,
                t!(
                    self.lang,
                    "по умолчанию — 1280×800, как у steam deck. экран можно не включать сейчас: это займёт один клик позже.",
                    "default is 1280×800, the steam deck size. you can skip this and start it later with one click."
                ),
                t.muted,
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.setup_screen_mode,
                    MonitorMode::Extend,
                    t!(self.lang, "дополнительный монитор", "extra monitor"),
                );
                ui.selectable_value(
                    &mut self.setup_screen_mode,
                    MonitorMode::Mirror,
                    t!(self.lang, "зеркало экрана", "screen mirror"),
                );
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (i, (w, h, label)) in [
                    (1280u32, 800u32, "steam deck 1280×800"),
                    (1920, 1080, "1920×1080"),
                    (2560, 1440, "2560×1440"),
                ]
                .iter()
                .enumerate()
                {
                    if ui
                        .selectable_label(self.setup_preset == i, *label)
                        .clicked()
                    {
                        self.setup_preset = i;
                        self.screen.width = *w;
                        self.screen.height = *h;
                    }
                }
            });
            ui.add_space(8.0);
            ui.checkbox(
                &mut self.setup_start_screen,
                t!(
                    self.lang,
                    "запустить экран после настройки",
                    "start the screen after setup"
                ),
            );
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ghost(ui, t!(self.lang, "назад", "back"), t).clicked() {
                self.setup_step = 2;
            }
            if primary(ui, t!(self.lang, "далее", "next"), t).clicked() {
                self.setup_step = 4;
            }
        });
    }

    fn setup_done(&mut self, ui: &mut Ui, t: &Tokens) {
        card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(self.lang, "всё готово", "all set"))
                    .size(20.0)
                    .strong(),
            );
            ui.add_space(4.0);
            small(
                ui,
                t!(
                    self.lang,
                    "откройте sd-widgets на steam deck, нажмите «добавить компьютер» и выберите этот пк. пароль — тот, что вы только что задали.",
                    "open sd-widgets on the steam deck, press “add computer” and pick this pc. use the password you just set."
                ),
                t.muted,
            );
            ui.add_space(10.0);
            for addr in &self.status.addresses {
                ui.horizontal(|ui| {
                    mono(ui, addr, t.accent_ink);
                    theme::copy_button(ui, addr, t);
                });
            }
        });
        ui.add_space(12.0);
        if primary(ui, t!(self.lang, "открыть панель", "open the panel"), t).clicked()
        {
            self.save(|c| c.setup_done = true);
            self.status = self.state.status();
            if self.setup_start_screen {
                self.screen.mode = self.setup_screen_mode;
                self.screen.width = self.screen.width.max(1280);
                self.screen.height = self.screen.height.max(800);
                self.screen.sudo = self.setup_sudo.clone();
                self.start_screen();
            }
            if self.status.viiper.installed && !self.status.viiper.running {
                self.start_viiper(false);
            }
            self.page = Page::Overview;
        }
    }
}

// ---------------------------------------------------------------- overview

impl App {
    fn overview_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        let s = self.status.clone();
        self.page_header(
            ui,
            t,
            t!(self.lang, "обзор", "overview"),
            t!(
                self.lang,
                "состояние компьютера и быстрые действия",
                "computer state and quick actions"
            ),
        );

        if let Some(info) = self.update_info.clone().filter(|i| i.newer) {
            card(ui, t, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::icon(theme::icon::CHECK, 18.0, t.good));
                    ui.label(
                        RichText::new(format!(
                            "{} {} → {}",
                            t!(self.lang, "доступно обновление", "update available"),
                            info.current,
                            info.latest
                        ))
                        .size(13.5)
                        .strong()
                        .color(t.strong),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ghost(ui, t!(self.lang, "настройки", "settings"), t).clicked() {
                            self.page = Page::Settings;
                        }
                        small(
                            ui,
                            t!(
                                self.lang,
                                "там можно скачать и установить",
                                "download and install there"
                            ),
                            t.faint,
                        );
                    });
                });
            });
            ui.add_space(10.0);
        }

        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&s.name).size(19.0).strong());
                    small(
                        ui,
                        &format!("{} · {} · {}", s.hostname, s.platform, s.version),
                        t.muted,
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    tag(
                        ui,
                        if s.setup_done {
                            t!(self.lang, "настроен", "configured")
                        } else {
                            t!(self.lang, "настройка", "setup")
                        },
                        if s.setup_done { t.good } else { t.warn },
                    );
                    tag(
                        ui,
                        &format!(
                            "{} {}",
                            s.connected_clients,
                            t!(self.lang, "клиентов", "clients")
                        ),
                        t.muted,
                    );
                });
            });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                for addr in &s.addresses {
                    theme::copy_button(ui, addr, t);
                    mono(ui, addr, t.accent_ink);
                    ui.add_space(8.0);
                }
            });
        });
        ui.add_space(10.0);

        let mut go: Option<Page> = None;
        egui::Grid::new("overview-grid")
            .num_columns(2)
            .spacing([12.0, 12.0])
            .show(ui, |ui| {
                // viiper
                card(ui, t, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("viiper").size(15.0).strong());
                        tag(
                            ui,
                            if s.viiper.running {
                                t!(self.lang, "работает", "running")
                            } else {
                                t!(self.lang, "выключен", "off")
                            },
                            if s.viiper.running { t.good } else { t.bad },
                        );
                    });
                    small(
                        ui,
                        &if s.viiper.installed {
                            format!("{} · {}", s.viiper.version, s.viiper.api_addr)
                        } else {
                            t!(self.lang, "не установлен", "not installed").to_string()
                        },
                        t.muted,
                    );
                    ui.add_space(6.0);
                    small(
                        ui,
                        &format!(
                            "{} {} · {} {}",
                            s.pads.len(),
                            t!(self.lang, "виртуальных падов", "virtual pads"),
                            s.viiper.usb_addr,
                            t!(self.lang, "usb", "usb")
                        ),
                        t.faint,
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if s.viiper.running {
                            if ghost(ui, t!(self.lang, "стоп", "stop"), t).clicked() {
                                self.stop_viiper();
                            }
                            if ghost(ui, t!(self.lang, "перезапуск", "restart"), t).clicked()
                            {
                                self.start_viiper(true);
                            }
                        } else if s.viiper.installed {
                            if primary(ui, t!(self.lang, "запустить", "start"), t).clicked()
                            {
                                self.start_viiper(false);
                            }
                        } else if primary(ui, t!(self.lang, "установить", "install"), t).clicked()
                        {
                            go = Some(Page::Install);
                        }
                        if ui
                            .add(egui::Button::new(hud(
                                t!(self.lang, "подробнее", "details"),
                                t.faint,
                            )))
                            .clicked()
                        {
                            go = Some(Page::Gamepads);
                        }
                    });
                });

                // screen
                card(ui, t, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(t!(self.lang, "экран", "screen"))
                                .size(15.0)
                                .strong(),
                        );
                        tag(
                            ui,
                            if s.monitor.running {
                                t!(self.lang, "в эфире", "live")
                            } else {
                                t!(self.lang, "выключен", "off")
                            },
                            if s.monitor.running { t.good } else { t.muted },
                        );
                    });
                    small(
                        ui,
                        &if s.monitor.running {
                            format!(
                                "{} · {}×{} · {}",
                                s.monitor.backend_label,
                                s.monitor.width,
                                s.monitor.height,
                                s.monitor.output
                            )
                        } else {
                            t!(
                                self.lang,
                                "дополнительный монитор или зеркало",
                                "extra monitor or mirror"
                            )
                            .to_string()
                        },
                        t.muted,
                    );
                    ui.add_space(6.0);
                    if s.monitor.running {
                        small(
                            ui,
                            &format!(
                                "{} {} · {} {}",
                                s.monitor.frames,
                                t!(self.lang, "кадров", "frames"),
                                s.monitor.frame_age_ms,
                                t!(self.lang, "мс", "ms")
                            ),
                            t.faint,
                        );
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if s.monitor.running {
                            if ghost(ui, t!(self.lang, "остановить", "stop"), t).clicked()
                            {
                                self.state.monitor.stop();
                            }
                        } else if primary(ui, t!(self.lang, "настроить", "set up"), t).clicked()
                        {
                            go = Some(Page::Screen);
                        }
                        if ui
                            .add(egui::Button::new(hud(
                                t!(self.lang, "открыть", "open"),
                                t.faint,
                            )))
                            .clicked()
                        {
                            go = Some(Page::Screen);
                        }
                    });
                });
                ui.end_row();

                // notifications
                card(ui, t, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(t!(self.lang, "уведомления", "notifications"))
                                .size(15.0)
                                .strong(),
                        );
                        tag(
                            ui,
                            &format!("{}", s.notifications.count),
                            if s.notifications.listening {
                                t.good
                            } else {
                                t.warn
                            },
                        );
                    });
                    small(
                        ui,
                        if s.notifications.listening {
                            t!(self.lang, "зеркалирование включено", "mirroring is on")
                        } else {
                            t!(
                                self.lang,
                                "ожидает демон уведомлений",
                                "waiting for a notification daemon"
                            )
                        },
                        t.muted,
                    );
                    ui.add_space(6.0);
                    for n in s.notifications.recent.iter().take(2) {
                        small(
                            ui,
                            &format!("{} · {}", n.app, sdw_core::util::truncate(&n.summary, 44)),
                            t.ink,
                        );
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ghost(ui, t!(self.lang, "проверить", "test"), t).clicked() {
                            self.state.notifications.add_test();
                        }
                        if ui
                            .add(egui::Button::new(hud(t!(self.lang, "все", "all"), t.faint)))
                            .clicked()
                        {
                            go = Some(Page::Notifications);
                        }
                    });
                });

                // ha
                card(ui, t, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("home assistant").size(15.0).strong());
                        tag(
                            ui,
                            if s.ha.connected {
                                t!(self.lang, "онлайн", "online")
                            } else if s.ha.configured {
                                t!(self.lang, "ошибка", "error")
                            } else {
                                t!(self.lang, "не настроен", "not set")
                            },
                            if s.ha.connected {
                                t.good
                            } else if s.ha.configured {
                                t.bad
                            } else {
                                t.muted
                            },
                        );
                    });
                    small(
                        ui,
                        &if s.ha.configured {
                            format!(
                                "{} · {} {}",
                                s.ha.url,
                                s.ha.entities,
                                t!(self.lang, "сущностей", "entities")
                            )
                        } else {
                            t!(
                                self.lang,
                                "свет, сцены и датчики на деке",
                                "lights, scenes and sensors on the deck"
                            )
                            .to_string()
                        },
                        t.muted,
                    );
                    ui.add_space(8.0);
                    if primary(ui, t!(self.lang, "открыть", "open"), t).clicked() {
                        go = Some(Page::HomeAssistant);
                    }
                });
                ui.end_row();
            });

        ui.add_space(10.0);
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!(self.lang, "подключённые клиенты", "paired clients"))
                        .size(15.0)
                        .strong(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(hud(
                            t!(self.lang, "безопасность", "security"),
                            t.faint,
                        )))
                        .clicked()
                    {
                        go = Some(Page::Security);
                    }
                });
            });
            let cfg = self.state.cfg();
            if cfg.clients.is_empty() {
                small(
                    ui,
                    t!(
                        self.lang,
                        "пока никого. откройте sd-widgets на steam deck и добавьте этот пк.",
                        "nobody yet. open sd-widgets on the steam deck and add this pc."
                    ),
                    t.muted,
                );
            } else {
                for c in &cfg.clients {
                    ui.horizontal(|ui| {
                        theme::dot(ui, t.good, true);
                        ui.label(RichText::new(&c.name).size(13.0).color(t.ink));
                        small(ui, &format!("{} · {}", c.id, ago(c.last_seen_ms)), t.faint);
                    });
                }
            }
        });

        if let Some(page) = go {
            self.page = page;
        }
    }
}

// ---------------------------------------------------------------- gamepads

impl App {
    fn gamepads_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        let s = self.status.clone();
        self.page_header(
            ui,
            t,
            t!(self.lang, "геймпады", "gamepads"),
            t!(
                self.lang,
                "виртуальные usb-пады через viiper",
                "virtual usb pads through viiper"
            ),
        );

        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("viiper").size(16.0).strong());
                tag(
                    ui,
                    if s.viiper.running { "running" } else { "off" },
                    if s.viiper.running { t.good } else { t.bad },
                );
                tag(
                    ui,
                    if s.viiper.password_set {
                        t!(self.lang, "ключ есть", "key found")
                    } else {
                        t!(self.lang, "без пароля", "no key")
                    },
                    if s.viiper.password_set {
                        t.good
                    } else {
                        t.muted
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if s.viiper.running {
                        if ghost(ui, "restart", t).clicked() {
                            self.start_viiper(true);
                        }
                        if ghost(ui, "stop", t).clicked() {
                            self.stop_viiper();
                        }
                    } else if s.viiper.installed && primary(ui, "start", t).clicked() {
                        self.start_viiper(false);
                    }
                });
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                mono(ui, &format!("api {}", s.viiper.api_addr), t.muted);
                ui.add_space(10.0);
                mono(ui, &format!("usb {}", s.viiper.usb_addr), t.muted);
                ui.add_space(10.0);
                mono(
                    ui,
                    &format!(
                        "usbip {}",
                        if s.viiper.usbip_installed {
                            "ok"
                        } else {
                            "missing"
                        }
                    ),
                    if s.viiper.usbip_installed {
                        t.good
                    } else {
                        t.bad
                    },
                );
                if !s.viiper.version.is_empty() {
                    ui.add_space(10.0);
                    mono(ui, &s.viiper.version, t.faint);
                }
            });
            if !s.viiper.error.is_empty() {
                small(ui, &s.viiper.error, t.bad);
            }
            ui.add_space(8.0);
            ui.label(hud(
                t!(self.lang, "пароль viiper", "viiper password"),
                t.faint,
            ));
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.viiper_pass)
                        .password(!self.viiper_show)
                        .desired_width(280.0),
                );
                if ui
                    .add(egui::Button::new(hud(
                        if self.viiper_show { "hide" } else { "show" },
                        t.muted,
                    )))
                    .clicked()
                {
                    self.viiper_show = !self.viiper_show;
                }
                if ui
                    .add(egui::Button::new(hud(
                        t!(self.lang, "сохранить", "save"),
                        t.muted,
                    )))
                    .clicked()
                {
                    let pass = self.viiper_pass.trim().to_string();
                    let _ = self.state.viiper.write_key(&pass);
                    self.save(|c| c.viiper.password = pass);
                    self.toast(
                        t!(self.lang, "пароль viiper сохранён", "viiper password saved"),
                        true,
                    );
                }
                small(
                    ui,
                    &format!(
                        "{} · {}",
                        s.viiper.key_path,
                        if s.viiper.password_set {
                            t!(self.lang, "ключ найден", "key found")
                        } else {
                            t!(
                                self.lang,
                                "локальный доступ без пароля — это нормально",
                                "localhost works without a key — that is fine"
                            )
                        }
                    ),
                    t.faint,
                );
            });
        });
        ui.add_space(10.0);

        card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(self.lang, "новый пад", "new pad"))
                    .size(15.0)
                    .strong(),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.pad_name)
                        .hint_text(t!(self.lang, "имя (необязательно)", "name (optional)"))
                        .desired_width(260.0),
                );
                egui::ComboBox::from_id_salt("pad-kind")
                    .selected_text(self.pad_kind.label())
                    .show_ui(ui, |ui| {
                        for k in PadKind::ALL {
                            ui.selectable_value(&mut self.pad_kind, k, k.label());
                        }
                    });
                if primary(ui, t!(self.lang, "создать", "create"), t).clicked() {
                    let name = if self.pad_name.trim().is_empty() {
                        format!("steam deck {}", s.pads.len() + 1)
                    } else {
                        self.pad_name.trim().to_string()
                    };
                    let kind = self.pad_kind;
                    let state = self.state.clone();
                    self.toast(t!(self.lang, "создаю пад…", "creating pad…"), true);
                    self.spawn(move || {
                        TaskResult::Pad(state.viiper.create_pad(&state.cfg(), &name, kind))
                    });
                }
            });
            small(ui, pad_hint(self.lang, self.pad_kind), t.faint);
        });

        if let Some(p) = self.created_pad.clone() {
            ui.add_space(10.0);
            card(ui, t, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(t!(self.lang, "пад создан", "pad created"))
                            .size(15.0)
                            .strong()
                            .color(t.good),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(theme::icon(
                                theme::icon::CLOSE,
                                13.0,
                                t.muted,
                            )))
                            .clicked()
                        {
                            self.created_pad = None;
                        }
                    });
                });
                mono(
                    ui,
                    &format!(
                        "{} · {} · bus {} · {}",
                        p.pad.name,
                        p.pad.kind.label(),
                        p.pad.bus_id,
                        p.pad.dev_id
                    ),
                    t.ink,
                );
                ui.add_space(4.0);
                small(
                    ui,
                    t!(
                        self.lang,
                        "секрет udp показывается один раз. steam deck получает его автоматически при создании пада.",
                        "the udp secret is shown once. the steam deck receives it automatically when it creates a pad."
                    ),
                    t.muted,
                );
                ui.horizontal(|ui| {
                    let secret = if self.show_pad_secret {
                        p.udp_secret.clone()
                    } else {
                        "••••••••••••".into()
                    };
                    mono(ui, &secret, t.accent_ink);
                    if ui
                        .add(egui::Button::new(hud(
                            if self.show_pad_secret { "hide" } else { "show" },
                            t.muted,
                        )))
                        .clicked()
                    {
                        self.show_pad_secret = !self.show_pad_secret;
                    }
                    theme::copy_button(ui, &p.udp_secret, t);
                });
            });
        }

        ui.add_space(10.0);
        if s.pads.is_empty() {
            card(ui, t, |ui| {
                small(
                    ui,
                    t!(
                        self.lang,
                        "падов пока нет. steam deck создаёт их сам, когда вы включаете «виртуальный геймпад».",
                        "no pads yet. the steam deck creates them when you enable “virtual gamepad”."
                    ),
                    t.muted,
                );
            });
        } else {
            for pad in &s.pads {
                let pad = pad.clone();
                ui.add_space(8.0);
                card(ui, t, |ui| {
                    ui.horizontal(|ui| {
                        theme::dot(ui, if pad.connected { t.good } else { t.faint }, true);
                        ui.label(RichText::new(&pad.name).size(14.5).strong());
                        tag(ui, pad.kind.label(), t.muted);
                        if pad.connected {
                            tag(ui, t!(self.lang, "онлайн", "online"), t.good);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(egui::Button::new(hud(
                                    t!(self.lang, "удалить", "remove"),
                                    t.bad,
                                )))
                                .clicked()
                            {
                                let id = pad.id.clone();
                                let state = self.state.clone();
                                self.spawn(move || {
                                    TaskResult::Msg(
                                        state.viiper.delete_pad(&id).map(|_| "пад удалён".into()),
                                    )
                                });
                            }
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        mono(ui, &format!("id {}", pad.id), t.faint);
                        ui.add_space(8.0);
                        mono(ui, &format!("bus {} · {}", pad.bus_id, pad.dev_id), t.faint);
                        ui.add_space(8.0);
                        mono(
                            ui,
                            &format!(
                                "{} {}",
                                t!(self.lang, "ввод", "input"),
                                ago(pad.last_input_ms)
                            ),
                            t.faint,
                        );
                    });
                    if pad.rumble != [0, 0] {
                        ui.horizontal(|ui| {
                            ui.label(hud("rumble", t.faint));
                            for v in pad.rumble {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(60.0, 8.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(rect, 0.0, t.well);
                                let w = rect.width() * (v as f32 / 255.0);
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_size(
                                        rect.min,
                                        egui::vec2(w, rect.height()),
                                    ),
                                    0.0,
                                    t.accent,
                                );
                            }
                        });
                    }
                    if !pad.error.is_empty() {
                        small(ui, &pad.error, t.bad);
                    }
                });
            }
        }
    }
}

// ---------------------------------------------------------------- screen

impl App {
    fn screen_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        let s = self.status.clone();
        if !self.screen_loaded {
            self.screen.mode = s.monitor.mode;
            self.screen.backend = s.monitor.backend;
            self.screen.position = s.monitor.position;
            self.screen.output = s.monitor.output.clone();
            self.screen.width = s.monitor.width;
            self.screen.height = s.monitor.height;
            self.screen.fps = s.monitor.fps;
            self.screen.quality = s.monitor.quality;
            self.screen.input = s.monitor.input_enabled;
            self.screen_loaded = true;
        }
        self.page_header(
            ui,
            t,
            t!(self.lang, "экран", "screen"),
            t!(
                self.lang,
                "дополнительный монитор или зеркало, 1280×800 по умолчанию",
                "extra monitor or mirror, 1280×800 by default"
            ),
        );

        if s.monitor.running {
            card(ui, t, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(t!(self.lang, "в эфире", "live"))
                            .size(16.0)
                            .strong()
                            .color(t.good),
                    );
                    tag(ui, &s.monitor.backend_label, t.muted);
                    tag(ui, &s.monitor.output, t.muted);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ghost(ui, t!(self.lang, "остановить", "stop"), t).clicked() {
                            self.state.monitor.stop();
                            self.toast(t!(self.lang, "экран остановлен", "screen stopped"), true);
                        }
                    });
                });
                small(
                    ui,
                    &format!(
                        "{}×{} · {} fps · jpeg {} · {} {} · {} {}",
                        s.monitor.width,
                        s.monitor.height,
                        s.monitor.fps,
                        s.monitor.quality,
                        s.monitor.frames,
                        t!(self.lang, "кадров", "frames"),
                        s.monitor.frame_age_ms,
                        t!(self.lang, "мс", "ms")
                    ),
                    t.muted,
                );
                if !s.monitor.last_error.is_empty() {
                    small(ui, &s.monitor.last_error, t.bad);
                }
                if !s.monitor.note.is_empty() {
                    small(ui, &s.monitor.note, t.warn);
                }
                ui.add_space(8.0);
                if let Some(tex) = &self.frame_tex {
                    let avail = ui.available_width().min(720.0);
                    let aspect = s.monitor.width as f32 / s.monitor.height.max(1) as f32;
                    let size = egui::vec2(avail, avail / aspect.max(0.2));
                    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                    ui.painter().rect_filled(rect, 0.0, t.well);
                    ui.painter().image(
                        tex.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    ui.painter()
                        .rect_stroke(rect, 0.0, t.hairline(), egui::StrokeKind::Inside);
                    if s.monitor.input_enabled && resp.clicked() {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            let x = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                            let y = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
                            let state = self.state.clone();
                            let input = MonitorInput {
                                x,
                                y,
                                action: "click".into(),
                                amount: 0,
                            };
                            self.spawn(move || {
                                TaskResult::Msg(
                                    state.monitor.input(&input).map(|_| "клик отправлен".into()),
                                )
                            });
                        }
                    }
                    small(
                        ui,
                        if s.monitor.input_enabled {
                            t!(
                                self.lang,
                                "клик по превью = клик на компьютере",
                                "click the preview = click on the computer"
                            )
                        } else {
                            t!(
                                self.lang,
                                "управление мышью выключено",
                                "pointer control is off"
                            )
                        },
                        t.faint,
                    );
                } else {
                    small(
                        ui,
                        t!(
                            self.lang,
                            "ждём первый кадр…",
                            "waiting for the first frame…"
                        ),
                        t.muted,
                    );
                }
            });
            ui.add_space(10.0);
        }

        // questionnaire
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!(self.lang, "мастер", "wizard"))
                        .size(16.0)
                        .strong(),
                );
                for i in 0..4 {
                    let active = self.screen.step == i;
                    ui.label(
                        RichText::new(format!("{}", i + 1))
                            .family(FontFamily::Monospace)
                            .size(11.0)
                            .color(if active { t.accent_ink } else { t.faint }),
                    );
                }
            });
            divider(ui);
            match self.screen.step {
                0 => {
                    small(
                        ui,
                        t!(
                            self.lang,
                            "что нужно сделать?",
                            "what should the extra screen do?"
                        ),
                        t.muted,
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        for (mode, title, hint) in [
                            (
                                MonitorMode::Extend,
                                t!(self.lang, "дополнительный монитор", "extra monitor"),
                                t!(
                                    self.lang,
                                    "второй рабочий стол справа от основного",
                                    "a second desktop to the right"
                                ),
                            ),
                            (
                                MonitorMode::Mirror,
                                t!(self.lang, "зеркало", "mirror"),
                                t!(
                                    self.lang,
                                    "копия основного экрана",
                                    "a copy of the main screen"
                                ),
                            ),
                        ] {
                            let selected = self.screen.mode == mode;
                            egui::Frame::new()
                                .fill(if selected { t.well } else { t.bg })
                                .stroke(Stroke::new(
                                    1.0,
                                    if selected { t.line_strong } else { t.line },
                                ))
                                .inner_margin(Margin::same(12))
                                .show(ui, |ui| {
                                    ui.set_width(260.0);
                                    ui.vertical(|ui| {
                                        if ui
                                            .selectable_label(
                                                selected,
                                                RichText::new(title).size(14.0).strong(),
                                            )
                                            .clicked()
                                        {
                                            self.screen.mode = mode;
                                        }
                                        small(ui, hint, t.muted);
                                    });
                                });
                        }
                    });
                }
                1 => {
                    small(ui, t!(self.lang, "размер", "size"), t.muted);
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        for (i, (w, h, label)) in [
                            (1280u32, 800u32, "steam deck · 1280×800"),
                            (1920, 1080, "full hd · 1920×1080"),
                            (2560, 1440, "qhd · 2560×1440"),
                            (3840, 2160, "4k · 3840×2160"),
                        ]
                        .iter()
                        .enumerate()
                        {
                            if ui
                                .selectable_label(self.screen.preset == i, *label)
                                .clicked()
                            {
                                self.screen.preset = i;
                                self.screen.width = *w;
                                self.screen.height = *h;
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(hud("w", t.faint));
                        ui.add(egui::DragValue::new(&mut self.screen.width).range(320..=3840));
                        ui.label(hud("h", t.faint));
                        ui.add(egui::DragValue::new(&mut self.screen.height).range(240..=2400));
                        if self.screen.width == 1280 && self.screen.height == 800 {
                            tag(ui, t!(self.lang, "рекомендуется", "recommended"), t.good);
                        }
                    });
                }
                2 => {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(hud(t!(self.lang, "механизм", "mechanism"), t.faint));
                            egui::ComboBox::from_id_salt("backend")
                                .selected_text(backend_label(self.lang, self.screen.backend))
                                .show_ui(ui, |ui| {
                                    for b in [
                                        MonitorBackend::Auto,
                                        MonitorBackend::Hyprland,
                                        MonitorBackend::Vkms,
                                        MonitorBackend::Krfb,
                                        MonitorBackend::Mirror,
                                        MonitorBackend::X11,
                                        MonitorBackend::Gdigrab,
                                    ] {
                                        ui.selectable_value(
                                            &mut self.screen.backend,
                                            b,
                                            backend_label(self.lang, b),
                                        );
                                    }
                                });
                        });
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            ui.label(hud(t!(self.lang, "выход", "output"), t.faint));
                            egui::ComboBox::from_id_salt("output")
                                .selected_text(if self.screen.output.is_empty() {
                                    t!(self.lang, "авто", "auto").to_string()
                                } else {
                                    self.screen.output.clone()
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.screen.output,
                                        String::new(),
                                        t!(self.lang, "авто", "auto"),
                                    );
                                    for o in &s.monitor.outputs {
                                        ui.selectable_value(
                                            &mut self.screen.output,
                                            o.name.clone(),
                                            format!("{} · {}×{}", o.name, o.width, o.height),
                                        );
                                    }
                                });
                        });
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            ui.label(hud(t!(self.lang, "позиция", "position"), t.faint));
                            let enabled = self.screen.mode == MonitorMode::Extend;
                            ui.add_enabled_ui(enabled, |ui| {
                                egui::ComboBox::from_id_salt("position")
                                    .selected_text(self.screen.position.label_lang(self.lang))
                                    .width(150.0)
                                    .show_ui(ui, |ui| {
                                        for p in MonitorPosition::ALL {
                                            ui.selectable_value(
                                                &mut self.screen.position,
                                                p,
                                                p.label_lang(self.lang),
                                            );
                                        }
                                    });
                            });
                        });
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(hud("fps", t.faint));
                        ui.add(egui::Slider::new(&mut self.screen.fps, 5..=30).show_value(true));
                        ui.add_space(14.0);
                        ui.label(hud(t!(self.lang, "качество", "quality"), t.faint));
                        ui.add(
                            egui::Slider::new(&mut self.screen.quality, 30..=95).show_value(true),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(
                            &mut self.screen.input,
                            t!(
                                self.lang,
                                "управление мышью с дека",
                                "pointer control from the deck"
                            ),
                        );
                    });
                    // VKMS is the part that makes niri, sway and a virtual
                    // machine work, so show its readiness and prepare it in
                    // one press instead of leaving the user with a snippet.
                    let wants_vkms = matches!(
                        self.screen.backend,
                        MonitorBackend::Vkms | MonitorBackend::Auto
                    ) && self.screen.mode == MonitorMode::Extend
                        && cfg!(target_os = "linux");
                    if wants_vkms {
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(hud(t!(self.lang, "готовность", "readiness"), t.faint));
                            let checks = [
                                (
                                    t!(self.lang, "vkms в ядре", "vkms in the kernel"),
                                    s.monitor.vkms,
                                ),
                                (
                                    t!(self.lang, "vkms загружен", "vkms loaded"),
                                    s.monitor.vkms_loaded,
                                ),
                                (
                                    t!(self.lang, "выход найден", "output found"),
                                    !s.monitor.output.is_empty(),
                                ),
                                (
                                    t!(self.lang, "конфиг", "config"),
                                    !s.monitor.config_file.is_empty(),
                                ),
                            ];
                            for (label, ok) in checks {
                                tag(ui, label, if ok { t.good } else { t.warn });
                            }
                            if !s.monitor.vm.is_empty() {
                                tag(ui, &s.monitor.vm, t.muted);
                            }
                        });
                        small(
                            ui,
                            t!(
                                self.lang,
                                "одна кнопка загрузит vkms и оставит его включённым после перезагрузки — вручную ничего писать не нужно",
                                "one button loads vkms and keeps it enabled after a reboot — no manual files to write"
                            ),
                            t.faint,
                        );
                        ui.horizontal(|ui| {
                            ui.add(
                                TextEdit::singleline(&mut self.screen.sudo)
                                    .password(true)
                                    .hint_text(t!(
                                        self.lang,
                                        "пароль sudo, если спросят",
                                        "sudo password, if asked"
                                    ))
                                    .desired_width(240.0),
                            );
                            if primary(ui, t!(self.lang, "подготовить vkms", "prepare vkms"), t)
                                .clicked()
                            {
                                let state = self.state.clone();
                                let password = self.screen.sudo.clone();
                                self.spawn(move || {
                                    TaskResult::Msg(state.monitor.prepare_vkms(&password))
                                });
                            }
                        });
                    }
                }
                _ => {
                    let preset = if self.screen.mode == MonitorMode::Extend {
                        t!(self.lang, "дополнительный монитор", "extra monitor")
                    } else {
                        t!(self.lang, "зеркало экрана", "screen mirror")
                    };
                    let pos = if self.screen.mode == MonitorMode::Extend {
                        format!(" · {}", self.screen.position.label_lang(self.lang))
                    } else {
                        String::new()
                    };
                    ui.label(
                        RichText::new(format!(
                            "{} · {}×{} · {} fps · {}{pos}",
                            preset,
                            self.screen.width,
                            self.screen.height,
                            self.screen.fps,
                            backend_label(self.lang, self.screen.backend)
                        ))
                        .size(14.0),
                    );
                    if !self.screen.output.is_empty() {
                        mono(ui, &format!("output {}", self.screen.output), t.muted);
                    }
                    ui.add_space(6.0);
                    if !s.monitor.last_error.is_empty() && !s.monitor.running {
                        small(ui, &s.monitor.last_error, t.bad);
                    }
                    if !s.monitor.note.is_empty() {
                        small(ui, &s.monitor.note, t.warn);
                    }
                    ui.horizontal(|ui| {
                        if primary(ui, t!(self.lang, "запустить экран", "start the screen"), t)
                            .clicked()
                        {
                            self.start_screen();
                        }
                        if ui
                            .add(egui::Button::new(hud(
                                t!(self.lang, "подсказка по конфигу", "config hint"),
                                t.faint,
                            )))
                            .clicked()
                        {
                            self.show_hint = !self.show_hint;
                        }
                        if ui
                            .add(egui::Button::new(hud(
                                t!(self.lang, "обновить выходы", "refresh outputs"),
                                t.faint,
                            )))
                            .clicked()
                        {
                            self.state.monitor.refresh();
                            self.toast(
                                t!(self.lang, "выходы обновлены", "outputs refreshed"),
                                true,
                            );
                        }
                    });
                    if self.show_hint {
                        ui.add_space(8.0);
                        egui::Frame::new()
                            .fill(t.well)
                            .stroke(t.hairline())
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(hud(
                                        t!(
                                            self.lang,
                                            "что добавить в конфиг",
                                            "what to add to the config"
                                        ),
                                        t.muted,
                                    ));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        theme::copy_button(ui, &s.monitor.config_hint, t);
                                    });
                                });
                                if !s.monitor.config_file.is_empty() {
                                    mono(ui, &s.monitor.config_file, t.faint);
                                }
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&s.monitor.config_hint)
                                            .family(FontFamily::Monospace)
                                            .size(11.5)
                                            .color(t.ink),
                                    )
                                    .wrap(),
                                );
                            });
                    }
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if self.screen.step > 0
                    && ui
                        .add(egui::Button::new(hud(
                            t!(self.lang, "назад", "back"),
                            t.muted,
                        )))
                        .clicked()
                {
                    self.screen.step -= 1;
                }
                if self.screen.step < 3 && primary(ui, t!(self.lang, "далее", "next"), t).clicked()
                {
                    self.screen.step += 1;
                }
            });
        });

        ui.add_space(10.0);
        card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(self.lang, "инструменты", "tools"))
                    .size(15.0)
                    .strong(),
            );
            ui.add_space(4.0);
            egui::Grid::new("tools")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    for tool in &s.monitor.tools {
                        ui.horizontal(|ui| {
                            theme::dot(
                                ui,
                                if tool.installed { t.good } else { t.bad },
                                tool.installed,
                            );
                            ui.label(
                                RichText::new(&tool.name)
                                    .family(FontFamily::Monospace)
                                    .size(12.0)
                                    .color(t.ink),
                            );
                        });
                        if tool.installed {
                            small(ui, &tool.purpose, t.muted);
                        } else {
                            small(ui, &tool.install_hint, t.faint);
                        }
                        ui.end_row();
                    }
                });
        });
    }
}

// ---------------------------------------------------------------- notifications

impl App {
    fn notifications_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        let s = self.status.clone();
        self.page_header(
            ui,
            t,
            t!(self.lang, "уведомления", "notifications"),
            t!(
                self.lang,
                "зеркалирование системных уведомлений на steam deck",
                "mirror desktop notifications to the steam deck"
            ),
        );
        card(ui, t, |ui| {
            row(
                ui,
                t,
                t!(self.lang, "зеркалирование", "mirroring"),
                &format!(
                    "{} · {} · {} {}",
                    s.notifications.backend,
                    if s.notifications.listening {
                        t!(self.lang, "слушает", "listening")
                    } else {
                        t!(self.lang, "не слушает", "not listening")
                    },
                    s.notifications.count,
                    t!(self.lang, "в буфере", "buffered")
                ),
                |ui| {
                    if switch(ui, &mut self.notif_enabled, t).changed() {
                        let on = self.notif_enabled;
                        self.state.notifications.set_enabled(on);
                        self.save(|c| c.notifications.enabled = on);
                    }
                },
            );
            if !s.notifications.error.is_empty() {
                small(ui, &s.notifications.error, t.bad);
            } else if !s.notifications.listening {
                small(
                    ui,
                    t!(
                        self.lang,
                        "нужен запущенный демон уведомлений: mako, swaync, dunst, kde или gnome.",
                        "a notification daemon must be running: mako, swaync, dunst, kde or gnome."
                    ),
                    t.warn,
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ghost(ui, t!(self.lang, "проверить", "test"), t).clicked() {
                    self.state.notifications.add_test();
                }
                if ghost(ui, t!(self.lang, "очистить", "clear"), t).clicked() {
                    self.state.notifications.clear();
                }
            });
        });
        ui.add_space(10.0);
        if s.notifications.recent.is_empty() {
            card(ui, t, |ui| {
                small(
                    ui,
                    t!(
                        self.lang,
                        "пока пусто. уведомления появятся здесь.",
                        "nothing yet. notifications will appear here."
                    ),
                    t.muted,
                );
            });
        } else {
            for n in &s.notifications.recent {
                let n = n.clone();
                ui.add_space(6.0);
                card(ui, t, |ui| {
                    ui.horizontal(|ui| {
                        theme::dot(
                            ui,
                            if n.urgency >= 2 {
                                t.bad
                            } else if n.urgency == 1 {
                                t.warn
                            } else {
                                t.accent
                            },
                            true,
                        );
                        ui.label(RichText::new(&n.app).size(12.5).strong().color(t.strong));
                        small(ui, &ago(n.ts), t.faint);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(egui::Button::new(theme::icon(
                                    theme::icon::CLOSE,
                                    13.0,
                                    t.muted,
                                )))
                                .clicked()
                            {
                                self.state.notifications.dismiss(n.id);
                            }
                        });
                    });
                    ui.label(RichText::new(&n.summary).size(14.0).color(t.ink));
                    if !n.body.is_empty() {
                        small(ui, &n.body, t.muted);
                    }
                });
            }
        }
    }
}

// ---------------------------------------------------------------- home assistant

impl App {
    fn ha_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        let s = self.status.clone();
        self.page_header(
            ui,
            t,
            "home assistant",
            t!(
                self.lang,
                "свет, сцены и датчики прямо с дека",
                "lights, scenes and sensors from the deck"
            ),
        );
        if self.last_ha_fetch.elapsed() > Duration::from_secs(6) {
            self.refresh_ha();
        }
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("подключение").size(15.0).strong());
                tag(
                    ui,
                    if s.ha.connected {
                        t!(self.lang, "онлайн", "online")
                    } else if s.ha.configured {
                        t!(self.lang, "ошибка", "error")
                    } else {
                        t!(self.lang, "не настроен", "not set")
                    },
                    if s.ha.connected {
                        t.good
                    } else if s.ha.configured {
                        t.bad
                    } else {
                        t.muted
                    },
                );
            });
            ui.add_space(6.0);
            ui.label(hud(t!(self.lang, "адрес", "url"), t.faint));
            ui.add(
                TextEdit::singleline(&mut self.ha_url)
                    .hint_text("http://homeassistant.local:8123")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);
            ui.label(hud(
                t!(self.lang, "долгоживущий токен", "long-lived token"),
                t.faint,
            ));
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.ha_token)
                        .password(!self.ha_show_token)
                        .desired_width(420.0),
                );
                if ui
                    .add(egui::Button::new(hud(
                        if self.ha_show_token { "hide" } else { "show" },
                        t.muted,
                    )))
                    .clicked()
                {
                    self.ha_show_token = !self.ha_show_token;
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if primary(ui, t!(self.lang, "подключить", "connect"), t).clicked() {
                    let state = self.state.clone();
                    let url = self.ha_url.trim().to_string();
                    let token = self.ha_token.trim().to_string();
                    self.toast(
                        t!(
                            self.lang,
                            "проверяю home assistant…",
                            "checking home assistant…"
                        ),
                        true,
                    );
                    self.spawn(move || {
                        // Ping first, then install: no lock is held while the
                        // server is being reached.
                        let client = sdw_core::ha::HaClient::new(&url, &token);
                        let message = match client.ping() {
                            Ok(m) => m,
                            Err(e) => return TaskResult::Msg(Err(e)),
                        };
                        let mut cfg = state.cfg();
                        let fav = cfg.ha.favorites.clone();
                        if let Ok(mut h) = state.ha.lock() {
                            h.install(&mut cfg, client, &message, &fav);
                        }
                        let _ = state.update_cfg(|c| {
                            c.ha.url = url;
                            c.ha.token = token;
                        });
                        TaskResult::Msg(Ok("home assistant подключён".into()))
                    });
                }
                if ghost(ui, t!(self.lang, "проверить", "test"), t).clicked() {
                    let state = self.state.clone();
                    self.spawn(move || {
                        let client = state.ha.lock().ok().and_then(|h| h.client.clone());
                        let Some(client) = client else {
                            return TaskResult::Msg(Err("home assistant не настроен".into()));
                        };
                        let entities = client.states();
                        let ok = entities.is_ok();
                        let rooms = Some(client.rooms());
                        let views = Some(sdw_core::ha::views_guarded(&client));
                        if let Ok(mut h) = state.ha.lock() {
                            h.apply(entities, rooms, views);
                        }
                        TaskResult::Msg(if ok {
                            Ok("home assistant отвечает".into())
                        } else {
                            Err("home assistant не отвечает".into())
                        })
                    });
                }
                if !s.ha.error.is_empty() {
                    small(ui, &s.ha.error, if s.ha.connected { t.good } else { t.bad });
                }
            });
        });

        ui.add_space(10.0);
        let mut cat_changed = false;
        let mut cat_remove: Option<usize> = None;
        let mut cat_swap: Option<(usize, usize)> = None;
        let cat_count = self.ha_categories.len();
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!(self.lang, "категории", "categories"))
                        .size(15.0)
                        .strong(),
                );
                small(
                    ui,
                    t!(
                        self.lang,
                        "вкладки на деке: порядок вкладок и плиток внутри них",
                        "deck tabs: tab order and tile order inside each one"
                    ),
                    t.faint,
                );
            });
            ui.add_space(6.0);
            if self.ha_categories.is_empty() {
                small(
                    ui,
                    t!(
                        self.lang,
                        "пока нет категорий. создайте первую ниже.",
                        "no categories yet. create the first one below."
                    ),
                    t.muted,
                );
            }
            for (i, c) in self.ha_categories.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if ui
                        .add(TextEdit::singleline(&mut c.name).desired_width(200.0))
                        .lost_focus()
                    {
                        cat_changed = true;
                    }
                    tag(ui, &format!("{}", c.entities.len()), t.muted);
                    if ui
                        .add_enabled(
                            i > 0,
                            egui::Button::new(hud(theme::icon::ARROW_LEFT, t.muted)),
                        )
                        .clicked()
                    {
                        cat_swap = Some((i, i - 1));
                    }
                    if ui
                        .add_enabled(
                            i + 1 < cat_count,
                            egui::Button::new(hud(theme::icon::ARROW_RIGHT, t.muted)),
                        )
                        .clicked()
                    {
                        cat_swap = Some((i, i + 1));
                    }
                    if ui
                        .add(egui::Button::new(hud(theme::icon::CLOSE, t.bad)))
                        .on_hover_text(t!(self.lang, "удалить категорию", "delete category"))
                        .clicked()
                    {
                        cat_remove = Some(i);
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.ha_new_category)
                        .hint_text(t!(self.lang, "новая категория", "new category"))
                        .desired_width(220.0),
                );
                if ghost(ui, t!(self.lang, "добавить", "add"), t).clicked()
                    && !self.ha_new_category.trim().is_empty()
                {
                    self.ha_categories
                        .push(HaCategory::new(&self.ha_new_category));
                    self.ha_new_category.clear();
                    cat_changed = true;
                }
            });
            small(
                ui,
                t!(
                    self.lang,
                    "добавляйте и сортируйте сущности через кнопку со стрелками в строке сущности.",
                    "add and sort entities with the arrows button on each entity row."
                ),
                t.faint,
            );
        });
        if let Some((a, b)) = cat_swap {
            if a < self.ha_categories.len() && b < self.ha_categories.len() {
                self.ha_categories.swap(a, b);
                cat_changed = true;
            }
        }
        if let Some(i) = cat_remove {
            if i < self.ha_categories.len() {
                self.ha_categories.remove(i);
                cat_changed = true;
            }
        }
        if cat_changed {
            self.save_ha_categories();
        }

        // Presets are shared with every deck through the daemon config, so a
        // preset created here shows up on the deck (and the other way round).
        ui.add_space(10.0);
        let mut preset_changed = false;
        let mut preset_remove: Option<usize> = None;
        let mut preset_swap: Option<(usize, usize)> = None;
        let mut preset_apply: Option<Vec<HaPresetAction>> = None;
        let mut preset_capture: Option<usize> = None;
        let mut add_from_lights = false;
        let preset_count = self.ha_presets.len();
        let mut new_preset = self.ha_new_preset.clone();
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!(self.lang, "пресеты света", "light presets"))
                        .size(15.0)
                        .strong(),
                );
                small(
                    ui,
                    t!(
                        self.lang,
                        "яркость, температура и цвет всех включённых ламп одним нажатием",
                        "brightness, temperature and colour of every lit lamp in one tap"
                    ),
                    t.faint,
                );
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut new_preset)
                        .hint_text(t!(self.lang, "имя пресета", "preset name"))
                        .desired_width(220.0),
                );
                if ghost(
                    ui,
                    t!(self.lang, "снять текущий свет", "capture current lights"),
                    t,
                )
                .on_hover_text(t!(
                    self.lang,
                    "снимок яркости и цвета всех включённых ламп",
                    "snapshot of brightness and colour of every lamp that is on"
                ))
                .clicked()
                {
                    add_from_lights = true;
                }
            });
            if self.ha_presets.is_empty() {
                small(
                    ui,
                    t!(
                        self.lang,
                        "пока нет пресетов. включите лампы и снимите текущий свет.",
                        "no presets yet. turn lamps on and capture the current lights."
                    ),
                    t.muted,
                );
            }
            for (i, p) in self.ha_presets.iter_mut().enumerate() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(TextEdit::singleline(&mut p.name).desired_width(180.0))
                        .lost_focus()
                    {
                        preset_changed = true;
                    }
                    tag(ui, &format!("{}", p.actions.len()), t.muted);
                    if ui
                        .add(egui::Button::new(hud(
                            t!(self.lang, "применить", "apply"),
                            t.muted,
                        )))
                        .clicked()
                    {
                        preset_apply = Some(p.actions.clone());
                    }
                    if ui
                        .add_enabled(
                            i > 0,
                            egui::Button::new(hud(theme::icon::ARROW_LEFT, t.muted)),
                        )
                        .clicked()
                    {
                        preset_swap = Some((i, i - 1));
                    }
                    if ui
                        .add_enabled(
                            i + 1 < preset_count,
                            egui::Button::new(hud(theme::icon::ARROW_RIGHT, t.muted)),
                        )
                        .clicked()
                    {
                        preset_swap = Some((i, i + 1));
                    }
                    if ui
                        .add(egui::Button::new(hud(
                            t!(self.lang, "снять", "recapture"),
                            t.faint,
                        )))
                        .on_hover_text(t!(
                            self.lang,
                            "заменить содержимое текущим светом",
                            "replace the contents with the current lights"
                        ))
                        .clicked()
                    {
                        preset_capture = Some(i);
                    }
                    if ui
                        .add(egui::Button::new(hud(theme::icon::CLOSE, t.bad)))
                        .on_hover_text(t!(self.lang, "удалить пресет", "delete preset"))
                        .clicked()
                    {
                        preset_remove = Some(i);
                    }
                });
            }
        });
        if add_from_lights {
            let actions = sdw_core::ha::capture_light_preset(&self.ha_entities);
            if actions.is_empty() {
                self.toast(
                    t!(
                        self.lang,
                        "нет включённых ламп — пресет пустой",
                        "no lights are on — the preset would be empty"
                    ),
                    false,
                );
            } else {
                let mut preset = HaPreset::new(&new_preset);
                preset.actions = actions;
                self.ha_presets.push(preset);
                new_preset.clear();
                preset_changed = true;
            }
        }
        if let Some((a, b)) = preset_swap {
            if a < self.ha_presets.len() && b < self.ha_presets.len() {
                self.ha_presets.swap(a, b);
                preset_changed = true;
            }
        }
        if let Some(i) = preset_capture {
            let actions = sdw_core::ha::capture_light_preset(&self.ha_entities);
            if !actions.is_empty() {
                if let Some(p) = self.ha_presets.get_mut(i) {
                    p.actions = actions;
                    preset_changed = true;
                }
            }
        }
        if let Some(i) = preset_remove {
            if i < self.ha_presets.len() {
                self.ha_presets.remove(i);
                preset_changed = true;
            }
        }
        self.ha_new_preset = new_preset;
        if preset_changed {
            self.save_ha_presets();
        }
        if let Some(actions) = preset_apply {
            let state = self.state.clone();
            self.spawn(move || {
                let res = state
                    .ha
                    .lock()
                    .map(|h| {
                        for a in &actions {
                            let call = HaCall {
                                domain: "light".into(),
                                service: a.service.clone(),
                                target: serde_json::json!({ "entity_id": a.entity_id }),
                                data: a.data.clone(),
                            };
                            if let Err(e) = h.call(&call) {
                                return Err(e);
                            }
                        }
                        Ok(())
                    })
                    .unwrap_or(Err("home assistant lock".into()));
                TaskResult::Msg(res.map(|_| "пресет применён".into()))
            });
        }

        ui.add_space(10.0);
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!(self.lang, "сущности", "entities"))
                        .size(15.0)
                        .strong(),
                );
                ui.add(
                    TextEdit::singleline(&mut self.ha_search)
                        .hint_text(t!(self.lang, "поиск", "search"))
                        .desired_width(220.0),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(hud(
                            t!(self.lang, "обновить", "refresh"),
                            t.faint,
                        )))
                        .clicked()
                    {
                        self.last_ha_fetch = Instant::now() - Duration::from_secs(60);
                    }
                });
            });
            if self.ha_entities.is_empty() {
                small(
                    ui,
                    t!(
                        self.lang,
                        "нет данных. подключите home assistant выше.",
                        "no data. connect home assistant above."
                    ),
                    t.muted,
                );
                return;
            }
            let cfg = self.state.cfg();
            let q = self.ha_search.to_lowercase();
            let mut shown = 0;
            let favorites: Vec<HaEntity> = self
                .ha_entities
                .iter()
                .filter(|e| cfg.ha.favorites.contains(&e.entity_id))
                .cloned()
                .collect();
            if !favorites.is_empty() && q.is_empty() {
                ui.add_space(4.0);
                ui.label(hud(t!(self.lang, "избранное", "favorites"), t.faint));
                for e in favorites {
                    self.ha_entity_row(ui, t, &e);
                    shown += 1;
                }
                ui.add_space(6.0);
                ui.label(hud(
                    t!(self.lang, "всё остальное", "everything else"),
                    t.faint,
                ));
            }
            let mut all: Vec<HaEntity> = self
                .ha_entities
                .iter()
                .filter(|e| sdw_core::ha::is_controllable(e) || sdw_core::ha::is_sensor(e))
                .cloned()
                .collect();
            all.sort_by(|a, b| a.name().cmp(&b.name()));
            for e in all {
                if shown > 120 {
                    break;
                }
                if !q.is_empty()
                    && !e.name().to_lowercase().contains(&q)
                    && !e.entity_id.to_lowercase().contains(&q)
                {
                    continue;
                }
                if cfg.ha.favorites.contains(&e.entity_id) && q.is_empty() {
                    continue;
                }
                self.ha_entity_row(ui, t, &e);
                shown += 1;
            }
        });

        if let Some((entity, target)) = self.ha_pending_category.take() {
            self.assign_category(&entity, target.as_deref());
        }
    }

    fn ha_entity_row(&mut self, ui: &mut Ui, t: &Tokens, e: &HaEntity) {
        let cfg = self.state.cfg();
        let fav = cfg.ha.favorites.contains(&e.entity_id);
        let controllable = sdw_core::ha::is_controllable(e);
        let on = e.is_on();
        ui.horizontal(|ui| {
            theme::dot(ui, if on { t.good } else { t.faint }, on);
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - 220.0).max(180.0));
                ui.label(RichText::new(e.name()).size(13.0).color(t.ink));
                small(
                    ui,
                    &format!("{} · {}", e.entity_id, state_label(e, self.lang)),
                    t.faint,
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !self.ha_categories.is_empty() {
                    let cats = self.ha_categories.clone();
                    let current = cats
                        .iter()
                        .find(|c| c.entities.iter().any(|x| x == &e.entity_id))
                        .map(|c| c.id.clone());
                    ui.menu_button(theme::icon(theme::icon::DRAG, 16.0, t.faint), |ui| {
                        ui.label(hud(t!(self.lang, "категория", "category"), t.faint));
                        for c in &cats {
                            let inside = current.as_deref() == Some(c.id.as_str());
                            if ui.selectable_label(inside, &c.name).clicked() {
                                self.ha_pending_category =
                                    Some((e.entity_id.clone(), Some(c.id.clone())));
                            }
                        }
                        if current.is_some()
                            && ui
                                .button(t!(
                                    self.lang,
                                    "убрать из категории",
                                    "remove from category"
                                ))
                                .clicked()
                        {
                            self.ha_pending_category = Some((e.entity_id.clone(), None));
                        }
                    });
                }
                if ui
                    .add(egui::Button::new(theme::icon(
                        theme::icon::STAR,
                        15.0,
                        if fav { t.warn } else { t.faint },
                    )))
                    .clicked()
                {
                    let id = e.entity_id.clone();
                    self.save(|c| {
                        if let Some(pos) = c.ha.favorites.iter().position(|x| x == &id) {
                            c.ha.favorites.remove(pos);
                        } else {
                            c.ha.favorites.push(id);
                        }
                    });
                }
                if controllable {
                    if e.domain() == "scene" || e.domain() == "script" || e.domain() == "button" {
                        if ghost(ui, t!(self.lang, "запустить", "run"), t).clicked() {
                            let entity = e.entity_id.clone();
                            let domain = e.domain().to_string();
                            let state = self.state.clone();
                            self.spawn(move || {
                                let call = HaCall {
                                    domain,
                                    service: "turn_on".into(),
                                    target: serde_json::json!({"entity_id": entity}),
                                    data: serde_json::Value::Null,
                                };
                                TaskResult::Msg(
                                    state
                                        .ha
                                        .lock()
                                        .map(|h| h.call(&call))
                                        .unwrap_or(Err("lock".into()))
                                        .map(|_| "команда отправлена".into()),
                                )
                            });
                        }
                    } else {
                        if ghost(
                            ui,
                            if on {
                                t!(self.lang, "выкл", "off")
                            } else {
                                t!(self.lang, "вкл", "on")
                            },
                            t,
                        )
                        .clicked()
                        {
                            let entity = e.entity_id.clone();
                            let state = self.state.clone();
                            self.spawn(move || {
                                TaskResult::Msg(
                                    state
                                        .ha
                                        .lock()
                                        .map(|h| h.toggle(&entity, !on))
                                        .unwrap_or(Err("lock".into()))
                                        .map(|_| "готово".into()),
                                )
                            });
                        }
                    }
                }
            });
        });
        if e.domain() == "light" {
            if let Some(b) = e.brightness() {
                let mut v = b as f32 / 255.0 * 100.0;
                if ui
                    .add(egui::Slider::new(&mut v, 0.0..=100.0).show_value(false))
                    .changed()
                {
                    let entity = e.entity_id.clone();
                    let state = self.state.clone();
                    let pct = v.round() as u8;
                    self.spawn(move || {
                        TaskResult::Msg(
                            state
                                .ha
                                .lock()
                                .map(|h| h.set_brightness(&entity, pct))
                                .unwrap_or(Err("lock".into()))
                                .map(|_| "яркость".into()),
                        )
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------- security, install, settings, logs

impl App {
    fn security_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        let cfg = self.state.cfg();
        self.page_header(
            ui,
            t,
            t!(self.lang, "безопасность", "security"),
            t!(
                self.lang,
                "пароль сопряжения, токены и доступ из сети",
                "pairing password, tokens and network access"
            ),
        );
        card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(self.lang, "сменить пароль", "change password"))
                    .size(15.0)
                    .strong(),
            );
            ui.add_space(6.0);
            ui.label(hud(t!(self.lang, "новый пароль", "new password"), t.faint));
            ui.add(
                TextEdit::singleline(&mut self.new_pass)
                    .password(!self.show_pass)
                    .desired_width(f32::INFINITY),
            );
            ui.label(hud(t!(self.lang, "ещё раз", "repeat"), t.faint));
            ui.add(
                TextEdit::singleline(&mut self.new_pass2)
                    .password(!self.show_pass)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.show_pass, t!(self.lang, "показать", "show"));
                let valid = self.new_pass.chars().count() >= 4 && self.new_pass == self.new_pass2;
                if ui
                    .add_enabled(
                        valid,
                        egui::Button::new(hud(t!(self.lang, "сменить", "change"), t.muted)),
                    )
                    .clicked()
                {
                    let pass = self.new_pass.clone();
                    self.save(|c| c.set_password(&pass));
                    self.new_pass.clear();
                    self.new_pass2.clear();
                    self.toast(
                        t!(
                            self.lang,
                            "пароль изменён. клиенты должны спариться заново.",
                            "password changed. clients must pair again."
                        ),
                        true,
                    );
                }
            });
            ui.add_space(8.0);
            divider(ui);
            row(
                ui,
                t,
                t!(
                    self.lang,
                    "доступ из публичных сетей",
                    "public network access"
                ),
                t!(
                    self.lang,
                    "по умолчанию только локальная сеть. включайте только с vpn.",
                    "local network only by default. enable only behind a vpn."
                ),
                |ui| {
                    let mut on = cfg.allow_public;
                    if switch(ui, &mut on, t).changed() {
                        self.save(|c| c.allow_public = on);
                    }
                },
            );
            ui.add_space(8.0);
            divider(ui);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!(self.lang, "токен api", "api token"))
                        .size(15.0)
                        .strong(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost(ui, t!(self.lang, "создать новый", "rotate"), t).clicked()
                    {
                        let mut token = String::new();
                        self.save(|c| token = c.rotate_token());
                        self.new_token = Some(token);
                        self.toast(t!(self.lang, "токен обновлён", "token rotated"), true);
                    }
                });
            });
            small(
                ui,
                t!(
                    self.lang,
                    "при смене токена все подключённые деки должны спариться заново.",
                    "rotating the token makes every paired deck pair again."
                ),
                t.muted,
            );
            if let Some(token) = &self.new_token {
                ui.horizontal(|ui| {
                    mono(ui, token, t.accent_ink);
                    theme::copy_button(ui, token, t);
                });
            }
        });
        ui.add_space(10.0);
        card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(self.lang, "клиенты", "clients"))
                    .size(15.0)
                    .strong(),
            );
            if cfg.clients.is_empty() {
                small(ui, t!(self.lang, "никого", "none"), t.muted);
            } else {
                for c in &cfg.clients {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&c.name).size(13.0).color(t.ink));
                        small(ui, &format!("{} · {}", c.id, ago(c.last_seen_ms)), t.faint);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(egui::Button::new(hud(
                                    t!(self.lang, "забыть", "forget"),
                                    t.bad,
                                )))
                                .clicked()
                            {
                                let id = c.id.clone();
                                self.save(|cfg| cfg.clients.retain(|x| x.id != id));
                            }
                        });
                    });
                }
            }
        });
        ui.add_space(10.0);
        card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(self.lang, "сброс настройки", "reset setup"))
                    .size(15.0)
                    .strong(),
            );
            small(
                ui,
                t!(
                    self.lang,
                    "удаляет пароль сопряжения и снова открывает мастер.",
                    "removes the pairing password and opens the wizard again."
                ),
                t.muted,
            );
            if ghost(ui, t!(self.lang, "сбросить пароль", "reset password"), t).clicked()
            {
                self.save(|c| {
                    c.api_password_hash.clear();
                    c.api_token = random_id(24);
                    c.setup_done = false;
                });
                self.status = self.state.status();
                self.setup_step = 0;
            }
        });
    }

    fn install_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        let job = self.state.jobs.status();
        self.page_header(
            ui,
            t,
            t!(self.lang, "установка", "install"),
            t!(
                self.lang,
                "viiper и системные инструменты",
                "viiper and the system tools"
            ),
        );
        if job.id != self.last_job {
            self.last_job = job.id;
        }
        card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(self.lang, "компоненты", "components"))
                    .size(15.0)
                    .strong(),
            );
            ui.add_space(4.0);
            for c in self.install_components.clone() {
                let mut checked = self.install_sel.contains(&c.id);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut checked, "").changed() {
                        if checked {
                            self.install_sel.insert(c.id.clone());
                        } else {
                            self.install_sel.remove(&c.id);
                        }
                    }
                    theme::dot(ui, if c.installed { t.good } else { t.bad }, c.installed);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&c.name).size(13.0).color(t.strong));
                        small(ui, &c.purpose, t.muted);
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if c.installed {
                            tag(ui, t!(self.lang, "есть", "installed"), t.good);
                        } else {
                            tag(ui, t!(self.lang, "нет", "missing"), t.bad);
                        }
                    });
                });
            }
            ui.add_space(8.0);
            ui.label(hud(
                t!(
                    self.lang,
                    "пароль sudo (для пакетов)",
                    "sudo password (for packages)"
                ),
                t.faint,
            ));
            ui.add(
                TextEdit::singleline(&mut self.install_sudo)
                    .password(true)
                    .hint_text(t!(
                        self.lang,
                        "нужен для pacman/apt",
                        "needed for pacman/apt"
                    ))
                    .desired_width(320.0),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if job.running {
                    small(ui, &format!("{}…", job.name), t.warn);
                } else if primary(
                    ui,
                    t!(self.lang, "установить выбранное", "install selected"),
                    t,
                )
                .clicked()
                {
                    let components: Vec<String> = self.install_sel.iter().cloned().collect();
                    let sudo = self.install_sudo.clone();
                    let logger = self.state.logger.clone();
                    let jobs = self.state.jobs.clone();
                    let _ = jobs.start("установка", move |ctx| {
                        let _ = install::install(&components, &sudo, &ctx, &logger);
                    });
                }
                if ui
                    .add(egui::Button::new(hud(
                        t!(self.lang, "обновить", "refresh"),
                        t.faint,
                    )))
                    .clicked()
                {
                    self.install_components = install::component_status();
                }
            });
        });
        ui.add_space(10.0);
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!(self.lang, "журнал установки", "install log"))
                        .size(15.0)
                        .strong(),
                );
                if job.running {
                    tag(ui, t!(self.lang, "идёт", "running"), t.warn);
                } else if job.done.is_some() {
                    tag(ui, t!(self.lang, "готово", "done"), t.good);
                }
            });
            if job.lines.is_empty() {
                small(ui, t!(self.lang, "пока пусто", "empty"), t.muted);
            } else {
                ScrollArea::vertical()
                    .max_height(300.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &job.lines {
                            mono(
                                ui,
                                line,
                                if line.starts_with("Ошибка") {
                                    t.bad
                                } else {
                                    t.muted
                                },
                            );
                        }
                    });
            }
            if let Some(e) = &job.error {
                small(ui, e, t.bad);
            }
        });
    }

    fn updates_card(&mut self, ui: &mut Ui, t: &Tokens) {
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!(self.lang, "обновления", "updates"))
                        .size(15.0)
                        .strong(),
                );
                tag(ui, &format!("v{}", VERSION), t.muted);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost(ui, t!(self.lang, "проверить", "check"), t).clicked() {
                        self.check_updates();
                    }
                });
            });
            small(
                ui,
                &format!("git.entitybtw.ru · entitybtw/sd-widgets · {}", VERSION),
                t.faint,
            );
            if self.update_checking {
                ui.horizontal(|ui| {
                    ui.spinner();
                    small(
                        ui,
                        t!(self.lang, "проверяю релизы…", "checking releases…"),
                        t.muted,
                    );
                });
            }
            if !self.update_error.is_empty() {
                small(ui, &self.update_error, t.bad);
            }
            if let Some(info) = self.update_info.clone() {
                ui.add_space(4.0);
                divider(ui);
                ui.horizontal(|ui| {
                    if info.newer {
                        tag(ui, &format!("{} → {}", info.current, info.latest), t.good);
                    } else {
                        tag(ui, t!(self.lang, "последняя версия", "up to date"), t.good);
                    }
                    small(
                        ui,
                        &info.published.chars().take(10).collect::<String>(),
                        t.faint,
                    );
                });
                if info.newer {
                    if !info.notes.is_empty() {
                        ui.label(
                            RichText::new(sdw_core::util::truncate(&info.notes, 420))
                                .size(11.5)
                                .color(t.muted),
                        );
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if self.update_downloading {
                            let done = self.update_progress.load(Ordering::Relaxed);
                            let total = self.update_total.max(1);
                            ui.add(
                                egui::ProgressBar::new(
                                    (done as f32 / total as f32).clamp(0.0, 1.0),
                                )
                                .desired_width(260.0)
                                .text(format!("{}%", done * 100 / total)),
                            );
                        } else if primary(
                            ui,
                            t!(self.lang, "скачать обновление", "download update"),
                            t,
                        )
                        .clicked()
                        {
                            self.download_update();
                        }
                        if ghost(ui, t!(self.lang, "страница релиза", "release page"), t).clicked()
                        {
                            let _ = sdw_core::util::open_url(&info.page);
                        }
                    });
                }
                if !self.update_path.is_empty() {
                    mono(ui, &self.update_path, t.muted);
                    ui.horizontal(|ui| {
                        if ghost(ui, t!(self.lang, "установить", "install"), t).clicked()
                        {
                            let path = self.update_path.clone();
                            self.spawn(move || {
                                TaskResult::Installed(update::install(std::path::Path::new(&path)))
                            });
                        }
                        if ghost(ui, t!(self.lang, "открыть папку", "open folder"), t).clicked()
                        {
                            let _ = sdw_core::util::open_url(
                                &update::updates_dir().display().to_string(),
                            );
                        }
                    });
                }
            }
        });
        ui.add_space(10.0);
    }

    /// Bluetooth devices and their battery, with pairing on the computer.
    fn bluetooth_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        self.page_header(
            ui,
            t,
            t!(self.lang, "bluetooth", "bluetooth"),
            t!(
                self.lang,
                "наушники, клавиатуры и другие устройства; заряд виден сразу.",
                "headphones, keyboards and other devices, with battery where reported."
            ),
        );
        if !self.bluetooth_loaded && !self.bluetooth_pending {
            self.bluetooth_pending = true;
            self.spawn(|| TaskResult::Bluetooth(Ok(crate::bluetooth::status())));
        }

        let status = self.bluetooth.clone();
        let mut action: Option<(&'static str, String)> = None;
        let mut remove_ask: Option<String> = None;
        let mut refresh = false;

        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(hud(t!(self.lang, "адаптер", "adapter"), t.faint));
                if self.bluetooth_pending {
                    ui.spinner();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost(ui, t!(self.lang, "обновить", "refresh"), t).clicked() {
                        refresh = true;
                    }
                });
            });
            match &status {
                Some(bt) if bt.available => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(&bt.adapter)
                                .family(FontFamily::Monospace)
                                .size(11.5)
                                .color(t.muted),
                        );
                        small(
                            ui,
                            if bt.powered {
                                t!(self.lang, "включён", "on")
                            } else {
                                t!(self.lang, "выключен", "off")
                            },
                            if bt.powered { t.good } else { t.faint },
                        );
                        if bt.discovering {
                            small(ui, t!(self.lang, "идёт поиск", "scanning"), t.accent);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if primary(ui, t!(self.lang, "искать устройства", "scan"), t).clicked()
                        {
                            action = Some(("scan", String::new()));
                        }
                        if bt.powered {
                            if ghost(ui, t!(self.lang, "выключить bluetooth", "bluetooth off"), t)
                                .clicked()
                            {
                                action = Some(("power_off", String::new()));
                            }
                        } else if ghost(ui, t!(self.lang, "включить bluetooth", "bluetooth on"), t)
                            .clicked()
                        {
                            action = Some(("power_on", String::new()));
                        }
                    });
                }
                _ => small(
                    ui,
                    t!(
                        self.lang,
                        "bluetooth недоступен; проверьте bluez и адаптер.",
                        "bluetooth is unavailable; check bluez and the adapter."
                    ),
                    t.muted,
                ),
            }
            if !self.bluetooth_err.is_empty() {
                small(
                    ui,
                    &sdw_core::util::truncate(&self.bluetooth_err, 200),
                    t.warn,
                );
            }
        });
        ui.add_space(8.0);

        let devices = status.map(|b| b.devices).unwrap_or_default();
        if devices.is_empty() {
            card(ui, t, |ui| {
                small(
                    ui,
                    t!(
                        self.lang,
                        "устройств пока нет. включите bluetooth и запустите поиск.",
                        "no devices yet. turn bluetooth on and scan."
                    ),
                    t.muted,
                );
            });
        }
        for d in devices {
            let armed = self.bluetooth_remove.as_deref() == Some(d.address.as_str());
            card(ui, t, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::icon(
                        if d.is_audio() {
                            theme::icon::HEADPHONES
                        } else {
                            theme::icon::BLUETOOTH
                        },
                        20.0,
                        if d.connected { t.good } else { t.faint },
                    ));
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(if d.name.is_empty() {
                                &d.address
                            } else {
                                &d.name
                            })
                            .size(14.5)
                            .strong()
                            .color(t.strong),
                        );
                        ui.label(
                            RichText::new(&d.address)
                                .family(FontFamily::Monospace)
                                .size(10.5)
                                .color(t.faint),
                        );
                    });
                    if d.connected {
                        small(ui, t!(self.lang, "подключено", "connected"), t.good);
                    }
                    if d.paired {
                        small(ui, t!(self.lang, "спарено", "paired"), t.accent);
                    }
                    if let Some(pct) = d.battery {
                        small(
                            ui,
                            &format!("{pct}%"),
                            if pct >= 30 { t.good } else { t.warn },
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if armed {
                            if primary(ui, t!(self.lang, "точно удалить", "remove it"), t).clicked()
                            {
                                action = Some(("remove", d.address.clone()));
                            }
                            if ghost(ui, t!(self.lang, "отмена", "cancel"), t).clicked() {
                                remove_ask = Some(String::new());
                            }
                        } else {
                            if ghost(ui, t!(self.lang, "удалить", "remove"), t).clicked() {
                                remove_ask = Some(d.address.clone());
                            }
                            if !d.trusted
                                && !d.connected
                                && ghost(ui, t!(self.lang, "доверять", "trust"), t).clicked()
                            {
                                action = Some(("trust", d.address.clone()));
                            }
                            if !d.paired {
                                if ghost(ui, t!(self.lang, "спарить", "pair"), t).clicked() {
                                    action = Some(("pair", d.address.clone()));
                                }
                            } else if d.connected {
                                if ghost(ui, t!(self.lang, "отключить", "disconnect"), t).clicked()
                                {
                                    action = Some(("disconnect", d.address.clone()));
                                }
                            } else if primary(ui, t!(self.lang, "подключить", "connect"), t)
                                .clicked()
                            {
                                action = Some(("connect", d.address.clone()));
                            }
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }

        if refresh {
            self.bluetooth_loaded = false;
        }
        if let Some(address) = remove_ask {
            self.bluetooth_remove = (!address.is_empty()).then_some(address);
        }
        if let Some((action, address)) = action {
            self.bluetooth_remove = None;
            self.spawn(move || TaskResult::BtAction(crate::bluetooth::action(action, &address)));
        }
    }

    /// Power and application launchers: the same list the deck sees, with
    /// the same declared commands behind it.
    fn control_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        self.page_header(
            ui,
            t,
            t!(self.lang, "пульт", "control"),
            t!(
                self.lang,
                "питание и запуск приложений; то же самое умеет дека.",
                "power actions and application launch; the deck can do the same."
            ),
        );
        if !self.control_scanned && !self.control_pending {
            self.control_pending = true;
            self.control_scanned = true;
            self.spawn(|| {
                TaskResult::Apps(crate::apps::scan().into_iter().map(|a| a.entry).collect())
            });
        }

        card(ui, t, |ui| {
            ui.label(hud(t!(self.lang, "питание", "power"), t.faint));
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for action in PowerAction::ALL {
                    if ghost(ui, power_label(self.lang, action), t).clicked() {
                        if action.destructive() {
                            self.toast(
                                t!(
                                    self.lang,
                                    "перезагрузка и выключение — только с деки",
                                    "restart and shutdown are only available from the deck"
                                ),
                                false,
                            );
                        } else {
                            let action_now = action;
                            self.spawn(move || TaskResult::Msg(crate::apps::power(action_now)));
                        }
                    }
                }
            });
            small(
                ui,
                t!(
                    self.lang,
                    "выключение и перезагрузка требуют подтверждения с деки.",
                    "shut down and restart ask for confirmation on the deck."
                ),
                t.faint,
            );
        });
        ui.add_space(10.0);

        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(hud(t!(self.lang, "приложения", "applications"), t.faint));
                if self.control_pending {
                    ui.spinner();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost(ui, t!(self.lang, "обновить", "refresh"), t).clicked() {
                        self.control_pending = true;
                        self.control_scanned = true;
                        self.spawn(|| {
                            TaskResult::Apps(
                                crate::apps::scan().into_iter().map(|a| a.entry).collect(),
                            )
                        });
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.control_filter)
                        .hint_text(t!(self.lang, "поиск приложения", "search applications"))
                        .desired_width(320.0),
                );
                ui.label(
                    RichText::new(format!("{}", self.control_apps.len()))
                        .family(FontFamily::Monospace)
                        .size(11.0)
                        .color(t.faint),
                );
            });
            let needle = self.control_filter.trim().to_lowercase();
            let mut launch: Option<String> = None;
            for entry in self.control_apps.iter().filter(|a| {
                needle.is_empty()
                    || a.name.to_lowercase().contains(&needle)
                    || a.comment.to_lowercase().contains(&needle)
            }) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&entry.name).size(13.5).color(t.strong));
                    if !entry.source.is_empty() {
                        small(ui, &sdw_core::util::truncate(&entry.source, 40), t.faint);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ghost(ui, t!(self.lang, "запустить", "start"), t).clicked() {
                            launch = Some(entry.id.clone());
                        }
                    });
                });
            }
            if let Some(id) = launch {
                self.spawn(move || TaskResult::Msg(crate::apps::launch(&id)));
            }
        });
    }

    /// Store bases and their catalogues. The daemon installs on itself; the
    /// deck asks it to do the same for the package it just installed.
    fn addons_store_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        let repos = self.state.cfg().addons.repos.clone();
        let stale = self
            .addon_catalog_at
            .map(|at| at.elapsed() > Duration::from_secs(600))
            .unwrap_or(true);
        if stale && !repos.is_empty() {
            self.load_addon_catalog();
        }
        let mut wanted: Option<sdw_core::addons::CatalogEntry> = None;
        let mut drop_repo: Option<String> = None;
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(hud(t!(self.lang, "магазин", "store"), t.faint));
                if self.addon_catalog_pending {
                    ui.spinner();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost(ui, t!(self.lang, "обновить", "refresh"), t).clicked() {
                        self.addon_catalog_at = None;
                        self.load_addon_catalog();
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.addon_repo_input)
                        .hint_text(t!(
                            self.lang,
                            "ссылка на магазин или на файл .sdwaddon",
                            "a store URL or a .sdwaddon file"
                        ))
                        .desired_width(380.0),
                );
                let typed = self.addon_repo_input.trim().to_string();
                let is_file = typed.ends_with(".sdwaddon")
                    || typed.ends_with(".zip")
                    || typed.ends_with(".json")
                    || typed.ends_with(".css");
                if primary(
                    ui,
                    if is_file {
                        t!(self.lang, "установить файл", "install file")
                    } else {
                        t!(self.lang, "добавить магазин", "add store")
                    },
                    t,
                )
                .clicked()
                {
                    if !(typed.starts_with("http://") || typed.starts_with("https://")) {
                        self.toast(
                            t!(
                                self.lang,
                                "нужна ссылка http:// или https://",
                                "a http:// or https:// link is required"
                            ),
                            false,
                        );
                    } else if is_file {
                        self.addon_installing = Some(typed.clone());
                        let entry = sdw_core::addons::CatalogEntry {
                            url: typed,
                            ..Default::default()
                        };
                        self.spawn(move || {
                            TaskResult::AddonInstalled(sdw_core::addons::install_entry(&entry))
                        });
                        self.addon_repo_input.clear();
                    } else {
                        let known = self
                            .state
                            .cfg()
                            .addons
                            .repos
                            .iter()
                            .any(|r| r.trim_end_matches('/') == typed);
                        if !known {
                            let _ = self.state.update_cfg(|c| {
                                c.addons.add_repo(&typed);
                            });
                        }
                        if !known {
                            self.addon_repo_input.clear();
                            self.addon_catalog_at = None;
                            self.load_addon_catalog();
                        } else {
                            self.toast(
                                t!(self.lang, "магазин уже добавлен", "store already added"),
                                true,
                            );
                        }
                    }
                }
            });
            for repo in &repos {
                ui.horizontal(|ui| {
                    ui.label(theme::icon(theme::icon::STORE, 15.0, t.faint));
                    ui.label(
                        RichText::new(repo)
                            .family(FontFamily::Monospace)
                            .size(11.0)
                            .color(t.muted),
                    );
                    if ghost(ui, t!(self.lang, "убрать", "remove"), t).clicked() {
                        drop_repo = Some(repo.clone());
                    }
                });
            }
            if !self.addon_catalog_err.is_empty() {
                small(
                    ui,
                    &sdw_core::util::truncate(&self.addon_catalog_err, 220),
                    t.warn,
                );
            }
            if self.addon_catalog.is_empty() {
                if repos.is_empty() {
                    small(
                        ui,
                        t!(
                            self.lang,
                            "добавьте ссылку на магазин — он покажет свои пакеты здесь; ссылка на .sdwaddon ставится сразу.",
                            "add a store link and its packages appear here; a .sdwaddon link installs right away."
                        ),
                        t.muted,
                    );
                }
                return;
            }
            let cols = if ui.available_width() >= 820.0 { 3 } else { 2 };
            let width =
                ((ui.available_width() - 8.0 * (cols as f32 - 1.0)) / cols as f32).max(170.0);
            let entries = self.addon_catalog.clone();
            for row in entries.chunks(cols) {
                ui.horizontal_top(|ui| {
                    for entry in row {
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, 0.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.set_width(ui.available_width());
                                egui::Frame::new()
                                    .fill(t.well)
                                    .stroke(Stroke::new(1.0, t.line))
                                    .corner_radius(0)
                                    .inner_margin(egui::Margin::same(10))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&entry.name)
                                                    .size(14.5)
                                                    .strong()
                                                    .color(t.strong),
                                            )
                                            .truncate(),
                                        );
                                        let tagline = if entry.tagline.is_empty() {
                                            &entry.description
                                        } else {
                                            &entry.tagline
                                        };
                                        if !tagline.is_empty() {
                                            small(
                                                ui,
                                                &sdw_core::util::truncate(tagline, 110),
                                                t.muted,
                                            );
                                        }
                                        ui.horizontal(|ui| {
                                            let meta =
                                                [entry.version.as_str(), entry.author.as_str()]
                                                    .into_iter()
                                                    .filter(|x| !x.trim().is_empty())
                                                    .collect::<Vec<_>>()
                                                    .join(" · ");
                                            if !meta.is_empty() {
                                                small(ui, &meta, t.faint);
                                            }
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    if self.addon_installing.as_deref()
                                                        == Some(entry.id.as_str())
                                                    {
                                                        ui.spinner();
                                                    } else if ghost(
                                                        ui,
                                                        t!(self.lang, "установить", "install"),
                                                        t,
                                                    )
                                                    .clicked()
                                                    {
                                                        wanted = Some(entry.clone());
                                                    }
                                                },
                                            );
                                        });
                                    });
                            },
                        );
                    }
                });
                ui.add_space(6.0);
            }
        });
        if let Some(repo) = drop_repo {
            let _ = self.state.update_cfg(|c| c.addons.remove_repo(&repo));
            self.addon_catalog.retain(|e| !e.url.starts_with(&repo));
            self.addon_catalog_at = None;
        }
        if let Some(entry) = wanted {
            if self.addon_installing.is_none() {
                self.addon_installing = Some(entry.id.clone());
                self.spawn(move || {
                    TaskResult::AddonInstalled(sdw_core::addons::install_entry(&entry))
                });
            }
        }
    }

    fn addons_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        self.page_header(
            ui,
            t,
            t!(self.lang, "аддоны", "addons"),
            t!(
                self.lang,
                "пакеты расширяют деку и демон: действия здесь, темы и часы на деке",
                "packages extend the deck and the daemon: actions here, themes and clocks on the deck"
            ),
        );

        card(ui, t, |ui| {
            ui.label(hud(t!(self.lang, "импорт", "import"), t.faint));
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.addon_import)
                        .hint_text(t!(
                            self.lang,
                            "путь к папке, addon.json, .css или .sdwaddon",
                            "a folder, addon.json, .css or .sdwaddon"
                        ))
                        .desired_width(360.0),
                );
                if primary(ui, t!(self.lang, "установить", "install"), t).clicked() {
                    let raw = expand_path(&self.addon_import);
                    match sdw_core::addons::install(&raw) {
                        Ok(addon) => {
                            let id = addon.id().to_string();
                            self.save(|c| c.addons.set(&id, true));
                            self.addon_import.clear();
                            for field in &addon.manifest.settings {
                                let key = field.key.clone();
                                let value = field.default.clone();
                                self.save(|c| {
                                    if c.addons.value(&id, &key).is_null() {
                                        c.addons.set_value(&id, &key, value);
                                    }
                                });
                            }
                            self.toast(
                                format!(
                                    "{}: {}",
                                    t!(self.lang, "аддон установлен", "addon installed"),
                                    addon.manifest.name
                                ),
                                true,
                            );
                            self.status = self.state.status();
                        }
                        Err(e) => self.toast(e, false),
                    }
                }
                if ghost(ui, t!(self.lang, "папка", "folder"), t).clicked() {
                    let _ =
                        sdw_core::util::open_url(&sdw_core::addons::root().display().to_string());
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ghost(ui, t!(self.lang, "пример", "example"), t).clicked() {
                    let dir = sdw_core::addons::root().join("example");
                    let _ = std::fs::create_dir_all(&dir);
                    let text = serde_json::to_string_pretty(&sdw_core::addons::example_manifest())
                        .unwrap_or_default();
                    match std::fs::write(dir.join(sdw_core::addons::MANIFEST), text) {
                        Ok(()) => self.toast(
                            format!("{}: {}", t!(self.lang, "пример", "example"), dir.display()),
                            true,
                        ),
                        Err(e) => self.toast(e.to_string(), false),
                    }
                }
                let path = sdw_core::addons::root();
                small(ui, &path.display().to_string(), t.faint);
            });
        });
        ui.add_space(10.0);
        self.addons_store_ui(ui, t);
        ui.add_space(10.0);

        let cfg = self.state.cfg();
        if self.status.addons.is_empty() {
            card(ui, t, |ui| {
                small(
                    ui,
                    t!(
                        self.lang,
                        "пока нет аддонов. установите папку, addon.json или .sdwaddon.",
                        "no addons yet. install a folder, addon.json or .sdwaddon."
                    ),
                    t.muted,
                );
            });
            return;
        }

        for info in self.status.addons.clone() {
            let mut on = info.enabled;
            let id = info.id.clone();
            let full = sdw_core::addons::find(&info.id);
            let mut remove = false;
            card(ui, t, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&info.name)
                                .size(17.0)
                                .strong()
                                .color(t.strong),
                        );
                        let meta = [info.version.as_str(), info.scope.as_str()]
                            .into_iter()
                            .filter(|x| !x.trim().is_empty())
                            .collect::<Vec<_>>()
                            .join(" · ");
                        if !meta.is_empty() {
                            small(ui, &meta, t.faint);
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(hud(t!(self.lang, "удалить", "remove"), t.bad))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::new(1.0, t.bad.gamma_multiply(0.4)))
                                    .corner_radius(0),
                            )
                            .clicked()
                        {
                            remove = true;
                        }
                        switch(ui, &mut on, t);
                        small(
                            ui,
                            if on {
                                t!(self.lang, "включён", "on")
                            } else {
                                t!(self.lang, "выключен", "off")
                            },
                            if on { t.good } else { t.faint },
                        );
                    });
                });
                if !info.description.is_empty() {
                    small(ui, &info.description, t.muted);
                }
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if info.has_theme {
                        tag(ui, "theme", t.accent);
                    }
                    if info.has_clock {
                        tag(ui, "clock", t.accent);
                    }
                    if info.tiles > 0 {
                        tag(ui, &format!("{} tiles", info.tiles), t.accent);
                    }
                    for action in &info.actions {
                        let label = if action.label.is_empty() {
                            action.id.clone()
                        } else {
                            action.label.clone()
                        };
                        if ghost(ui, &label, t).clicked() {
                            let state = self.state.clone();
                            let id = id.clone();
                            let aid = action.id.clone();
                            self.spawn(move || {
                                TaskResult::Msg(crate::server::run_addon_action(&state, &id, &aid))
                            });
                        }
                    }
                });
                if let Some(addon) = &full {
                    if !addon.manifest.settings.is_empty() {
                        ui.add_space(6.0);
                        ui.label(hud(
                            t!(self.lang, "настройки аддона", "addon settings"),
                            t.faint,
                        ));
                        for field in &addon.manifest.settings {
                            let key = field.key.clone();
                            let value = cfg.addons.value(&id, &key);
                            let value = if value.is_null() {
                                field.default.clone()
                            } else {
                                value
                            };
                            let label = if field.label.is_empty() {
                                key.clone()
                            } else {
                                field.label.clone()
                            };
                            ui.horizontal_wrapped(|ui| {
                                small(ui, &label, t.muted);
                                match field.kind.as_str() {
                                    "bool" => {
                                        let mut b = value.as_bool().unwrap_or(false);
                                        if switch(ui, &mut b, t).changed() {
                                            self.save(|c| {
                                                c.addons.set_value(&id, &key, serde_json::json!(b))
                                            });
                                        }
                                    }
                                    "number" => {
                                        let mut n = value.as_f64().unwrap_or(0.0);
                                        let mut drag = egui::DragValue::new(&mut n);
                                        if let Some(min) = field.min {
                                            drag = drag.range(min..=field.max.unwrap_or(f64::MAX));
                                        }
                                        if ui.add(drag).changed() {
                                            self.save(|c| {
                                                c.addons.set_value(&id, &key, serde_json::json!(n))
                                            });
                                        }
                                    }
                                    "color" => {
                                        let raw = value.as_str().unwrap_or("#3fd2ff");
                                        let mut c = Color32::from_hex(raw).unwrap_or(t.accent);
                                        if ui.color_edit_button_srgba(&mut c).changed() {
                                            let raw =
                                                format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
                                            self.save(|c| {
                                                c.addons.set_value(
                                                    &id,
                                                    &key,
                                                    serde_json::json!(raw),
                                                )
                                            });
                                        }
                                    }
                                    "choice" => {
                                        let current = value.as_str().unwrap_or("").to_string();
                                        for opt in &field.options {
                                            if ui
                                                .selectable_label(
                                                    &current == opt,
                                                    RichText::new(opt).size(12.0),
                                                )
                                                .clicked()
                                            {
                                                let opt = opt.clone();
                                                self.save(|c| {
                                                    c.addons.set_value(
                                                        &id,
                                                        &key,
                                                        serde_json::json!(opt),
                                                    )
                                                });
                                            }
                                        }
                                    }
                                    _ => {
                                        let mut text = value.as_str().unwrap_or("").to_string();
                                        if ui
                                            .add(
                                                TextEdit::singleline(&mut text)
                                                    .desired_width(220.0),
                                            )
                                            .changed()
                                        {
                                            self.save(|c| {
                                                c.addons.set_value(
                                                    &id,
                                                    &key,
                                                    serde_json::json!(text),
                                                )
                                            });
                                        }
                                    }
                                }
                                if !field.hint.is_empty() {
                                    small(ui, &field.hint, t.faint);
                                }
                            });
                        }
                    }
                }
            });
            ui.add_space(8.0);
            if on != info.enabled {
                self.save(|c| c.addons.set(&id, on));
                self.status = self.state.status();
            }
            if remove {
                let _ = sdw_core::addons::remove(&id);
                self.save(|c| {
                    c.addons.set(&id, false);
                    c.addons.values.remove(&id);
                });
                self.toast(t!(self.lang, "аддон удалён", "addon removed"), true);
                self.status = self.state.status();
            }
        }
    }

    fn audio_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        self.page_header(
            ui,
            t,
            t!(self.lang, "звук", "audio"),
            t!(
                self.lang,
                "трансляция звука компьютера или выбранных приложений на steam deck",
                "stream the computer's sound, or picked applications, to the steam deck"
            ),
        );
        let status = self.status.audio.clone();
        let cfg = self.state.cfg();

        card(ui, t, |ui| {
            row(
                ui,
                t,
                t!(self.lang, "разрешить звук", "allow audio"),
                t!(
                    self.lang,
                    "без этого демон не отдаёт ни одного сэмпла",
                    "without this the daemon never sends a sample"
                ),
                |ui| {
                    let mut on = cfg.audio.enabled;
                    if switch(ui, &mut on, t).changed() {
                        self.save(|c| c.audio.enabled = on);
                        if !on {
                            self.state.audio.stop();
                            self.status = self.state.status();
                        }
                    }
                },
            );
            row(
                ui,
                t,
                t!(self.lang, "запуск с деки", "start from the deck"),
                t!(
                    self.lang,
                    "если выключено, звук запускается только из этого окна",
                    "when off, only this window can start the stream"
                ),
                |ui| {
                    let mut on = cfg.audio.allow_remote;
                    if switch(ui, &mut on, t).changed() {
                        self.save(|c| c.audio.allow_remote = on);
                    }
                },
            );
            small(
                ui,
                t!(
                    self.lang,
                    "нужны pipewire или pulseaudio и pactl; при выборе приложений создаётся временный выход sdw_audio, а при остановке звук возвращается на прежнее устройство.",
                    "pipewire or pulseaudio and pactl are required; picking applications creates a temporary sdw_audio sink, and stopping moves the sound back."
                ),
                t.faint,
            );
        });
        ui.add_space(10.0);

        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                theme::dot(
                    ui,
                    if status.running { t.good } else { t.faint },
                    status.running,
                );
                ui.label(
                    RichText::new(if status.running {
                        t!(self.lang, "идёт на деку", "streaming to the deck")
                    } else {
                        t!(self.lang, "выключено", "off")
                    })
                    .size(17.0)
                    .strong()
                    .color(t.strong),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if status.running {
                        if ghost(ui, t!(self.lang, "остановить", "stop"), t).clicked() {
                            self.state.audio.stop();
                            self.status = self.state.status();
                        }
                    } else {
                        if primary(ui, t!(self.lang, "запустить", "start"), t).clicked() {
                            self.start_audio();
                        }
                    }
                });
            });
            if status.running {
                small(
                    ui,
                    &format!(
                        "{} · {} · {} {} · {} {} · {:.1} MB · {} s",
                        status.backend,
                        if status.source == "apps" {
                            t!(self.lang, "приложения", "applications")
                        } else {
                            t!(self.lang, "весь пк", "whole pc")
                        },
                        status.rate,
                        t!(self.lang, "гц", "hz"),
                        status.channels,
                        t!(self.lang, "кан", "ch"),
                        status.bytes as f64 / 1_048_576.0,
                        status.seconds
                    ),
                    t.muted,
                );
            } else if !status.available {
                small(
                    ui,
                    t!(
                        self.lang,
                        "в системе нет pactl/parec — установите pipewire-pulse или pulseaudio-utils.",
                        "pactl/parec are missing — install pipewire-pulse or pulseaudio-utils."
                    ),
                    t.warn,
                );
            }
            if !status.error.is_empty() {
                small(ui, &status.error, t.bad);
            }
        });
        ui.add_space(10.0);

        card(ui, t, |ui| {
            ui.label(hud(t!(self.lang, "источник", "source"), t.faint));
            ui.horizontal_wrapped(|ui| {
                for (key, ru, en) in [
                    ("system", "весь компьютер", "whole computer"),
                    ("apps", "приложения", "applications"),
                ] {
                    let active = self.audio_source == key;
                    if ui
                        .add(
                            egui::Button::new(theme::nav_text(
                                "",
                                t!(self.lang, ru, en),
                                12.0,
                                if active { t.strong } else { t.muted },
                            ))
                            .fill(if active {
                                t.accent.gamma_multiply(0.22)
                            } else {
                                t.well
                            })
                            .stroke(Stroke::new(1.0, if active { t.accent } else { t.line }))
                            .corner_radius(0),
                        )
                        .clicked()
                    {
                        self.audio_source = key.to_string();
                    }
                }
                if ghost(ui, t!(self.lang, "обновить", "refresh"), t).clicked() {
                    self.audio_apps = self.state.audio.list_apps();
                    self.audio_apps_at = Instant::now();
                }
            });
            if self.audio_source == "apps" {
                if self.audio_apps_at.elapsed() > Duration::from_secs(10) {
                    self.audio_apps = self.state.audio.list_apps();
                    self.audio_apps_at = Instant::now();
                }
                ui.add_space(4.0);
                if self.audio_apps.is_empty() {
                    small(
                        ui,
                        t!(
                            self.lang,
                            "сейчас никто не играет звук.",
                            "nothing is playing right now."
                        ),
                        t.muted,
                    );
                } else {
                    for app in self.audio_apps.clone() {
                        let mut on = self.audio_app_sel.contains(&app.name);
                        if ui
                            .add(
                                egui::Button::new(RichText::new(format!(
                                    "{}  {}",
                                    app.name, app.binary
                                )))
                                .fill(if on {
                                    t.accent.gamma_multiply(0.22)
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .stroke(Stroke::new(1.0, if on { t.accent } else { t.line }))
                                .corner_radius(0),
                            )
                            .clicked()
                        {
                            on = !on;
                            if on {
                                self.audio_app_sel.insert(app.name.clone());
                            } else {
                                self.audio_app_sel.remove(&app.name);
                            }
                        }
                    }
                }
            }
        });
    }

    fn start_audio(&mut self) {
        let cfg = self.state.cfg();
        let apps: Vec<String> = self.audio_app_sel.iter().cloned().collect();
        let req = AudioStart {
            source: self.audio_source.clone(),
            apps,
            rate: cfg.audio.rate,
            channels: cfg.audio.channels,
        };
        match self.state.audio.start(&cfg.audio, &req, false) {
            Ok(_) => {
                self.toast(
                    t!(self.lang, "звук идёт на деку", "audio is on the deck"),
                    true,
                );
                self.status = self.state.status();
            }
            Err(e) => self.toast(e, false),
        }
    }

    fn settings_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        let cfg = self.state.cfg();
        self.page_header(
            ui,
            t,
            t!(self.lang, "настройки", "settings"),
            t!(
                self.lang,
                "имя, язык, тема и запуск",
                "name, language, theme and startup"
            ),
        );
        card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(self.lang, "компьютер", "computer"))
                    .size(15.0)
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(hud(t!(self.lang, "имя в сети", "network name"), t.faint));
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut self.name_edit).desired_width(320.0));
                if ui
                    .add(egui::Button::new(hud(
                        t!(self.lang, "сохранить", "save"),
                        t.muted,
                    )))
                    .clicked()
                {
                    let name = self.name_edit.trim().to_string();
                    if !name.is_empty() {
                        self.save(|c| c.name = name);
                        self.toast(t!(self.lang, "имя сохранено", "name saved"), true);
                    }
                }
            });
            ui.add_space(8.0);
            divider(ui);
            row(ui, t, t!(self.lang, "язык", "language"), "", |ui| {
                if ui
                    .selectable_label(self.lang == Lang::Ru, "русский")
                    .clicked()
                {
                    self.set_lang(Lang::Ru);
                }
                if ui
                    .selectable_label(self.lang == Lang::En, "english")
                    .clicked()
                {
                    self.set_lang(Lang::En);
                }
            });
            row(ui, t, t!(self.lang, "тема", "theme"), "", |ui| {
                if ui
                    .selectable_label(self.dark, t!(self.lang, "тёмная", "dark"))
                    .clicked()
                {
                    self.set_dark(true);
                }
                if ui
                    .selectable_label(!self.dark, t!(self.lang, "светлая", "light"))
                    .clicked()
                {
                    self.set_dark(false);
                }
            });
            row(
                ui,
                t,
                t!(
                    self.lang,
                    "запускать viiper автоматически",
                    "start viiper automatically"
                ),
                "",
                |ui| {
                    let mut on = cfg.viiper.auto_start;
                    if switch(ui, &mut on, t).changed() {
                        self.save(|c| c.viiper.auto_start = on);
                    }
                },
            );
            row(
                ui,
                t,
                t!(
                    self.lang,
                    "запускать с системой",
                    "start with the system"
                ),
                t!(
                    self.lang,
                    "запись в автозапуске и строка в конфиге niri/hyprland; окно при входе скрыто",
                    "an autostart entry and a line in the niri/hyprland config; the window starts hidden"
                ),
                |ui| {
                    let mut on = cfg.autostart;
                    if switch(ui, &mut on, t).changed() {
                        self.save(|c| c.autostart = on);
                        let logger = self.state.logger.clone();
                        autostart::apply(on, &logger);
                    }
                },
            );
            row(
                ui,
                t,
                t!(self.lang, "иконка в трее", "tray icon"),
                t!(
                    self.lang,
                    "применяется после перезапуска демона",
                    "applies after the daemon restarts"
                ),
                |ui| {
                    let mut on = cfg.tray;
                    if switch(ui, &mut on, t).changed() {
                        self.save(|c| c.tray = on);
                    }
                },
            );
            row(
                ui,
                t,
                t!(self.lang, "работать в фоне", "keep running in the background"),
                t!(
                    self.lang,
                    "окно скрывается: демон, API и дека продолжают работать; второе нажатие на крестик остановит",
                    "the window hides: daemon, API and Deck keep working; a second close press stops it"
                ),
                |ui| {
                    if ghost(ui, t!(self.lang, "остановить демон", "stop the daemon"), t)
                        .clicked()
                    {
                        config::request("quit");
                        self.toast(
                            t!(self.lang, "демон останавливается", "the daemon is stopping"),
                            true,
                        );
                    }
                },
            );
            ui.add_space(8.0);
            divider(ui);
            ui.horizontal(|ui| {
                ui.label(hud(
                    t!(self.lang, "папка конфига", "config folder"),
                    t.faint,
                ));
                mono(
                    ui,
                    &sdw_core::util::config_dir().display().to_string(),
                    t.muted,
                );
            });
            ui.horizontal(|ui| {
                if ghost(ui, t!(self.lang, "открыть папку", "open folder"), t).clicked()
                {
                    let _ = sdw_core::util::open_url(
                        &sdw_core::util::config_dir().display().to_string(),
                    );
                }
                if ghost(ui, t!(self.lang, "открыть журнал", "open log"), t).clicked()
                {
                    let _ = sdw_core::util::open_url(&config::log_path().display().to_string());
                }
                if ghost(
                    ui,
                    t!(self.lang, "перезапустить viiper", "restart viiper"),
                    t,
                )
                .clicked()
                {
                    self.start_viiper(true);
                }
            });
        });
        ui.add_space(10.0);
        self.updates_card(ui, t);
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("sd-widgets").size(15.0).strong());
                tag(ui, &format!("v{}", VERSION), t.muted);
            });
            small(
                ui,
                &format!(
                    "{} · {} · {}",
                    cfg.daemon_id,
                    platform(),
                    t!(self.lang, "лицензия mit", "mit license")
                ),
                t.faint,
            );
            ui.add_space(4.0);
            if ui
                .add(egui::Button::new(hud(
                    t!(self.lang, "страница проекта", "project page"),
                    t.faint,
                )))
                .clicked()
            {
                let _ = sdw_core::util::open_url("https://git.entitybtw.ru/entitybtw/sd-widgets");
            }
        });
    }

    fn logs_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        let s = self.status.clone();
        self.page_header(
            ui,
            t,
            t!(self.lang, "журнал", "log"),
            t!(
                self.lang,
                "последние события демона",
                "recent daemon events"
            ),
        );
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut self.logs_follow,
                t!(self.lang, "следовать за концом", "follow the end"),
            );
            if ghost(ui, t!(self.lang, "обновить", "refresh"), t).clicked() {
                self.last_poll = Instant::now() - Duration::from_secs(2);
            }
        });
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(t.well)
            .stroke(t.hairline())
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .max_height(560.0)
                    .stick_to_bottom(self.logs_follow)
                    .show(ui, |ui| {
                        for e in &s.log_tail {
                            let color = match e.level.as_str() {
                                "error" => t.bad,
                                "warn" => t.warn,
                                _ => t.muted,
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{:>5}", e.level))
                                        .family(FontFamily::Monospace)
                                        .size(11.0)
                                        .color(color),
                                );
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&e.message)
                                            .family(FontFamily::Monospace)
                                            .size(11.5)
                                            .color(t.ink),
                                    )
                                    .wrap(),
                                );
                            });
                        }
                    });
            });
    }
}

// ---------------------------------------------------------------- helpers

fn ago(ts: u64) -> String {
    if ts == 0 {
        return "—".into();
    }
    let d = now_ms().saturating_sub(ts);
    if d < 60_000 {
        format!("{}с", d / 1000)
    } else if d < 3_600_000 {
        format!("{}м", d / 60_000)
    } else if d < 86_400_000 {
        format!("{}ч", d / 3_600_000)
    } else {
        format!("{}д", d / 86_400_000)
    }
}

fn backend_label(l: Lang, b: MonitorBackend) -> &'static str {
    match b {
        MonitorBackend::Auto => t!(l, "авто", "auto"),
        MonitorBackend::Hyprland => "hyprland headless",
        MonitorBackend::Vkms => "vkms",
        MonitorBackend::Krfb => "kde krfb",
        MonitorBackend::Mirror => t!(l, "зеркало", "mirror"),
        MonitorBackend::X11 => "x11",
        MonitorBackend::Gdigrab => "windows gdigrab",
    }
}

fn pad_hint(l: Lang, k: PadKind) -> &'static str {
    match k {
        PadKind::Xbox360 => t!(
            l,
            "самый совместимый: xinput, steam и почти все игры.",
            "the most compatible: xinput, steam and almost every game."
        ),
        PadKind::DualShock4 => t!(
            l,
            "ps4-пад, гироскоп и тачпад.",
            "ps4 pad, gyro and touchpad."
        ),
        PadKind::DualSense => t!(
            l,
            "ps5-пад, гироскоп и тачпад.",
            "ps5 pad, gyro and touchpad."
        ),
        PadKind::SwitchPro => t!(
            l,
            "nintendo switch 2 pro, гироскоп.",
            "nintendo switch 2 pro, gyro."
        ),
    }
}

fn state_label(e: &HaEntity, l: Lang) -> String {
    match e.state.as_str() {
        "on" => t!(l, "включено", "on").into(),
        "off" => t!(l, "выключено", "off").into(),
        "unavailable" => t!(l, "недоступно", "unavailable").into(),
        "unknown" => t!(l, "неизвестно", "unknown").into(),
        "home" => t!(l, "дома", "home").into(),
        "not_home" => t!(l, "не дома", "away").into(),
        "playing" => t!(l, "играет", "playing").into(),
        "paused" => t!(l, "пауза", "paused").into(),
        other => {
            let unit = e.unit();
            if unit.is_empty() {
                other.to_string()
            } else {
                format!("{other} {unit}")
            }
        }
    }
}

fn expand_path(raw: &str) -> std::path::PathBuf {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("~/") {
        return std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join(rest))
            .unwrap_or_else(|| std::path::PathBuf::from(raw));
    }
    std::path::PathBuf::from(raw)
}

fn power_label(lang: Lang, action: PowerAction) -> &'static str {
    match action {
        PowerAction::Lock => t!(lang, "заблокировать", "lock"),
        PowerAction::Sleep => t!(lang, "сон", "sleep"),
        PowerAction::Reboot => t!(lang, "перезагрузка", "restart"),
        PowerAction::Shutdown => t!(lang, "выключить", "shut down"),
        PowerAction::ScreenOff => t!(lang, "погасить экран", "screen off"),
    }
}
