//! The HTTP API the Deck talks to. Every route except `/ping`, `/pair` and the landing page
//! requires the bearer token issued during pairing. Requests from public addresses are
//! refused unless the user explicitly allowed them.

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

    // A deck asks before it shares: an idle deck stays quiet, and a deck
    // that shares a few frames per second costs nothing while nobody looks.
    if method == Method::Get && path == "/api/v1/deck/want" {
        if !public_client_allowed(&state, ip) {
            return error(
                req,
                403,
                "Запросы из публичных сетей выключены в настройках",
            );
        }
        if !authorized(&state, &req, query) {
            return error(req, 401, "Неверный или отсутствующий токен");
        }
        return respond_json(
            req,
            200,
            &json!({
                "want": state.mirror.is_watched(),
                "fps": sdw_core::model::DECK_SHARE_FPS_MAX,
            }),
        );
    }

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
        return error(
            req,
            403,
            "Запросы из публичных сетей выключены в настройках",
        );
    }
    if !authorized(&state, &req, query) {
        return error(req, 401, "Неверный или отсутствующий токен");
    }
    if let Ok(mut seen) = state.last_seen.lock() {
        seen.insert(ip.to_string(), Instant::now());
    }

    match (method.clone(), path) {
        (Method::Get, "/api/v1/status") => respond_json(req, 200, &state.status()),
        // -------------------------------------------------------- deck mirror
        // A frame is not a viewer: the deck sends one quiet frame every few
        // seconds to keep its row alive. Marking the deck watched here made
        // that frame flip `want` on, so the deck immediately started a
        // full-rate stream (and its sound) with nobody looking. Only the
        // window's deck page and `GET /api/v1/deck` hold the watch open.
        (Method::Post, "/api/v1/deck/frame") => deck_frame(state, req, query),
        (Method::Post, "/api/v1/deck/audio") => deck_audio_chunk(state, req, query),
        (Method::Get, "/api/v1/deck") => {
            state.mirror.watching();
            respond_json(req, 200, &state.mirror.state())
        }
        (Method::Get, "/api/v1/deck/frame") => match state.mirror.frame() {
            Some((bytes, _)) => respond_bytes(req, 200, "image/jpeg", bytes),
            None => error(req, 404, "дека ещё не присылала кадров"),
        },
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
        (Method::Post, "/api/v1/addons/install") => addons_install(state, req),
        (Method::Post, "/api/v1/monitor/start") => monitor_start(state, req),
        (Method::Post, "/api/v1/monitor/stop") => {
            state.monitor.stop();
            respond_json(req, 200, &state.monitor.status())
        }
        (Method::Get, "/api/v1/monitor/frame") => monitor_frame(state, req),

        (Method::Get, "/api/v1/audio") => respond_json(req, 200, &state.audio.status()),
        (Method::Get, "/api/v1/audio/sources") => respond_json(req, 200, &state.audio.list_apps()),
        (Method::Post, "/api/v1/audio/start") => audio_start(state, req),
        (Method::Post, "/api/v1/audio/stop") => {
            state.audio.stop();
            respond_json(req, 200, &state.audio.status())
        }
        (Method::Get, "/api/v1/audio/stream") => audio_stream(state, req),

        (Method::Post, "/api/v1/remote") => remote_command(state, req),
        (Method::Post, "/api/v1/power") => power_command(state, req),
        (Method::Get, "/api/v1/bluetooth") => respond_json(
            req,
            200,
            &crate::bluetooth::status(sdw_core::Lang::from_code(&state.cfg().lang)),
        ),
        (Method::Post, "/api/v1/bluetooth/action") => bluetooth_action(state, req),
        (Method::Get, "/api/v1/apps") => apps_list(req),
        (Method::Post, "/api/v1/apps/launch") => app_launch(state, req),
        (Method::Get, "/api/v1/media") => respond_json(req, 200, &state.media.snapshot()),
        (Method::Post, "/api/v1/media/control") => media_control(state, req),
        (Method::Get, "/api/v1/media/art") => {
            let url = query_param(query, "url").unwrap_or_default();
            match state.media.art(&url) {
                Ok(bytes) => respond_bytes(req, 200, super::media::art_mime(&bytes), bytes),
                Err(e) => error(req, 404, &e),
            }
        }
        // The full history the store still remembers, newest first: what the
        // deck shows when the user wants to look back, not only what arrived
        // since the last poll.
        (Method::Get, "/api/v1/notifications/log") => {
            let items = state.notifications.log();
            respond_json(req, 200, &items)
        }
        (Method::Get, "/api/v1/notifications") => {
            let after_id = query_u64(query, "after_id");
            let since = query_u64(query, "since").unwrap_or(0);
            let items: Vec<Notification> = state
                .notifications
                .status()
                .recent
                .into_iter()
                .filter(|n| match after_id {
                    Some(id) => n.id > id,
                    None => n.ts > since,
                })
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
        (Method::Get, "/api/v1/ha/categories") => ha_categories_get(state, req),
        (Method::Post, "/api/v1/ha/categories") => ha_categories_set(state, req),
        (Method::Get, "/api/v1/ha/presets") => ha_presets_get(state, req),
        (Method::Post, "/api/v1/ha/presets") => ha_presets_set(state, req),

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
                // A tap on the notification on the deck runs the app's own
                // action on the computer, exactly like clicking it there.
                if let Some(id) = id.strip_suffix("/activate") {
                    if method == Method::Post {
                        if let Ok(id) = id.parse::<u64>() {
                            return match state.notifications.activate(id) {
                                Ok(()) => respond_json(req, 200, &json!({"ok": true})),
                                Err(e) => error(req, 409, &e),
                            };
                        }
                    }
                }
            }
            if let Some(id) = path.strip_prefix("/api/v1/apps/") {
                if let Some(id) = id.strip_suffix("/icon") {
                    if method == Method::Get {
                        return match crate::apps::icon(id) {
                            Ok((bytes, mime)) => respond_bytes(req, 200, mime, bytes),
                            Err(e) => error(req, 404, &e),
                        };
                    }
                }
            }
            error(req, 404, "Неизвестный маршрут")
        }
    }
}

// ---------------------------------------------------------------- deck mirror

/// One JPEG of the deck's screen, with what it shows in the query. The body
/// is the picture itself, so nothing is base64-encoded twice on the way in.
fn deck_frame(state: Arc<AppState>, mut req: Request, query: &str) {
    const LIMIT: u64 = 4 * 1024 * 1024;
    let mut body = Vec::new();
    if let Err(e) = req.as_reader().take(LIMIT).read_to_end(&mut body) {
        return error(req, 400, &format!("кадр не прочитан: {e}"));
    }
    if body.is_empty() {
        return error(req, 400, "пустой кадр");
    }
    if !body.starts_with(&[0xFF, 0xD8]) {
        return error(req, 415, "кадр должен быть jpeg");
    }
    let get = |key: &str| query_param(query, key).unwrap_or_default();
    let id = get("device");
    let name = get("name");
    let page = get("page");
    let version = get("version");
    let width = query_u64(query, "width").unwrap_or(0).min(8192) as u32;
    let height = query_u64(query, "height").unwrap_or(0).min(8192) as u32;
    state
        .mirror
        .ingest(&id, &name, &page, &version, width, height, body);
    respond_json(req, 200, &json!({"ok": true}))
}

/// One chunk of the deck's own sound, raw `s16le`, played on this computer.
/// Only while somebody watches the deck: a stray deck cannot open a sink.
fn deck_audio_chunk(state: Arc<AppState>, mut req: Request, query: &str) {
    if !state.mirror.is_watched() {
        return error(req, 409, "экран деки никто не смотрит");
    }
    const LIMIT: u64 = 512 * 1024;
    let mut body = Vec::new();
    if let Err(e) = req.as_reader().take(LIMIT).read_to_end(&mut body) {
        return error(req, 400, &format!("звук не прочитан: {e}"));
    }
    if body.is_empty() {
        return error(req, 400, "пустой кусок звука");
    }
    let rate = query_u64(query, "rate").unwrap_or(48_000) as u32;
    let channels = query_u64(query, "channels").unwrap_or(2) as u8;
    match state.deck_audio.play(rate, channels, &body) {
        Ok(()) => respond_json(
            req,
            200,
            &json!({"ok": true, "bytes": state.deck_audio.bytes()}),
        ),
        Err(e) => error(req, 503, &e),
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
        .find(|h| {
            h.field
                .as_str()
                .as_str()
                .eq_ignore_ascii_case("authorization")
        })
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
            state
                .logger
                .warn(format!("Неудачная попытка спаривания с {ip}"));
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

/// Install a package from a URL on the computer. The deck's store links a
/// package once and asks both sides to install it, so an addon with daemon
/// actions works on the computer without copying files by hand.
fn addons_install(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let url = body.get("url").and_then(Value::as_str).unwrap_or("").trim();
    if url.is_empty() {
        return error(req, 400, "Нужна ссылка на аддон");
    }
    match sdw_core::addons::install_url(url) {
        Ok(addon) => {
            let id = addon.id().to_string();
            let info = addon.manifest.clone();
            let fields = addon.manifest.settings.clone();
            let _ = state.update_cfg(|c| {
                c.addons.set(&id, true);
                // Seed declared defaults once; a reinstall keeps the values
                // the user already typed.
                for field in &fields {
                    if c.addons.value(&id, &field.key).is_null() {
                        c.addons.set_value(&id, &field.key, field.default.clone());
                    }
                }
            });
            state
                .logger
                .info(format!("Аддон установлен из ссылки: {}", info.name));
            respond_json(
                req,
                200,
                &json!({
                    "ok": true,
                    "id": id,
                    "name": info.name,
                    "version": info.version,
                }),
            )
        }
        Err(e) => error(req, 400, &e),
    }
}

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
                // Keep the requested output when the compositor made us fall
                // back to a mirror, so extend is retried on the next start.
                if status.mode == body.mode {
                    c.monitor.output = status.output.clone();
                }
                c.monitor.width = status.width;
                c.monitor.height = status.height;
                c.monitor.fps = status.fps;
                c.monitor.quality = status.quality;
            });
            respond_json(req, 200, &status)
        }
        Err(e) => error(req, 500, &e),
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
            if let Some(action) = PowerAction::parse(&command) {
                crate::apps::power(action)
            } else if let Some(url) = command.strip_prefix("open:") {
                let url = url.trim();
                // Only schemes that a browser or Steam handles on purpose.
                // `file:` and friends stay closed: the deck is not allowed to
                // turn a bind into arbitrary local data access.
                if url.starts_with("http://")
                    || url.starts_with("https://")
                    || url.starts_with("steam://")
                {
                    sdw_core::util::open_url(url).map(|_| "opened".into())
                } else {
                    Err("Можно открывать только http(s) или steam ссылки".into())
                }
            } else if let Some(rest) = command.strip_prefix("addon:") {
                let (id, action) = rest.split_once(':').unwrap_or((rest, ""));
                run_addon_action(&state, id, action)
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

/// Declared power actions over HTTP. The body may carry either the enum name
/// (`"lock"`) or a full object for a future version that needs options.
fn power_command(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let name = body.get("action").and_then(Value::as_str).unwrap_or("");
    let Some(action) = PowerAction::parse(name) else {
        return error(req, 400, "Неизвестное действие питания");
    };
    match crate::apps::power(action) {
        Ok(message) => {
            state
                .logger
                .info(format!("Питание: {} ({message})", action.key()));
            respond_json(req, 200, &json!({"ok": true, "message": message}))
        }
        Err(e) => error(req, 400, &e),
    }
}

/// Bluetooth actions: scan, connect, pair, trust, remove. The address is
/// validated in the Bluetooth module before it reaches `bluetoothctl`.
fn bluetooth_action(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let action = body.get("action").and_then(Value::as_str).unwrap_or("");
    let address = body
        .get("address")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let lang = sdw_core::Lang::from_code(&state.cfg().lang);
    match crate::bluetooth::action(action, address, lang) {
        Ok(message) => {
            state
                .logger
                .info(format!("Bluetooth {action} {address}: {message}"));
            respond_json(req, 200, &json!({"ok": true, "message": message}))
        }
        Err(e) => error(req, 400, &e),
    }
}

/// The application list. Sanitized entries only: no command line leaves the
/// daemon, so the deck can only ask for an app that is already installed here.
fn apps_list(req: Request) {
    let list: Vec<AppEntry> = crate::apps::scan().into_iter().map(|a| a.entry).collect();
    respond_json(req, 200, &list);
}

fn app_launch(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let id = body.get("id").and_then(Value::as_str).unwrap_or("");
    match crate::apps::launch(id) {
        Ok(message) => {
            state.logger.info(format!("Приложение: {message}"));
            respond_json(req, 200, &json!({"ok": true, "message": message}))
        }
        Err(e) => error(req, 400, &e),
    }
}

fn media_control(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let action = body
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if action.is_empty() {
        return error(req, 400, "Пустое действие");
    }
    let value = body.get("value").and_then(Value::as_f64).unwrap_or(0.0);
    let player = body
        .get("player")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match state.media.control(&action, value, &player) {
        Ok(message) => respond_json(req, 200, &json!({"ok": true, "message": message})),
        Err(e) => error(req, 400, &e),
    }
}

/// Run an action declared by an enabled addon. Addon actions are installed by
/// the user themselves, so `Shell` is allowed here, unlike arbitrary deck input.
pub(crate) fn run_addon_action(
    state: &Arc<AppState>,
    id: &str,
    action_id: &str,
) -> Result<String, String> {
    if id.trim().is_empty() || action_id.trim().is_empty() {
        return Err("Формат: addon:<id>:<action>".into());
    }
    let cfg = state.cfg();
    if !cfg.addons.is_on(id) {
        return Err(format!("Аддон {id} выключен"));
    }
    let addon = sdw_core::addons::find(id).ok_or_else(|| format!("Нет аддона {id}"))?;
    let entry = addon
        .manifest
        .daemon
        .actions
        .iter()
        .find(|a| a.id == action_id)
        .ok_or_else(|| format!("Нет действия {action_id} у аддона {id}"))?;
    // A package declares its own settings; fill `{key}` right before running,
    // so changing a value applies to the next call without reinstalling.
    let vars = sdw_core::addons::variables(&cfg.addons, id);
    let action = sdw_core::addons::expand_action(&entry.action, &vars);
    let label = addon.manifest.name.clone();
    let result = match &action {
        Action::None => Ok("ok".into()),
        Action::Shell { command } => run_shell(command),
        Action::Launch { target } => sdw_core::util::run("gtk-launch", &[target])
            .or_else(|_| sdw_core::util::run("xdg-open", &[target]))
            .map(|_| "launched".into()),
        Action::Url { url } => {
            if url.starts_with("http://") || url.starts_with("https://") {
                sdw_core::util::open_url(url).map(|_| "opened".into())
            } else {
                Err("Можно открывать только http(s) ссылки".into())
            }
        }
        Action::Volume { delta } => volume_step(&format!("{delta:+}%")),
        Action::VolumeSet { percent } => {
            if sdw_core::util::has("wpctl") {
                sdw_core::util::run(
                    "wpctl",
                    &["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{percent}%")],
                )
                .map(|_| "volume".into())
            } else {
                Err("Нужен wpctl".into())
            }
        }
        Action::Mute => volume_mute(),
        Action::Media { command } => {
            let action = command.replace('_', "-");
            media_command(&action)
        }
        Action::Brightness { delta } => brightness_step(*delta),
        Action::BrightnessSet { percent } => {
            if sdw_core::util::has("brightnessctl") {
                sdw_core::util::run("brightnessctl", &["set", &format!("{percent}%")])
                    .map(|_| "brightness".into())
            } else {
                Err("Нужен brightnessctl".into())
            }
        }
        Action::Key { combo } => {
            if sdw_core::util::has("wtype") {
                sdw_core::util::run("wtype", &["-k", combo]).map(|_| "key".into())
            } else if sdw_core::util::has("ydotool") {
                sdw_core::util::run("ydotool", &["key", combo]).map(|_| "key".into())
            } else {
                Err("Нужен wtype или ydotool".into())
            }
        }
        Action::Page { .. } => Err("Страницы деки не работают на компьютере".into()),
        Action::Remote { .. } => Err("Вложенные удалённые команды запрещены".into()),
        Action::Ha { .. } => Err("Home Assistant выполняется на деке".into()),
        Action::Http {
            method,
            url,
            headers,
            body,
        } => sdw_core::util::http_call(method, url, headers, body),
    };
    result.map(|message| {
        state
            .logger
            .info(format!("Аддон {label}: {action_id} -> {message}"));
        format!("{label}: {message}")
    })
}

fn run_shell(command: &str) -> Result<String, String> {
    let out = std::process::Command::new("sh")
        .arg("-lc")
        .arg(command)
        .output()
        .map_err(|e| format!("sh: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.success() {
        let line = stdout.lines().next().unwrap_or("").trim().to_string();
        Ok(if line.is_empty() { "ok".into() } else { line })
    } else {
        let line = stderr.lines().next().unwrap_or("").trim().to_string();
        Err(if line.is_empty() {
            format!("код {}", out.status.code().unwrap_or(-1))
        } else {
            line
        })
    }
}

fn brightness_step(delta: i32) -> Result<String, String> {
    if sdw_core::util::has("brightnessctl") {
        let arg = if delta >= 0 {
            format!("{delta}%+")
        } else {
            format!("{}%-", -delta)
        };
        sdw_core::util::run("brightnessctl", &["set", &arg]).map(|_| "brightness".into())
    } else {
        Err("Нужен brightnessctl".into())
    }
}

// ---------------------------------------------------------------- audio

fn audio_start(state: Arc<AppState>, mut req: Request) {
    let body: AudioStart = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let cfg = state.cfg();
    match state.audio.start(&cfg.audio, &body, true) {
        Ok(status) => {
            state.logger.info(format!(
                "Звук с ПК включён: {} · {} Гц",
                status.source, status.rate
            ));
            respond_json(req, 200, &status)
        }
        Err(e) => error(req, 400, &e),
    }
}

/// Raw `s16le` PCM until the stream stops or the client goes away.
fn audio_stream(state: Arc<AppState>, req: Request) {
    let Some(reader) = state.audio.reader() else {
        return error(req, 409, "Звук с ПК не запущен");
    };
    let status = state.audio.status();
    let header =
        |name: &str, value: &str| Header::from_bytes(name.as_bytes(), value.as_bytes()).ok();
    let mut headers = Vec::new();
    if let Some(h) = header(
        "Content-Type",
        &format!(
            "audio/L16;rate={};channels={}",
            status.rate, status.channels
        ),
    ) {
        headers.push(h);
    }
    if let Some(h) = header("Cache-Control", "no-store") {
        headers.push(h);
    }
    let _ = req.respond(Response::new(
        tiny_http::StatusCode(200),
        headers,
        reader,
        None,
        None,
    ));
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
    // Never fetch here: the worker does the I/O, this returns what it has plus
    // the age of the data. A slow Home Assistant must not stall the deck.
    let mut ha = match state.ha.lock() {
        Ok(h) => h,
        Err(_) => return error(req, 500, "Home Assistant lock"),
    };
    if ha.fetched_ms == 0 && ha.last_attempt.elapsed() > Duration::from_secs(15) {
        ha.mark_dirty();
    }
    let states = HaStates {
        entities: ha.entities.clone(),
        fetched_ms: ha.fetched_ms,
        age_ms: ha.age_ms(),
        connected: ha.status.connected,
        error: ha.error.clone(),
        areas: ha.areas.clone(),
        views: ha.views.clone(),
        categories: ha.categories.clone(),
        presets: ha.presets.clone(),
    };
    drop(ha);
    respond_json(req, 200, &states)
}

fn ha_config(state: Arc<AppState>, mut req: Request) {
    let body: HaConfigRequest = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let client = sdw_core::ha::HaClient::new(&body.url, &body.token);
    // The ping runs before any lock is taken: a wrong address times out here,
    // and the daemon keeps answering everything else meanwhile.
    let message = match client.ping() {
        Ok(m) => m,
        Err(e) => return error(req, 400, &e),
    };
    let mut cfg = state.cfg();
    let mut ha = match state.ha.lock() {
        Ok(h) => h,
        Err(_) => return error(req, 500, "Home Assistant lock"),
    };
    ha.install(&mut cfg, client, &message, &body.favorites);
    let favorites = cfg.ha.favorites.clone();
    let url = cfg.ha.url.clone();
    let token = body.token.trim().to_string();
    drop(ha);
    let _ = state.update_cfg(|c| {
        c.ha.url = url;
        c.ha.token = token;
        c.ha.favorites = favorites;
    });
    respond_json(
        req,
        200,
        &state
            .ha
            .lock()
            .map(|h| h.status.clone())
            .unwrap_or_default(),
    )
}

/// Reach Home Assistant now and report the result. Used by "проверить".
fn ha_test(state: Arc<AppState>, req: Request) {
    let client = match state.ha.lock() {
        Ok(h) => h.client.clone(),
        Err(_) => return error(req, 500, "Home Assistant lock"),
    };
    let Some(client) = client else {
        return error(req, 400, "Home Assistant не настроен");
    };
    let entities = match client.states() {
        Ok(list) => list,
        Err(e) => {
            if let Ok(mut h) = state.ha.lock() {
                h.apply(Err(e.clone()), None, None);
            }
            return error(req, 400, &e);
        }
    };
    let rooms = Some(client.rooms());
    let views = Some(sdw_core::ha::views_guarded(&client));
    if let Ok(mut h) = state.ha.lock() {
        h.apply(Ok(entities), rooms, views);
    }
    respond_json(
        req,
        200,
        &state
            .ha
            .lock()
            .map(|h| h.status.clone())
            .unwrap_or_default(),
    )
}

fn ha_call(state: Arc<AppState>, mut req: Request) {
    let body: HaCall = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let client = match state.ha.lock() {
        Ok(h) => h.client.clone(),
        Err(_) => return error(req, 500, "Home Assistant lock"),
    };
    let Some(client) = client else {
        return error(req, 400, "Home Assistant не настроен");
    };
    match client.call(&body) {
        Ok(v) => {
            // Show the new state right away instead of up to three seconds later.
            if let Ok(mut h) = state.ha.lock() {
                h.mark_dirty();
            }
            respond_json(req, 200, &v)
        }
        Err(e) => error(req, 400, &e),
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
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
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

fn ha_categories_get(state: Arc<AppState>, req: Request) {
    let list = state
        .ha
        .lock()
        .map(|h| h.categories.clone())
        .unwrap_or_default();
    respond_json(req, 200, &list)
}

fn ha_categories_set(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let incoming: Vec<HaCategory> =
        serde_json::from_value(body.get("categories").cloned().unwrap_or(json!([])))
            .unwrap_or_default();
    let clean = match state.ha.lock() {
        Ok(mut h) => h.set_categories(incoming),
        Err(_) => return error(req, 500, "Home Assistant lock"),
    };
    let saved = clean.clone();
    if let Err(e) = state.update_cfg(|c| c.ha.categories = saved) {
        return error(req, 500, &e);
    }
    respond_json(req, 200, &clean)
}

fn ha_presets_get(state: Arc<AppState>, req: Request) {
    let list = state
        .ha
        .lock()
        .map(|h| h.presets.clone())
        .unwrap_or_default();
    respond_json(req, 200, &list)
}

fn ha_presets_set(state: Arc<AppState>, mut req: Request) {
    let body: Value = match read_json(&mut req) {
        Ok(b) => b,
        Err(e) => return error(req, 400, &e),
    };
    let incoming: Vec<HaPreset> =
        serde_json::from_value(body.get("presets").cloned().unwrap_or(json!([])))
            .unwrap_or_default();
    let clean = match state.ha.lock() {
        Ok(mut h) => h.set_presets(incoming),
        Err(_) => return error(req, 500, "Home Assistant lock"),
    };
    let saved = clean.clone();
    if let Err(e) = state.update_cfg(|c| c.ha.presets = saved) {
        return error(req, 500, &e);
    }
    respond_json(req, 200, &clean)
}

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
        if let Some(v) = body
            .get("notifications")
            .and_then(|v| v.get("enabled"))
            .and_then(Value::as_bool)
        {
            c.notifications.enabled = v;
        }
        if let Some(v) = body
            .get("notifications")
            .and_then(|v| v.get("own_service"))
            .and_then(Value::as_bool)
        {
            c.notifications.own_service = v;
        }
        if let Some(m) = body.get("monitor") {
            if let Some(v) = m
                .get("mode")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
            {
                c.monitor.mode = v;
            }
            if let Some(v) = m
                .get("backend")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
            {
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
            if let Some(k) = v
                .get("default_pad")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
            {
                c.viiper.default_pad = k;
            }
        }
    });
    match result {
        Ok(()) => {
            if let Some(enabled) = body.get("autostart").and_then(Value::as_bool) {
                crate::autostart::apply(enabled, &state.logger);
            }
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
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
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
        },
        "notifications": { "enabled": cfg.notifications.enabled, "own_service": cfg.notifications.own_service },
        "ha": {
            "url": cfg.ha.url,
            "configured": !cfg.ha.url.is_empty() && !cfg.ha.token.is_empty(),
            "favorites": cfg.ha.favorites,
            "categories": cfg.ha.categories,
            "presets": cfg.ha.presets,
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

fn respond_bytes(req: Request, code: u16, content_type: &str, bytes: Vec<u8>) {
    let resp = Response::from_data(bytes)
        .with_status_code(code)
        .with_header(header("Content-Type", content_type))
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
