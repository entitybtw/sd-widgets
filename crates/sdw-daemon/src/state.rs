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
    pub client: Option<HaClient>,
    pub last_sync: Instant,
}

impl HaRuntime {
    pub fn from_config(cfg: &DaemonConfig) -> Self {
        let configured = !cfg.ha.url.trim().is_empty() && !cfg.ha.token.trim().is_empty();
        let client = configured.then(|| HaClient::new(&cfg.ha.url, &cfg.ha.token));
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
            client,
            last_sync: Instant::now() - Duration::from_secs(3600),
        }
    }

    pub fn reload(&mut self, cfg: &DaemonConfig) {
        *self = Self::from_config(cfg);
    }

    pub fn refresh(&mut self) -> Result<(), String> {
        let Some(client) = &self.client else {
            return Err("Home Assistant не настроен".into());
        };
        match client.states() {
            Ok(states) => {
                self.status.connected = true;
                self.status.entities = states.entities.len();
                self.status.last_sync_ms = states.fetched_ms;
                self.status.error.clear();
                self.entities = states.entities;
                self.areas = states.areas;
                self.last_sync = Instant::now();
                Ok(())
            }
            Err(e) => {
                self.status.connected = false;
                self.status.error = e.clone();
                self.last_sync = Instant::now();
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

    pub fn set_brightness(&self, entity_id: &str, brightness: u8) -> Result<serde_json::Value, String> {
        let Some(client) = &self.client else {
            return Err("Home Assistant не настроен".into());
        };
        client.set_brightness(entity_id, brightness)
    }

    pub fn set(&mut self, cfg: &mut DaemonConfig, url: &str, token: &str, favorites: &[String]) -> Result<(), String> {
        cfg.ha.url = url.trim().to_string();
        cfg.ha.token = token.trim().to_string();
        cfg.ha.favorites = favorites.to_vec();
        let client = HaClient::new(&cfg.ha.url, &cfg.ha.token);
        let message = client.ping()?;
        self.client = Some(client);
        self.status.configured = true;
        self.status.connected = true;
        self.status.url = cfg.ha.url.clone();
        self.status.favorites = cfg.ha.favorites.clone();
        self.status.error = format!("Подключено: {message}");
        let _ = self.refresh();
        Ok(())
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
        self.line(format!("Ошибка: {text}"));
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
    pub viiper: Arc<ViiperManager>,
    pub monitor: Arc<MonitorManager>,
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
        let notifications = Arc::new(NotificationStore::new(cfg.notifications.enabled, logger.clone()));
        let viiper = Arc::new(ViiperManager::new(logger.clone()));
        let monitor = Arc::new(MonitorManager::new(logger.clone()));
        let ha = Arc::new(Mutex::new(HaRuntime::from_config(&cfg)));
        let jobs = Arc::new(JobManager::new());
        let stop = Arc::new(AtomicBool::new(false));
        let st = Arc::new(Self {
            cfg: Arc::new(Mutex::new(cfg)),
            logger,
            notifications,
            viiper,
            monitor,
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
        let ha = self
            .ha
            .lock()
            .map(|h| h.status.clone())
            .unwrap_or_default();
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
            config_dir: sdw_core::util::config_dir().display().to_string(),
            log_tail: self.logger.tail(40),
        }
    }

    fn start_background(self: &Arc<Self>) {
        // LAN discovery
        let cfg = self.cfg();
        let stop = self.stop.clone();
        let (name, id, port, setup) = (cfg.name.clone(), cfg.daemon_id.clone(), cfg.api_port, cfg.setup_done);
        let log = self.logger.clone();
        std::thread::spawn(move || {
            if let Err(e) = sdw_core::discovery::responder(name, id, port, setup, stop) {
                log.warn(format!("discovery: {e}"));
            }
        });

        // Notification mirroring (Linux only)
        self.notifications.clone().start(self.stop.clone());

        // Home Assistant polling
        let ha = self.ha.clone();
        let cfg_arc = self.cfg.clone();
        let log = self.logger.clone();
        let stop = self.stop.clone();
        std::thread::spawn(move || {
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let configured = cfg_arc
                    .lock()
                    .map(|c| !c.ha.url.is_empty() && !c.ha.token.is_empty())
                    .unwrap_or(false);
                if configured {
                    if let Ok(mut h) = ha.lock() {
                        if h.last_sync.elapsed() > Duration::from_secs(10) {
                            if let Err(e) = h.refresh() {
                                log.warn(format!("Home Assistant: {e}"));
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });

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
        self.monitor.stop();
        self.viiper.stop();
    }
}
