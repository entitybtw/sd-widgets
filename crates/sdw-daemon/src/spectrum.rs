//! The computer's own sound, measured for the deck's visualizer.
//!
//! The daemon reads the default output through `parec` on Linux (PipeWire or
//! PulseAudio) or through WASAPI loopback on Windows, analyzes it with the same
//! bands the deck uses, and keeps the newest frame. Nothing runs while nobody
//! looks: the capture starts on the first spectrum request and stops a few
//! seconds after the last one, so an idle computer pays neither CPU nor a spare
//! process.

use sdw_core::model::Spectrum;
use sdw_core::spectrum::{self, BANDS};
#[cfg(not(windows))]
use std::io::Read;
use std::process::Child;
#[cfg(not(windows))]
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
/// How long the capture keeps running after the last request.
const IDLE: Duration = Duration::from_secs(6);
/// A failed `parec` is retried after this pause, so a broken sink does not
/// spawn a process on every poll.
const RETRY: Duration = Duration::from_secs(15);
/// Bytes in one analysis window: 2048 stereo frames, about 43 ms.
const WINDOW: usize = 2048 * CHANNELS * 2;

struct State {
    seq: u64,
    bands: [f32; BANDS],
    last_audio: Option<Instant>,
    source: String,
    last_poll: Instant,
    failed_at: Option<Instant>,
    error: String,
    running: bool,
    starting: bool,
}

pub struct SpectrumService {
    state: Arc<Mutex<State>>,
}

impl Default for SpectrumService {
    fn default() -> Self {
        Self::new()
    }
}

impl SpectrumService {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                seq: 0,
                bands: [0.0; BANDS],
                last_audio: None,
                source: String::new(),
                last_poll: Instant::now() - IDLE - Duration::from_secs(1),
                failed_at: None,
                error: String::new(),
                running: false,
                starting: false,
            })),
        }
    }

    /// The current frame, and the nudge that starts or stops the capture.
    pub fn snapshot(&self) -> Spectrum {
        let start = {
            let Ok(mut st) = self.state.lock() else {
                return Spectrum::default();
            };
            st.last_poll = Instant::now();
            let retry = st.failed_at.map(|at| at.elapsed() >= RETRY).unwrap_or(true);
            if !st.running && !st.starting && retry {
                st.starting = true;
                true
            } else {
                false
            }
        };
        if start {
            start_capture(Arc::clone(&self.state));
        }
        let Ok(st) = self.state.lock() else {
            return Spectrum::default();
        };
        let fresh = st
            .last_audio
            .map(|at| at.elapsed() < Duration::from_secs(2))
            .unwrap_or(false);
        Spectrum {
            seq: st.seq,
            active: st.running,
            source: st.source.clone(),
            bands: if fresh { st.bands.to_vec() } else { Vec::new() },
        }
    }

    /// The last capture error, for the daemon window and the log.
    pub fn error(&self) -> String {
        self.state
            .lock()
            .map(|st| st.error.clone())
            .unwrap_or_default()
    }
}

/// One capture pipe: Linux reads a child's stdout, Windows reads loopback
/// frames that may simply not exist while the computer is silent.
trait ChunkSource: Send {
    /// `Some(0)` is the end, `None` means "nothing yet, keep the capture open".
    fn chunk(&mut self, out: &mut [u8]) -> std::io::Result<Option<usize>>;
    fn error(&self) -> String {
        String::new()
    }
}

#[cfg(not(windows))]
struct Blocking<R: Read + Send>(R);

#[cfg(not(windows))]
impl<R: Read + Send> ChunkSource for Blocking<R> {
    fn chunk(&mut self, out: &mut [u8]) -> std::io::Result<Option<usize>> {
        self.0.read(out).map(Some)
    }
}

#[cfg(windows)]
impl ChunkSource for crate::audio::StreamReader {
    fn chunk(&mut self, out: &mut [u8]) -> std::io::Result<Option<usize>> {
        crate::audio::StreamReader::chunk(self, out, Duration::from_millis(250))
    }

    fn error(&self) -> String {
        crate::audio::StreamReader::error(self)
    }
}

/// Open the capture and describe it for the status row.
#[cfg(not(windows))]
fn open_capture() -> Result<(Box<dyn ChunkSource>, Option<Child>, String), String> {
    let device = capture_source()?;
    let mut child = Command::new("parec")
        .args([
            "--raw",
            "--format=s16le",
            &format!("--rate={RATE}"),
            &format!("--channels={CHANNELS}"),
            "--latency-msec=40",
            &format!("--device={device}"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("parec: {e}"))?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        return Err("parec: no stdout".into());
    };
    Ok((Box::new(Blocking(stdout)), Some(child), device))
}

#[cfg(windows)]
fn open_capture() -> Result<(Box<dyn ChunkSource>, Option<Child>, String), String> {
    let reader = crate::audio::StreamReader::system(RATE)?;
    Ok((Box::new(reader), None, "системный выход".into()))
}

/// Start the capture on its own thread. The caller must hold the state lock,
/// and the thread takes it again only to publish a frame.
fn start_capture(state: Arc<Mutex<State>>) {
    let (source, child, label) = match open_capture() {
        Ok(parts) => parts,
        Err(e) => {
            if let Ok(mut st) = state.lock() {
                st.failed_at = Some(Instant::now());
                st.error = e;
                st.source.clear();
                st.running = false;
                st.starting = false;
            }
            return;
        }
    };
    if let Ok(mut st) = state.lock() {
        st.running = true;
        st.starting = false;
        st.source = label;
        st.error.clear();
        st.failed_at = None;
    }
    std::thread::spawn(move || {
        let capture = Capture {
            state: Arc::clone(&state),
            source,
            child,
        };
        capture.read_loop();
    });
}

struct Capture {
    state: Arc<Mutex<State>>,
    source: Box<dyn ChunkSource>,
    child: Option<Child>,
}

impl Capture {
    fn read_loop(mut self) {
        let mut chunk = vec![0u8; 8192];
        let mut acc: Vec<u8> = Vec::with_capacity(WINDOW * 2);
        let mut last = [0.0f32; BANDS];
        let mut died = false;
        loop {
            match self.source.chunk(&mut chunk) {
                Ok(Some(0)) => {
                    died = true;
                    break;
                }
                Ok(Some(n)) => {
                    acc.extend_from_slice(&chunk[..n]);
                    if acc.len() >= WINDOW {
                        let from = acc.len() - WINDOW;
                        let next = spectrum::analyze(&acc[from..], CHANNELS, RATE);
                        last = spectrum::follow(&last, &next, 0.78);
                        if let Ok(mut st) = self.state.lock() {
                            st.bands = last;
                            st.seq = st.seq.wrapping_add(1);
                            st.last_audio = Some(Instant::now());
                        }
                        acc.drain(..from);
                    }
                }
                // The output is silent; loopback has nothing to give yet.
                Ok(None) => {}
                Err(_) => {
                    died = true;
                    break;
                }
            }
            let idle = self
                .state
                .lock()
                .map(|st| st.last_poll.elapsed() >= IDLE)
                .unwrap_or(true);
            if idle {
                break;
            }
        }
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let detail = self.source.error();
        if let Ok(mut st) = self.state.lock() {
            st.running = false;
            st.source.clear();
            if died {
                // The sink may have been switched off or restarted; try again
                // a little later instead of on every poll.
                st.failed_at = Some(Instant::now());
                st.error = if detail.is_empty() {
                    "захват остановлен".into()
                } else {
                    detail
                };
            } else {
                st.error.clear();
            }
        }
    }
}

/// The source to capture, checked against the live list before it is used.
///
/// `parec --device=<name>` does not fail on an unknown name: it quietly records
/// the default source instead, and that source can be a microphone. So the name
/// is resolved and verified here, and a missing one stops the capture rather
/// than recording something the user never asked for.
#[cfg(not(windows))]
fn capture_source() -> Result<String, String> {
    // A test handler can point the capture at a null sink without touching the
    // real default output; unset in normal use.
    if let Ok(device) = std::env::var("SDW_SPECTRUM_DEVICE") {
        let device = device.trim();
        if !device.is_empty() {
            return check_source(device.to_string());
        }
    }
    let sink = default_sink()?;
    check_source(format!("{sink}.monitor"))
}

/// PulseAudio and PipeWire both expose a sink's monitor as `<sink>.monitor`.
#[cfg(not(windows))]
fn default_sink() -> Result<String, String> {
    let out = Command::new("pactl")
        .args(["get-default-sink"])
        .output()
        .map_err(|e| format!("pactl: {e}"))?;
    if !out.status.success() {
        return Err("pactl: нет выходного устройства".into());
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() {
        return Err("pactl: имя выходного устройства пустое".into());
    }
    Ok(name)
}

#[cfg(not(windows))]
fn check_source(name: String) -> Result<String, String> {
    // A pulse directive such as `@DEFAULT_MONITOR@` names a device the server
    // picks, and it always means a monitor; no capture, no guessing.
    if name.starts_with('@') {
        return Ok(name);
    }
    let out = Command::new("pactl")
        .args(["list", "short", "sources"])
        .output()
        .map_err(|e| format!("pactl: {e}"))?;
    let list = String::from_utf8_lossy(&out.stdout);
    if source_names(&list).iter().any(|source| source == &name) {
        Ok(name)
    } else {
        Err(format!("нет такого источника записи: {name}"))
    }
}

/// Names from `pactl list short sources`: `id<TAB>name<TAB>driver ...`.
#[cfg(not(windows))]
fn source_names(list: &str) -> Vec<String> {
    list.lines()
        .filter_map(|line| line.split('\t').nth(1))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_names_reads_the_second_column() {
        let list = "13402\tbluez_output.AA.monitor\tPipeWire\ts16le 2ch 48000Hz\tRUNNING\n\
                    67\talsa_input.usb.mic\tPipeWire\ts24le 1ch 48000Hz\tSUSPENDED\n";
        assert_eq!(
            source_names(list),
            vec![
                "bluez_output.AA.monitor".to_string(),
                "alsa_input.usb.mic".to_string()
            ]
        );
    }

    #[test]
    fn source_names_ignores_empty_and_broken_lines() {
        assert!(source_names("").is_empty());
        assert!(source_names("garbage without tabs\n\n").is_empty());
    }
}
