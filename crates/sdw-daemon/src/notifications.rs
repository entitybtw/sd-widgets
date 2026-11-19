//! Mirrors desktop notifications into a small ring buffer for the Deck.
//!
//! On Linux this listens to the session bus. It does not replace the desktop notification
//! daemon: it only observes `org.freedesktop.Notifications.Notify` calls. On Windows and
//! macOS the platform keeps notifications in a private store; the daemon reports that
//! mirroring is unavailable instead of pretending.

use crate::state::Logger;
use sdw_core::model::{now_ms, Notification, NotificationStatus};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Inner {
    items: VecDeque<Notification>,
    next_id: u64,
    status: NotificationStatus,
    enabled: bool,
}

pub struct NotificationStore {
    inner: Mutex<Inner>,
    logger: Logger,
    owns_service: AtomicBool,
}

impl NotificationStore {
    pub fn new(enabled: bool, logger: Logger) -> Self {
        let backend = if cfg!(target_os = "linux") {
            "D-Bus"
        } else if cfg!(target_os = "windows") {
            "Windows"
        } else {
            "macOS"
        };
        Self {
            inner: Mutex::new(Inner {
                items: VecDeque::new(),
                next_id: 1,
                status: NotificationStatus {
                    enabled,
                    listening: false,
                    count: 0,
                    unread: 0,
                    backend: backend.into(),
                    error: if cfg!(target_os = "linux") {
                        String::new()
                    } else {
                        "Зеркалирование уведомлений пока доступно на Linux".into()
                    },
                    recent: Vec::new(),
                },
                enabled,
            }),
            logger,
            owns_service: AtomicBool::new(false),
        }
    }

    pub fn status(&self) -> NotificationStatus {
        let mut s = self
            .inner
            .lock()
            .map(|i| i.status.clone())
            .unwrap_or_default();
        if let Ok(i) = self.inner.lock() {
            s.count = i.items.len() as u32;
            s.unread = i.items.iter().filter(|n| n.urgency == 0).count() as u32;
            s.recent = i.items.iter().rev().take(30).cloned().collect();
        }
        s
    }

    pub fn enabled(&self) -> bool {
        self.inner.lock().map(|i| i.enabled).unwrap_or(false)
    }

    pub fn set_enabled(&self, on: bool) {
        if let Ok(mut i) = self.inner.lock() {
            i.enabled = on;
            i.status.enabled = on;
            if !on {
                i.status.listening = false;
            }
        }
    }

    pub fn add_test(&self) {
        self.push(Notification {
            id: 0,
            app: "SD Widgets".into(),
            summary: "Проверка связи".into(),
            body: "Уведомления с компьютера приходят на Steam Deck.".into(),
            icon: String::new(),
            urgency: 0,
            ts: now_ms(),
            has_actions: false,
        });
    }

    /// Store a notification delivered by the built-in D-Bus service.
    /// `replaces` replaces an existing notification id (libnotify uses this).
    pub fn record(
        &self,
        app: &str,
        summary: &str,
        body: &str,
        icon: &str,
        urgency: u8,
        has_actions: bool,
        replaces: u64,
    ) -> u64 {
        let mut inner = match self.inner.lock() {
            Ok(i) => i,
            Err(_) => return 0,
        };
        if !inner.enabled {
            return 0;
        }
        if replaces != 0 {
            inner.items.retain(|n| n.id != replaces);
        }
        let id = if replaces != 0 {
            replaces
        } else {
            inner.next_id
        };
        inner.next_id = inner.next_id.max(id + 1);
        let n = Notification {
            id,
            app: if app.trim().is_empty() {
                "Система".into()
            } else {
                app.trim().to_string()
            },
            summary: summary.trim().to_string(),
            body: body.trim().to_string(),
            icon: icon.trim().to_string(),
            urgency,
            ts: now_ms(),
            has_actions,
        };
        if n.summary.is_empty() && n.body.is_empty() {
            return 0;
        }
        // Apps often repeat the same notification: collapse exact repeats within 2 seconds.
        if replaces == 0 {
            if let Some(last) = inner.items.back() {
                if last.app == n.app
                    && last.summary == n.summary
                    && last.body == n.body
                    && n.ts.saturating_sub(last.ts) < 2000
                {
                    return last.id;
                }
            }
        }
        inner.items.push_back(n);
        while inner.items.len() > 120 {
            inner.items.pop_front();
        }
        id
    }

    pub fn clear(&self) {
        if let Ok(mut i) = self.inner.lock() {
            i.items.clear();
        }
    }

    /// Remove from the ring buffer without talking to the bus.
    pub fn dismiss_local(&self, id: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.items.retain(|n| n.id != id);
        }
    }

    pub fn dismiss(&self, id: u64) {
        let dbus_id = {
            let mut inner = match self.inner.lock() {
                Ok(i) => i,
                Err(_) => return,
            };
            inner.items.retain(|n| n.id != id);
            id
        };
        #[cfg(target_os = "linux")]
        if !self.owns_service.load(Ordering::Relaxed) {
            let _ = Command::new("gdbus")
                .args([
                    "call",
                    "--session",
                    "--dest",
                    "org.freedesktop.Notifications",
                    "--object-path",
                    "/org/freedesktop/Notifications",
                    "--method",
                    "org.freedesktop.Notifications.CloseNotification",
                    &dbus_id.to_string(),
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }

    fn push(&self, mut n: Notification) {
        let mut inner = match self.inner.lock() {
            Ok(i) => i,
            Err(_) => return,
        };
        if !inner.enabled {
            return;
        }
        if n.id == 0 {
            n.id = inner.next_id;
            inner.next_id += 1;
        } else {
            inner.next_id = inner.next_id.max(n.id + 1);
        }
        // Apps often repeat the same notification: collapse exact repeats within 2 seconds.
        if let Some(last) = inner.items.back() {
            if last.app == n.app
                && last.summary == n.summary
                && last.body == n.body
                && n.ts.saturating_sub(last.ts) < 2000
            {
                return;
            }
        }
        inner.items.push_back(n);
        while inner.items.len() > 120 {
            inner.items.pop_front();
        }
    }

    pub fn start(self: Arc<Self>, stop: Arc<AtomicBool>, own_service: bool) {
        if !cfg!(target_os = "linux") {
            return;
        }

        #[cfg(target_os = "linux")]
        if own_service {
            let service = self.clone();
            let stop2 = stop.clone();
            std::thread::spawn(move || {
                if service.run_service(&stop2) {
                    return;
                }
                // A real desktop daemon owns the name; observe its calls instead.
                service.observe(stop2);
            });
            return;
        }

        self.observe(stop);
    }

    /// The old passive mode: watch `Notify` calls made to some other daemon.
    fn observe(self: Arc<Self>, stop: Arc<AtomicBool>) {
        std::thread::spawn(move || {
            let mut backoff = Duration::from_secs(2);
            while !stop.load(Ordering::Relaxed) {
                if !self.enabled() {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
                match Command::new("dbus-monitor")
                    .args(["--session", "interface='org.freedesktop.Notifications'"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(mut child) => {
                        if let Ok(mut inner) = self.inner.lock() {
                            inner.status.listening = true;
                            inner.status.backend = "D-Bus · наблюдение".into();
                            inner.status.error.clear();
                        }
                        if let Some(out) = child.stdout.take() {
                            self.read_loop(out, &stop);
                        }
                        let _ = child.kill();
                        let _ = child.wait();
                        if let Ok(mut inner) = self.inner.lock() {
                            inner.status.listening = false;
                        }
                    }
                    Err(e) => {
                        if let Ok(mut inner) = self.inner.lock() {
                            inner.status.error = format!("dbus-monitor: {e}");
                        }
                        self.logger.warn(format!("dbus-monitor: {e}"));
                    }
                }
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(20));
            }
        });
    }

    /// Own `org.freedesktop.Notifications` until shutdown. Returns false when
    /// another daemon already has the name (or the bus is unreachable).
    #[cfg(target_os = "linux")]
    fn run_service(self: &Arc<Self>, stop: &Arc<AtomicBool>) -> bool {
        let server = NotifServer {
            store: self.clone(),
        };
        let built = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.serve_at("/org/freedesktop/Notifications", server))
            .and_then(|b| b.name("org.freedesktop.Notifications"))
            .and_then(|b| b.build());
        let conn = match built {
            Ok(c) => c,
            Err(e) => {
                self.logger.info(format!(
                    "встроенный сервер уведомлений не занял имя ({e}); включаю наблюдение"
                ));
                return false;
            }
        };
        self.owns_service.store(true, Ordering::Relaxed);
        if let Ok(mut inner) = self.inner.lock() {
            inner.status.listening = true;
            inner.status.backend = "D-Bus · встроенный".into();
            inner.status.error.clear();
        }
        self.logger
            .info("встроенный сервер уведомлений слушает org.freedesktop.Notifications");
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(400));
        }
        self.owns_service.store(false, Ordering::Relaxed);
        drop(conn);
        true
    }

    fn read_loop(&self, out: impl std::io::Read, stop: &AtomicBool) {
        let mut reader = BufReader::new(out);
        let mut line = String::new();
        let mut pending: Option<Pending> = None;
        let mut last = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    last = Instant::now();
                    let t = line.trim_end();
                    if t.starts_with("method call") {
                        if let Some(p) = pending.take() {
                            self.commit(p);
                        }
                        if t.contains("member=Notify") {
                            pending = Some(Pending::from_call(t));
                        }
                    } else if let Some(p) = pending.as_mut() {
                        if t.starts_with("method return") {
                            if let Some(serial) = field(t, "reply_serial=") {
                                if serial == p.serial {
                                    p.awaiting_reply = true;
                                }
                            }
                        } else if t.starts_with("method call") || t.starts_with("signal ") {
                            let p = pending.take().unwrap();
                            self.commit(p);
                        } else {
                            p.feed(t);
                        }
                    }
                }
                Err(_) => {
                    if last.elapsed() > Duration::from_secs(60) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
        if let Some(p) = pending.take() {
            self.commit(p);
        }
    }

    fn commit(&self, p: Pending) {
        if p.strings.len() < 4 {
            return;
        }
        let app = unescape(&p.strings[0]);
        let summary = unescape(&p.strings[2]);
        let body = unescape(&p.strings[3]);
        if summary.trim().is_empty() && body.trim().is_empty() {
            return;
        }
        let urgency = p.urgency;
        self.push(Notification {
            id: p.notification_id,
            app: if app.trim().is_empty() {
                "Система".into()
            } else {
                app
            },
            summary,
            body,
            icon: unescape(&p.strings[1]),
            urgency,
            ts: now_ms(),
            has_actions: p.strings.len() > 4,
        });
    }
}

struct Pending {
    serial: String,
    strings: Vec<String>,
    ints: Vec<u64>,
    awaiting_reply: bool,
    notification_id: u64,
    urgency: u8,
}

impl Pending {
    fn from_call(line: &str) -> Self {
        Self {
            serial: field(line, "serial=").unwrap_or_default(),
            strings: Vec::new(),
            ints: Vec::new(),
            awaiting_reply: false,
            notification_id: 0,
            urgency: 0,
        }
    }

    fn feed(&mut self, line: &str) {
        let t = line.trim();
        if t.starts_with("string ") {
            if let Some(s) = quoted(t) {
                if self.strings.len() < 32 {
                    self.strings.push(s);
                }
            }
        } else if t.starts_with("uint32 ") {
            if let Ok(v) = t[7..].trim().parse::<u64>() {
                if self.awaiting_reply {
                    if self.notification_id == 0 {
                        self.notification_id = v;
                    }
                } else {
                    self.ints.push(v);
                }
            }
        } else if t.starts_with("byte ") {
            if let Ok(v) = t[5..].trim().parse::<u8>() {
                // Hints are printed as string "urgency" followed by variant byte N.
                if self.strings.last().map(|s| s == "urgency").unwrap_or(false) {
                    self.urgency = v;
                }
            }
        }
    }
}

fn field<'a>(line: &'a str, key: &str) -> Option<String> {
    let idx = line.find(key)? + key.len();
    let rest = &line[idx..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == ';' || c == ',')
        .unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

fn quoted(line: &str) -> Option<String> {
    let start = line.find('"')? + 1;
    let mut out = String::new();
    let mut escape = false;
    for c in line[start..].chars() {
        if escape {
            out.push(c);
            escape = false;
        } else if c == '\\' {
            escape = true;
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None
}

fn unescape(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

// ---------------------------------------------------------------- built-in server

#[cfg(target_os = "linux")]
struct NotifServer {
    store: Arc<NotificationStore>,
}

#[cfg(target_os = "linux")]
#[zbus::interface(name = "org.freedesktop.Notifications")]
impl NotifServer {
    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".into(), "body-markup".into(), "persistence".into()]
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "sd-widgets".into(),
            "entitybtw".into(),
            env!("CARGO_PKG_VERSION").into(),
            "1.2".into(),
        )
    }

    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<String>,
        hints: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        _expire_timeout: i32,
    ) -> u32 {
        let urgency = hints
            .get("urgency")
            .and_then(|v| u8::try_from(v).ok())
            .or_else(|| {
                hints
                    .get("urgency")
                    .and_then(|v| u32::try_from(v).ok())
                    .map(|v| v as u8)
            })
            .unwrap_or(0);
        let id = self.store.record(
            app_name,
            summary,
            body,
            app_icon,
            urgency,
            !actions.is_empty(),
            replaces_id as u64,
        );
        id as u32
    }

    fn close_notification(&self, id: u32) {
        self.store.dismiss_local(id as u64);
    }
}
