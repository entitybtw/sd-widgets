//! The HTTP API the Deck talks to. Every route except `/ping`, `/pair` and the landing page
//! requires the bearer token issued during pairing. Requests from public addresses are
//! refused unless the user explicitly allowed them.

use crate::config;
use crate::install;
use crate::state::AppState;
use sdw_core::model::*;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::Read;
use std::net::IpAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Request, Response, Server};

pub fn run(state: Arc<AppState>) -> Result<(), String> {
    let port = state.cfg().api_port;
    let addr = format!("0.0.0.0:{port}");
    let server = Server::http(&addr).map_err(|e| format!("HTTP {addr}: {e}"))?;
    state
        .logger
        .info(format!("HTTP API на http://0.0.0.0:{port}"));
    while !state.stop.load(Ordering::Relaxed) {
        match server.recv_timeout(Duration::from_millis(400)) {
            Ok(Some(request)) => {
                let st = state.clone();
                std::thread::spawn(move || handle(st, request));
            }
            Ok(None) => {}
            Err(e) => state.logger.warn(format!("HTTP: {e}")),
        }
    }
    Ok(())
}

fn handle(state: Arc<AppState>, mut req: Request) {
    let method = req.method().clone();
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let ip = req
        .remote_addr()
        .map(|a| a.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]));

    // Public routes first.
    if method == Method::Get && (path == "/" || path == "/index.html") {
        return respond_html(req, landing_page(&state));
    }
    if method == Method::Get && path == "/api/v1/ping" {
        let cfg = state.cfg();
        return respond_json(
            req,
            200,
            &Ping {
                name: cfg.name.clone(),
                version: VERSION.to_string(),
                protocol: PROTOCOL,
                daemon_id: cfg.daemon_id.clone(),
                setup_done: cfg.setup_done,
                auth_required: cfg.password_set(),
                platform: platform().to_string(),
            },
        );
    }
    if method == Method::Post && path == "/api/v1/pair" {
        return pair(state, req, ip);
    }
    if method == Method::Options {
        return respond_empty(req, 204);
    }

    if !public_client_allowed(&state, ip) {
        return error(req, 403, "Запросы из публичных сетей выключены в настройках");
    }
    if !authorized(&state, &req, query) {
        return error(req, 401, "Неверный или отсутствующий токен");
    }
    if let Ok(mut seen) = state.last_seen.lock() {
        seen.insert(ip.to_string(), Instant::now());
    }

    match (method.clone(), path) {
        (Method::Get, "/api/v1/status") => respond_json(req, 200, &state.status()),
        (Method::Get, "/api/v1/config") => respond_json(req, 200, &redacted_config(&state)),
        (Method::Patch, "/api/v1/config") => patch_config(state, req),
        (Method::Get, "/api/v1/logs") => {
            let since = query_u64(query, "since").unwrap_or(0);
            let entries = if since == 0 {
                state.logger.tail(200)
            } else {
                state.logger.since(since)
            };
            respond_json(req, 200, &entries)
        }

        (Method::Get, "/api/v1/pads") => respond_json(req, 200, &state.viiper.pads()),
        (Method::Post, "/api/v1/pads") => create_pad(state, req),
        (Method::Get, "/api/v1/viiper") => {
            let cfg = state.cfg();
            respond_json(req, 200, &state.viiper.status(&cfg))
        }
        (Method::Post, "/api/v1/viiper/action") => viiper_action(state, req),
        (Method::Post, "/api/v1/viiper/password") => viiper_password(state, req),

        (Method::Get, "/api/v1/monitor") => respond_json(req, 200, &state.monitor.status()),
        (Method::Post, "/api/v1/monitor/start") => monitor_start(state, req),
        (Method::Post, "/api/v1/monitor/stop") => {
            state.monitor.stop();
            respond_json(req, 200, &state.monitor.status())
        }
        (Method::Post, "/api/v1/monitor/input") => monitor_input(state, req),
        (Method::Get, "/api/v1/monitor/frame") => monitor_frame(state, req),

        (Method::Post, "/api/v1/remote") => remote_command(state, req),
        (Method::Get, "/api/v1/notifications") => {
            let since = query_u64(query, "since").unwrap_or(0);
            let items: Vec<Notification> = state
                .notifications
                .status()
                .recent
                .into_iter()
                .filter(|n| n.ts > since)
                .collect();
            respond_json(req, 200, &items)
        }
        (Method::Post, "/api/v1/notifications/clear") => {
            state.notifications.clear();
            respond_json(req, 200, &json!({"ok": true}))
        }
        (Method::Post, "/api/v1/notifications/test") => {
            state.notifications.add_test();
            respond_json(req, 200, &json!({"ok": true}))
        }

        (Method::Get, "/api/v1/ha/states") => ha_states(state, req),
        (Method::Post, "/api/v1/ha/config") => ha_config(state, req),
        (Method::Post, "/api/v1/ha/test") => ha_test(state, req),
        (Method::Post, "/api/v1/ha/call") => ha_call(state, req),
        (Method::Post, "/api/v1/ha/favorites") => ha_favorites(state, req),

        (Method::Post, "/api/v1/security/password") => security_password(state, req),
        (Method::Post, "/api/v1/security/rotate") => security_rotate(state, req),

        (Method::Post, "/api/v1/install") => install_start(state, req),
        (Method::Get, "/api/v1/install/status") => {
            let job = state.jobs.status();
            let mut v = install::install_status();
            if let Some(o) = v.as_object_mut() {
                o.insert("job".into(), job.to_json());
            }
            respond_json(req, 200, &v)
        }

        _ => {
            if let Some(id) = path.strip_prefix("/api/v1/pads/") {
                if let Some(id) = id.strip_suffix("/feedback") {
                    if method == Method::Get {
                        if let Some(pad) = state.viiper.pad(id) {
                            let info = pad.info();
                            return respond_json(
                                req,
                                200,
                                &PadFeedback {
                                    rumble: info.rumble,
                                    last_input_ms: info.last_input_ms,
                                    connected: info.connected,
                                },
                            );
                        }
                        return error(req, 404, "пад не найден");
                    }
                }
                if method == Method::Delete {
                    return match state.viiper.delete_pad(id) {
                        Ok(()) => respond_json(req, 200, &json!({"ok": true})),
                        Err(e) => error(req, 404, &e),
                    };
                }
            }
            if let Some(id) = path.strip_prefix("/api/v1/notifications/") {
                if let Some(id) = id.strip_suffix("/dismiss") {
                    if method == Method::Post {
                        if let Ok(id) = id.parse::<u64>() {
                            state.notifications.dismiss(id);
                            return respond_json(req, 200, &json!({"ok": true}));
                        }
                    }
                }
            }
            error(req, 404, "Неизвестный маршрут")
        }
    }
}

// ---------------------------------------------------------------- auth

fn authorized(state: &AppState, req: &Request, query: &str) -> bool {
    let cfg = state.cfg();
    if !cfg.password_set() {
        return false;
    }
    let token = cfg.api_token;
    let header = req
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("authorization"))
        .map(|h| h.value.as_str().to_string())
        .unwrap_or_default();
    let bearer = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .unwrap_or("");
    let q = query_param(query, "token").unwrap_or_default();
    (bearer == token && !token.is_empty()) || (q == token && !token.is_empty())
}

fn public_client_allowed(state: &AppState, ip: IpAddr) -> bool {
    let cfg = state.cfg();
    cfg.allow_public || ip.is_loopback() || sdw_core::util::is_private_ip(ip)
}

fn pair(state: Arc<AppState>, mut req: Request, ip: IpAddr) {
    let body: PairRequest = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let cfg = state.cfg();
    if !cfg.password_set() {
        return error(
            req,
            409,
            "На компьютере ещё не задан пароль. Откройте SD Widgets на ПК и завершите настройку.",
        );
    }
    // Five attempts per ten minutes per address.
    {
        let mut attempts = match state.pairing_attempts.lock() {
            Ok(a) => a,
            Err(_) => return error(req, 500, "внутренняя ошибка"),
        };
        let entry = attempts
            .entry(ip.to_string())
            .or_insert((0, Instant::now()));
        if entry.1.elapsed() > Duration::from_secs(600) {
            *entry = (0, Instant::now());
        }
        if entry.0 >= 5 {
            return error(req, 429, "Слишком много попыток. Подождите 10 минут.");
        }
        if !sdw_core::auth::verify_password(&body.password, &cfg.api_password_hash) {
            entry.0 += 1;
            state.logger.warn(format!("Неудачная попытка спаривания с {ip}"));
            return error(req, 403, "Неверный пароль");
        }
        *entry = (0, Instant::now());
    }
    let client_id = if body.client_id.trim().is_empty() {
        random_id(8)
    } else {
        body.client_id.trim().to_string()
    };
    let client_name = if body.client_name.trim().is_empty() {
        "Steam Deck".to_string()
    } else {
        body.client_name.trim().to_string()
    };
    if let Err(e) = state.update_cfg(|c| c.record_client(&client_id, &client_name)) {
        return error(req, 500, &e);
    }
    state.logger.info(format!(
        "Спарен клиент «{client_name}» ({client_id}) с {ip}"
    ));
    let cfg = state.cfg();
    respond_json(
        req,
        200,
        &PairResponse {
            token: cfg.api_token,
            daemon_name: cfg.name,
            daemon_id: cfg.daemon_id,
            version: VERSION.to_string(),
            api_port: cfg.api_port,
        },
    )
}

// ---------------------------------------------------------------- pads & viiper

fn create_pad(state: Arc<AppState>, mut req: Request) {
    let body: PadCreate = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let cfg = state.cfg();
    let name = if body.name.trim().is_empty() {
        format!("SD Widgets {}", state.viiper.pads().len() + 1)
    } else {
        body.name
    };
    match state.viiper.create_pad(&cfg, &name, body.kind) {
        Ok(created) => respond_json(req, 200, &created),
        Err(e) => error(req, 500, &e),
    }
}

fn viiper_action(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let action = body.get("action").and_then(Value::as_str).unwrap_or("");
    let cfg = state.cfg();
    let result = match action {
        "start" => state.viiper.start(&cfg),
        "stop" => {
            state.viiper.stop();
            Ok(())
        }
        "restart" => state.viiper.restart(&cfg),
        other => Err(format!("неизвестное действие: {other}")),
    };
    match result {
        Ok(()) => respond_json(req, 200, &state.viiper.status(&state.cfg())),
        Err(e) => error(req, 500, &e),
    }
}

fn viiper_password(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let password = body
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if let Err(e) = state.viiper.write_key(&password) {
        return error(req, 500, &e);
    }
    if let Err(e) = state.update_cfg(|c| c.viiper.password = password.clone()) {
        return error(req, 500, &e);
    }
    let cfg = state.cfg();
    if state.viiper.status(&cfg).running {
        if let Err(e) = state.viiper.restart(&cfg) {
            state.logger.warn(format!("Перезапуск VIIPER: {e}"));
        }
    }
    respond_json(req, 200, &state.viiper.status(&state.cfg()))
}

// ---------------------------------------------------------------- monitor

fn monitor_start(state: Arc<AppState>, mut req: Request) {
    let body: MonitorStart = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let cfg = state.cfg();
    match state.monitor.start(&body, &cfg) {
        Ok(status) => {
            let _ = state.update_cfg(|c| {
                c.monitor.mode = body.mode;
                c.monitor.backend = body.backend;
                c.monitor.output = status.output.clone();
                c.monitor.width = status.width;
                c.monitor.height = status.height;
                c.monitor.fps = status.fps;
                c.monitor.quality = status.quality;
                c.monitor.input_enabled = body.input_enabled;
            });
            respond_json(req, 200, &status)
        }
        Err(e) => error(req, 500, &e),
    }
}

fn monitor_input(state: Arc<AppState>, mut req: Request) {
    let body: MonitorInput = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    match state.monitor.input(&body) {
        Ok(()) => respond_json(req, 200, &json!({"ok": true})),
        Err(e) => error(req, 400, &e),
    }
}

fn monitor_frame(state: Arc<AppState>, req: Request) {
    match state.monitor.latest_frame() {
        Some(frame) => {
            let resp = Response::from_data(frame)
                .with_status_code(200)
                .with_header(header("Content-Type", "image/jpeg"))
                .with_header(header("Cache-Control", "no-store, max-age=0"))
                .with_header(header("Access-Control-Allow-Origin", "*"));
            let _ = req.respond(resp);
        }
        None => error(req, 404, "Кадр ещё не готов"),
    }
}

// ---------------------------------------------------------------- remote commands

/// A deliberately small whitelist: the deck can control media, sound and open things,
/// but cannot run arbitrary shell commands on the computer.
fn remote_command(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let command = body
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if command.is_empty() {
        return error(req, 400, "Пустая команда");
    }
    let result = match command.as_str() {
        "media_play_pause" | "media_next" | "media_prev" | "media_stop" => {
            let action = command.trim_start_matches("media_");
            let action = match action {
                "play_pause" => "play-pause",
                other => other,
            };
            media_command(action)
        }
        "volume_up" => volume_step("5%+"),
        "volume_down" => volume_step("5%-"),
        "volume_mute" => volume_mute(),
        "lock" => sdw_core::util::run("loginctl", &["lock-session"]).map(|_| "locked".into()),
        "monitor_start" => {
            let cfg = state.cfg();
            let req = MonitorStart {
                mode: cfg.monitor.mode,
                backend: cfg.monitor.backend,
                position: cfg.monitor.position,
                output: cfg.monitor.output.clone(),
                width: cfg.monitor.width,
                height: cfg.monitor.height,
                fps: cfg.monitor.fps,
                quality: cfg.monitor.quality,
                input_enabled: cfg.monitor.input_enabled,
                sudo_password: String::new(),
            };
            state.monitor.start(&req, &cfg).map(|s| {
                if s.running {
                    "screen started".to_string()
                } else {
                    "screen stopped".to_string()
                }
            })
        }
        "monitor_stop" => {
            state.monitor.stop();
            Ok("screen stopped".into())
        }
        _ => {
            if let Some(url) = command.strip_prefix("open:") {
                let url = url.trim();
                if url.starts_with("http://") || url.starts_with("https://") {
                    sdw_core::util::open_url(url).map(|_| "opened".into())
                } else {
                    Err("Можно открывать только http(s) ссылки".into())
                }
            } else if let Some(app) = command.strip_prefix("launch:") {
                let app = app.trim();
                if app.is_empty()
                    || !app
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                {
                    Err("Недопустимое имя приложения".into())
                } else {
                    sdw_core::util::run("gtk-launch", &[app])
                        .or_else(|_| sdw_core::util::run("xdg-open", &[app]))
                        .map(|_| "launched".into())
                }
            } else {
                Err("Неизвестная команда".into())
            }
        }
    };
    match result {
        Ok(message) => {
            state.logger.info(format!("Удалённая команда: {command}"));
            respond_json(req, 200, &json!({"ok": true, "message": message}))
        }
        Err(e) => error(req, 400, &e),
    }
}

fn media_command(action: &str) -> Result<String, String> {
    if sdw_core::util::has("playerctl") {
        sdw_core::util::run("playerctl", &[action]).map(|_| action.to_string())
    } else {
        Err("playerctl не установлен".into())
    }
}

fn volume_step(step: &str) -> Result<String, String> {
    if sdw_core::util::has("wpctl") {
        sdw_core::util::run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", step])
            .map(|_| "volume".into())
    } else if sdw_core::util::has("pactl") {
        let arg = if step.ends_with('+') { "+5%" } else { "-5%" };
        sdw_core::util::run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", arg])
            .map(|_| "volume".into())
    } else if sdw_core::util::has("amixer") {
        let arg = if step.ends_with('+') { "5%+" } else { "5%-" };
        sdw_core::util::run("amixer", &["sset", "Master", arg]).map(|_| "volume".into())
    } else {
        Err("Нет wpctl, pactl или amixer".into())
    }
}

fn volume_mute() -> Result<String, String> {
    if sdw_core::util::has("wpctl") {
        sdw_core::util::run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
            .map(|_| "mute".into())
    } else if sdw_core::util::has("pactl") {
        sdw_core::util::run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
            .map(|_| "mute".into())
    } else if sdw_core::util::has("amixer") {
        sdw_core::util::run("amixer", &["sset", "Master", "toggle"]).map(|_| "mute".into())
    } else {
        Err("Нет wpctl, pactl или amixer".into())
    }
}

// ---------------------------------------------------------------- notifications & HA

fn ha_states(state: Arc<AppState>, req: Request) {
    let mut ha = match state.ha.lock() {
        Ok(h) => h,
        Err(_) => return error(req, 500, "Home Assistant lock"),
    };
    if ha.entities.is_empty() || ha.last_sync.elapsed() > Duration::from_secs(10) {
        let _ = ha.refresh();
    }
    let states = HaStates {
        entities: ha.entities.clone(),
        fetched_ms: ha.status.last_sync_ms,
    };
    drop(ha);
    respond_json(req, 200, &states)
}

fn ha_config(state: Arc<AppState>, mut req: Request) {
    let body: HaConfigRequest = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let mut cfg = state.cfg();
    let mut ha = match state.ha.lock() {
        Ok(h) => h,
        Err(_) => return error(req, 500, "Home Assistant lock"),
    };
    match ha.set(&mut cfg, &body.url, &body.token, &body.favorites) {
        Ok(()) => {
            let favorites = cfg.ha.favorites.clone();
            let url = cfg.ha.url.clone();
            let token = cfg.ha.token.clone();
            drop(ha);
            let _ = state.update_cfg(|c| {
                c.ha.url = url;
                c.ha.token = token;
                c.ha.favorites = favorites;
            });
            respond_json(req, 200, &state.ha.lock().map(|h| h.status.clone()).unwrap_or_default())
        }
        Err(e) => error(req, 400, &e),
    }
}

fn ha_test(state: Arc<AppState>, req: Request) {
    let result = state.ha.lock().map(|mut h| h.refresh());
    match result {
        Ok(Ok(())) => respond_json(
            req,
            200,
            &state.ha.lock().map(|h| h.status.clone()).unwrap_or_default(),
        ),
        Ok(Err(e)) => error(req, 400, &e),
        Err(_) => error(req, 500, "Home Assistant lock"),
    }
}

fn ha_call(state: Arc<AppState>, mut req: Request) {
    let body: HaCall = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let result = state.ha.lock().map(|h| h.call(&body));
    match result {
        Ok(Ok(v)) => respond_json(req, 200, &v),
        Ok(Err(e)) => error(req, 400, &e),
        Err(_) => error(req, 500, "Home Assistant lock"),
    }
}

fn ha_favorites(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let favorites: Vec<String> = body
        .get("favorites")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let fav2 = favorites.clone();
    if let Err(e) = state.update_cfg(|c| c.ha.favorites = fav2) {
        return error(req, 500, &e);
    }
    if let Ok(mut ha) = state.ha.lock() {
        ha.status.favorites = favorites;
    }
    respond_json(req, 200, &json!({"ok": true}))
}

// ---------------------------------------------------------------- security & config

fn security_password(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let password = body.get("password").and_then(Value::as_str).unwrap_or("");
    if password.chars().count() < 4 {
        return error(req, 400, "Пароль должен быть не короче 4 символов");
    }
    if let Err(e) = state.update_cfg(|c| c.set_password(password)) {
        return error(req, 500, &e);
    }
    let cfg = state.cfg();
    respond_json(
        req,
        200,
        &PairResponse {
            token: cfg.api_token,
            daemon_name: cfg.name,
            daemon_id: cfg.daemon_id,
            version: VERSION.to_string(),
            api_port: cfg.api_port,
        },
    )
}

fn security_rotate(state: Arc<AppState>, req: Request) {
    let mut token = String::new();
    if let Err(e) = state.update_cfg(|c| token = c.rotate_token()) {
        return error(req, 500, &e);
    }
    let cfg = state.cfg();
    respond_json(
        req,
        200,
        &PairResponse {
            token,
            daemon_name: cfg.name,
            daemon_id: cfg.daemon_id,
            version: VERSION.to_string(),
            api_port: cfg.api_port,
        },
    )
}

fn patch_config(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let result = state.update_cfg(|c| {
        if let Some(v) = body.get("name").and_then(Value::as_str) {
            if !v.trim().is_empty() {
                c.name = v.trim().to_string();
            }
        }
        if let Some(v) = body.get("lang").and_then(Value::as_str) {
            c.lang = v.to_string();
        }
        if let Some(v) = body.get("dark").and_then(Value::as_bool) {
            c.dark = v;
        }
        if let Some(v) = body.get("autostart").and_then(Value::as_bool) {
            c.autostart = v;
        }
        if let Some(v) = body.get("allow_public").and_then(Value::as_bool) {
            c.allow_public = v;
        }
        if let Some(v) = body.get("notifications").and_then(|v| v.get("enabled")).and_then(Value::as_bool) {
            c.notifications.enabled = v;
        }
        if let Some(m) = body.get("monitor") {
            if let Some(v) = m.get("mode").and_then(|v| serde_json::from_value(v.clone()).ok()) {
                c.monitor.mode = v;
            }
            if let Some(v) = m.get("backend").and_then(|v| serde_json::from_value(v.clone()).ok()) {
                c.monitor.backend = v;
            }
            if let Some(v) = m.get("output").and_then(Value::as_str) {
                c.monitor.output = v.to_string();
            }
            if let Some(v) = m.get("width").and_then(Value::as_u64) {
                c.monitor.width = v as u32;
            }
            if let Some(v) = m.get("height").and_then(Value::as_u64) {
                c.monitor.height = v as u32;
            }
            if let Some(v) = m.get("fps").and_then(Value::as_u64) {
                c.monitor.fps = v as u32;
            }
            if let Some(v) = m.get("quality").and_then(Value::as_u64) {
                c.monitor.quality = v as u8;
            }
            if let Some(v) = m.get("input_enabled").and_then(Value::as_bool) {
                c.monitor.input_enabled = v;
            }
        }
        if let Some(v) = body.get("viiper") {
            if let Some(b) = v.get("enabled").and_then(Value::as_bool) {
                c.viiper.enabled = b;
            }
            if let Some(b) = v.get("auto_start").and_then(Value::as_bool) {
                c.viiper.auto_start = b;
            }
            if let Some(s) = v.get("api_addr").and_then(Value::as_str) {
                c.viiper.api_addr = s.to_string();
            }
            if let Some(s) = v.get("usb_addr").and_then(Value::as_str) {
                c.viiper.usb_addr = s.to_string();
            }
            if let Some(k) = v.get("default_pad").and_then(|v| serde_json::from_value(v.clone()).ok()) {
                c.viiper.default_pad = k;
            }
        }
    });
    match result {
        Ok(()) => {
            if let Ok(mut ha) = state.ha.lock() {
                let cfg = state.cfg();
                ha.status.favorites = cfg.ha.favorites.clone();
            }
            respond_json(req, 200, &redacted_config(&state))
        }
        Err(e) => error(req, 500, &e),
    }
}

fn install_start(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let components: Vec<String> = body
        .get("components")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if components.is_empty() {
        return error(req, 400, "Не выбрано ни одного компонента");
    }
    let sudo = body
        .get("sudo_password")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let state2 = state.clone();
    let id = match state.jobs.start("Установка компонентов", move |ctx| {
        let logger = state2.logger.clone();
        let _ = install::install(&components, &sudo, &ctx, &logger);
    }) {
        Ok(id) => id,
        Err(e) => return error(req, 409, &e),
    };
    respond_json(req, 200, &json!({"job_id": id}))
}

fn redacted_config(state: &AppState) -> Value {
    let cfg = state.cfg();
    json!({
        "name": cfg.name,
        "daemon_id": cfg.daemon_id,
        "api_port": cfg.api_port,
        "input_port": cfg.input_port,
        "setup_done": cfg.setup_done,
        "password_set": cfg.password_set(),
        "lang": cfg.lang,
        "dark": cfg.dark,
        "autostart": cfg.autostart,
        "allow_public": cfg.allow_public,
        "viiper": {
            "enabled": cfg.viiper.enabled,
            "auto_start": cfg.viiper.auto_start,
            "api_addr": cfg.viiper.api_addr,
            "usb_addr": cfg.viiper.usb_addr,
            "default_pad": cfg.viiper.default_pad,
            "password_set": !cfg.viiper.password.is_empty(),
        },
        "monitor": {
            "mode": cfg.monitor.mode,
            "backend": cfg.monitor.backend,
            "output": cfg.monitor.output,
            "width": cfg.monitor.width,
            "height": cfg.monitor.height,
            "fps": cfg.monitor.fps,
            "quality": cfg.monitor.quality,
            "input_enabled": cfg.monitor.input_enabled,
        },
        "notifications": { "enabled": cfg.notifications.enabled },
        "ha": {
            "url": cfg.ha.url,
            "configured": !cfg.ha.url.is_empty() && !cfg.ha.token.is_empty(),
            "favorites": cfg.ha.favorites,
        },
        "clients": cfg.clients,
        "config_dir": sdw_core::util::config_dir().display().to_string(),
    })
}

// ---------------------------------------------------------------- helpers

fn read_json<T: serde::de::DeserializeOwned>(req: &mut Request) -> Result<T, String> {
    let mut body = String::new();
    req.as_reader()
        .take(2 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| format!("JSON: {e}"))
}

fn query_param(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(percent_decode(v));
        }
    }
    None
}

fn query_u64(query: &str, key: &str) -> Option<u64> {
    query_param(query, key)?.parse().ok()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push((a * 16 + b) as u8);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

fn respond_json<T: Serialize>(req: Request, code: u16, value: &T) {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    let resp = Response::from_data(body)
        .with_status_code(code)
        .with_header(header("Content-Type", "application/json; charset=utf-8"))
        .with_header(header("Access-Control-Allow-Origin", "*"))
        .with_header(header("Cache-Control", "no-store"));
    let _ = req.respond(resp);
}

fn respond_html(req: Request, html: String) {
    let resp = Response::from_string(html)
        .with_status_code(200)
        .with_header(header("Content-Type", "text/html; charset=utf-8"))
        .with_header(header("Cache-Control", "no-store"));
    let _ = req.respond(resp);
}

fn respond_empty(req: Request, code: u16) {
    let _ = req.respond(Response::empty(code));
}

fn error(req: Request, code: u16, message: &str) {
    respond_json(req, code, &json!({"error": message}));
}

fn landing_page(state: &AppState) -> String {
    let cfg = state.cfg();
    let st = state.status();
    let addresses = st
        .addresses
        .iter()
        .map(|a| format!("<li><code>{a}</code></li>"))
        .collect::<Vec<_>>()
        .join("");
    let setup = if cfg.setup_done {
        "<span class=\"ok\">настроен</span>"
    } else {
        "<span class=\"warn\">требуется настройка</span>"
    };
    let lang = cfg.lang();
    let title = sdw_core::t!(lang, "SD Widgets · компьютер", "SD Widgets · computer");
    format!(
        r#"<!doctype html>
<html lang="ru"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>
:root{{color-scheme:dark;--bg:#111214;--ink:#eceae6;--muted:#8b8a86;--line:#2a2b2e;--accent:#3b82f6;--ok:#34d3b0;--warn:#ffc24a}}
*{{box-sizing:border-box}}body{{margin:0;background:var(--bg);color:var(--ink);font:14px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace}}
main{{max-width:720px;margin:8vh auto;padding:0 24px}}h1{{font-size:28px;letter-spacing:-.02em;margin:0 0 4px}}
p{{color:var(--muted)}}code{{background:#08090a;border:1px solid var(--line);padding:2px 6px;color:var(--ink)}}
ul{{padding-left:18px}}.ok{{color:var(--ok)}}.warn{{color:var(--warn)}}
.card{{border:1px solid var(--line);padding:18px;margin:18px 0}}a{{color:var(--accent)}}
.hud{{font-size:11px;letter-spacing:.16em;text-transform:uppercase;color:var(--muted)}}
</style></head><body><main>
<div class="hud">SD Widgets {version} · {platform}</div>
<h1>{name}</h1>
<p>{hostname} · {setup}</p>
<div class="card"><div class="hud">Адреса API</div><ul>{addresses}</ul></div>
<div class="card"><div class="hud">Как подключить Steam Deck</div>
<ol><li>Установите SD Widgets на Steam Deck.</li>
<li>Нажмите «Добавить компьютер» и выберите этот ПК в локальной сети.</li>
<li>Введите пароль, который вы задали в приложении на ПК.</li></ol>
<p>Пароль и токен передаются только по локальной сети. Публичный доступ выключен, если вы не включили его явно.</p></div>
<p><a href="https://git.entitybtw.ru/entitybtw/sd-widgets">git.entitybtw.ru/entitybtw/sd-widgets</a> · MIT</p>
</main></body></html>"#,
        version = VERSION,
        platform = platform(),
        name = html_escape(&cfg.name),
        hostname = html_escape(&st.hostname),
        setup = setup,
        addresses = addresses,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
