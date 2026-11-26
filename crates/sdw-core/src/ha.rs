//! Home Assistant REST client and entity helpers.

use crate::model::{HaCall, HaEntity, HaPresetAction, HaStates, HaView};
use serde_json::Value;
use std::time::Duration;

/// Snapshot every lamp that is on into preset actions: service data only, so
/// the preset can be re-applied after any restart.
pub fn capture_light_preset(entities: &[HaEntity]) -> Vec<HaPresetAction> {
    let mut actions = Vec::new();
    for e in entities {
        if e.domain() != "light" || !e.is_on() {
            continue;
        }
        let mut data = serde_json::Map::new();
        if let Some(pct) = e.brightness_pct() {
            data.insert("brightness_pct".into(), serde_json::json!(pct));
        }
        if let Some(k) = e.color_temp_kelvin() {
            data.insert("color_temp_kelvin".into(), serde_json::json!(k));
        }
        if let Some(rgb) = e.rgb_color() {
            data.insert("rgb_color".into(), serde_json::json!(rgb));
        }
        actions.push(HaPresetAction {
            entity_id: e.entity_id.clone(),
            service: "turn_on".into(),
            data: serde_json::Value::Object(data),
        });
    }
    actions
}

/// Which Home Assistant service a plain action means for a domain.
///
/// The deck and the daemon speak plain words — on, off, open, close, run — so a
/// scenario or a bind never has to know that a cover opens with `open_cover`
/// while a lock unlocks with `unlock`. `None` means the domain has no such
/// action.
pub fn service_for(domain: &str, action: &str) -> Option<&'static str> {
    match (domain, action) {
        ("light" | "switch" | "input_boolean" | "siren" | "humidifier" | "fan", "on") => {
            Some("turn_on")
        }
        ("light" | "switch" | "input_boolean" | "siren" | "humidifier" | "fan", "off") => {
            Some("turn_off")
        }
        ("light" | "switch" | "input_boolean" | "siren" | "humidifier" | "fan", "toggle") => {
            Some("toggle")
        }
        ("scene", "run") | ("scene", "on") => Some("turn_on"),
        ("script", "run") | ("script", "on") => Some("turn_on"),
        ("script", "off") => Some("turn_off"),
        ("script", "toggle") => Some("toggle"),
        ("automation", "run") => Some("trigger"),
        ("automation", "on") => Some("turn_on"),
        ("automation", "off") => Some("turn_off"),
        ("automation", "toggle") => Some("toggle"),
        ("button", "run") | ("button", "press") => Some("press"),
        ("cover", "open") => Some("open_cover"),
        ("cover", "close") => Some("close_cover"),
        ("cover", "stop") => Some("stop_cover"),
        ("cover", "toggle") => Some("toggle"),
        ("valve", "open") => Some("open_valve"),
        ("valve", "close") => Some("close_valve"),
        ("valve", "stop") => Some("stop_valve"),
        ("lock", "unlock") | ("lock", "open") | ("lock", "on") => Some("unlock"),
        ("lock", "lock") | ("lock", "close") | ("lock", "off") => Some("lock"),
        ("media_player", "play") => Some("media_play"),
        ("media_player", "pause") => Some("media_pause"),
        ("media_player", "play_pause") => Some("media_play_pause"),
        ("media_player", "next") => Some("media_next_track"),
        ("media_player", "previous") => Some("media_previous_track"),
        ("media_player", "stop") => Some("media_stop"),
        ("vacuum", "start") => Some("start"),
        ("vacuum", "stop") => Some("stop"),
        ("vacuum", "dock") => Some("return_to_base"),
        ("vacuum", "pause") => Some("pause"),
        ("update", "install") => Some("install"),
        ("homeassistant", "on") => Some("turn_on"),
        ("homeassistant", "off") => Some("turn_off"),
        ("homeassistant", "toggle") => Some("toggle"),
        _ => None,
    }
}

/// The next command for a cover driven by one button, like a gate remote.
///
/// A moving cover stops, an open one (including a partly open one, which
/// Home Assistant reports as `open`) closes, and anything else opens. Every
/// press is a single service call, so a slow cover does not need a second
/// tap to choose a direction.
pub fn cover_step(state: &str) -> &'static str {
    match state {
        "opening" | "closing" => "stop_cover",
        "open" => "close_cover",
        _ => "open_cover",
    }
}

/// True when `service` is a real service for `domain`.
///
/// A scenario arrives from another deck over the network, so the daemon keeps a
/// tight list of the services its UI can produce instead of forwarding any
/// string into `/api/services/{domain}/{service}`.
pub fn known_service(domain: &str, service: &str) -> bool {
    let list: &[&str] = match domain {
        "light" | "switch" | "input_boolean" | "siren" | "humidifier" => {
            &["turn_on", "turn_off", "toggle"]
        }
        "fan" => &[
            "turn_on",
            "turn_off",
            "toggle",
            "set_percentage",
            "increase_speed",
            "decrease_speed",
        ],
        "cover" => &[
            "open_cover",
            "close_cover",
            "stop_cover",
            "toggle",
            "set_cover_position",
            // One-button control: resolved to open/stop/close from the
            // cover's state by whoever sends the call.
            "step",
        ],
        "valve" => &["open_valve", "close_valve", "stop_valve"],
        "lock" => &["lock", "unlock", "open"],
        "scene" => &["turn_on"],
        "script" => &["turn_on", "turn_off", "toggle"],
        "automation" => &["trigger", "turn_on", "turn_off", "toggle"],
        "button" => &["press"],
        "media_player" => &[
            "media_play",
            "media_pause",
            "media_play_pause",
            "media_next_track",
            "media_previous_track",
            "media_stop",
            "volume_set",
            "volume_up",
            "volume_down",
        ],
        "climate" => &[
            "set_temperature",
            "set_hvac_mode",
            "set_fan_mode",
            "turn_on",
            "turn_off",
        ],
        "input_number" | "number" => &["set_value"],
        "input_select" | "select" => &["select_option"],
        "water_heater" => &["turn_on", "turn_off", "set_temperature"],
        "vacuum" => &["start", "stop", "return_to_base", "pause"],
        "update" => &["install"],
        "homeassistant" => &["turn_on", "turn_off", "toggle"],
        _ => &[],
    };
    list.contains(&service)
}

#[derive(Clone)]
pub struct HaClient {
    base: String,
    token: String,
    agent: ureq::Agent,
}

impl HaClient {
    pub fn new(url: &str, token: &str) -> Self {
        let base = url.trim().trim_end_matches('/').to_string();
        let base = if base.starts_with("http://") || base.starts_with("https://") {
            base
        } else {
            format!("http://{base}")
        };
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .user_agent(concat!("sd-widgets/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self {
            base,
            token: token.trim().to_string(),
            agent,
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        let (status, text) = self.get_text(path)?;
        if status != 200 {
            return Err(format!(
                "Home Assistant: HTTP {status}: {}",
                crate::util::truncate(&text, 240)
            ));
        }
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    fn get_text(&self, path: &str) -> Result<(u16, String), String> {
        let mut r = self
            .agent
            .get(&format!("{}{}", self.base, path))
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| e.to_string())?;
        let status = r.status().as_u16();
        let text = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
        Ok((status, text))
    }

    pub fn ping(&self) -> Result<String, String> {
        let v = self.get("/api/")?;
        Ok(v.get("message")
            .and_then(Value::as_str)
            .unwrap_or("API OK")
            .to_string())
    }

    /// Render a Home Assistant template. Used for area (room) names, which the
    /// plain REST API does not expose.
    ///
    /// `/api/template` lives on the front end, so it is not there on every
    /// installation and a 404 for it is normal: the caller then keeps the
    /// rooms as plain entities instead of failing the whole fetch.
    pub fn template(&self, tpl: &str) -> Result<String, String> {
        let mut r = self
            .agent
            .post(&format!("{}/api/template", self.base))
            .header("Authorization", &format!("Bearer {}", self.token))
            .send_json(serde_json::json!({ "template": tpl }))
            .map_err(|e| e.to_string())?;
        let status = r.status().as_u16();
        let text = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "Home Assistant: HTTP {status}: {}",
                crate::util::truncate(&text, 240)
            ));
        }
        Ok(text)
    }

    /// Area names and the entity → area mapping. Two template outputs are
    /// combined in one request; a failure is not fatal (the deck then falls
    /// back to a single "home" room).
    pub fn rooms(&self) -> Result<(Vec<String>, Vec<(String, String)>), String> {
        let tpl = "{% for a in areas() %}~A~{{ a }}\t{{ area_name(a) or a }}\n{% endfor %}{% for s in states %}{% set a = area_id(s.entity_id) %}{% if a %}~E~{{ s.entity_id }}\t{{ area_name(a) or a }}\n{% endif %}{% endfor %}";
        let text = self.template(tpl)?;
        let mut areas = Vec::new();
        let mut entities = Vec::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("~A~") {
                let (id, name) = rest.split_once('\t').unwrap_or((rest, rest));
                let name = if name.trim().is_empty() { id } else { name };
                if !name.trim().is_empty() {
                    areas.push(name.trim().to_string());
                }
            } else if let Some(rest) = line.strip_prefix("~E~") {
                let (id, name) = rest.split_once('\t').unwrap_or((rest, ""));
                if !id.trim().is_empty() {
                    let area = if name.trim().is_empty() {
                        String::new()
                    } else {
                        name.trim().to_string()
                    };
                    if !area.is_empty() {
                        entities.push((id.trim().to_string(), area));
                    }
                }
            }
        }
        areas.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
        areas.dedup();
        Ok((areas, entities))
    }

    /// Every state, without the area lookup: this is the cheap call the daemon
    /// polls. `rooms` is a separate template that is worth caching.
    pub fn states(&self) -> Result<Vec<HaEntity>, String> {
        let v = self.get("/api/states")?;
        serde_json::from_value(v).map_err(|e| e.to_string())
    }

    /// Apply a cached entity → area map to a fresh state list.
    pub fn apply_areas(entities: &mut [HaEntity], map: &[(String, String)]) {
        for e in entities {
            if let Some((_, area)) = map.iter().find(|(id, _)| id == &e.entity_id) {
                e.area = area.clone();
            }
        }
    }

    pub fn snapshot(&self) -> Result<HaStates, String> {
        let entities = self.states()?;
        let (areas, _) = self.rooms().unwrap_or_default();
        Ok(HaStates {
            entities,
            fetched_ms: crate::model::now_ms(),
            areas,
            ..HaStates::default()
        })
    }

    /// Tabs of the Lovelace dashboard, read through the WebSocket API: the
    /// plain REST API does not expose them. A dashboard that is generated from
    /// areas has no stored views, which is a normal answer, not an error.
    pub fn views(&self) -> Result<Vec<HaView>, String> {
        let ws = if let Some(rest) = self.base.strip_prefix("https://") {
            format!("wss://{rest}/api/websocket")
        } else if let Some(rest) = self.base.strip_prefix("http://") {
            format!("ws://{rest}/api/websocket")
        } else {
            return Err("Home Assistant: неподдерживаемый адрес".into());
        };
        let (mut socket, _) = tungstenite::connect(&ws).map_err(|e| e.to_string())?;
        let first = read_json(&mut socket)?;
        if first.get("type").and_then(Value::as_str) != Some("auth_required") {
            return Err("Home Assistant: неожиданный ответ WebSocket".into());
        }
        socket
            .send(tungstenite::Message::text(
                serde_json::json!({ "type": "auth", "access_token": self.token }).to_string(),
            ))
            .map_err(|e| e.to_string())?;
        let auth = read_json(&mut socket)?;
        match auth.get("type").and_then(Value::as_str) {
            Some("auth_ok") => {}
            Some("auth_invalid") => return Err("Home Assistant: токен отклонён".into()),
            _ => return Err("Home Assistant: не удалось авторизоваться".into()),
        }
        socket
            .send(tungstenite::Message::text(
                serde_json::json!({ "id": 1, "type": "lovelace/config" }).to_string(),
            ))
            .map_err(|e| e.to_string())?;
        let reply = read_json(&mut socket)?;
        let _ = socket.close(None);
        if reply.get("success").and_then(Value::as_bool) != Some(true) {
            let msg = reply
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("дашборд недоступен");
            return Err(format!("Home Assistant: {msg}"));
        }
        let result = reply.get("result").cloned().unwrap_or(Value::Null);
        let views = result
            .get("views")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::new();
        for (i, v) in views.iter().enumerate() {
            let mut entities = Vec::new();
            collect_entities(v.get("cards").unwrap_or(&Value::Null), &mut entities);
            let title = v
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| v.get("path").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| format!("{}", i + 1));
            out.push(HaView {
                title,
                path: v
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                icon: v
                    .get("icon")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                entities,
            });
        }
        Ok(out)
    }

    pub fn call(&self, call: &HaCall) -> Result<Value, String> {
        let mut body = serde_json::Map::new();
        if let Some(t) = call.target.as_object() {
            for (k, v) in t {
                body.insert(k.clone(), v.clone());
            }
        }
        if let Some(d) = call.data.as_object() {
            for (k, v) in d {
                body.insert(k.clone(), v.clone());
            }
        }
        let mut r = self
            .agent
            .post(&format!(
                "{}/api/services/{}/{}",
                self.base, call.domain, call.service
            ))
            .header("Authorization", &format!("Bearer {}", self.token))
            .send_json(Value::Object(body))
            .map_err(|e| e.to_string())?;
        let status = r.status().as_u16();
        let text = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "Home Assistant: HTTP {status}: {}",
                crate::util::truncate(&text, 240)
            ));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub fn toggle(&self, entity_id: &str, on: bool) -> Result<Value, String> {
        self.call(&HaCall {
            domain: "homeassistant".into(),
            service: if on {
                "turn_on".into()
            } else {
                "turn_off".into()
            },
            target: serde_json::json!({ "entity_id": entity_id }),
            data: Value::Null,
        })
    }

    pub fn set_brightness(&self, entity_id: &str, brightness: u8) -> Result<Value, String> {
        self.call(&HaCall {
            domain: "light".into(),
            service: "turn_on".into(),
            target: serde_json::json!({ "entity_id": entity_id }),
            data: serde_json::json!({ "brightness": (brightness as u16 * 255 / 100).min(255) }),
        })
    }

    pub fn set_temperature(&self, entity_id: &str, temp: f32) -> Result<Value, String> {
        self.call(&HaCall {
            domain: "climate".into(),
            service: "set_temperature".into(),
            target: serde_json::json!({ "entity_id": entity_id }),
            data: serde_json::json!({ "temperature": temp }),
        })
    }

    pub fn media(&self, entity_id: &str, command: &str) -> Result<Value, String> {
        let service = match command {
            "play" => "media_play",
            "pause" => "media_pause",
            "play_pause" => "media_play_pause",
            "next" => "media_next_track",
            "prev" => "media_previous_track",
            "stop" => "media_stop",
            other => other,
        };
        self.call(&HaCall {
            domain: "media_player".into(),
            service: service.into(),
            target: serde_json::json!({ "entity_id": entity_id }),
            data: Value::Null,
        })
    }
}

/// The Lovelace WebSocket can block on a half-dead server. Run it in a thread
/// of its own and stop waiting after a few seconds so a caller never hangs.
pub fn views_guarded(client: &HaClient) -> Result<Vec<HaView>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let client = client.clone();
    std::thread::spawn(move || {
        let _ = tx.send(client.views());
    });
    match rx.recv_timeout(Duration::from_secs(8)) {
        Ok(result) => result,
        Err(_) => Err("Home Assistant: дашборд не ответил".into()),
    }
}

fn read_json(
    socket: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
) -> Result<Value, String> {
    loop {
        let msg = socket.read().map_err(|e| e.to_string())?;
        if msg.is_text() {
            let text = msg.into_text().map_err(|e| e.to_string())?;
            return serde_json::from_str(text.as_str()).map_err(|e| e.to_string());
        }
    }
}

/// Walk a dashboard card tree and keep entity ids in the order they appear.
fn collect_entities(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Array(list) => {
            for v in list {
                collect_entities(v, out);
            }
        }
        Value::Object(map) => {
            if let Some(id) = map.get("entity").and_then(Value::as_str) {
                push_id(id, out);
            }
            if let Some(id) = map.get("entity_id").and_then(Value::as_str) {
                push_id(id, out);
            }
            for key in ["entities", "cards", "sections", "badges", "rows"] {
                if let Some(v) = map.get(key) {
                    collect_entities(v, out);
                }
            }
        }
        Value::String(s) => push_id(s, out),
        _ => {}
    }
}

fn push_id(id: &str, out: &mut Vec<String>) {
    let id = id.trim();
    let ok = id
        .split_once('.')
        .map(|(d, _)| !d.is_empty() && !d.contains(' '))
        .unwrap_or(false);
    if ok && !out.iter().any(|x| x == id) {
        out.push(id.to_string());
    }
}

pub fn is_controllable(e: &HaEntity) -> bool {
    matches!(
        e.domain(),
        "light"
            | "switch"
            | "input_boolean"
            | "input_number"
            | "input_select"
            | "number"
            | "select"
            | "fan"
            | "media_player"
            | "climate"
            | "cover"
            | "lock"
            | "humidifier"
            | "water_heater"
            | "valve"
            | "siren"
            | "vacuum"
            | "update"
            | "scene"
            | "script"
            | "button"
            | "automation"
    )
}

pub fn is_sensor(e: &HaEntity) -> bool {
    matches!(
        e.domain(),
        "sensor" | "binary_sensor" | "weather" | "person" | "device_tracker"
    )
}

pub fn domain_label(domain: &str, lang: crate::i18n::Lang) -> &'static str {
    let (ru, en) = match domain {
        "light" => ("Свет", "Lights"),
        "switch" => ("Выключатели", "Switches"),
        "climate" => ("Климат", "Climate"),
        "media_player" => ("Медиа", "Media"),
        "scene" => ("Сцены", "Scenes"),
        "script" => ("Скрипты", "Scripts"),
        "automation" => ("Автоматизации", "Automations"),
        "button" => ("Кнопки", "Buttons"),
        "cover" => ("Шторы", "Covers"),
        "fan" => ("Вентиляторы", "Fans"),
        "sensor" | "binary_sensor" => ("Датчики", "Sensors"),
        _ => ("Другое", "Other"),
    };
    word(lang, ru, en)
}

// ---------------------------------------------------------------- state text

fn word(lang: crate::i18n::Lang, ru: &'static str, en: &'static str) -> &'static str {
    crate::i18n::tr(lang, ru, en)
}

/// `8589.934592` in a 22 px tile is noise. Round it to something a person can
/// read, group thousands and keep at most three significant decimals.
pub fn human_number(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    let a = v.abs();
    if a == 0.0 {
        return "0".into();
    }
    if a >= 1000.0 {
        let n = v.round() as i64;
        let digits = n.unsigned_abs().to_string();
        let mut out = String::new();
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                out.push(' ');
            }
            out.push(ch);
        }
        return if n < 0 { format!("-{out}") } else { out };
    }
    let s = if a >= 100.0 {
        format!("{v:.1}")
    } else if a >= 1.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.3}")
    };
    trim_zeros(s)
}

fn trim_zeros(s: String) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// `15453340 s` becomes `178 д 20 ч`: an uptime in seconds is unreadable.
pub fn human_duration(seconds: f64, lang: crate::i18n::Lang) -> String {
    let total = seconds.abs().round() as u64;
    let (d, h, m) = (total / 86400, (total % 86400) / 3600, (total % 3600) / 60);
    let unit = |n: u64, ru: &'static str, en: &'static str| format!("{n} {}", word(lang, ru, en));
    if d > 0 {
        if h > 0 {
            format!("{} {}", unit(d, "д", "d"), unit(h, "ч", "h"))
        } else {
            unit(d, "д", "d")
        }
    } else if h > 0 {
        if m > 0 && h < 10 {
            format!("{} {}", unit(h, "ч", "h"), unit(m, "мин", "min"))
        } else {
            unit(h, "ч", "h")
        }
    } else {
        unit(m.max(1), "мин", "min")
    }
}

/// A binary sensor reads "вкл", which is wrong for a door or a leak. Use the
/// words the device class implies; fall back to on/off.
fn binary_text(e: &HaEntity, on: bool, lang: crate::i18n::Lang) -> String {
    let class = e
        .attributes
        .get("device_class")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let ru = match class {
        "door" | "window" | "opening" | "garage_door" => ("открыто", "закрыто"),
        "motion" | "moving" => ("движение", "тихо"),
        "occupancy" | "presence" => ("есть", "нет"),
        "moisture" | "water" => ("протечка", "сухо"),
        "smoke" | "gas" | "carbon_monoxide" | "heat" => ("тревога", "спокойно"),
        "connectivity" | "plug" => ("онлайн", "офлайн"),
        "problem" => ("проблема", "ок"),
        "update" => ("есть обновление", "актуально"),
        "battery" | "battery_charging" => ("да", "нет"),
        "power" => ("есть", "нет"),
        _ => ("вкл", "выкл"),
    };
    let en = match class {
        "door" | "window" | "opening" | "garage_door" => ("open", "closed"),
        "motion" | "moving" => ("motion", "clear"),
        "occupancy" | "presence" => ("present", "away"),
        "moisture" | "water" => ("wet", "dry"),
        "smoke" | "gas" | "carbon_monoxide" | "heat" => ("alert", "clear"),
        "connectivity" | "plug" => ("online", "offline"),
        "problem" => ("problem", "ok"),
        "update" => ("update available", "up to date"),
        "battery" | "battery_charging" => ("yes", "no"),
        "power" => ("on", "off"),
        _ => ("on", "off"),
    };
    if on {
        word(lang, ru.0, en.0).to_string()
    } else {
        word(lang, ru.1, en.1).to_string()
    }
}

/// State plus its unit, ready for a big value label. Numbers, durations and
/// binary sensors are translated; everything else keeps Home Assistant's text.
pub fn state_with_unit(e: &HaEntity, lang: crate::i18n::Lang) -> String {
    let (text, has_unit) = value_text(e, lang);
    let unit = e.unit();
    if has_unit || unit.is_empty() || text == "—" {
        text
    } else {
        format!("{text} {unit}")
    }
}

/// State without the unit, for subtitles and menus.
pub fn state_text(e: &HaEntity, lang: crate::i18n::Lang) -> String {
    value_text(e, lang).0
}

fn value_text(e: &HaEntity, lang: crate::i18n::Lang) -> (String, bool) {
    let raw = e.state.trim();
    if raw.is_empty() {
        return ("—".into(), true);
    }
    match raw {
        "unavailable" => return (word(lang, "недоступно", "unavailable").into(), true),
        "unknown" => return (word(lang, "неизвестно", "unknown").into(), true),
        "open" => return (word(lang, "открыто", "open").into(), true),
        "closed" => return (word(lang, "закрыто", "closed").into(), true),
        "opening" => return (word(lang, "открывается", "opening").into(), true),
        "closing" => return (word(lang, "закрывается", "closing").into(), true),
        "playing" => return (word(lang, "играет", "playing").into(), true),
        "paused" => return (word(lang, "пауза", "paused").into(), true),
        "idle" => return (word(lang, "ожидание", "idle").into(), true),
        "home" => return (word(lang, "дома", "home").into(), true),
        "not_home" => return (word(lang, "не дома", "away").into(), true),
        "on" | "off" => {
            let on = raw == "on";
            if e.domain() == "binary_sensor" {
                return (binary_text(e, on, lang), true);
            }
            if e.domain() == "update" {
                return (
                    word(
                        lang,
                        if on {
                            "есть обновление"
                        } else {
                            "актуально"
                        },
                        if on { "update available" } else { "up to date" },
                    )
                    .into(),
                    true,
                );
            }
            return (
                word(
                    lang,
                    if on { "вкл" } else { "выкл" },
                    if on { "on" } else { "off" },
                )
                .into(),
                true,
            );
        }
        _ => {}
    }
    if let Ok(v) = raw.parse::<f64>() {
        let unit = e.unit();
        if unit == "s" && v.abs() >= 60.0 {
            return (human_duration(v, lang), true);
        }
        return (human_number(v), false);
    }
    // ISO timestamps (sun, next alarm, …) are 25 characters wide and do not
    // fit a tile. Keep the date and time, say UTC when it is UTC.
    if let Some((date, rest)) = raw.split_once('T') {
        if date.len() == 10 && rest.len() >= 5 {
            let hm = &rest[..5];
            let bytes = date.as_bytes();
            if bytes.get(4) == Some(&b'-') && bytes.get(7) == Some(&b'-') {
                let stamp = format!("{}.{} {}", &date[8..10], &date[5..7], hm);
                return if rest.ends_with("+00:00") || rest.ends_with('Z') {
                    (format!("{stamp} UTC"), true)
                } else {
                    (stamp, true)
                };
            }
        }
    }
    (raw.to_string(), false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::HaEntity;
    use serde_json::json;

    fn entity(domain: &str, state: &str, attrs: serde_json::Value) -> HaEntity {
        HaEntity {
            entity_id: format!("{domain}.x"),
            state: state.into(),
            attributes: attrs,
            last_changed: String::new(),
            last_updated: String::new(),
            area: String::new(),
        }
    }

    #[test]
    fn numbers_lose_the_noise() {
        assert_eq!(human_number(8589.934592), "8 590");
        assert_eq!(human_number(4294.967296), "4 295");
        assert_eq!(human_number(21.7000), "21.7");
        assert_eq!(human_number(0.0), "0");
        assert_eq!(human_number(-1234.6), "-1 235");
    }

    #[test]
    fn durations_read_like_durations() {
        use crate::i18n::Lang;
        assert_eq!(human_duration(15_453_340.0, Lang::Ru), "178 д 20 ч");
        assert_eq!(human_duration(3900.0, Lang::En), "1 h 5 min");
        assert_eq!(human_duration(90.0, Lang::Ru), "1 мин");
    }

    #[test]
    fn binary_sensors_use_their_device_class() {
        use crate::i18n::Lang;
        let door = entity("binary_sensor", "on", json!({"device_class": "door"}));
        assert_eq!(state_text(&door, Lang::Ru), "открыто");
        let leak = entity("binary_sensor", "off", json!({"device_class": "moisture"}));
        assert_eq!(state_text(&leak, Lang::En), "dry");
    }

    #[test]
    fn units_are_added_once() {
        use crate::i18n::Lang;
        let temp = entity("sensor", "21.75", json!({"unit_of_measurement": "°C"}));
        assert_eq!(state_with_unit(&temp, Lang::Ru), "21.75 °C");
        let uptime = entity("sensor", "15453340", json!({"unit_of_measurement": "s"}));
        assert_eq!(state_with_unit(&uptime, Lang::Ru), "178 д 20 ч");
    }

    #[test]
    fn actions_map_to_real_services() {
        assert_eq!(service_for("scene", "run"), Some("turn_on"));
        assert_eq!(service_for("automation", "run"), Some("trigger"));
        assert_eq!(service_for("cover", "open"), Some("open_cover"));
        // A gate-style press: stop while moving, else go the other way.
        assert_eq!(cover_step("opening"), "stop_cover");
        assert_eq!(cover_step("closing"), "stop_cover");
        assert_eq!(cover_step("open"), "close_cover");
        assert_eq!(cover_step("closed"), "open_cover");
        assert_eq!(cover_step("unavailable"), "open_cover");
        assert_eq!(service_for("lock", "on"), Some("unlock"));
        assert_eq!(service_for("lock", "off"), Some("lock"));
        assert_eq!(service_for("button", "run"), Some("press"));
        assert_eq!(service_for("light", "on"), Some("turn_on"));
        assert_eq!(service_for("sensor", "on"), None);
    }

    #[test]
    fn only_known_services_pass() {
        assert!(known_service("light", "turn_on"));
        assert!(known_service("automation", "trigger"));
        assert!(known_service("cover", "open_cover"));
        assert!(known_service("cover", "step"));
        assert!(!known_service("light", "open_cover"));
        assert!(!known_service("automation", "press"));
        assert!(!known_service("sensor", "turn_on"));
        // Every service `service_for` can produce must pass the whitelist, so a
        // scenario built in the UI is never rejected by the daemon.
        for (domain, action) in [
            ("light", "on"),
            ("light", "off"),
            ("light", "toggle"),
            ("scene", "run"),
            ("script", "run"),
            ("automation", "run"),
            ("automation", "on"),
            ("automation", "off"),
            ("automation", "toggle"),
            ("button", "run"),
            ("cover", "open"),
            ("cover", "close"),
            ("cover", "stop"),
            ("lock", "on"),
            ("lock", "off"),
            ("valve", "open"),
            ("media_player", "play"),
            ("vacuum", "start"),
            ("update", "install"),
            ("homeassistant", "on"),
        ] {
            let svc = service_for(domain, action).unwrap_or_else(|| panic!("{domain}.{action}"));
            assert!(known_service(domain, svc), "{domain}.{svc} should be known");
        }
    }
}
