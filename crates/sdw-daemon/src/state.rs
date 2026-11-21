//! One shared state for the HTTP server and the GUI. Threads never hold a lock across I/O.

use crate::config::{self, DaemonConfig};
use crate::monitor::MonitorManager;
use crate::notifications::NotificationStore;
use crate::viiper::ViiperManager;
use sdw_core::ha::HaClient;
use sdw_core::model::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Logger {
    inner: Arc<Mutex<VecDeque<LogEntry>>>,
}

impl Logger {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
    pub fn log(&self, level: &str, message: impl Into<String>) {
        let entry = LogEntry {
            ts: now_ms(),
            level: level.to_string(),
            message: message.into(),
        };
        eprintln!("[{}] {}", entry.level, entry.message);
        if let Ok(mut q) = self.inner.lock() {
            q.push_back(entry);
            while q.len() > 600 {
                q.pop_front();
            }
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(config::log_path())
        {
            use std::io::Write;
            if let Some(e) = self.inner.lock().ok().and_then(|q| q.back().cloned()) {
                let _ = writeln!(f, "{} {} {}", e.ts, e.level, e.message);
            }
        }
    }
    pub fn info(&self, m: impl Into<String>) {
        self.log("info", m);
    }
    pub fn warn(&self, m: impl Into<String>) {
        self.log("warn", m);
    }
    pub fn error(&self, m: impl Into<String>) {
        self.log("error", m);
    }
    pub fn tail(&self, n: usize) -> Vec<LogEntry> {
        self.inner
            .lock()
            .map(|q| q.iter().rev().take(n).rev().cloned().collect())
            .unwrap_or_default()
    }
    pub fn since(&self, ts: u64) -> Vec<LogEntry> {
        self.inner
            .lock()
            .map(|q| q.iter().filter(|e| e.ts > ts).cloned().collect())
            .unwrap_or_default()
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------- Home Assistant runtime

pub struct HaRuntime {
    pub status: HaStatus,
    pub entities: Vec<HaEntity>,
    pub areas: Vec<String>,
    pub views: Vec<HaView>,
    pub categories: Vec<HaCategory>,
    pub presets: Vec<HaPreset>,
    pub client: Option<HaClient>,
    /// Last successful sync and last attempt, both monotonic.
    pub last_sync: Instant,
    pub last_attempt: Instant,
    pub fetched_ms: u64,
    /// A fetch is in flight; another one must not start.
    pub refreshing: bool,
    /// A command just ran, so the next fetch is due immediately.
    pub dirty: bool,
    pub area_map: Vec<(String, String)>,
    pub areas_at: Instant,
    pub views_at: Instant,
    pub error: String,
}

impl HaRuntime {
    pub fn from_config(cfg: &DaemonConfig) -> Self {
        let configured = !cfg.ha.url.trim().is_empty() && !cfg.ha.token.trim().is_empty();
        let client = configured.then(|| HaClient::new(&cfg.ha.url, &cfg.ha.token));
        let long_ago = Instant::now() - Duration::from_secs(3600);
        Self {
            status: HaStatus {
                configured,
                connected: false,
                url: cfg.ha.url.clone(),
                entities: 0,
                last_sync_ms: 0,
                error: String::new(),
                favorites: cfg.ha.favorites.clone(),
            },
            entities: Vec::new(),
            areas: Vec::new(),
            views: Vec::new(),
            categories: cfg.ha.categories.clone(),
            presets: cfg.ha.presets.clone(),
            client,
            last_sync: long_ago,
            last_attempt: long_ago,
            fetched_ms: 0,
            refreshing: false,
            dirty: false,
            area_map: Vec::new(),
            areas_at: long_ago,
            views_at: long_ago,
            error: String::new(),
        }
    }

    pub fn reload(&mut self, cfg: &DaemonConfig) {
        *self = Self::from_config(cfg);
    }

    pub fn configured(&self) -> bool {
        self.client.is_some()
    }

    /// Called with the lock held: decide whether a fetch is due and take
    /// everything it needs. No I/O happens while the caller still holds the
    /// lock, which is what used to freeze status, toggles and the whole UI.
    pub fn begin(&mut self) -> Option<(HaClient, bool, bool)> {
        let client = self.client.clone()?;
        if self.refreshing {
            return None;
        }
        let wait = if self.dirty || self.fetched_ms == 0 {
            Duration::from_millis(500)
        } else {
            Duration::from_secs(3)
        };
        if self.last_attempt.elapsed() < wait {
            return None;
        }
        self.refreshing = true;
        self.dirty = false;
        self.last_attempt = Instant::now();
        let want_rooms =
            self.area_map.is_empty() || self.areas_at.elapsed() > Duration::from_secs(600);
        let want_views = self.views_at.elapsed() > Duration::from_secs(600);
        Some((client, want_rooms, want_views))
    }

    pub fn apply(
        &mut self,
        entities: Result<Vec<HaEntity>, String>,
        rooms: Option<Result<(Vec<String>, Vec<(String, String)>), String>>,
        views: Option<Result<Vec<HaView>, String>>,
    ) {
        self.refreshing = false;
        if let Some(result) = rooms {
            match result {
                Ok((areas, map)) => {
                    self.areas = areas;
                    self.area_map = map;
                }
                // Older builds or a restricted token: keep the old mapping
                // instead of dropping every room.
                Err(_) => {}
            }
            self.areas_at = Instant::now();
        }
        if let Some(result) = views {
            match result {
                Ok(list) => {
                    if !list.is_empty() {
                        self.views = list;
                    }
                }
                Err(e) => {
                    if self.views.is_empty() {
                        self.status.error = e;
                    }
                }
            }
            self.views_at = Instant::now();
        }
        match entities {
            Ok(mut list) => {
                HaClient::apply_areas(&mut list, &self.area_map);
                self.entities = list;
                self.status.entities = self.entities.len();
                self.status.connected = true;
                self.status.error.clear();
                self.status.last_sync_ms = now_ms();
                self.fetched_ms = self.status.last_sync_ms;
                self.error.clear();
                self.last_sync = Instant::now();
            }
            Err(e) => {
                self.status.connected = false;
                self.status.error = e.clone();
                self.error = e;
            }
        }
    }

    /// A service call changes something right now: fetch the new states as
    /// soon as the worker can, without waiting for the next regular tick.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_attempt = Instant::now() - Duration::from_secs(2);
    }

    pub fn age_ms(&self) -> u64 {
        if self.fetched_ms == 0 {
            0
        } else {
            self.last_sync.elapsed().as_millis() as u64
        }
    }

    /// Clean up and store the categories. Returns what was actually kept.
    pub fn set_categories(&mut self, mut list: Vec<HaCategory>) -> Vec<HaCategory> {
        list.truncate(40);
        for (i, c) in list.iter_mut().enumerate() {
            c.name = c.name.trim().to_string();
            if c.name.is_empty() {
                c.name = format!("Категория {}", i + 1);
            }
            if c.id.trim().is_empty() {
                c.id = random_id(8);
            }
            let mut seen = std::collections::HashSet::new();
            c.entities.retain(|e| {
                let e = e.trim();
                !e.is_empty() && seen.insert(e.to_string())
            });
            c.entities.truncate(500);
        }
        self.categories = list.clone();
        list
    }

    /// Clean up and store the presets. Returns what was actually kept.
    pub fn set_presets(&mut self, mut list: Vec<HaPreset>) -> Vec<HaPreset> {
        list.truncate(60);
        for (i, p) in list.iter_mut().enumerate() {
            p.name = p.name.trim().to_string();
            if p.name.is_empty() {
                p.name = format!("Пресет {}", i + 1);
            }
            if p.id.trim().is_empty() {
                p.id = random_id(8);
            }
            p.actions.retain(|a| {
                !a.entity_id.trim().is_empty()
                    && matches!(a.service.as_str(), "turn_on" | "turn_off" | "toggle")
            });
            p.actions.truncate(200);
        }
        self.presets = list.clone();
        list
    }

    pub fn refresh(&mut self) -> Result<(), String> {
        let Some(client) = &self.client else {
            return Err("Home Assistant не настроен".into());
        };
        match client.states() {
            Ok(entities) => {
                self.apply(Ok(entities), Some(client.rooms()), None);
                Ok(())
            }
            Err(e) => {
                self.apply(Err(e.clone()), None, None);
                Err(e)
            }
        }
    }

    pub fn call(&self, call: &HaCall) -> Result<serde_json::Value, String> {
        let Some(client) = &self.client else {
            return Err("Home Assistant не настроен".into());
        };
        client.call(call)
    }

    pub fn toggle(&self, entity_id: &str, on: bool) -> Result<serde_json::Value, String> {
        let Some(client) = &self.client else {
            return Err("Home Assistant не настроен".into());
        };
        client.toggle(entity_id, on)
    }

    pub fn set_brightness(
        &self,
        entity_id: &str,
        brightness: u8,
    ) -> Result<serde_json::Value, String> {
        let Some(client) = &self.client else {
            return Err("Home Assistant не настроен".into());
        };
        client.set_brightness(entity_id, brightness)
    }

    /// Install a client that has already answered `ping`, then let the worker
    /// fetch states. The ping itself must happen outside this lock.
    pub fn install(
        &mut self,
        cfg: &mut DaemonConfig,
        client: HaClient,
        message: &str,
        favorites: &[String],
    ) {
        cfg.ha.url = client.base().to_string();
        cfg.ha.favorites = favorites.to_vec();
        self.client = Some(client);
        self.status.configured = true;
        self.status.connected = true;
        self.status.url = cfg.ha.url.clone();
        self.status.favorites = cfg.ha.favorites.clone();
        self.status.error = format!("Подключено: {message}");
        self.error.clear();
        self.fetched_ms = 0;
        self.entities.clear();
        self.area_map.clear();
        self.views.clear();
        self.last_sync = Instant::now() - Duration::from_secs(3600);
        self.last_attempt = self.last_sync;
        self.areas_at = self.last_sync;
        self.views_at = self.last_sync;
        self.dirty = true;
        self.refreshing = false;
    }
}

// ---------------------------------------------------------------- jobs

#[derive(Debug, Clone, Default)]
pub struct Job {
    pub id: u64,
    pub name: String,
    pub running: bool,
    pub lines: Vec<String>,
    pub error: Option<String>,
    pub done: Option<String>,
    pub started_ms: u64,
}

impl Job {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "running": self.running,
            "lines": self.lines,
            "error": self.error,
            "done": self.done,
            "started_ms": self.started_ms,
        })
    }
}

pub struct JobManager {
    job: Arc<Mutex<Option<Job>>>,
    seq: AtomicU64,
}

impl JobManager {
    pub fn new() -> Self {
        Self {
            job: Arc::new(Mutex::new(None)),
            seq: AtomicU64::new(1),
        }
    }

    pub fn start<F>(&self, name: &str, f: F) -> Result<u64, String>
    where
        F: FnOnce(JobCtx) + Send + 'static,
    {
        let mut slot = self.job.lock().unwrap();
        if slot.as_ref().is_some_and(|j| j.running) {
            return Err("Уже выполняется другая операция".into());
        }
        let id = self.seq.fetch_add(1, Ordering::Relaxed);
        *slot = Some(Job {
            id,
            name: name.to_string(),
            running: true,
            lines: Vec::new(),
            error: None,
            done: None,
            started_ms: now_ms(),
        });
        let job = self.job.clone();
        let ctx = JobCtx { job };
        std::thread::spawn(move || {
            f(ctx.clone());
            ctx.finish();
        });
        Ok(id)
    }

    pub fn status(&self) -> Job {
        self.job
            .lock()
            .ok()
            .and_then(|j| j.clone())
            .unwrap_or_default()
    }

    pub fn is_running(&self) -> bool {
        self.job
            .lock()
            .ok()
            .and_then(|j| j.as_ref().map(|j| j.running))
            .unwrap_or(false)
    }
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct JobCtx {
    job: Arc<Mutex<Option<Job>>>,
}

impl JobCtx {
    pub fn line(&self, text: impl Into<String>) {
        let text = text.into();
        eprintln!("  {text}");
        if let Ok(mut slot) = self.job.lock() {
            if let Some(j) = slot.as_mut() {
                j.lines.push(text);
                if j.lines.len() > 300 {
                    j.lines.remove(0);
                }
            }
        }
    }
    pub fn error(&self, text: impl Into<String>) {
        let text = text.into();
        self.line(format!("error: {text}"));
        if let Ok(mut slot) = self.job.lock() {
            if let Some(j) = slot.as_mut() {
                j.error = Some(text);
            }
        }
    }
    pub fn done(&self, text: impl Into<String>) {
        if let Ok(mut slot) = self.job.lock() {
            if let Some(j) = slot.as_mut() {
                j.done = Some(text.into());
            }
        }
    }
    fn finish(&self) {
        if let Ok(mut slot) = self.job.lock() {
            if let Some(j) = slot.as_mut() {
                j.running = false;
                if j.error.is_none() && j.done.is_none() {
                    j.done = Some("Готово".into());
                }
            }
        }
    }
}

// ---------------------------------------------------------------- app state

pub struct AppState {
    pub cfg: Arc<Mutex<DaemonConfig>>,
    pub logger: Logger,
    pub notifications: Arc<NotificationStore>,
    pub media: Arc<crate::media::MediaService>,
    pub audio: Arc<crate::audio::Audio>,
    pub viiper: Arc<ViiperManager>,
    pub monitor: Arc<MonitorManager>,
    /// The decks that send their screen here, and the newest picture.
    pub mirror: Arc<crate::mirror::Mirror>,
    /// The deck's own sound while somebody watches its picture.
    pub deck_audio: Arc<crate::deck_audio::DeckAudio>,
    pub ha: Arc<Mutex<HaRuntime>>,
    pub jobs: Arc<JobManager>,
    pub clients: AtomicU32,
    pub started: Instant,
    pub stop: Arc<AtomicBool>,
    pub pairing_attempts: Arc<Mutex<std::collections::HashMap<String, (u32, Instant)>>>,
    pub last_seen: Arc<Mutex<std::collections::HashMap<String, Instant>>>,
}

impl AppState {
    pub fn new() -> Arc<Self> {
        let cfg = config::load();
        let logger = Logger::new();
        logger.info(format!(
            "SD Widgets daemon {} запускается на {}",
            VERSION,
            sdw_core::util::hostname()
        ));
        let notifications = Arc::new(NotificationStore::new(
            cfg.notifications.enabled,
            logger.clone(),
        ));
        let media = Arc::new(crate::media::MediaService::new());
        let mirror = Arc::new(crate::mirror::Mirror::new());
        let deck_audio = Arc::new(crate::deck_audio::DeckAudio::new());
        let audio = Arc::new(crate::audio::Audio::new());
        let viiper = Arc::new(ViiperManager::new(logger.clone()));
        let monitor = Arc::new(MonitorManager::new(logger.clone()));
        let ha = Arc::new(Mutex::new(HaRuntime::from_config(&cfg)));
        let jobs = Arc::new(JobManager::new());
        let stop = Arc::new(AtomicBool::new(false));
        let st = Arc::new(Self {
            cfg: Arc::new(Mutex::new(cfg)),
            logger,
            notifications,
            media,
            audio,
            viiper,
            monitor,
            mirror,
            deck_audio,
            ha,
            jobs,
            clients: AtomicU32::new(0),
            started: Instant::now(),
            stop,
            pairing_attempts: Arc::new(Mutex::new(std::collections::HashMap::new())),
            last_seen: Arc::new(Mutex::new(std::collections::HashMap::new())),
        });
        st.start_background();
        st
    }

    pub fn cfg(&self) -> DaemonConfig {
        self.cfg.lock().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn update_cfg<F>(&self, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut DaemonConfig),
    {
        let mut cfg = self.cfg.lock().map_err(|_| "config lock")?;
        f(&mut cfg);
        config::save(&cfg)
    }

    pub fn log(&self, level: &str, msg: impl Into<String>) {
        self.logger.log(level, msg);
    }

    pub fn status(&self) -> DaemonStatus {
        let cfg = self.cfg();
        let viiper = self.viiper.status(&cfg);
        let monitor = self.monitor.status();
        let notifications = self.notifications.status();
        let ha = self.ha.lock().map(|h| h.status.clone()).unwrap_or_default();
        let pads = self.viiper.pads();
        DaemonStatus {
            name: cfg.name.clone(),
            hostname: sdw_core::util::hostname(),
            version: VERSION.to_string(),
            daemon_id: cfg.daemon_id.clone(),
            platform: platform().to_string(),
            uptime_secs: self.started.elapsed().as_secs(),
            api_port: cfg.api_port,
            addresses: sdw_core::util::addresses(cfg.api_port),
            setup_done: cfg.setup_done,
            connected_clients: cfg.clients.len() as u32,
            viiper,
            monitor,
            notifications,
            ha,
            pads,
            audio: self.audio.status(),
            addons: sdw_core::addons::info(&cfg.addons),
            config_dir: sdw_core::util::config_dir().display().to_string(),
            log_tail: self.logger.tail(40),
        }
    }

    fn start_background(self: &Arc<Self>) {
        // LAN discovery
        let cfg = self.cfg();
        let stop = self.stop.clone();
        let (name, id, port, setup) = (
            cfg.name.clone(),
            cfg.daemon_id.clone(),
            cfg.api_port,
            cfg.setup_done,
        );
        let log = self.logger.clone();
        std::thread::spawn(move || {
            if let Err(e) = sdw_core::discovery::responder(name, id, port, setup, stop) {
                log.warn(format!("discovery: {e}"));
            }
        });

        // The deck's own sound goes to one player process; a small tick
        // closes it when the deck goes quiet.
        crate::deck_audio::watch(self.deck_audio.clone(), self.stop.clone());

        // Notification mirroring (Linux only). When nothing else owns the
        // notification bus name we become the daemon ourselves, so apps can
        // finally deliver notifications to the deck.
        let own_service = cfg.notifications.own_service;
        self.notifications
            .clone()
            .start(self.stop.clone(), own_service);

        // Home Assistant: one worker owns every fetch. It takes the client,
        // releases the lock, does the HTTP work, then stores the result. The
        // lock is never held across a network call, so `/status`, a toggle or
        // a state poll can answer immediately while a slow HA is timing out.
        let ha = self.ha.clone();
        let log = self.logger.clone();
        let stop = self.stop.clone();
        std::thread::Builder::new()
            .name("sdw-ha".into())
            .spawn(move || loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let job = ha.lock().ok().and_then(|mut h| h.begin());
                let Some((client, want_rooms, want_views)) = job else {
                    std::thread::sleep(Duration::from_millis(350));
                    continue;
                };
                let entities = client.states();
                if let Err(e) = &entities {
                    log.warn(format!("Home Assistant: {e}"));
                }
                let rooms = want_rooms.then(|| client.rooms());
                let views = want_views.then(|| fetch_views(&client));
                if let Ok(mut h) = ha.lock() {
                    h.apply(entities, rooms, views);
                }
            })
            .ok();

        // UDP controller input
        let viiper = self.viiper.clone();
        let cfg = self.cfg();
        let stop = self.stop.clone();
        let log = self.logger.clone();
        std::thread::spawn(move || {
            viiper.run_udp_listener(cfg.input_port, stop, log);
        });

        // VIIPER autostart
        let viiper = self.viiper.clone();
        let cfg = self.cfg();
        let log = self.logger.clone();
        if cfg.viiper.enabled && cfg.viiper.auto_start {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(900));
                if let Err(e) = viiper.start(&cfg) {
                    log.warn(format!("VIIPER автозапуск: {e}"));
                }
            });
        }

        // Auto-start the extra screen if it was running last time
        // (kept manual: compositor outputs can surprise the user after login).
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.deck_audio.stop();
        self.monitor.stop();
        self.viiper.stop();
    }
}

fn fetch_views(client: &HaClient) -> Result<Vec<HaView>, String> {
    sdw_core::ha::views_guarded(client)
}
