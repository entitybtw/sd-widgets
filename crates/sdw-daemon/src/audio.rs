//! PC audio to the Deck, over the same authenticated HTTP connection.
//!
//! The daemon records on the PulseAudio protocol (which PipeWire implements)
//! and serves raw `s16le` PCM.
//!
//! * `system` records `@DEFAULT_MONITOR@` with `parec`, so it captures whatever
//!   the computer plays on its default output.
//! * `apps` records every picked application stream with one `pw-record`
//!   process and sums the samples in Rust. Moving streams into a private null
//!   sink is not reliable on PipeWire (`move-sink-input` answers "Invalid
//!   argument"), so the mixer never touches the user's routing.

use sdw_core::model::{AudioApp, AudioStart, AudioStatus};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// One second of stereo 48 kHz s16 is 192 000 bytes; keep latency low.
const CAPACITY: usize = 192_000;

struct Buf {
    data: Mutex<VecDeque<u8>>,
    cv: Condvar,
    closed: AtomicBool,
    total: AtomicU64,
    error: Mutex<String>,
}

impl Buf {
    fn new() -> Self {
        Self {
            data: Mutex::new(VecDeque::with_capacity(CAPACITY)),
            cv: Condvar::new(),
            closed: AtomicBool::new(true),
            total: AtomicU64::new(0),
            error: Mutex::new(String::new()),
        }
    }

    fn reset(&self) {
        self.closed.store(false, Ordering::SeqCst);
        self.total.store(0, Ordering::Relaxed);
        if let Ok(mut d) = self.data.lock() {
            d.clear();
        }
        if let Ok(mut e) = self.error.lock() {
            e.clear();
        }
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.cv.notify_all();
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn push(&self, bytes: &[u8]) {
        if let Ok(mut d) = self.data.lock() {
            d.extend(bytes.iter().copied());
            while d.len() > CAPACITY {
                d.pop_front();
            }
            self.total.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        }
        self.cv.notify_all();
    }

    fn set_error(&self, text: String) {
        if let Ok(mut e) = self.error.lock() {
            if !text.trim().is_empty() {
                *e = text.trim().to_string();
            }
        }
    }

    fn error(&self) -> String {
        self.error.lock().map(|e| e.clone()).unwrap_or_default()
    }
}

struct Session {
    children: Vec<Child>,
    started: Instant,
    source: String,
    apps: Vec<String>,
    rate: u32,
    channels: u8,
    /// Set on the plain-PulseAudio `apps` path: streams to move back and the
    /// temporary null sink to unload.
    pulse: Option<PulseMoved>,
}

struct PulseMoved {
    module: String,
    streams: Vec<u32>,
}

pub struct Audio {
    session: Mutex<Option<Session>>,
    buf: Arc<Buf>,
    last_error: Mutex<String>,
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

impl Audio {
    pub fn new() -> Self {
        Self {
            session: Mutex::new(None),
            buf: Arc::new(Buf::new()),
            last_error: Mutex::new(String::new()),
        }
    }

    /// `pactl` lists the applications; `parec` or `pw-record` does the capture.
    pub fn available(&self) -> bool {
        sdw_core::util::has("pactl")
            && (sdw_core::util::has("parec") || sdw_core::util::has("pw-record"))
    }

    fn backend(&self) -> String {
        if sdw_core::util::has("pw-cli") || sdw_core::util::has("pw-play") {
            "pipewire".into()
        } else {
            "pulseaudio".into()
        }
    }

    pub fn is_running(&self) -> bool {
        self.session.lock().map(|s| s.is_some()).unwrap_or(false)
    }

    pub fn status(&self) -> AudioStatus {
        let session = self.session.lock().ok();
        let running = session.as_ref().map(|s| s.is_some()).unwrap_or(false);
        let mut out = AudioStatus {
            available: self.available(),
            backend: self.backend(),
            running,
            ..Default::default()
        };
        if let Some(s) = session.as_ref().and_then(|s| s.as_ref()) {
            out.source = s.source.clone();
            out.apps = s.apps.clone();
            out.rate = s.rate;
            out.channels = s.channels;
            out.seconds = s.started.elapsed().as_secs();
            out.bytes = self.buf.total.load(Ordering::Relaxed);
            out.error = self.buf.error();
        } else {
            out.error = self
                .last_error
                .lock()
                .map(|e| e.clone())
                .unwrap_or_default();
        }
        out
    }

    /// Applications that are playing sound right now, grouped by name.
    pub fn list_apps(&self) -> Vec<AudioApp> {
        let mut map: BTreeMap<String, AudioApp> = BTreeMap::new();
        for item in self.raw_inputs() {
            map.entry(item.name.clone()).or_insert(item);
        }
        map.into_values().collect()
    }

    /// Start streaming. `remote` marks a request that came over HTTP, so the
    /// daemon's `allow_remote` switch applies; the GUI starts with `false`.
    pub fn start(
        &self,
        cfg: &crate::config::AudioCfg,
        req: &AudioStart,
        remote: bool,
    ) -> Result<AudioStatus, String> {
        if !cfg.enabled {
            return Err("звук с пк выключен в настройках демона".into());
        }
        if remote && !cfg.allow_remote {
            return Err("запуск звука с деки выключен в настройках демона".into());
        }
        if !self.available() {
            return Err("нужны pactl и parec/pw-record (PipeWire или PulseAudio)".into());
        }
        let rate = match req.rate {
            0 => cfg.rate.clamp(8000, 48_000),
            r => r.clamp(8000, 48_000),
        };
        let channels = req.channels.clamp(1, 2);
        let source = if req.source == "apps" {
            "apps"
        } else {
            "system"
        };
        let apps: Vec<String> = req
            .apps
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if source == "apps" && apps.is_empty() {
            return Err("выберите хотя бы одно приложение".into());
        }

        self.stop();

        self.buf.reset();
        let mut children: Vec<Child> = Vec::new();
        let mut pulse: Option<PulseMoved> = None;

        if source == "apps" {
            let mut targets: Vec<u32> = Vec::new();
            for input in self.raw_inputs() {
                if apps.iter().any(|a| a == &input.name || a == &input.binary)
                    && !targets.contains(&input.index)
                {
                    targets.push(input.index);
                }
            }

            if sdw_core::util::has("pw-record") {
                // PipeWire: one recorder per stream, summed below. `pactl`
                // indices equal `object.serial`, a valid `--target`.
                if targets.is_empty() {
                    return Err("выбранные приложения сейчас не играют звук".into());
                }
                let mut sources: Vec<Arc<SourceBuf>> = Vec::new();
                for target in &targets {
                    let mut child = match Command::new("pw-record")
                        .arg(format!("--target={target}"))
                        .arg(format!("--rate={rate}"))
                        .arg(format!("--channels={channels}"))
                        .arg("--format=s16")
                        .arg("--latency=20ms")
                        .arg("--raw")
                        .arg("-")
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(child) => child,
                        Err(e) => {
                            kill_all(&mut children);
                            return Err(format!("pw-record: {e}"));
                        }
                    };
                    if let Some(mut out) = child.stdout.take() {
                        let source = Arc::new(SourceBuf::new(rate, channels));
                        sources.push(source.clone());
                        std::thread::spawn(move || {
                            let mut chunk = vec![0u8; 4096];
                            loop {
                                match out.read(&mut chunk) {
                                    Ok(0) => break,
                                    Ok(n) => source.push_bytes(&chunk[..n]),
                                    Err(_) => break,
                                }
                            }
                            source.alive.store(false, Ordering::SeqCst);
                        });
                    }
                    children.push(child);
                }

                let buf = self.buf.clone();
                std::thread::spawn(move || mix_loop(buf, sources, rate, channels));
            } else {
                // Pure PulseAudio fallback: a null sink and moved streams.
                // Kept as a last resort because this is the only way there.
                pulse = self.start_apps_pulse(&apps, rate, channels, &mut children)?;
            }
        } else {
            let mut child = match Command::new("parec")
                .arg("--device=@DEFAULT_MONITOR@")
                .arg(format!("--rate={rate}"))
                .arg(format!("--channels={channels}"))
                .arg("--format=s16le")
                .arg("--latency-msec=30")
                .arg("--raw")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(e) => return Err(format!("parec: {e}")),
            };
            if let Some(mut out) = child.stdout.take() {
                let buf = self.buf.clone();
                std::thread::spawn(move || {
                    let mut chunk = [0u8; 8192];
                    loop {
                        match out.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(n) => buf.push(&chunk[..n]),
                            Err(_) => break,
                        }
                    }
                    buf.close();
                });
            }
            if let Some(mut err) = child.stderr.take() {
                let buf = self.buf.clone();
                std::thread::spawn(move || {
                    let mut text = String::new();
                    let mut chunk = [0u8; 2048];
                    loop {
                        match err.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(n) => {
                                text.push_str(&String::from_utf8_lossy(&chunk[..n]));
                                if text.len() > 4096 {
                                    text = text[text.len() - 2048..].to_string();
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    if !text.trim().is_empty() {
                        buf.set_error(text);
                    }
                });
            }
            children.push(child);
        }

        std::thread::sleep(Duration::from_millis(180));
        if children
            .iter_mut()
            .all(|c| matches!(c.try_wait(), Ok(Some(_))))
        {
            let error = self.buf.error();
            self.buf.close();
            kill_all(&mut children);
            let message = if error.is_empty() {
                "захват звука завершился сразу".to_string()
            } else {
                error
            };
            if let Ok(mut last) = self.last_error.lock() {
                *last = message.clone();
            }
            return Err(message);
        }

        if let Ok(mut last) = self.last_error.lock() {
            last.clear();
        }
        let session = Session {
            children,
            started: Instant::now(),
            source: source.to_string(),
            apps,
            rate,
            channels,
            pulse,
        };
        if let Ok(mut slot) = self.session.lock() {
            *slot = Some(session);
        }
        Ok(self.status())
    }

    /// `apps` mode on a plain PulseAudio server: create a null sink, move the
    /// picked streams into it and record its monitor.
    fn start_apps_pulse(
        &self,
        apps: &[String],
        rate: u32,
        channels: u8,
        children: &mut Vec<Child>,
    ) -> Result<Option<PulseMoved>, String> {
        const NULL_SINK: &str = "sdw_audio";
        let module = sdw_core::util::run(
            "pactl",
            &[
                "load-module",
                "module-null-sink",
                &format!("sink_name={NULL_SINK}"),
                "sink_properties=device.description=sd-widgets",
            ],
        )
        .map_err(|e| format!("не удалось создать виртуальный выход: {e}"))?
        .trim()
        .to_string();
        std::thread::sleep(Duration::from_millis(320));

        let mut moved = Vec::new();
        for input in self.raw_inputs() {
            if !apps.iter().any(|a| a == &input.name || a == &input.binary) {
                continue;
            }
            let index = input.index.to_string();
            if sdw_core::util::run("pactl", &["move-sink-input", &index, NULL_SINK]).is_ok() {
                moved.push(input.index);
            }
        }
        if moved.is_empty() {
            let _ = sdw_core::util::run("pactl", &["unload-module", &module]);
            return Err("выбранные приложения сейчас не играют звук".into());
        }

        let mut child = match Command::new("parec")
            .arg(format!("--device={NULL_SINK}.monitor"))
            .arg(format!("--rate={rate}"))
            .arg(format!("--channels={channels}"))
            .arg("--format=s16le")
            .arg("--latency-msec=30")
            .arg("--raw")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                for index in &moved {
                    let index = index.to_string();
                    let _ = sdw_core::util::run(
                        "pactl",
                        &["move-sink-input", &index, "@DEFAULT_SINK@"],
                    );
                }
                let _ = sdw_core::util::run("pactl", &["unload-module", &module]);
                return Err(format!("parec: {e}"));
            }
        };
        if let Some(mut out) = child.stdout.take() {
            let buf = self.buf.clone();
            std::thread::spawn(move || {
                let mut chunk = [0u8; 8192];
                loop {
                    match out.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => buf.push(&chunk[..n]),
                        Err(_) => break,
                    }
                }
                buf.close();
            });
        }
        children.push(child);
        Ok(Some(PulseMoved {
            module,
            streams: moved,
        }))
    }

    pub fn stop(&self) {
        let session = self.session.lock().ok().and_then(|mut s| s.take());
        self.buf.close();
        if let Some(mut session) = session {
            kill_all(&mut session.children);
            if let Some(pulse) = session.pulse {
                for index in &pulse.streams {
                    let index = index.to_string();
                    let _ = sdw_core::util::run(
                        "pactl",
                        &["move-sink-input", &index, "@DEFAULT_SINK@"],
                    );
                }
                let _ = sdw_core::util::run("pactl", &["unload-module", &pulse.module]);
            }
        }
    }

    /// A blocking reader for one HTTP client, or `None` when idle.
    pub fn reader(&self) -> Option<PcmReader> {
        if !self.is_running() {
            return None;
        }
        Some(PcmReader {
            buf: self.buf.clone(),
        })
    }

    /// Every sink-input, without grouping by application name.
    fn raw_inputs(&self) -> Vec<AudioApp> {
        let raw = match sdw_core::util::run("pactl", &["-f", "json", "list", "sink-inputs"]) {
            Ok(text) => text,
            Err(_) => return Vec::new(),
        };
        let Ok(items) = serde_json::from_str::<Value>(&raw) else {
            return Vec::new();
        };
        let Some(items) = items.as_array() else {
            return Vec::new();
        };
        items
            .iter()
            .filter_map(|item| {
                let index = item.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
                let props = item.get("properties").cloned().unwrap_or(Value::Null);
                let name = prop(&props, "application.name");
                let binary = prop(&props, "application.process.binary");
                let media = prop(&props, "media.name");
                let label = if !name.is_empty() {
                    name.clone()
                } else if !binary.is_empty() {
                    binary.clone()
                } else {
                    media.clone()
                };
                if label.trim().is_empty() || index == 0 {
                    return None;
                }
                Some(AudioApp {
                    name: label,
                    binary,
                    index,
                    media,
                })
            })
            .collect()
    }
}

fn prop(props: &Value, key: &str) -> String {
    match props.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .first()
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

fn kill_all(children: &mut Vec<Child>) {
    for child in children.iter_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Samples of one application stream, pushed by its `pw-record` reader.
struct SourceBuf {
    data: Mutex<VecDeque<i16>>,
    alive: AtomicBool,
    cap: usize,
}

impl SourceBuf {
    fn new(rate: u32, channels: u8) -> Self {
        Self {
            data: Mutex::new(VecDeque::new()),
            alive: AtomicBool::new(true),
            cap: (rate as usize * channels as usize) * 2,
        }
    }

    fn push_bytes(&self, bytes: &[u8]) {
        if let Ok(mut data) = self.data.lock() {
            for pair in bytes.chunks_exact(2) {
                data.push_back(i16::from_le_bytes([pair[0], pair[1]]));
            }
            while data.len() > self.cap {
                data.pop_front();
            }
        }
    }

    fn take(&self, n: usize, out: &mut Vec<i16>) {
        out.clear();
        if let Ok(mut data) = self.data.lock() {
            for _ in 0..n {
                match data.pop_front() {
                    Some(v) => out.push(v),
                    None => break,
                }
            }
        }
    }
}

/// Sum the per-application streams into one stereo PCM stream.
fn mix_loop(buf: Arc<Buf>, sources: Vec<Arc<SourceBuf>>, rate: u32, channels: u8) {
    if sources.is_empty() {
        buf.close();
        return;
    }
    let frames = (rate as usize / 50).max(64); // 20 ms
    let per_source = frames * channels as usize;
    let mut scratch: Vec<i16> = Vec::with_capacity(per_source);
    let mut mix: Vec<i16> = vec![0; per_source];
    let step = Duration::from_millis(20);
    while !buf.is_closed() {
        std::thread::sleep(step);
        if buf.is_closed() {
            break;
        }
        if !sources.iter().any(|s| s.alive.load(Ordering::SeqCst)) {
            buf.close();
            break;
        }
        mix.iter_mut().for_each(|v| *v = 0);
        for source in &sources {
            source.take(per_source, &mut scratch);
            for (i, value) in scratch.iter().enumerate() {
                let sum = mix[i] as i32 + *value as i32;
                mix[i] = sum.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            }
        }
        let mut bytes = Vec::with_capacity(mix.len() * 2);
        for value in &mix {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        buf.push(&bytes);
    }
}

/// `Read` adapter that waits on the shared buffer and ends when capture stops.
pub struct PcmReader {
    buf: Arc<Buf>,
}

impl Read for PcmReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut data = match self.buf.data.lock() {
            Ok(d) => d,
            Err(_) => return Ok(0),
        };
        loop {
            if !data.is_empty() {
                let n = out.len().min(data.len());
                for slot in out.iter_mut().take(n) {
                    *slot = data.pop_front().unwrap_or(0);
                }
                return Ok(n);
            }
            if self.buf.closed.load(Ordering::SeqCst) {
                return Ok(0);
            }
            let (guard, _) = match self.buf.cv.wait_timeout(data, Duration::from_millis(250)) {
                Ok(pair) => pair,
                Err(_) => return Ok(0),
            };
            data = guard;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual smoke test: records the default monitor for a moment.
    /// `cargo test -p sdw-daemon audio_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn audio_smoke() {
        let audio = Audio::new();
        if !audio.available() {
            eprintln!("skipped: pactl/parec/pw-record missing");
            return;
        }
        let cfg = crate::config::AudioCfg {
            enabled: true,
            allow_remote: true,
            ..Default::default()
        };
        let req = AudioStart {
            source: "system".into(),
            ..Default::default()
        };
        match audio.start(&cfg, &req, false) {
            Ok(status) => {
                eprintln!("started: {status:?}");
                std::thread::sleep(Duration::from_millis(700));
                let mut reader = audio.reader().expect("reader");
                let mut buf = [0u8; 4096];
                let n = reader.read(&mut buf).expect("read");
                eprintln!("read {n} bytes");
                assert!(n > 0, "no pcm arrived");
                audio.stop();
                assert!(!audio.is_running());
            }
            Err(e) => eprintln!("capture unavailable: {e}"),
        }
    }
}
