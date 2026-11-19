//! Home Assistant REST client and entity helpers.

use crate::model::{HaCall, HaEntity, HaStates};
use serde_json::Value;
use std::time::Duration;

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
        let mut r = self
            .agent
            .get(&format!("{}{}", self.base, path))
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| e.to_string())?;
        let status = r.status().as_u16();
        let text = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
        if status != 200 {
            return Err(format!("Home Assistant: HTTP {status}: {}", crate::util::truncate(&text, 240)));
        }
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    pub fn ping(&self) -> Result<String, String> {
        let v = self.get("/api/")?;
        Ok(v.get("message")
            .and_then(Value::as_str)
            .unwrap_or("API OK")
            .to_string())
    }

    pub fn states(&self) -> Result<HaStates, String> {
        let v = self.get("/api/states")?;
        let entities: Vec<HaEntity> = serde_json::from_value(v).map_err(|e| e.to_string())?;
        Ok(HaStates {
            entities,
            fetched_ms: crate::model::now_ms(),
        })
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
            return Err(format!("Home Assistant: HTTP {status}: {}", crate::util::truncate(&text, 240)));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub fn toggle(&self, entity_id: &str, on: bool) -> Result<Value, String> {
        self.call(&HaCall {
            domain: "homeassistant".into(),
            service: if on { "turn_on".into() } else { "turn_off".into() },
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

pub fn is_controllable(e: &HaEntity) -> bool {
    matches!(
        e.domain(),
        "light" | "switch" | "fan" | "input_boolean" | "media_player" | "climate" | "cover" | "scene" | "script" | "button"
    )
}

pub fn is_sensor(e: &HaEntity) -> bool {
    matches!(e.domain(), "sensor" | "binary_sensor" | "weather" | "person" | "device_tracker")
}

pub fn domain_label(domain: &str, lang: crate::i18n::Lang) -> &'static str {
    use crate::i18n::Lang;
    match (domain, lang) {
        ("light", Lang::Ru) => "Свет",
        ("light", Lang::En) => "Lights",
        ("switch", Lang::Ru) => "Выключатели",
        ("switch", Lang::En) => "Switches",
        ("climate", Lang::Ru) => "Климат",
        ("climate", Lang::En) => "Climate",
        ("media_player", Lang::Ru) => "Медиа",
        ("media_player", Lang::En) => "Media",
        ("scene", Lang::Ru) => "Сцены",
        ("scene", Lang::En) => "Scenes",
        ("script", Lang::Ru) => "Скрипты",
        ("script", Lang::En) => "Scripts",
        ("cover", Lang::Ru) => "Шторы",
        ("cover", Lang::En) => "Covers",
        ("fan", Lang::Ru) => "Вентиляторы",
        ("fan", Lang::En) => "Fans",
        ("sensor", Lang::Ru) => "Датчики",
        ("sensor", Lang::En) => "Sensors",
        ("binary_sensor", Lang::Ru) => "Датчики",
        ("binary_sensor", Lang::En) => "Sensors",
        (_, Lang::Ru) => "Другое",
        (_, Lang::En) => "Other",
    }
}
