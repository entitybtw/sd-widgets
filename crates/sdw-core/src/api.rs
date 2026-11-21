//! Blocking HTTP client for the daemon API. Used by the Deck app and by the daemon's own tests.

use crate::model::*;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

const UA: &str = concat!("sd-widgets/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.status == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{} (HTTP {})", self.message, self.status)
        }
    }
}

impl std::error::Error for ApiError {}

pub type ApiResult<T> = Result<T, ApiError>;

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .user_agent(UA)
        .build()
        .into()
}

pub fn normalize_base(base: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    if base.starts_with("http://") || base.starts_with("https://") {
        base.to_string()
    } else {
        format!("http://{base}")
    }
}

pub fn ping(base: &str) -> ApiResult<Ping> {
    let base = normalize_base(base);
    let a = agent(Duration::from_secs(5));
    let mut r = a
        .get(&format!("{base}/api/v1/ping"))
        .call()
        .map_err(|e| ApiError {
            status: 0,
            message: e.to_string(),
        })?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().map_err(|e| ApiError {
        status,
        message: e.to_string(),
    })?;
    if status != 200 {
        return Err(ApiError {
            status,
            message: error_from_body(&text),
        });
    }
    serde_json::from_str(&text).map_err(|e| ApiError {
        status,
        message: e.to_string(),
    })
}

pub fn pair(
    base: &str,
    password: &str,
    client_name: &str,
    client_id: &str,
) -> ApiResult<PairResponse> {
    let base = normalize_base(base);
    let a = agent(Duration::from_secs(10));
    let body = PairRequest {
        password: password.to_string(),
        client_name: client_name.to_string(),
        client_id: client_id.to_string(),
    };
    let mut r = a
        .post(&format!("{base}/api/v1/pair"))
        .send_json(&body)
        .map_err(|e| ApiError {
            status: 0,
            message: e.to_string(),
        })?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().map_err(|e| ApiError {
        status,
        message: e.to_string(),
    })?;
    if status != 200 {
        return Err(ApiError {
            status,
            message: error_from_body(&text),
        });
    }
    serde_json::from_str(&text).map_err(|e| ApiError {
        status,
        message: e.to_string(),
    })
}

fn error_from_body(text: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        if let Some(e) = v.get("error").and_then(Value::as_str) {
            return e.to_string();
        }
        if let Some(e) = v.get("detail").and_then(Value::as_str) {
            return e.to_string();
        }
    }
    if text.trim().is_empty() {
        "сервер не ответил".into()
    } else {
        crate::util::truncate(text.trim(), 300)
    }
}

#[derive(Clone)]
pub struct Client {
    base: String,
    token: String,
    agent: ureq::Agent,
    /// Polls and lists that must answer quickly. A stuck daemon should not
    /// leave the deck waiting twelve seconds before it can say "offline".
    fast: ureq::Agent,
}

impl Client {
    pub fn new(base: &str, token: &str) -> Self {
        Self {
            base: normalize_base(base),
            token: token.to_string(),
            agent: agent(Duration::from_secs(12)),
            fast: agent(Duration::from_secs(5)),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with('/') {
            format!("{}{}", self.base, path)
        } else {
            format!("{}/{}", self.base, path)
        }
    }

    fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> ApiResult<T> {
        self.request_with(&self.agent, method, path, body)
    }

    /// A short-timeout request for a poll: status, lists, a screen frame.
    fn request_fast<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> ApiResult<T> {
        self.request_with(&self.fast, method, path, body)
    }

    fn request_with<T: DeserializeOwned>(
        &self,
        agent: &ureq::Agent,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> ApiResult<T> {
        let url = self.url(path);
        let auth = format!("Bearer {}", self.token);
        let mut r = match method {
            "POST" => {
                let req = agent.post(&url).header("Authorization", &auth);
                req.send_json(body.unwrap_or(&Value::Null))
            }
            "PATCH" => {
                let req = agent.patch(&url).header("Authorization", &auth);
                req.send_json(body.unwrap_or(&Value::Null))
            }
            "DELETE" => agent.delete(&url).header("Authorization", &auth).call(),
            _ => agent.get(&url).header("Authorization", &auth).call(),
        }
        .map_err(|e| ApiError {
            status: 0,
            message: e.to_string(),
        })?;
        let status = r.status().as_u16();
        let text = r.body_mut().read_to_string().map_err(|e| ApiError {
            status,
            message: e.to_string(),
        })?;
        if status != 200 {
            return Err(ApiError {
                status,
                message: error_from_body(&text),
            });
        }
        serde_json::from_str(&text).map_err(|e| ApiError {
            status,
            message: e.to_string(),
        })
    }

    pub fn status(&self) -> ApiResult<DaemonStatus> {
        self.request_fast("GET", "/api/v1/status", None)
    }

    pub fn config(&self) -> ApiResult<Value> {
        self.request("GET", "/api/v1/config", None)
    }

    pub fn patch_config(&self, patch: Value) -> ApiResult<Value> {
        self.request("PATCH", "/api/v1/config", Some(&patch))
    }

    /// Bluetooth adapter and known devices.
    pub fn bluetooth(&self) -> ApiResult<BluetoothStatus> {
        self.request_fast("GET", "/api/v1/bluetooth", None)
    }

    /// Scan, connect, pair, trust or remove a Bluetooth device.
    pub fn bluetooth_action(&self, action: &str, address: &str) -> ApiResult<Value> {
        let body = serde_json::json!({ "action": action, "address": address });
        self.request("POST", "/api/v1/bluetooth/action", Some(&body))
    }

    /// Applications installed on the computer.
    pub fn apps(&self) -> ApiResult<Vec<AppEntry>> {
        self.request_fast("GET", "/api/v1/apps", None)
    }

    /// Start one of the applications returned by [`Client::apps`].
    pub fn app_launch(&self, id: &str) -> ApiResult<Value> {
        let body = serde_json::json!({ "id": id });
        self.request("POST", "/api/v1/apps/launch", Some(&body))
    }

    /// The picture of one application, when its entry says `has_icon`.
    pub fn app_icon(&self, id: &str) -> ApiResult<Vec<u8>> {
        let url = self.url(&format!("/api/v1/apps/{id}/icon"));
        let mut r = self
            .fast
            .get(&url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| ApiError {
                status: 0,
                message: e.to_string(),
            })?;
        let status = r.status().as_u16();
        if status != 200 {
            let text = r.body_mut().read_to_string().unwrap_or_default();
            return Err(ApiError {
                status,
                message: error_from_body(&text),
            });
        }
        r.body_mut().read_to_vec().map_err(|e| ApiError {
            status,
            message: e.to_string(),
        })
    }

    /// Lock, suspend, restart or power off the computer.
    pub fn power(&self, action: PowerAction) -> ApiResult<Value> {
        let body = serde_json::json!({ "action": action.key() });
        self.request("POST", "/api/v1/power", Some(&body))
    }

    pub fn addon_install(&self, url: &str) -> ApiResult<Value> {
        let body = serde_json::json!({ "url": url });
        self.request("POST", "/api/v1/addons/install", Some(&body))
    }

    pub fn logs(&self, since: u64) -> ApiResult<Vec<LogEntry>> {
        self.request("GET", &format!("/api/v1/logs?since={since}"), None)
    }

    pub fn pads(&self) -> ApiResult<Vec<PadInfo>> {
        self.request_fast("GET", "/api/v1/pads", None)
    }

    pub fn create_pad(&self, name: &str, kind: PadKind) -> ApiResult<PadCreated> {
        let body = serde_json::json!({"name": name, "kind": kind});
        self.request("POST", "/api/v1/pads", Some(&body))
    }

    pub fn delete_pad(&self, id: &str) -> ApiResult<Value> {
        self.request("DELETE", &format!("/api/v1/pads/{id}"), None)
    }

    pub fn pad_feedback(&self, id: &str) -> ApiResult<PadFeedback> {
        self.request("GET", &format!("/api/v1/pads/{id}/feedback"), None)
    }

    pub fn viiper(&self) -> ApiResult<ViiperStatus> {
        self.request("GET", "/api/v1/viiper", None)
    }

    pub fn viiper_action(&self, action: &str) -> ApiResult<Value> {
        let body = serde_json::json!({ "action": action });
        self.request("POST", "/api/v1/viiper/action", Some(&body))
    }

    pub fn set_viiper_password(&self, password: &str) -> ApiResult<Value> {
        let body = serde_json::json!({ "password": password });
        self.request("POST", "/api/v1/viiper/password", Some(&body))
    }

    pub fn monitor(&self) -> ApiResult<MonitorStatus> {
        self.request("GET", "/api/v1/monitor", None)
    }

    pub fn start_monitor(&self, req: &MonitorStart) -> ApiResult<MonitorStatus> {
        let body = serde_json::to_value(req).unwrap_or_default();
        self.request("POST", "/api/v1/monitor/start", Some(&body))
    }

    pub fn stop_monitor(&self) -> ApiResult<MonitorStatus> {
        self.request("POST", "/api/v1/monitor/stop", None)
    }

    pub fn audio(&self) -> ApiResult<AudioStatus> {
        self.request("GET", "/api/v1/audio", None)
    }

    pub fn audio_sources(&self) -> ApiResult<Vec<AudioApp>> {
        self.request("GET", "/api/v1/audio/sources", None)
    }

    pub fn audio_start(&self, start: &AudioStart) -> ApiResult<AudioStatus> {
        let body = serde_json::to_value(start).unwrap_or_default();
        self.request("POST", "/api/v1/audio/start", Some(&body))
    }

    pub fn audio_stop(&self) -> ApiResult<AudioStatus> {
        self.request("POST", "/api/v1/audio/stop", None)
    }

    /// A blocking raw-PCM reader for the running stream. No global timeout:
    /// the connection lives until the daemon stops or the reader is dropped.
    pub fn audio_stream(&self) -> ApiResult<Box<dyn std::io::Read + Send>> {
        let url = self.url("/api/v1/audio/stream");
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(None)
            .http_status_as_error(false)
            .user_agent(UA)
            .build()
            .into();
        let mut r = agent
            .get(&url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| ApiError {
                status: 0,
                message: e.to_string(),
            })?;
        let status = r.status().as_u16();
        if status != 200 {
            let text = r.body_mut().read_to_string().unwrap_or_default();
            return Err(ApiError {
                status,
                message: error_from_body(&text),
            });
        }
        let (_, body) = r.into_parts();
        Ok(Box::new(body.into_reader()))
    }

    pub fn monitor_input(&self, input: &MonitorInput) -> ApiResult<Value> {
        let body = serde_json::to_value(input).unwrap_or_default();
        self.request("POST", "/api/v1/monitor/input", Some(&body))
    }

    pub fn monitor_frame(&self) -> ApiResult<Vec<u8>> {
        let url = self.url("/api/v1/monitor/frame");
        let mut r = self
            .agent
            .get(&url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| ApiError {
                status: 0,
                message: e.to_string(),
            })?;
        let status = r.status().as_u16();
        if status != 200 {
            let text = r.body_mut().read_to_string().unwrap_or_default();
            return Err(ApiError {
                status,
                message: error_from_body(&text),
            });
        }
        r.body_mut().read_to_vec().map_err(|e| ApiError {
            status,
            message: e.to_string(),
        })
    }

    pub fn notifications(&self, since_id: u64) -> ApiResult<Vec<Notification>> {
        self.request(
            "GET",
            &format!("/api/v1/notifications?after_id={since_id}"),
            None,
        )
    }

    pub fn clear_notifications(&self) -> ApiResult<Value> {
        self.request("POST", "/api/v1/notifications/clear", None)
    }

    pub fn dismiss_notification(&self, id: u64) -> ApiResult<Value> {
        self.request("POST", &format!("/api/v1/notifications/{id}/dismiss"), None)
    }

    /// Run the notification's own action on the computer, as a click on the
    /// desktop notification would.
    pub fn activate_notification(&self, id: u64) -> ApiResult<Value> {
        self.request(
            "POST",
            &format!("/api/v1/notifications/{id}/activate"),
            None,
        )
    }

    // ---------------------------------------------------------- deck mirror

    /// Ask whether anyone on the computer is watching this deck's screen.
    /// The deck only captures frames while the answer is yes.
    pub fn deck_want(&self) -> ApiResult<bool> {
        let v: Value = self.request_fast("GET", "/api/v1/deck/want", None)?;
        Ok(v.get("want").and_then(Value::as_bool).unwrap_or(false))
    }

    /// Push one JPEG of this deck's screen to the daemon, with the page and
    /// size that go with it. The daemon shows the newest frame and lists the
    /// decks that keep sending.
    pub fn push_deck_frame(
        &self,
        device: &str,
        name: &str,
        page: &str,
        width: u32,
        height: u32,
        jpeg: Vec<u8>,
    ) -> ApiResult<Value> {
        let query = format!(
            "device={}&name={}&page={}&version={}&width={width}&height={height}",
            urlencoding(device),
            urlencoding(name),
            urlencoding(page),
            urlencoding(VERSION)
        );
        let url = self.url(&format!("/api/v1/deck/frame?{query}"));
        let mut r = self
            .agent
            .post(&url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Content-Type", "image/jpeg")
            .send(jpeg)
            .map_err(|e| ApiError {
                status: 0,
                message: e.to_string(),
            })?;
        let status = r.status().as_u16();
        let text = r.body_mut().read_to_string().unwrap_or_default();
        if status != 200 {
            return Err(ApiError {
                status,
                message: error_from_body(&text),
            });
        }
        serde_json::from_str(&text).map_err(|e| ApiError {
            status,
            message: e.to_string(),
        })
    }

    pub fn test_notification(&self) -> ApiResult<Value> {
        self.request("POST", "/api/v1/notifications/test", None)
    }

    /// Current media player, system volume and output list.
    pub fn media(&self) -> ApiResult<MediaState> {
        self.request_fast("GET", "/api/v1/media", None)
    }

    /// `action`: play_pause, play, pause, next, prev, stop, seek, volume, mute,
    /// volume_up, volume_down, sink.
    pub fn media_control(&self, action: &str, value: f64, player: &str) -> ApiResult<Value> {
        let body = serde_json::json!({
            "action": action,
            "value": value,
            "player": player,
        });
        self.request("POST", "/api/v1/media/control", Some(&body))
    }

    /// Cover art bytes for one `mpris:artUrl`. Empty url means the current track.
    pub fn media_art(&self, url: &str) -> ApiResult<Vec<u8>> {
        let url = self.url(&format!("/api/v1/media/art?url={}", urlencoding(url)));
        let mut r = self
            .agent
            .get(&url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| ApiError {
                status: 0,
                message: e.to_string(),
            })?;
        let status = r.status().as_u16();
        if status != 200 {
            let text = r.body_mut().read_to_string().unwrap_or_default();
            return Err(ApiError {
                status,
                message: error_from_body(&text),
            });
        }
        r.body_mut().read_to_vec().map_err(|e| ApiError {
            status,
            message: e.to_string(),
        })
    }

    /// Run a whitelisted media / desktop command on the computer.
    pub fn remote(&self, command: &str) -> ApiResult<Value> {
        let body = serde_json::json!({ "command": command });
        self.request("POST", "/api/v1/remote", Some(&body))
    }

    pub fn ha_states(&self) -> ApiResult<HaStates> {
        self.request("GET", "/api/v1/ha/states", None)
    }

    pub fn ha_config(&self, url: &str, token: &str, favorites: &[String]) -> ApiResult<HaStatus> {
        let body = serde_json::json!({
            "url": url,
            "token": token,
            "favorites": favorites,
        });
        self.request("POST", "/api/v1/ha/config", Some(&body))
    }

    pub fn ha_test(&self) -> ApiResult<HaStatus> {
        self.request("POST", "/api/v1/ha/test", None)
    }

    pub fn ha_call(&self, call: &HaCall) -> ApiResult<Value> {
        let body = serde_json::to_value(call).unwrap_or_default();
        self.request("POST", "/api/v1/ha/call", Some(&body))
    }

    pub fn set_favorites(&self, favorites: &[String]) -> ApiResult<Value> {
        let body = serde_json::json!({ "favorites": favorites });
        self.request("POST", "/api/v1/ha/favorites", Some(&body))
    }

    pub fn set_password(&self, password: &str) -> ApiResult<PairResponse> {
        let body = serde_json::json!({ "password": password });
        self.request("POST", "/api/v1/security/password", Some(&body))
    }

    pub fn rotate_token(&self) -> ApiResult<PairResponse> {
        self.request("POST", "/api/v1/security/rotate", None)
    }

    pub fn install(&self, components: &[String], sudo_password: &str) -> ApiResult<Value> {
        let body = serde_json::json!({
            "components": components,
            "sudo_password": sudo_password,
        });
        self.request("POST", "/api/v1/install", Some(&body))
    }

    pub fn install_status(&self) -> ApiResult<Value> {
        self.request("GET", "/api/v1/install/status", None)
    }

    pub fn discover(&self) -> ApiResult<Vec<DiscoveryResponse>> {
        crate::discovery::scan(Duration::from_millis(1200))
            .map_err(|message| ApiError { status: 0, message })
    }

    pub fn ha_categories(&self) -> ApiResult<Vec<HaCategory>> {
        self.request("GET", "/api/v1/ha/categories", None)
    }

    pub fn set_ha_categories(&self, list: &[HaCategory]) -> ApiResult<Vec<HaCategory>> {
        let body = serde_json::json!({ "categories": list });
        self.request("POST", "/api/v1/ha/categories", Some(&body))
    }

    pub fn ha_presets(&self) -> ApiResult<Vec<HaPreset>> {
        self.request("GET", "/api/v1/ha/presets", None)
    }

    pub fn set_ha_presets(&self, list: &[HaPreset]) -> ApiResult<Vec<HaPreset>> {
        let body = serde_json::json!({ "presets": list });
        self.request("POST", "/api/v1/ha/presets", Some(&body))
    }
}

/// Percent-encode a query value (RFC 3986 unreserved characters pass through).
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
