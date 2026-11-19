//! the daemon window: setup, overview, gamepads, screen, notifications, home assistant,
//! security, install and logs. one column of hairline cards, nothing rounded.

mod theme;

use crate::config::{self, DaemonConfig};
use crate::install;
use crate::state::AppState;
use eframe::egui::{
    self, Align, Color32, FontFamily, Layout, Margin, RichText, ScrollArea, Stroke, TextEdit, Ui,
    Vec2,
};
use sdw_core::model::*;
use sdw_core::{t, Lang};
use std::collections::HashSet;
use sdw_core::update::{self, UpdateInfo};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use theme::{card, divider, ghost, hud, mono, primary, row, small, switch, tag, Tokens};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Gamepads,
    Screen,
    Notifications,
    HomeAssistant,
    Security,
    Install,
    Settings,
    Logs,
}

impl Page {
    const ALL: [Page; 9] = [
        Page::Overview,
        Page::Gamepads,
        Page::Screen,
        Page::Notifications,
        Page::HomeAssistant,
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
            Page::Notifications => t!(l, "уведомления", "notifications"),
            Page::HomeAssistant => t!(l, "home assistant", "home assistant"),
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
            Page::Notifications => theme::icon::NOTIFICATIONS,
            Page::HomeAssistant => theme::icon::HOME_ASSISTANT,
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

    // notifications
    notif_enabled: bool,

    // home assistant
    ha_url: String,
    ha_token: String,
    ha_show_token: bool,
    ha_search: String,
    ha_entities: Vec<HaEntity>,
    last_ha_fetch: Instant,

    // security
    new_pass: String,
    new_pass2: String,
    show_pass: bool,
    new_token: Option<String>,

    // settings
    name_edit: String,

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
}

pub fn run(state: Arc<AppState>) -> eframe::Result<()> {
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
    eframe::run_native(
        "sd-widgets",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            let app = App::new(state);
            theme::apply(&cc.egui_ctx, app.dark);
            Ok(Box::new(app))
        }),
    )
}

impl App {
    fn new(state: Arc<AppState>) -> Self {
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
            notif_enabled,
            ha_url: cfg.ha.url.clone(),
            ha_token: cfg.ha.token.clone(),
            ha_show_token: false,
            ha_search: String::new(),
            ha_entities: Vec::new(),
            last_ha_fetch: Instant::now() - Duration::from_secs(60),
            new_pass: String::new(),
            new_pass2: String::new(),
            show_pass: false,
            new_token: None,
            name_edit: cfg.name.clone(),
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

    fn poll(&mut self) {
        if self.last_poll.elapsed() >= Duration::from_millis(900) {
            self.last_poll = Instant::now();
            self.status = self.state.status();
            if self.toast.as_ref().is_some_and(|t| t.at.elapsed() > Duration::from_secs(6)) {
                self.toast = None;
            }
        }
        if !self.update_checking
            && self.last_update_check.elapsed() > Duration::from_secs(6 * 3600)
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
        self.toast(if restart { "перезапускаю viiper…" } else { "запускаю viiper…" }, true);
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
        }
        self.last_ha_fetch = Instant::now();
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
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
                        if self.status.viiper.running { t.good } else { t.warn },
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
                            .add(egui::Button::new(hud(if self.dark { "light" } else { "dark" }, t.muted)))
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
                        let addr = self
                            .status
                            .addresses
                            .first()
                            .cloned()
                            .unwrap_or_default();
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
                                self.toast(t!(self.lang, "адрес скопирован", "address copied"), true);
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
                                &format!("{} {}", self.status.pads.len(), t!(self.lang, "падов", "pads")),
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
                .frame(egui::Frame::new().fill(t.bg).inner_margin(Margin::symmetric(22, 16)))
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
                    .fill(if selected { t.well } else { Color32::TRANSPARENT })
                    .stroke(Stroke::new(
                        1.0,
                        if selected { t.line_strong } else { Color32::TRANSPARENT },
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
                let _ = sdw_core::util::open_url(
                    &sdw_core::util::config_dir().display().to_string(),
                );
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
            Page::Notifications => self.notifications_ui(ui, t),
            Page::HomeAssistant => self.ha_ui(ui, t),
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
                if ui.add(egui::Button::new(hud(self.lang.short(), t.muted))).clicked() {
                    self.set_lang(self.lang.toggle());
                }
                if ui
                    .add(egui::Button::new(hud(if self.dark { "light" } else { "dark" }, t.muted)))
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
            ui.label(RichText::new(t!(self.lang, "компьютерная часть", "the computer side")).size(19.0).strong());
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
                if ui.selectable_label(self.lang == Lang::Ru, "русский").clicked() {
                    self.set_lang(Lang::Ru);
                }
                if ui.selectable_label(self.lang == Lang::En, "english").clicked() {
                    self.set_lang(Lang::En);
                }
                ui.add_space(16.0);
                ui.label(hud(t!(self.lang, "тема", "theme"), t.faint));
                if ui.selectable_label(self.dark, t!(self.lang, "тёмная", "dark")).clicked() {
                    self.set_dark(true);
                }
                if ui.selectable_label(!self.dark, t!(self.lang, "светлая", "light")).clicked() {
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
            ui.label(RichText::new(t!(self.lang, "имя и пароль", "name and password")).size(18.0).strong());
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
            ui.label(hud(t!(self.lang, "имя компьютера", "computer name"), t.faint));
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
                small(ui, t!(self.lang, "минимум 4 символа", "at least 4 characters"), t.bad);
            }
            if !self.setup_pass2.is_empty() && self.setup_pass != self.setup_pass2 {
                small(ui, t!(self.lang, "пароли не совпадают", "passwords do not match"), t.bad);
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ghost(ui, t!(self.lang, "назад", "back"), t).clicked() {
                self.setup_step = 0;
            }
            let valid = self.setup_pass.chars().count() >= 4 && self.setup_pass == self.setup_pass2;
            if ui.add_enabled(valid, egui::Button::new(
                RichText::new(t!(self.lang, "далее", "next")).size(13.0).strong().color(t.inv_fg),
            ).fill(t.inv_bg).stroke(Stroke::new(1.0, t.inv_bg)).corner_radius(0).min_size(Vec2::new(0.0,30.0))).clicked() {
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
                    if v.running { t.good } else if v.installed { t.warn } else { t.bad },
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
                if primary(ui, t!(self.lang, "установить viiper", "install viiper"), t).clicked() {
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
            ui.label(hud(t!(self.lang, "пароль viiper (необязательно)", "viiper password (optional)"), t.faint));
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.setup_viiper_pass)
                        .password(!self.viiper_show)
                        .hint_text(t!(self.lang, "оставьте пустым, если без пароля", "leave empty for no password"))
                        .desired_width(320.0),
                );
                if ui.add(egui::Button::new(hud(
                    if self.viiper_show { t!(self.lang, "скрыть", "hide") } else { t!(self.lang, "показать", "show") },
                    t.muted,
                ))).clicked() {
                    self.viiper_show = !self.viiper_show;
                }
                if ui.add(egui::Button::new(hud(t!(self.lang, "сохранить", "save"), t.muted))).clicked() {
                    let pass = self.setup_viiper_pass.trim().to_string();
                    let _ = self.state.viiper.write_key(&pass);
                    self.save(|c| c.viiper.password = pass);
                    self.toast(t!(self.lang, "пароль viiper сохранён", "viiper password saved"), true);
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
            if ui.add(egui::Button::new(hud(t!(self.lang, "пропустить", "skip"), t.faint))).clicked() {
                self.setup_step = 3;
            }
        });
    }

    fn setup_screen(&mut self, ui: &mut Ui, t: &Tokens) {
        card(ui, t, |ui| {
            ui.label(RichText::new(t!(self.lang, "дополнительный экран", "extra screen")).size(18.0).strong());
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
                    if ui.selectable_label(self.setup_preset == i, *label).clicked() {
                        self.setup_preset = i;
                        self.screen.width = *w;
                        self.screen.height = *h;
                    }
                }
            });
            ui.add_space(8.0);
            ui.checkbox(
                &mut self.setup_start_screen,
                t!(self.lang, "запустить экран после настройки", "start the screen after setup"),
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
            ui.label(RichText::new(t!(self.lang, "всё готово", "all set")).size(20.0).strong());
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
        if primary(ui, t!(self.lang, "открыть панель", "open the panel"), t).clicked() {
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
                        &format!("{} {}", s.connected_clients, t!(self.lang, "клиентов", "clients")),
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
                            if ghost(ui, t!(self.lang, "перезапуск", "restart"), t).clicked() {
                                self.start_viiper(true);
                            }
                        } else if s.viiper.installed {
                            if primary(ui, t!(self.lang, "запустить", "start"), t).clicked() {
                                self.start_viiper(false);
                            }
                        } else if primary(ui, t!(self.lang, "установить", "install"), t).clicked() {
                            go = Some(Page::Install);
                        }
                        if ui.add(egui::Button::new(hud(t!(self.lang, "подробнее", "details"), t.faint))).clicked() {
                            go = Some(Page::Gamepads);
                        }
                    });
                });

                // screen
                card(ui, t, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t!(self.lang, "экран", "screen")).size(15.0).strong());
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
                                s.monitor.backend_label, s.monitor.width, s.monitor.height, s.monitor.output
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
                            if ghost(ui, t!(self.lang, "остановить", "stop"), t).clicked() {
                                self.state.monitor.stop();
                            }
                        } else if primary(ui, t!(self.lang, "настроить", "set up"), t).clicked() {
                            go = Some(Page::Screen);
                        }
                        if ui.add(egui::Button::new(hud(t!(self.lang, "открыть", "open"), t.faint))).clicked() {
                            go = Some(Page::Screen);
                        }
                    });
                });
                ui.end_row();

                // notifications
                card(ui, t, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t!(self.lang, "уведомления", "notifications")).size(15.0).strong());
                        tag(
                            ui,
                            &format!("{}", s.notifications.count),
                            if s.notifications.listening { t.good } else { t.warn },
                        );
                    });
                    small(
                        ui,
                        if s.notifications.listening {
                            t!(self.lang, "зеркалирование включено", "mirroring is on")
                        } else {
                            t!(self.lang, "ожидает демон уведомлений", "waiting for a notification daemon")
                        },
                        t.muted,
                    );
                    ui.add_space(6.0);
                    for n in s.notifications.recent.iter().take(2) {
                        small(ui, &format!("{} · {}", n.app, sdw_core::util::truncate(&n.summary, 44)), t.ink);
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ghost(ui, t!(self.lang, "проверить", "test"), t).clicked() {
                            self.state.notifications.add_test();
                        }
                        if ui.add(egui::Button::new(hud(t!(self.lang, "все", "all"), t.faint))).clicked() {
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
                            if s.ha.connected { t.good } else if s.ha.configured { t.bad } else { t.muted },
                        );
                    });
                    small(
                        ui,
                        &if s.ha.configured {
                            format!("{} · {} {}", s.ha.url, s.ha.entities, t!(self.lang, "сущностей", "entities"))
                        } else {
                            t!(self.lang, "свет, сцены и датчики на деке", "lights, scenes and sensors on the deck").to_string()
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
                ui.label(RichText::new(t!(self.lang, "подключённые клиенты", "paired clients")).size(15.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(hud(t!(self.lang, "безопасность", "security"), t.faint))).clicked() {
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
                    if s.viiper.password_set { t.good } else { t.muted },
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
                mono(ui, &format!("usbip {}", if s.viiper.usbip_installed { "ok" } else { "missing" }), if s.viiper.usbip_installed { t.good } else { t.bad });
                if !s.viiper.version.is_empty() {
                    ui.add_space(10.0);
                    mono(ui, &s.viiper.version, t.faint);
                }
            });
            if !s.viiper.error.is_empty() {
                small(ui, &s.viiper.error, t.bad);
            }
            ui.add_space(8.0);
            ui.label(hud(t!(self.lang, "пароль viiper", "viiper password"), t.faint));
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.viiper_pass)
                        .password(!self.viiper_show)
                        .desired_width(280.0),
                );
                if ui.add(egui::Button::new(hud(if self.viiper_show { "hide" } else { "show" }, t.muted))).clicked() {
                    self.viiper_show = !self.viiper_show;
                }
                if ui.add(egui::Button::new(hud(t!(self.lang, "сохранить", "save"), t.muted))).clicked() {
                    let pass = self.viiper_pass.trim().to_string();
                    let _ = self.state.viiper.write_key(&pass);
                    self.save(|c| c.viiper.password = pass);
                    self.toast(t!(self.lang, "пароль viiper сохранён", "viiper password saved"), true);
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
            ui.label(RichText::new(t!(self.lang, "новый пад", "new pad")).size(15.0).strong());
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
                    self.spawn(move || TaskResult::Pad(state.viiper.create_pad(&state.cfg(), &name, kind)));
                }
            });
            small(ui, pad_hint(self.lang, self.pad_kind), t.faint);
        });

        if let Some(p) = self.created_pad.clone() {
            ui.add_space(10.0);
            card(ui, t, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t!(self.lang, "пад создан", "pad created")).size(15.0).strong().color(t.good));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(theme::icon(theme::icon::CLOSE, 13.0, t.muted))).clicked() {
                            self.created_pad = None;
                        }
                    });
                });
                mono(ui, &format!("{} · {} · bus {} · {}", p.pad.name, p.pad.kind.label(), p.pad.bus_id, p.pad.dev_id), t.ink);
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
                    let secret = if self.show_pad_secret { p.udp_secret.clone() } else { "••••••••••••".into() };
                    mono(ui, &secret, t.accent_ink);
                    if ui.add(egui::Button::new(hud(if self.show_pad_secret { "hide" } else { "show" }, t.muted))).clicked() {
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
                            if ui.add(egui::Button::new(hud(t!(self.lang, "удалить", "remove"), t.bad))).clicked() {
                                let id = pad.id.clone();
                                let state = self.state.clone();
                                self.spawn(move || {
                                    TaskResult::Msg(state.viiper.delete_pad(&id).map(|_| "пад удалён".into()))
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
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(60.0, 8.0), egui::Sense::hover());
                                ui.painter().rect_filled(rect, 0.0, t.well);
                                let w = rect.width() * (v as f32 / 255.0);
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_size(rect.min, egui::vec2(w, rect.height())),
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
                    ui.label(RichText::new(t!(self.lang, "в эфире", "live")).size(16.0).strong().color(t.good));
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
                    ui.painter().rect_stroke(rect, 0.0, t.hairline(), egui::StrokeKind::Inside);
                    if s.monitor.input_enabled && resp.clicked() {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            let x = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                            let y = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
                            let state = self.state.clone();
                            let input = MonitorInput { x, y, action: "click".into(), amount: 0 };
                            self.spawn(move || TaskResult::Msg(state.monitor.input(&input).map(|_| "клик отправлен".into())));
                        }
                    }
                    small(
                        ui,
                        if s.monitor.input_enabled {
                            t!(self.lang, "клик по превью = клик на компьютере", "click the preview = click on the computer")
                        } else {
                            t!(self.lang, "управление мышью выключено", "pointer control is off")
                        },
                        t.faint,
                    );
                } else {
                    small(ui, t!(self.lang, "ждём первый кадр…", "waiting for the first frame…"), t.muted);
                }
            });
            ui.add_space(10.0);
        }

        // questionnaire
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!(self.lang, "мастер", "wizard")).size(16.0).strong());
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
                    small(ui, t!(self.lang, "что нужно сделать?", "what should the extra screen do?"), t.muted);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        for (mode, title, hint) in [
                            (
                                MonitorMode::Extend,
                                t!(self.lang, "дополнительный монитор", "extra monitor"),
                                t!(self.lang, "второй рабочий стол справа от основного", "a second desktop to the right"),
                            ),
                            (
                                MonitorMode::Mirror,
                                t!(self.lang, "зеркало", "mirror"),
                                t!(self.lang, "копия основного экрана", "a copy of the main screen"),
                            ),
                        ] {
                            let selected = self.screen.mode == mode;
                            egui::Frame::new()
                                .fill(if selected { t.well } else { t.bg })
                                .stroke(Stroke::new(1.0, if selected { t.line_strong } else { t.line }))
                                .inner_margin(Margin::same(12))
                                .show(ui, |ui| {
                                    ui.set_width(260.0);
                                    ui.vertical(|ui| {
                                        if ui.selectable_label(selected, RichText::new(title).size(14.0).strong()).clicked() {
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
                            if ui.selectable_label(self.screen.preset == i, *label).clicked() {
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
                                        MonitorBackend::MacCapture,
                                    ] {
                                        ui.selectable_value(&mut self.screen.backend, b, backend_label(self.lang, b));
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
                                    ui.selectable_value(&mut self.screen.output, String::new(), t!(self.lang, "авто", "auto"));
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
                        ui.add(egui::Slider::new(&mut self.screen.quality, 30..=95).show_value(true));
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.screen.input, t!(self.lang, "управление мышью с дека", "pointer control from the deck"));
                    });
                    if matches!(self.screen.backend, MonitorBackend::Vkms | MonitorBackend::Auto)
                        && s.monitor.compositor == "niri"
                    {
                        ui.add_space(6.0);
                        ui.label(hud(t!(self.lang, "пароль sudo для vkms", "sudo password for vkms"), t.faint));
                        ui.add(
                            TextEdit::singleline(&mut self.screen.sudo)
                                .password(true)
                                .hint_text(t!(self.lang, "нужен для modprobe vkms", "needed for modprobe vkms"))
                                .desired_width(300.0),
                        );
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
                        if primary(ui, t!(self.lang, "запустить экран", "start the screen"), t).clicked() {
                            self.start_screen();
                        }
                        if ui.add(egui::Button::new(hud(t!(self.lang, "подсказка по конфигу", "config hint"), t.faint))).clicked() {
                            self.show_hint = !self.show_hint;
                        }
                        if ui.add(egui::Button::new(hud(t!(self.lang, "обновить выходы", "refresh outputs"), t.faint))).clicked() {
                            self.state.monitor.refresh();
                            self.toast(t!(self.lang, "выходы обновлены", "outputs refreshed"), true);
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
                                    ui.label(hud(t!(self.lang, "что добавить в конфиг", "what to add to the config"), t.muted));
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
                    && ui.add(egui::Button::new(hud(t!(self.lang, "назад", "back"), t.muted))).clicked()
                {
                    self.screen.step -= 1;
                }
                if self.screen.step < 3
                    && primary(ui, t!(self.lang, "далее", "next"), t).clicked()
                {
                    self.screen.step += 1;
                }
            });
        });

        ui.add_space(10.0);
        card(ui, t, |ui| {
            ui.label(RichText::new(t!(self.lang, "инструменты", "tools")).size(15.0).strong());
            ui.add_space(4.0);
            egui::Grid::new("tools").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                for tool in &s.monitor.tools {
                    ui.horizontal(|ui| {
                        theme::dot(ui, if tool.installed { t.good } else { t.bad }, tool.installed);
                        ui.label(RichText::new(&tool.name).family(FontFamily::Monospace).size(12.0).color(t.ink));
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
                    t!(self.lang, "пока пусто. уведомления появятся здесь.", "nothing yet. notifications will appear here."),
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
                            if n.urgency >= 2 { t.bad } else if n.urgency == 1 { t.warn } else { t.accent },
                            true,
                        );
                        ui.label(RichText::new(&n.app).size(12.5).strong().color(t.strong));
                        small(ui, &ago(n.ts), t.faint);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(egui::Button::new(theme::icon(theme::icon::CLOSE, 13.0, t.muted))).clicked() {
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
                    if s.ha.connected { t.good } else if s.ha.configured { t.bad } else { t.muted },
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
            ui.label(hud(t!(self.lang, "долгоживущий токен", "long-lived token"), t.faint));
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.ha_token)
                        .password(!self.ha_show_token)
                        .desired_width(420.0),
                );
                if ui.add(egui::Button::new(hud(if self.ha_show_token { "hide" } else { "show" }, t.muted))).clicked() {
                    self.ha_show_token = !self.ha_show_token;
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if primary(ui, t!(self.lang, "подключить", "connect"), t).clicked() {
                    let state = self.state.clone();
                    let url = self.ha_url.trim().to_string();
                    let token = self.ha_token.trim().to_string();
                    self.toast(t!(self.lang, "проверяю home assistant…", "checking home assistant…"), true);
                    self.spawn(move || {
                        let mut cfg = state.cfg();
                        let fav = cfg.ha.favorites.clone();
                        let res = state
                            .ha
                            .lock()
                            .map(|mut h| h.set(&mut cfg, &url, &token, &fav))
                            .unwrap_or(Err("home assistant lock".into()));
                        if res.is_ok() {
                            let url = cfg.ha.url.clone();
                            let token = cfg.ha.token.clone();
                            let _ = state.update_cfg(|c| {
                                c.ha.url = url;
                                c.ha.token = token;
                            });
                        }
                        TaskResult::Msg(res.map(|_| "home assistant подключён".into()))
                    });
                }
                if ghost(ui, t!(self.lang, "проверить", "test"), t).clicked() {
                    let state = self.state.clone();
                    self.spawn(move || {
                        let res = state.ha.lock().map(|mut h| h.refresh()).unwrap_or(Err("lock".into()));
                        TaskResult::Msg(res.map(|_| "home assistant отвечает".into()))
                    });
                }
                if !s.ha.error.is_empty() {
                    small(ui, &s.ha.error, if s.ha.connected { t.good } else { t.bad });
                }
            });
        });

        ui.add_space(10.0);
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!(self.lang, "сущности", "entities")).size(15.0).strong());
                ui.add(
                    TextEdit::singleline(&mut self.ha_search)
                        .hint_text(t!(self.lang, "поиск", "search"))
                        .desired_width(220.0),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(hud(t!(self.lang, "обновить", "refresh"), t.faint))).clicked() {
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
                ui.label(hud(t!(self.lang, "всё остальное", "everything else"), t.faint));
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
                        if ghost(ui, if on { t!(self.lang, "выкл", "off") } else { t!(self.lang, "вкл", "on") }, t).clicked() {
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
                if ui.add(egui::Slider::new(&mut v, 0.0..=100.0).show_value(false)).changed() {
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
            ui.label(RichText::new(t!(self.lang, "сменить пароль", "change password")).size(15.0).strong());
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
                if ui.add_enabled(
                    valid,
                    egui::Button::new(hud(t!(self.lang, "сменить", "change"), t.muted)),
                ).clicked() {
                    let pass = self.new_pass.clone();
                    self.save(|c| c.set_password(&pass));
                    self.new_pass.clear();
                    self.new_pass2.clear();
                    self.toast(t!(self.lang, "пароль изменён. клиенты должны спариться заново.", "password changed. clients must pair again."), true);
                }
            });
            ui.add_space(8.0);
            divider(ui);
            row(
                ui,
                t,
                t!(self.lang, "доступ из публичных сетей", "public network access"),
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
                ui.label(RichText::new(t!(self.lang, "токен api", "api token")).size(15.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost(ui, t!(self.lang, "создать новый", "rotate"), t).clicked() {
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
            ui.label(RichText::new(t!(self.lang, "клиенты", "clients")).size(15.0).strong());
            if cfg.clients.is_empty() {
                small(ui, t!(self.lang, "никого", "none"), t.muted);
            } else {
                for c in &cfg.clients {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&c.name).size(13.0).color(t.ink));
                        small(ui, &format!("{} · {}", c.id, ago(c.last_seen_ms)), t.faint);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(egui::Button::new(hud(t!(self.lang, "забыть", "forget"), t.bad))).clicked() {
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
            ui.label(RichText::new(t!(self.lang, "сброс настройки", "reset setup")).size(15.0).strong());
            small(
                ui,
                t!(
                    self.lang,
                    "удаляет пароль сопряжения и снова открывает мастер.",
                    "removes the pairing password and opens the wizard again."
                ),
                t.muted,
            );
            if ghost(ui, t!(self.lang, "сбросить пароль", "reset password"), t).clicked() {
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
            ui.label(RichText::new(t!(self.lang, "компоненты", "components")).size(15.0).strong());
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
            ui.label(hud(t!(self.lang, "пароль sudo (для пакетов)", "sudo password (for packages)"), t.faint));
            ui.add(
                TextEdit::singleline(&mut self.install_sudo)
                    .password(true)
                    .hint_text(t!(self.lang, "нужен для pacman/apt", "needed for pacman/apt"))
                    .desired_width(320.0),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if job.running {
                    small(ui, &format!("{}…", job.name), t.warn);
                } else if primary(ui, t!(self.lang, "установить выбранное", "install selected"), t).clicked() {
                    let components: Vec<String> = self.install_sel.iter().cloned().collect();
                    let sudo = self.install_sudo.clone();
                    let logger = self.state.logger.clone();
                    let jobs = self.state.jobs.clone();
                    let _ = jobs.start("установка", move |ctx| {
                        let _ = install::install(&components, &sudo, &ctx, &logger);
                    });
                }
                if ui.add(egui::Button::new(hud(t!(self.lang, "обновить", "refresh"), t.faint))).clicked() {
                    self.install_components = install::component_status();
                }
            });
        });
        ui.add_space(10.0);
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!(self.lang, "журнал установки", "install log")).size(15.0).strong());
                if job.running {
                    tag(ui, t!(self.lang, "идёт", "running"), t.warn);
                } else if job.done.is_some() {
                    tag(ui, t!(self.lang, "готово", "done"), t.good);
                }
            });
            if job.lines.is_empty() {
                small(ui, t!(self.lang, "пока пусто", "empty"), t.muted);
            } else {
                ScrollArea::vertical().max_height(300.0).stick_to_bottom(true).show(ui, |ui| {
                    for line in &job.lines {
                        mono(ui, line, if line.starts_with("Ошибка") { t.bad } else { t.muted });
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
                ui.label(RichText::new(t!(self.lang, "обновления", "updates")).size(15.0).strong());
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
                    small(ui, t!(self.lang, "проверяю релизы…", "checking releases…"), t.muted);
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
                    small(ui, &info.published.chars().take(10).collect::<String>(), t.faint);
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
                                egui::ProgressBar::new((done as f32 / total as f32).clamp(0.0, 1.0))
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
                        if ghost(ui, t!(self.lang, "страница релиза", "release page"), t).clicked() {
                            let _ = sdw_core::util::open_url(&info.page);
                        }
                    });
                }
                if !self.update_path.is_empty() {
                    mono(ui, &self.update_path, t.muted);
                    ui.horizontal(|ui| {
                        if ghost(ui, t!(self.lang, "установить", "install"), t).clicked() {
                            let path = self.update_path.clone();
                            self.spawn(move || {
                                TaskResult::Installed(update::install(std::path::Path::new(&path)))
                            });
                        }
                        if ghost(ui, t!(self.lang, "открыть папку", "open folder"), t).clicked() {
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

    fn settings_ui(&mut self, ui: &mut Ui, t: &Tokens) {
        let cfg = self.state.cfg();
        self.page_header(
            ui,
            t,
            t!(self.lang, "настройки", "settings"),
            t!(self.lang, "имя, язык, тема и запуск", "name, language, theme and startup"),
        );
        card(ui, t, |ui| {
            ui.label(RichText::new(t!(self.lang, "компьютер", "computer")).size(15.0).strong());
            ui.add_space(4.0);
            ui.label(hud(t!(self.lang, "имя в сети", "network name"), t.faint));
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut self.name_edit).desired_width(320.0));
                if ui.add(egui::Button::new(hud(t!(self.lang, "сохранить", "save"), t.muted))).clicked() {
                    let name = self.name_edit.trim().to_string();
                    if !name.is_empty() {
                        self.save(|c| c.name = name);
                        self.toast(t!(self.lang, "имя сохранено", "name saved"), true);
                    }
                }
            });
            ui.add_space(8.0);
            divider(ui);
            row(
                ui,
                t,
                t!(self.lang, "язык", "language"),
                "",
                |ui| {
                    if ui.selectable_label(self.lang == Lang::Ru, "русский").clicked() {
                        self.set_lang(Lang::Ru);
                    }
                    if ui.selectable_label(self.lang == Lang::En, "english").clicked() {
                        self.set_lang(Lang::En);
                    }
                },
            );
            row(
                ui,
                t,
                t!(self.lang, "тема", "theme"),
                "",
                |ui| {
                    if ui.selectable_label(self.dark, t!(self.lang, "тёмная", "dark")).clicked() {
                        self.set_dark(true);
                    }
                    if ui.selectable_label(!self.dark, t!(self.lang, "светлая", "light")).clicked() {
                        self.set_dark(false);
                    }
                },
            );
            row(
                ui,
                t,
                t!(self.lang, "запускать viiper автоматически", "start viiper automatically"),
                "",
                |ui| {
                    let mut on = cfg.viiper.auto_start;
                    if switch(ui, &mut on, t).changed() {
                        self.save(|c| c.viiper.auto_start = on);
                    }
                },
            );
            ui.add_space(8.0);
            divider(ui);
            ui.horizontal(|ui| {
                ui.label(hud(t!(self.lang, "папка конфига", "config folder"), t.faint));
                mono(ui, &sdw_core::util::config_dir().display().to_string(), t.muted);
            });
            ui.horizontal(|ui| {
                if ghost(ui, t!(self.lang, "открыть папку", "open folder"), t).clicked() {
                    let _ = sdw_core::util::open_url(&sdw_core::util::config_dir().display().to_string());
                }
                if ghost(ui, t!(self.lang, "открыть журнал", "open log"), t).clicked() {
                    let _ = sdw_core::util::open_url(&config::log_path().display().to_string());
                }
                if ghost(ui, t!(self.lang, "перезапустить viiper", "restart viiper"), t).clicked() {
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
            if ui.add(egui::Button::new(hud(t!(self.lang, "страница проекта", "project page"), t.faint))).clicked() {
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
            t!(self.lang, "последние события демона", "recent daemon events"),
        );
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.logs_follow, t!(self.lang, "следовать за концом", "follow the end"));
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
        MonitorBackend::MacCapture => "macos screencapture",
    }
}

fn pad_hint(l: Lang, k: PadKind) -> &'static str {
    match k {
        PadKind::Xbox360 => t!(
            l,
            "самый совместимый: xinput, steam и почти все игры.",
            "the most compatible: xinput, steam and almost every game."
        ),
        PadKind::DualShock4 => t!(l, "ps4-пад, гироскоп и тачпад.", "ps4 pad, gyro and touchpad."),
        PadKind::DualSense => t!(l, "ps5-пад, гироскоп и тачпад.", "ps5 pad, gyro and touchpad."),
        PadKind::SwitchPro => t!(l, "nintendo switch 2 pro, гироскоп.", "nintendo switch 2 pro, gyro."),
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
