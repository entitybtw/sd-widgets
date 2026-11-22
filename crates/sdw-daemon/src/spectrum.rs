//! The computer's own sound, measured for the deck's visualizer.
//!
//! The daemon reads the default sink's monitor through `parec` (PipeWire or
//! PulseAudio), analyzes it with the same bands the deck uses, and keeps the
//! newest frame. Nothing runs while nobody looks: the capture starts on the
//! first spectrum request and stops a few seconds after the last one, so an
//! idle computer pays neither CPU nor a spare process.

use sdw_core::model::Spectrum;
use sdw_core::spectrum::{self, BANDS};
use std::io::Read;
use std::process::{Child, Command, Stdio};
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

/// Spawn `parec` on its own thread. The caller must hold the state lock, and
/// the thread takes it again only to publish a frame.
fn start_capture(state: Arc<Mutex<State>>) {
    let device = monitor_device();
    let mut child = match Command::new("parec")
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
    {
        Ok(child) => child,
        Err(e) => {
            if let Ok(mut st) = state.lock() {
                st.failed_at = Some(Instant::now());
                st.error = format!("parec: {e}");
                st.source.clear();
                st.running = false;
                st.starting = false;
            }
            return;
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            if let Ok(mut st) = state.lock() {
                st.failed_at = Some(Instant::now());
                st.error = "parec: no stdout".into();
                st.running = false;
                st.starting = false;
            }
            return;
        }
    };
    if let Ok(mut st) = state.lock() {
        st.running = true;
        st.starting = false;
        st.source = device;
        st.error.clear();
        st.failed_at = None;
    }
    std::thread::spawn(move || {
        let capture = Capture {
            state: Arc::clone(&state),
            child,
        };
        capture.read_loop(stdout);
    });
}

struct Capture {
    state: Arc<Mutex<State>>,
    child: Child,
}

impl Capture {
    fn read_loop(mut self, mut stdout: impl Read) {
        let mut chunk = vec![0u8; 8192];
        let mut acc: Vec<u8> = Vec::with_capacity(WINDOW * 2);
        let mut last = [0.0f32; BANDS];
        let mut died = false;
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => {
                    died = true;
                    break;
                }
                Ok(n) => {
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
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Ok(mut st) = self.state.lock() {
            st.running = false;
            st.source.clear();
            if died {
                // The sink may have been switched off or restarted; try again
                // a little later instead of on every poll.
                st.failed_at = Some(Instant::now());
                st.error = "parec stopped".into();
            } else {
                st.error.clear();
            }
        }
    }
}

/// The default sink's monitor: PulseAudio and PipeWire both expose it as
/// `<sink>.monitor`, and `@DEFAULT_MONITOR@` is the fallback when `pactl`
/// cannot name the sink.
fn monitor_device() -> String {
    // A test handler can point the capture at a null sink without touching the
    // real default output; unset in normal use.
    if let Ok(device) = std::env::var("SDW_SPECTRUM_DEVICE") {
        if !device.trim().is_empty() {
            return device;
        }
    }
    let sink = Command::new("pactl")
        .args(["get-default-sink"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|name| !name.is_empty());
    match sink {
        Some(sink) => format!("{sink}.monitor"),
        None => "@DEFAULT_MONITOR@".into(),
    }
}
