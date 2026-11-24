//! Media and system volume on Windows.
//!
//! Players are read through the System Media Transport Controls (SMTC), the
//! same table Windows shows in the volume flyout: whatever the media keys can
//! control answers here too, packaged or not. The system output volume is the
//! endpoint volume of the default render device.
//!
//! Covers come from the session's own thumbnail and are proxied, so the deck
//! never has to reach the internet or read a file on the PC.

use crate::win::{wide_string, with_com, WaitAsync};

use sdw_core::model::{now_ms, MediaState};
use std::sync::Mutex;
use windows::core::GUID;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Storage::Streams::{DataReader, IRandomAccessStreamReference};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, StructuredStorage::PropVariantClear, CLSCTX_ALL, STGM_READ,
};

const MAX_ART: usize = 6 * 1024 * 1024;

/// The well-known `PKEY_Device_FriendlyName`, spelled out so the daemon does
/// not need the whole FunctionDiscovery header for one key.
const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};

/// VT_LPWSTR from `Win32::System::Variant`.
const VT_LPWSTR: u16 = 31;

#[derive(Default)]
struct ArtCache {
    url: String,
    bytes: Vec<u8>,
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
        let selected = self.selected.lock().map(|s| s.clone()).unwrap_or_default();
        let mut state = match with_com(|| snapshot(&selected)) {
            Ok(state) => state,
            Err(e) => {
                let mut state = MediaState::default();
                state.error = e;
                state
            }
        };
        state.updated_ms = now_ms();
        if state.players.is_empty() && state.error.is_empty() {
            state.error = "нет медиаплееров · запустите музыку или видео".into();
        }
        fill_volume(&mut state);
        state
    }

    /// Return the cover of the current track. `url` is the pseudo-address the
    /// snapshot published (`smtc:<hash>`); the bytes always come from the
    /// session that is playing now.
    pub fn art(&self, url: &str) -> Result<Vec<u8>, String> {
        let url = url.trim().to_string();
        if let Ok(cache) = self.art.lock() {
            if !url.is_empty() && cache.url == url && !cache.bytes.is_empty() {
                return Ok(cache.bytes.clone());
            }
        }
        let bytes = with_com(current_art)?;
        if bytes.len() > MAX_ART {
            return Err("обложка слишком большая".into());
        }
        if let Ok(mut cache) = self.art.lock() {
            cache.url = url;
            cache.bytes = bytes.clone();
        }
        Ok(bytes)
    }

    /// Apply one control action. Returns a short human message.
    pub fn control(&self, action: &str, value: f64, player: &str) -> Result<String, String> {
        if action == "sink" {
            return Err(
                "смену устройства вывода Windows не отдаёт приложениям: сделайте её в \
                 системном микшеере"
                    .into(),
            );
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
        let name = if player.trim().is_empty() {
            selected
        } else {
            player.trim().to_string()
        };

        match action {
            "play_pause" | "play" | "pause" | "next" | "prev" | "stop" => {
                with_com(|| transport(&name, action))
            }
            "seek" => with_com(|| seek(&name, value.max(0.0))),
            "volume" => set_volume(value.clamp(0.0, 100.0).round() as u8),
            "volume_up" => bump_volume(5),
            "volume_down" => bump_volume(-5),
            "mute" => toggle_mute(),
            other => Err(format!("неизвестное действие: {other}")),
        }
    }
}

// ---------------------------------------------------------------- players

/// Public display names of the players that have a session, and the session
/// table they come from.
fn session_table() -> Result<Vec<(String, GlobalSystemMediaTransportControlsSession)>, String> {
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .map_err(|e| format!("медиа: {e}"))?
        .wait()
        .map_err(|e| format!("медиа: {e}"))?;
    let list = manager.GetSessions().map_err(|e| format!("медиа: {e}"))?;
    let count = list.Size().unwrap_or(0);
    let mut out = Vec::new();
    for index in 0..count {
        let Ok(session) = list.GetAt(index) else {
            continue;
        };
        let Ok(id) = session.SourceAppUserModelId() else {
            continue;
        };
        out.push((player_name(&id.to_string()), session));
    }
    Ok(out)
}

/// `SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify` reads as `SpotifyMusic`.
fn player_name(aumid: &str) -> String {
    let base = aumid.split('!').next().unwrap_or(aumid);
    let base = base.split('_').next().unwrap_or(base);
    let name = base.rsplit('.').next().unwrap_or(base);
    if name.is_empty() {
        aumid.to_string()
    } else {
        name.to_string()
    }
}

fn status_of(session: &GlobalSystemMediaTransportControlsSession) -> String {
    let status = session
        .GetPlaybackInfo()
        .and_then(|info| info.PlaybackStatus())
        .unwrap_or(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Closed);
    if status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing {
        "Playing".into()
    } else if status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused {
        "Paused".into()
    } else if status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped {
        "Stopped".into()
    } else {
        "Closed".into()
    }
}

fn snapshot(selected: &str) -> Result<MediaState, String> {
    let table = session_table()?;
    let mut state = MediaState {
        players: table.iter().map(|(name, _)| name.clone()).collect(),
        ..Default::default()
    };
    if table.is_empty() {
        return Ok(state);
    }
    // Prefer the remembered player, then anything that is playing, then the first.
    let chosen = table
        .iter()
        .find(|(name, _)| !selected.is_empty() && name == selected)
        .or_else(|| {
            table.iter().find(|(_, session)| {
                session
                    .GetPlaybackInfo()
                    .and_then(|info| info.PlaybackStatus())
                    .map(|s| s == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
                    .unwrap_or(false)
            })
        })
        .or_else(|| table.first());
    let Some((name, session)) = chosen else {
        return Ok(state);
    };
    state.player = name.clone();
    state.status = status_of(session);
    state.available = true;

    if let Ok(props) = session
        .TryGetMediaPropertiesAsync()
        .and_then(|op| op.wait())
    {
        state.title = props.Title().map(|s| s.to_string()).unwrap_or_default();
        state.artist = props.Artist().map(|s| s.to_string()).unwrap_or_default();
        state.album = props
            .AlbumTitle()
            .map(|s| s.to_string())
            .unwrap_or_default();
        // The cover itself is fetched by `art`; the address only identifies it.
        let mut hash = 0u64;
        for byte in state
            .title
            .bytes()
            .chain(state.artist.bytes())
            .chain(state.album.bytes())
        {
            hash = hash.wrapping_mul(1099511628211).wrapping_add(byte as u64);
        }
        if hash != 0 {
            state.art_id = format!("{hash:016x}");
            state.art_url = format!("smtc:{hash:016x}");
        }
    }
    if let Ok(timeline) = session.GetTimelineProperties() {
        if let Ok(position) = timeline.Position() {
            state.position = ticks_to_seconds(position.Duration);
        }
        if let Ok(end) = timeline.EndTime() {
            state.length = ticks_to_seconds(end.Duration);
        }
    }
    if state.status.eq_ignore_ascii_case("stopped") && state.title.is_empty() {
        state.error = "плеер остановлен".into();
    }
    Ok(state)
}

fn ticks_to_seconds(ticks: i64) -> f64 {
    (ticks.max(0) as f64) / 10_000_000.0
}

fn transport(name: &str, action: &str) -> Result<String, String> {
    let table = session_table()?;
    let session = pick(&table, name).ok_or_else(|| "плеер не найден".to_string())?;
    let (verb, result) = match action {
        "play" => ("play", session.TryPlayAsync().and_then(|op| op.wait())),
        "pause" | "stop" => ("pause", session.TryPauseAsync().and_then(|op| op.wait())),
        "next" => ("next", session.TrySkipNextAsync().and_then(|op| op.wait())),
        "prev" => (
            "previous",
            session.TrySkipPreviousAsync().and_then(|op| op.wait()),
        ),
        _ => (
            "play-pause",
            session.TryTogglePlayPauseAsync().and_then(|op| op.wait()),
        ),
    };
    match result {
        Ok(true) => Ok(verb.to_string()),
        Ok(false) => Err(format!("{verb}: плеер отказал")),
        Err(e) => Err(format!("{verb}: {e}")),
    }
}

fn seek(name: &str, seconds: f64) -> Result<String, String> {
    let table = session_table()?;
    let session = pick(&table, name).ok_or_else(|| "плеер не найден".to_string())?;
    let ticks = (seconds * 10_000_000.0) as i64;
    match session
        .TryChangePlaybackPositionAsync(ticks)
        .and_then(|op| op.wait())
    {
        Ok(true) => Ok(format!("{seconds:.0} с")),
        Ok(false) => Err("перемотка: плеер отказал".into()),
        Err(e) => Err(format!("перемотка: {e}")),
    }
}

fn pick<'a>(
    table: &'a [(String, GlobalSystemMediaTransportControlsSession)],
    name: &str,
) -> Option<&'a GlobalSystemMediaTransportControlsSession> {
    table
        .iter()
        .find(|(player, _)| name.is_empty() || player == name)
        .or_else(|| table.first())
        .map(|(_, session)| session)
}

// ---------------------------------------------------------------- covers

fn current_art() -> Result<Vec<u8>, String> {
    let table = session_table()?;
    let session = pick(&table, "").ok_or_else(|| "у трека нет обложки".to_string())?;
    let props = session
        .TryGetMediaPropertiesAsync()
        .and_then(|op| op.wait())
        .map_err(|e| format!("обложка: {e}"))?;
    let reference = props
        .Thumbnail()
        .map_err(|_| "у трека нет обложки".to_string())?;
    read_stream(&reference)
}

fn read_stream(reference: &IRandomAccessStreamReference) -> Result<Vec<u8>, String> {
    let stream = reference
        .OpenReadAsync()
        .and_then(|op| op.wait())
        .map_err(|e| format!("обложка: {e}"))?;
    let size = stream.Size().unwrap_or(0) as usize;
    if size == 0 {
        return Err("обложка пустая".into());
    }
    if size > MAX_ART {
        return Err("обложка слишком большая".into());
    }
    let input = stream
        .GetInputStreamAt(0)
        .map_err(|e| format!("обложка: {e}"))?;
    let reader = DataReader::CreateDataReader(&input).map_err(|e| format!("обложка: {e}"))?;
    let loaded = reader
        .LoadAsync(size as u32)
        .and_then(|op| op.wait())
        .map_err(|e| format!("обложка: {e}"))? as usize;
    let mut bytes = vec![0u8; size];
    reader
        .ReadBytes(&mut bytes)
        .map_err(|e| format!("обложка: {e}"))?;
    bytes.truncate(loaded.min(size));
    Ok(bytes)
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

// ---------------------------------------------------------------- volume

fn default_endpoint() -> Result<IMMDevice, String> {
    // SAFETY: standard COM creation; the enumerator lives while the device is used.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("аудиоустройства: {e}"))?;
        enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| format!("нет устройства вывода: {e}"))
    }
}

fn endpoint_volume() -> Result<IAudioEndpointVolume, String> {
    let device = default_endpoint()?;
    // SAFETY: the device is a valid endpoint from the enumerator above.
    unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(|e| format!("громкость: {e}"))
}

fn fill_volume(state: &mut MediaState) {
    if let Ok((volume, muted, name)) = with_com(read_volume) {
        state.volume = volume;
        state.muted = muted;
        state.sink = name;
    }
}

fn read_volume() -> Result<(u8, bool, String), String> {
    let device = default_endpoint()?;
    let volume = endpoint_volume()?;
    // SAFETY: the endpoint volume is live for the duration of these calls.
    unsafe {
        let scalar = volume
            .GetMasterVolumeLevelScalar()
            .map_err(|e| format!("громкость: {e}"))?;
        let muted = volume.GetMute().map(|m| m.as_bool()).unwrap_or(false);
        Ok((
            (scalar * 100.0).round().clamp(0.0, 150.0) as u8,
            muted,
            endpoint_name(&device),
        ))
    }
}

fn endpoint_name(device: &IMMDevice) -> String {
    // SAFETY: the property store is COM-owned and dropped at the end.
    unsafe {
        let Ok(store) = device.OpenPropertyStore(STGM_READ) else {
            return String::new();
        };
        let Ok(mut value) = store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME) else {
            return String::new();
        };
        let name = if value.Anonymous.Anonymous.vt.0 == VT_LPWSTR {
            wide_string(value.Anonymous.Anonymous.Anonymous.pwszVal)
        } else {
            String::new()
        };
        let _ = PropVariantClear(&mut value);
        name
    }
}

fn set_volume(percent: u8) -> Result<String, String> {
    let percent = percent.min(100);
    let volume = endpoint_volume()?;
    // SAFETY: the endpoint volume is live for the duration of these calls.
    unsafe {
        volume
            .SetMasterVolumeLevelScalar(percent as f32 / 100.0, std::ptr::null())
            .map_err(|e| format!("громкость: {e}"))?;
        // The user dragged the slider, so they expect sound.
        volume
            .SetMute(false, std::ptr::null())
            .map_err(|e| format!("громкость: {e}"))?;
    }
    Ok(format!("{percent}%"))
}

fn bump_volume(delta: i32) -> Result<String, String> {
    let current = with_com(read_volume).map(|v| v.0 as i32).unwrap_or(50);
    set_volume((current + delta).clamp(0, 100) as u8)
}

fn toggle_mute() -> Result<String, String> {
    let volume = endpoint_volume()?;
    // SAFETY: the endpoint volume is live for the duration of these calls.
    let muted = unsafe { volume.GetMute() }
        .map(|m| m.as_bool())
        .unwrap_or(false);
    unsafe {
        volume
            .SetMute(!muted, std::ptr::null())
            .map_err(|e| format!("громкость: {e}"))?;
    }
    Ok("звук".into())
}
