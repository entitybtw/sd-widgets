//! Mirrors desktop notifications into a small ring buffer for the Deck.
//!
//! On Linux this listens to the session bus. It does not replace the desktop notification
//! daemon: it only observes `org.freedesktop.Notifications.Notify` calls. On Windows the
//! platform keeps notifications in a private store; the daemon reports that mirroring is
//! unavailable instead of pretending.

use crate::state::Logger;
use sdw_core::model::{now_ms, Notification, NotificationStatus};
use std::collections::{HashMap, VecDeque};
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
    /// Action keys declared by the app, by notification id: tapping the
    /// notification on the deck runs the first one on the computer.
    actions: HashMap<u64, Vec<String>>,
    /// The app that sent the notification, so its action signal goes to it.
    senders: HashMap<u64, String>,
}

pub struct NotificationStore {
    inner: Mutex<Inner>,
    logger: Logger,
    owns_service: AtomicBool,
    /// The session bus connection we serve notifications on, kept so an
    /// activation can signal the app from another thread.
    #[cfg(target_os = "linux")]
    service: Mutex<Option<zbus::blocking::Connection>>,
}

impl NotificationStore {
    pub fn new(enabled: bool, logger: Logger) -> Self {
        let backend = if cfg!(target_os = "linux") {
            "D-Bus"
        } else {
            "Windows"
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
                actions: HashMap::new(),
                senders: HashMap::new(),
            }),
            logger,
            owns_service: AtomicBool::new(false),
            #[cfg(target_os = "linux")]
            service: Mutex::new(None),
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
            image: String::new(),
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
        image: &[u8],
        actions: &[String],
        sender: Option<&str>,
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
        let summary = notification_text(summary);
        let body = notification_text(body);
        if summary.trim().is_empty() && body.trim().is_empty() {
            return 0;
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
            image: if image.is_empty() {
                String::new()
            } else {
                sdw_core::util::b64_encode(image)
            },
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
        // Remember what a tap on the deck should run on the computer: the
        // action keys the app declared and the app itself. `default` is kept
        // apart — it is the key the app uses for "open me".
        let keys: Vec<String> = actions
            .chunks(2)
            .filter_map(|pair| pair.first())
            .filter(|k| !k.is_empty() && k.as_str() != "default")
            .map(|k| k.to_string())
            .collect();
        if !keys.is_empty() {
            inner.actions.insert(id, keys);
        } else {
            inner.actions.remove(&id);
        }
        if let Some(sender) = sender.filter(|s| !s.is_empty()) {
            inner.senders.insert(id, sender.to_string());
        } else {
            inner.senders.remove(&id);
        }
        inner.items.push_back(n);
        while inner.items.len() > 120 {
            if let Some(dropped) = inner.items.pop_front() {
                inner.actions.remove(&dropped.id);
                inner.senders.remove(&dropped.id);
            }
        }
        id
    }

    /// Run the notification's own action on the computer, the way clicking it
    /// on the desktop would. Only possible while this daemon owns
    /// `org.freedesktop.Notifications`: the signal goes from the server to the
    /// app that sent the notification.
    pub fn activate(&self, id: u64) -> Result<(), String> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = id;
            return Err("уведомления на этой системе только читаются".into());
        }
        #[cfg(target_os = "linux")]
        {
            let (key, sender, known) = {
                let inner = self
                    .inner
                    .lock()
                    .map_err(|_| "уведомления недоступны".to_string())?;
                (
                    inner
                        .actions
                        .get(&id)
                        .and_then(|keys| keys.first().cloned())
                        .unwrap_or_else(|| "default".to_string()),
                    inner.senders.get(&id).cloned(),
                    inner.items.iter().any(|n| n.id == id),
                )
            };
            if !known {
                return Err("уведомление уже закрыто".into());
            }
            if !self.owns_service.load(Ordering::Relaxed) {
                return Err(
                    "встроенный сервер уведомлений выключен: нажатия работают только с ним".into(),
                );
            }
            let conn = self
                .service
                .lock()
                .ok()
                .and_then(|c| c.as_ref().cloned())
                .ok_or_else(|| "нет связи с сессионной шиной".to_string())?;
            conn.emit_signal(
                sender.as_deref(),
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
                "ActionInvoked",
                &(id as u32, key.as_str()),
            )
            .map_err(|e| format!("действие не отправлено: {e}"))?;
            self.logger
                .info(format!("уведомление {id}: приложению отправлено {key}"));
            self.dismiss_local(id);
            Ok(())
        }
    }

    pub fn clear(&self) {
        if let Ok(mut i) = self.inner.lock() {
            i.items.clear();
            i.actions.clear();
            i.senders.clear();
        }
    }

    /// Remove from the ring buffer without talking to the bus.
    pub fn dismiss_local(&self, id: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.items.retain(|n| n.id != id);
            inner.actions.remove(&id);
            inner.senders.remove(&id);
        }
    }

    pub fn dismiss(&self, id: u64) {
        let dbus_id = {
            let mut inner = match self.inner.lock() {
                Ok(i) => i,
                Err(_) => return,
            };
            inner.items.retain(|n| n.id != id);
            inner.actions.remove(&id);
            inner.senders.remove(&id);
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
        if let Ok(mut slot) = self.service.lock() {
            *slot = Some(conn.clone());
        }
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
        if let Ok(mut slot) = self.service.lock() {
            *slot = None;
        }
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
        let summary = notification_text(&unescape(&p.strings[2]));
        let body = notification_text(&unescape(&p.strings[3]));
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
            image: String::new(),
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
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match chars.next() {
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('r') => out.push(b'\r'),
            Some('"') => out.push(b'"'),
            Some('\'') => out.push(b'\''),
            Some('\\') => out.push(b'\\'),
            // dbus-monitor escapes individual bytes as octal; a UTF-8
            // character can therefore arrive as several escapes.
            Some(first @ '0'..='7') => {
                let mut code = first.to_digit(8).unwrap_or(0);
                for _ in 0..2 {
                    match chars.peek() {
                        Some(&next @ '0'..='7') => {
                            chars.next();
                            code = code * 8 + next.to_digit(8).unwrap_or(0);
                        }
                        _ => break,
                    }
                }
                out.push(code as u8);
            }
            Some(other) => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
            }
            None => out.push(b'\\'),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Pango markup, which libnotify sends once the server advertises
/// `body-markup`, must not reach the Deck as literal tags. Apps also write
/// emoji as `&#x1F600;`; decode those so the whole notification reads.
fn notification_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                let mut tag = String::new();
                let mut closed = false;
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next == '>' {
                        closed = true;
                        break;
                    }
                    tag.push(next);
                }
                if !closed {
                    // No `>` anywhere: this is a comparison, not markup.
                    out.push('<');
                    out.push_str(&tag);
                    break;
                }
                let name = tag
                    .trim()
                    .trim_start_matches('/')
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if name == "img" {
                    if let Some(alt) = attribute(&tag, "alt") {
                        out.push_str(&decode_entities(&alt));
                    }
                    continue;
                }
                if name == "br" {
                    out.push('\n');
                    continue;
                }
                if matches!(
                    name.as_str(),
                    "b" | "i" | "u" | "a" | "span" | "em" | "strong" | "small" | "big" | "tt" | "p"
                ) {
                    continue;
                }
                // A lone `<` in plain text is not markup; keep it.
                out.push('<');
                out.push_str(&tag);
                out.push('>');
            }
            '&' => {
                let mut entity = String::new();
                let mut closed = false;
                while let Some(&next) = chars.peek() {
                    if next == ';' {
                        chars.next();
                        closed = true;
                        break;
                    }
                    if entity.len() >= 16 || next.is_whitespace() {
                        break;
                    }
                    entity.push(next);
                    chars.next();
                }
                if closed {
                    match entity_char(&entity) {
                        Some(s) => out.push_str(&s),
                        None => {
                            out.push('&');
                            out.push_str(&entity);
                            out.push(';');
                        }
                    }
                } else {
                    out.push('&');
                    out.push_str(&entity);
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        let mut entity = String::new();
        let mut closed = false;
        while let Some(&next) = chars.peek() {
            if next == ';' {
                chars.next();
                closed = true;
                break;
            }
            if entity.len() >= 16 || next.is_whitespace() {
                break;
            }
            entity.push(next);
            chars.next();
        }
        if closed {
            match entity_char(&entity) {
                Some(s) => out.push_str(&s),
                None => {
                    out.push('&');
                    out.push_str(&entity);
                    out.push(';');
                }
            }
        } else {
            out.push('&');
            out.push_str(&entity);
        }
    }
    out
}

fn entity_char(entity: &str) -> Option<String> {
    let named = match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => None,
    };
    if let Some(c) = named {
        return Some(c.to_string());
    }
    let code = entity.strip_prefix('#')?;
    let value = if let Some(hex) = code.strip_prefix('x').or_else(|| code.strip_prefix('X')) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        code.parse::<u32>().ok()?
    };
    char::from_u32(value).map(|c| c.to_string())
}

fn attribute(tag: &str, key: &str) -> Option<String> {
    let mut rest = tag;
    loop {
        let at = rest.find(key)?;
        let before_ok = at == 0
            || rest[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_whitespace());
        let after = rest[at + key.len()..].trim_start();
        if before_ok && after.starts_with('=') {
            let value = after[1..].trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let end = value[1..].find(quote)? + 1;
                return Some(value[1..end].to_string());
            }
        }
        rest = &rest[at + key.len()..];
    }
}

/// The `image-data` hint of a notification: width, height, row stride, alpha,
/// bits per sample, channels, then the pixels. Converted to a PNG the Deck can
/// draw without knowing anything about the format.
#[cfg(target_os = "linux")]
fn image_hint(value: &zbus::zvariant::OwnedValue) -> Option<Vec<u8>> {
    type Raw = (i32, i32, i32, bool, i32, i32, Vec<u8>);
    let (w, h, rowstride, _alpha, _bits, channels, data) = Raw::try_from(&**value).ok()?;
    if w <= 0 || h <= 0 || w > 1024 || h > 1024 || !(3..=4).contains(&channels) {
        return None;
    }
    let (w, h, stride, ch) = (
        w as usize,
        h as usize,
        rowstride.max(0) as usize,
        channels as usize,
    );
    if stride < w * ch || data.len() < stride * h {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = &data[y * stride..y * stride + w * ch];
        for px in row.chunks_exact(ch) {
            rgba.extend_from_slice(&[px[0], px[1], px[2], if ch == 4 { px[3] } else { 255 }]);
        }
    }
    let img = image::RgbaImage::from_raw(w as u32, h as u32, rgba)?;
    let mut cursor = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut cursor, image::ImageFormat::Png)
        .ok()?;
    normalize_png(&cursor.into_inner(), 192)
}

/// Resolve a freedesktop icon name (or an absolute path) to a small PNG. The
/// Deck has none of the PC's icon themes, so the bytes travel with the
/// notification.
#[cfg(target_os = "linux")]
fn icon_png(name: &str) -> Option<Vec<u8>> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    let path = std::path::Path::new(name);
    if path.is_absolute() {
        candidates.push(path.to_path_buf());
    } else {
        let stem = name
            .strip_suffix(".png")
            .or_else(|| name.strip_suffix(".xpm"))
            .unwrap_or(name);
        for dir in ["/usr/share/icons/hicolor", "/usr/local/share/icons/hicolor"] {
            for size in ["256x256", "128x128", "96x96", "64x64", "48x48"] {
                candidates.push(
                    std::path::Path::new(dir)
                        .join(size)
                        .join("apps")
                        .join(format!("{stem}.png")),
                );
            }
        }
        candidates.push(std::path::Path::new("/usr/share/pixmaps").join(format!("{stem}.png")));
        if let Some(home) = dirs::home_dir() {
            candidates.push(
                home.join(".local/share/icons/hicolor/128x128/apps")
                    .join(format!("{stem}.png")),
            );
        }
    }
    for candidate in candidates {
        if let Ok(bytes) = std::fs::read(&candidate) {
            if let Some(png) = normalize_png(&bytes, 128) {
                return Some(png);
            }
        }
    }
    None
}

/// Decode, shrink and re-encode any image the `image` crate understands, so
/// the payload stays small and the Deck only ever sees PNG.
fn normalize_png(bytes: &[u8], max: u32) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > max || img.height() > max {
        img.resize(max, max, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    img.write_to(&mut cursor, image::ImageFormat::Png).ok()?;
    let data = cursor.into_inner();
    if data.len() > 512 * 1024 {
        None
    } else {
        Some(data)
    }
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
        #[zbus(header)] header: zbus::message::Header<'_>,
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
        let picture = hints
            .get("image-data")
            .or_else(|| hints.get("image_data"))
            .or_else(|| hints.get("icon_data"))
            .and_then(image_hint)
            .or_else(|| icon_png(app_icon));
        let id = self.store.record(
            app_name,
            summary,
            body,
            app_icon,
            urgency,
            !actions.is_empty(),
            replaces_id as u64,
            picture.as_deref().unwrap_or(&[]),
            &actions,
            header.sender().map(|s| s.to_string()).as_deref(),
        );
        id as u32
    }

    fn close_notification(&self, id: u32) {
        self.store.dismiss_local(id as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::{notification_text, unescape};

    #[test]
    fn markup_becomes_plain_text_with_emoji() {
        let body = "<b>Готово</b> &amp; <i>сохранено</i> &#x1F600; <a href=\"x\">ссылка</a>";
        assert_eq!(notification_text(body), "Готово & сохранено 😀 ссылка");
    }

    #[test]
    fn unknown_tags_and_entities_survive() {
        assert_eq!(notification_text("a < b & c"), "a < b & c");
        assert_eq!(notification_text("&unknown; &#xZZ;"), "&unknown; &#xZZ;");
        assert_eq!(
            notification_text("<img src=\"x\" alt=\"картинка\"/>"),
            "картинка"
        );
        assert_eq!(notification_text("первая<br>вторая"), "первая\nвторая");
    }

    #[test]
    fn dbus_octal_escapes_decode() {
        assert_eq!(unescape("\\320\\237\\321\\200\\320\\270"), "При");
        assert_eq!(unescape("a\\nb\\\"c\\\\d"), "a\nb\"c\\d");
    }
}
