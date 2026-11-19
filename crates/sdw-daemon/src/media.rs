//! Media and system volume on the computer.
//!
//! MPRIS players are read through `playerctl`; the system output goes through
//! `wpctl` (PipeWire), `pactl` (PulseAudio) or `amixer` (ALSA). Covers come from
//! `mpris:artUrl` and are proxied so the deck never has to reach the internet or
//! read a file on the PC.

use sdw_core::model::{now_ms, MediaState};
use std::sync::Mutex;
use std::time::Duration;

const MAX_ART: usize = 6 * 1024 * 1024;

#[derive(Default)]
struct ArtCache {
    url: String,
    bytes: Vec<u8>,
    checked: Option<std::time::Instant>,
}

pub struct MediaService {
    art: Mutex<ArtCache>,
    selected: Mutex<String>,
}

impl MediaService {
    pub fn new() -> Self {
        Self {
            art: Mutex::new(ArtCache::default()),
            selected: Mutex::new(String::new()),
        }
    }

    pub fn remember_player(&self, player: &str) {
        if let Ok(mut s) = self.selected.lock() {
            *s = player.to_string();
        }
    }

    pub fn snapshot(&self) -> MediaState {
        let players = players();
        let selected = self.selected.lock().map(|s| s.clone()).unwrap_or_default();
        let mut state = MediaState {
            players: players.clone(),
            updated_ms: now_ms(),
            ..Default::default()
        };
        if players.is_empty() {
            state.error = if cfg!(target_os = "linux") {
                "нет mpris-плееров · запустите музыку или видео".into()
            } else {
                "управление медиа пока доступно на Linux".into()
            };
            fill_volume(&mut state);
            return state;
        }
        // Prefer the remembered player, then anything that is playing, then the first.
        let mut player = if !selected.is_empty() && players.iter().any(|p| p == &selected) {
            selected
        } else {
            String::new()
        };
        if player.is_empty() {
            for p in &players {
                if status_of(p).eq_ignore_ascii_case("playing") {
                    player = p.clone();
                    break;
                }
            }
        }
        if player.is_empty() {
            player = players[0].clone();
        }
        state.player = player.clone();
        state.status = status_of(&player);
        state.available = true;

        if let Ok(out) = sdw_core::util::run(
            "playerctl",
            &[
                "--player",
                &player,
                "metadata",
                "--format",
                "{{title}}\t{{artist}}\t{{album}}\t{{mpris:artUrl}}\t{{mpris:length}}\t{{position}}\t{{volume}}",
            ],
        ) {
            let mut parts = out.split('\t');
            state.title = parts.next().unwrap_or("").trim().to_string();
            state.artist = parts.next().unwrap_or("").trim().to_string();
            state.album = parts.next().unwrap_or("").trim().to_string();
            state.art_url = parts.next().unwrap_or("").trim().to_string();
            state.length = parse_time(parts.next().unwrap_or(""));
            state.position = parse_time(parts.next().unwrap_or(""));
            state.player_volume = parts.next().and_then(|v| v.trim().parse::<f64>().ok());
        } else if state.status.eq_ignore_ascii_case("stopped") {
            state.error = "плеер остановлен".into();
        }

        // `mpris:length` is microseconds; some players report milliseconds or seconds.
        if state.length > 0.0 {
            state.length = normalise_time(state.length);
        }
        if state.position > 0.0 {
            state.position = normalise_time(state.position).min(if state.length > 0.0 {
                state.length
            } else {
                state.position
            });
        }
        state.art_id = if state.art_url.is_empty() {
            String::new()
        } else {
            let mut hash = 0u64;
            for b in state.art_url.bytes() {
                hash = hash.wrapping_mul(1099511628211).wrapping_add(b as u64);
            }
            format!("{hash:016x}")
        };

        fill_volume(&mut state);
        state
    }

    /// Return the cover for `url` (or the current track when empty).
    pub fn art(&self, url: &str) -> Result<Vec<u8>, String> {
        let url = if url.trim().is_empty() {
            self.snapshot().art_url
        } else {
            url.trim().to_string()
        };
        if url.is_empty() {
            return Err("у трека нет обложки".into());
        }
        if let Ok(cache) = self.art.lock() {
            if cache.url == url && !cache.bytes.is_empty() {
                return Ok(cache.bytes.clone());
            }
        }
        let bytes = fetch_art(&url)?;
        if bytes.len() > MAX_ART {
            return Err("обложка слишком большая".into());
        }
        if let Ok(mut cache) = self.art.lock() {
            cache.url = url;
            cache.bytes = bytes.clone();
            cache.checked = Some(std::time::Instant::now());
        }
        Ok(bytes)
    }

    /// Apply one control action. Returns a short human message.
    pub fn control(&self, action: &str, value: f64, player: &str) -> Result<String, String> {
        // Sink switching and player selection are not media-player commands, so
        // they must not change the remembered MPRIS player.
        if action == "sink" {
            return set_sink(player.trim());
        }
        if action == "select" {
            let name = player.trim();
            if name.is_empty() {
                return Err("не выбран плеер".into());
            }
            self.remember_player(name);
            return Ok(name.to_string());
        }
        if !player.trim().is_empty() {
            self.remember_player(player.trim());
        }
        let selected = self.selected.lock().map(|s| s.clone()).unwrap_or_default();
        let player = if player.trim().is_empty() {
            selected
        } else {
            player.trim().to_string()
        };

        match action {
            "play_pause" | "play" | "pause" | "next" | "prev" | "stop" => {
                let verb = match action {
                    "play_pause" => "play-pause",
                    "prev" => "previous",
                    other => other,
                };
                let mut args = vec![verb];
                if !player.is_empty() {
                    args = vec!["--player", &player, verb];
                }
                sdw_core::util::run("playerctl", &args).map(|_| verb.to_string())
            }
            "seek" => {
                let secs = value.max(0.0);
                let pos = format!("{secs:.3}");
                let mut args = vec!["position", &pos];
                if !player.is_empty() {
                    args = vec!["--player", &player, "position", &pos];
                }
                sdw_core::util::run("playerctl", &args).map(|_| format!("{secs:.0} с"))
            }
            "volume" => set_volume(value.clamp(0.0, 100.0).round() as u8),
            "volume_up" => bump_volume(5),
            "volume_down" => bump_volume(-5),
            "mute" => toggle_mute(),

            other => Err(format!("неизвестное действие: {other}")),
        }
    }
}

// ---------------------------------------------------------------- players

fn players() -> Vec<String> {
    if !sdw_core::util::has("playerctl") {
        return Vec::new();
    }
    sdw_core::util::run("playerctl", &["-l"])
        .map(|out| {
            out.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn status_of(player: &str) -> String {
    sdw_core::util::run("playerctl", &["--player", player, "status"])
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn parse_time(raw: &str) -> f64 {
    raw.trim().parse::<f64>().unwrap_or(0.0).max(0.0)
}

/// MPRIS times are microseconds. Convert anything that looks like ms or s too,
/// so a weird player does not produce a 300-hour track.
fn normalise_time(v: f64) -> f64 {
    if v > 100_000_000.0 {
        v / 1_000_000.0
    } else if v > 100_000.0 {
        v / 1_000.0
    } else {
        v
    }
}

// ---------------------------------------------------------------- volume

fn fill_volume(state: &mut MediaState) {
    if let Ok((volume, muted, sink)) = read_volume() {
        state.volume = volume;
        state.muted = muted;
        state.sink = sink;
    }
    state.sinks = list_sinks();
}

fn read_volume() -> Result<(u8, bool, String), String> {
    if cfg!(target_os = "linux") && sdw_core::util::has("wpctl") {
        let raw = sdw_core::util::run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?;
        // "Volume: 0.42" or "Volume: 0.42 [MUTED]"
        let muted = raw.contains("MUTED");
        let number = raw
            .split_whitespace()
            .find_map(|w| w.parse::<f64>().ok())
            .unwrap_or(0.0);
        let sink = current_sink();
        return Ok((
            (number * 100.0).round().clamp(0.0, 150.0) as u8,
            muted,
            sink,
        ));
    }
    if cfg!(target_os = "linux") && sdw_core::util::has("pactl") {
        let raw = sdw_core::util::run("pactl", &["get-sink-volume", "@DEFAULT_SINK@"])?;
        let pct = raw
            .split('%')
            .next()
            .and_then(|s| s.rsplit_once(' '))
            .and_then(|(_, n)| n.parse::<u8>().ok())
            .unwrap_or(0);
        let muted = sdw_core::util::run("pactl", &["get-sink-mute", "@DEFAULT_SINK@"])
            .map(|m| m.contains("yes"))
            .unwrap_or(false);
        return Ok((pct.min(150), muted, current_sink()));
    }
    if cfg!(target_os = "linux") && sdw_core::util::has("amixer") {
        let raw = sdw_core::util::run("amixer", &["get", "Master"])?;
        let pct = raw
            .split('[')
            .find_map(|s| {
                s.split('%')
                    .next()
                    .and_then(|n| n.trim().parse::<u8>().ok())
            })
            .unwrap_or(0);
        let muted = raw.contains("[off]");
        return Ok((pct, muted, "Master".into()));
    }
    Err("нет wpctl, pactl или amixer".into())
}

fn bump_volume(delta: i32) -> Result<String, String> {
    if cfg!(target_os = "linux") && sdw_core::util::has("wpctl") {
        let step = if delta >= 0 { "5%+" } else { "5%-" };
        sdw_core::util::run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", step])?;
        return Ok(format!("звук {step}"));
    }
    let current = read_volume().map(|v| v.0 as i32).unwrap_or(50);
    set_volume((current + delta).clamp(0, 100) as u8)
}

fn set_volume(percent: u8) -> Result<String, String> {
    let percent = percent.min(100);
    if cfg!(target_os = "linux") && sdw_core::util::has("wpctl") {
        let arg = format!("{percent}%");
        sdw_core::util::run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &arg])?;
        // Setting a volume while muted keeps it silent; unmute on purpose? No:
        // the user dragged the slider, so they expect sound.
        if read_volume().map(|v| v.1).unwrap_or(false) {
            let _ = sdw_core::util::run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "0"]);
        }
        return Ok(format!("{percent}%"));
    }
    if cfg!(target_os = "linux") && sdw_core::util::has("pactl") {
        let arg = format!("{percent}%");
        sdw_core::util::run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &arg])?;
        let _ = sdw_core::util::run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "0"]);
        return Ok(format!("{percent}%"));
    }
    if cfg!(target_os = "linux") && sdw_core::util::has("amixer") {
        let arg = format!("{percent}%");
        sdw_core::util::run("amixer", &["sset", "Master", &arg])?;
        let _ = sdw_core::util::run("amixer", &["sset", "Master", "unmute"]);
        return Ok(format!("{percent}%"));
    }
    Err("нет wpctl, pactl или amixer".into())
}

fn toggle_mute() -> Result<String, String> {
    if cfg!(target_os = "linux") && sdw_core::util::has("wpctl") {
        sdw_core::util::run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])?;
        return Ok("звук".into());
    }
    if cfg!(target_os = "linux") && sdw_core::util::has("pactl") {
        sdw_core::util::run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "toggle"])?;
        return Ok("звук".into());
    }
    if cfg!(target_os = "linux") && sdw_core::util::has("amixer") {
        sdw_core::util::run("amixer", &["sset", "Master", "toggle"])?;
        return Ok("звук".into());
    }
    Err("нет wpctl, pactl или amixer".into())
}

fn list_sinks() -> Vec<String> {
    if cfg!(target_os = "linux") && sdw_core::util::has("pactl") {
        if let Ok(out) = sdw_core::util::run("pactl", &["list", "short", "sinks"]) {
            return out
                .lines()
                .filter_map(|l| l.split('\t').nth(1))
                .map(str::to_string)
                .collect();
        }
    }
    Vec::new()
}

fn current_sink() -> String {
    if cfg!(target_os = "linux") && sdw_core::util::has("pactl") {
        if let Ok(out) = sdw_core::util::run("pactl", &["get-default-sink"]) {
            return out.trim().to_string();
        }
    }
    if cfg!(target_os = "linux") && sdw_core::util::has("wpctl") {
        if let Ok(out) = sdw_core::util::run("wpctl", &["inspect", "@DEFAULT_AUDIO_SINK@"]) {
            for line in out.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("* node.description = ") {
                    return rest.trim_matches('"').to_string();
                }
            }
        }
    }
    String::new()
}

fn set_sink(sink: &str) -> Result<String, String> {
    let sink = sink.trim();
    if sink.is_empty() {
        return Err("не выбран выход".into());
    }
    if cfg!(target_os = "linux") && sdw_core::util::has("pactl") {
        sdw_core::util::run("pactl", &["set-default-sink", sink])?;
        return Ok(format!("выход: {sink}"));
    }
    Err("смена выхода доступна через pactl".into())
}

// ---------------------------------------------------------------- covers

fn fetch_art(url: &str) -> Result<Vec<u8>, String> {
    if let Some(rest) = url.strip_prefix("file://") {
        let path = percent_decode(rest);
        return std::fs::read(&path).map_err(|e| format!("обложка: {e}"));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(8)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut r = agent.get(url).call().map_err(|e| e.to_string())?;
        if r.status().as_u16() != 200 {
            return Err(format!("обложка: HTTP {}", r.status().as_u16()));
        }
        return r
            .body_mut()
            .read_to_vec()
            .map_err(|e| e.to_string())
            .and_then(|b| {
                if b.len() > MAX_ART {
                    Err("обложка слишком большая".into())
                } else {
                    Ok(b)
                }
            });
    }
    if let Some(rest) = url.strip_prefix("data:") {
        let (meta, data) = rest.split_once(',').ok_or("обложка: плохой data url")?;
        if meta.contains(";base64") {
            return base64_decode(data);
        }
        return Ok(data.as_bytes().to_vec());
    }
    // Some players report a bare path.
    if url.starts_with('/') {
        return std::fs::read(url).map_err(|e| format!("обложка: {e}"));
    }
    Err("неизвестный формат обложки".into())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(a), Some(c)) = (
                (b[i + 1] as char).to_digit(16),
                (b[i + 2] as char).to_digit(16),
            ) {
                out.push((a * 16 + c) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// Guess an image mime type from the first bytes.
pub fn art_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}
