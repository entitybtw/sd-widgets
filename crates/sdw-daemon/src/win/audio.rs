//! PC audio to the Deck through WASAPI.
//!
//! The daemon captures the computer's sound with the Windows Audio Session API
//! and serves raw `s16le` PCM over the same authenticated HTTP connection the
//! Linux build uses.
//!
//! * `system` records the default output device in shared loopback mode, so it
//!   captures whatever the computer plays.
//! * `apps` starts one process loopback per picked application (Windows 10
//!   2004 and newer) and sums the streams in Rust, the way the PipeWire path
//!   does on Linux. Nothing is rerouted: the user keeps hearing everything.
//!
//! WASAPI hands out the engine's own mix format (usually 32-bit float at
//! 48 kHz); [`crate::dsp`] converts it to the stereo `s16le` the deck plays.

use crate::dsp::{Converter, SampleKind};
use crate::win::{wide_string, with_com};
use sdw_core::model::{AudioApp, AudioStart, AudioStatus};
use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use windows::core::{implement, Interface, Ref};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    eConsole, eRender, ActivateAudioInterfaceAsync, AudioSessionStateActive,
    IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
    IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
    VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemFree, StructuredStorage::PROPVARIANT, BLOB, CLSCTX_ALL,
};
use windows::Win32::System::Threading::{
    CreateEventW, OpenProcess, QueryFullProcessImageNameW, SetEvent, WaitForSingleObject,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::Variant::VT_BLOB;

/// One second of stereo 48 kHz s16 is 192 000 bytes; keep latency low.
const CAPACITY: usize = 192_000;
/// How long the activation of a process loopback may take.
const ACTIVATE_MS: u32 = 8000;

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
    stop: Arc<AtomicBool>,
    workers: Vec<std::thread::JoinHandle<()>>,
    started: Instant,
    source: String,
    apps: Vec<String>,
    rate: u32,
    channels: u8,
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

    /// WASAPI is part of Windows; only the output device can be missing, and
    /// that is reported by the start with a message the user can act on.
    pub fn available(&self) -> bool {
        true
    }

    fn backend(&self) -> String {
        "wasapi".into()
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
        for item in sessions() {
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
        let rate = match req.rate {
            0 => cfg.rate.clamp(8000, 48_000),
            r => r.clamp(8000, 48_000),
        };
        // The deck's player always opens two channels; capture in stereo and
        // duplicate a mono mix instead of sending a format it cannot play.
        let channels = 2u8;
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
        let stop = Arc::new(AtomicBool::new(false));
        let mut workers = Vec::new();

        if source == "apps" {
            let mut pids: Vec<u32> = Vec::new();
            for input in sessions() {
                if apps.iter().any(|a| a == &input.name || a == &input.binary)
                    && !pids.contains(&input.index)
                {
                    pids.push(input.index);
                }
            }
            if pids.is_empty() {
                self.buf.close();
                return Err("выбранные приложения сейчас не играют звук".into());
            }
            let mut sources: Vec<Arc<SourceBuf>> = Vec::new();
            for pid in pids {
                let source_buf = Arc::new(SourceBuf::new(rate, channels));
                sources.push(source_buf.clone());
                let stop = stop.clone();
                workers.push(std::thread::spawn(move || {
                    let result = with_com(|| {
                        let mut capture = Capture::open_process(pid)?;
                        pump(&*source_buf, &mut capture, &stop, rate)
                    });
                    if let Err(e) = result {
                        source_buf.fail(e);
                    }
                    source_buf.alive.store(false, Ordering::SeqCst);
                }));
            }
            let buf = self.buf.clone();
            let stop = stop.clone();
            workers.push(std::thread::spawn(move || {
                mix_loop(buf, sources, stop, rate);
            }));
        } else {
            let buf = self.buf.clone();
            let stop = stop.clone();
            workers.push(std::thread::spawn(move || {
                let result = with_com(|| {
                    let mut capture = Capture::open_system()?;
                    pump(&*buf, &mut capture, &stop, rate)
                });
                if let Err(e) = result {
                    buf.set_error(e);
                }
                buf.close();
            }));
        }

        std::thread::sleep(Duration::from_millis(250));
        if let Some(error) = worker_error(&self.buf, &workers) {
            stop.store(true, Ordering::SeqCst);
            join_all(workers);
            self.buf.close();
            if let Ok(mut last) = self.last_error.lock() {
                *last = error.clone();
            }
            return Err(error);
        }

        if let Ok(mut last) = self.last_error.lock() {
            last.clear();
        }
        let session = Session {
            stop,
            workers,
            started: Instant::now(),
            source: source.to_string(),
            apps,
            rate,
            channels,
        };
        if let Ok(mut slot) = self.session.lock() {
            *slot = Some(session);
        }
        Ok(self.status())
    }

    pub fn stop(&self) {
        let session = self.session.lock().ok().and_then(|mut s| s.take());
        self.buf.close();
        if let Some(session) = session {
            session.stop.store(true, Ordering::SeqCst);
            join_all(session.workers);
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
}

/// Waits a moment after the start and reports the capture thread's error.
fn worker_error(buf: &Buf, workers: &[std::thread::JoinHandle<()>]) -> Option<String> {
    let error = buf.error();
    if !error.trim().is_empty() {
        return Some(error);
    }
    // A thread that already finished without writing anything failed before it
    // could set a message (a missing device, for instance).
    if workers.iter().all(|w| w.is_finished()) && buf.total.load(Ordering::Relaxed) == 0 {
        return Some("захват звука завершился сразу".into());
    }
    None
}

fn join_all(workers: Vec<std::thread::JoinHandle<()>>) {
    for worker in workers {
        let _ = worker.join();
    }
}

// ---------------------------------------------------------------- capture

/// One open WASAPI loopback stream: the client, its capture side and the mix
/// format the engine chose.
struct Capture {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    kind: SampleKind,
    channels: u16,
    rate: u32,
}

impl Capture {
    fn open_system() -> Result<Self, String> {
        // SAFETY: every call below is a plain Win32/COM call; the pointers from
        // `GetMixFormat` are owned by COM and freed once they are copied.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("аудиоустройства: {e}"))?;
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| format!("нет устройства вывода: {e}"))?;
            let client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| format!("аудиоклиент: {e}"))?;
            Self::prepare(client, 0)
        }
    }

    fn open_process(pid: u32) -> Result<Self, String> {
        let client = activate_process(pid)?;
        // A positive buffer duration is required for process loopback; the
        // engine rounds it to its own period anyway.
        Self::prepare(client, 5_000_000)
    }

    fn prepare(client: IAudioClient, buffer_duration: i64) -> Result<Self, String> {
        // SAFETY: `mix` is a COM-owned WAVEFORMATEX; it is read and freed here.
        unsafe {
            let mix = client
                .GetMixFormat()
                .map_err(|e| format!("формат устройства: {e}"))?;
            let (kind, channels, rate) = read_format(mix);
            let init = client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                buffer_duration,
                0,
                mix,
                None,
            );
            CoTaskMemFree(Some(mix as *const _));
            init.map_err(|e| format!("захват звука: {e}"))?;
            let capture: IAudioCaptureClient = client
                .GetService()
                .map_err(|e| format!("захват звука: {e}"))?;
            Ok(Self {
                client,
                capture,
                kind,
                channels,
                rate,
            })
        }
    }
}

/// Read the sample layout from a WAVEFORMATEX (or its extensible form).
///
/// # Safety
/// `format` must point at a valid WAVEFORMATEX whose `cbSize` matches an
/// extensible header when the tag says so.
unsafe fn read_format(format: *const WAVEFORMATEX) -> (SampleKind, u16, u32) {
    let base = *format;
    let tag = if base.wFormatTag == 0xFFFE && base.cbSize >= 22 {
        let extensible = format as *const WAVEFORMATEXTENSIBLE;
        // SAFETY: the caller promised a complete extensible header.
        (*extensible).SubFormat.data1
    } else {
        base.wFormatTag as u32
    };
    let kind = match (tag, base.wBitsPerSample) {
        (3, 32) => SampleKind::F32,
        (1, 16) => SampleKind::I16,
        (1, 32) => SampleKind::I32,
        // Anything else is rare enough that guessing would play noise.
        _ => SampleKind::F32,
    };
    (kind, base.nChannels.max(1), base.nSamplesPerSec.max(1))
}

fn pump(
    sink: &dyn Sink,
    capture: &mut Capture,
    stop: &AtomicBool,
    rate: u32,
) -> Result<(), String> {
    let mut converter = Converter::new(capture.kind, capture.channels, capture.rate, rate);
    let mut samples: Vec<i16> = Vec::new();
    let mut bytes: Vec<u8> = Vec::new();
    // SAFETY: the client was initialized above and is owned by this thread.
    unsafe { capture.client.Start() }.map_err(|e| format!("запуск захвата: {e}"))?;
    let result = (|| -> Result<(), String> {
        while !stop.load(Ordering::SeqCst) {
            let mut moved = false;
            loop {
                // SAFETY: the pointers below are filled by WASAPI and only read
                // inside this iteration; `ReleaseBuffer` hands them back.
                let size = unsafe { capture.capture.GetNextPacketSize() }
                    .map_err(|e| format!("звук: {e}"))?;
                if size == 0 {
                    break;
                }
                let mut data: *mut u8 = std::ptr::null_mut();
                let mut frames: u32 = 0;
                let mut flags: u32 = 0;
                unsafe {
                    capture
                        .capture
                        .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                }
                .map_err(|e| format!("звук: {e}"))?;
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                if !silent && !data.is_null() && frames > 0 {
                    let len = frames as usize * capture.channels as usize * capture.kind.bytes();
                    // SAFETY: WASAPI guarantees at least `frames` whole frames.
                    let packet = unsafe { std::slice::from_raw_parts(data, len) };
                    samples.clear();
                    converter.push(packet, &mut samples);
                    if !samples.is_empty() {
                        bytes.clear();
                        bytes.reserve(samples.len() * 2);
                        for value in &samples {
                            bytes.extend_from_slice(&value.to_le_bytes());
                        }
                        sink.push_pcm(&bytes);
                    }
                }
                unsafe { capture.capture.ReleaseBuffer(frames) }
                    .map_err(|e| format!("звук: {e}"))?;
                moved = true;
            }
            if !moved {
                std::thread::sleep(Duration::from_millis(8));
            }
        }
        Ok(())
    })();
    // SAFETY: the same client that was started above.
    unsafe {
        let _ = capture.client.Stop();
    };
    result
}

/// Open a process loopback for one application through the asynchronous
/// activation API, which is the only way to ask for a private capture.
fn activate_process(pid: u32) -> Result<IAudioClient, String> {
    // The activation params must outlive the asynchronous call: the API reads
    // them until the operation completes.
    let params: Box<AUDIOCLIENT_ACTIVATION_PARAMS> = Box::new(AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: windows::Win32::Media::Audio::AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    });
    // The whole COM dance below is unsafe by nature; each step checks its
    // result before the next one uses it.
    unsafe {
        let event =
            CreateEventW(None, true, false, None).map_err(|e| format!("событие звука: {e}"))?;
        let handler = ActivateHandler { event };
        let handler: IActivateAudioInterfaceCompletionHandler = handler.into();

        let prop = PROPVARIANT {
            Anonymous: windows::Win32::System::Com::StructuredStorage::PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(
                    windows::Win32::System::Com::StructuredStorage::PROPVARIANT_0_0 {
                        vt: VT_BLOB,
                        ..Default::default()
                    },
                ),
            },
        };
        // A blob activation parameter: the driver reads this struct during the
        // asynchronous activation, so it stays alive in `params` above.
        let prop = unsafe {
            let mut prop = prop;
            (*prop.Anonymous.Anonymous).Anonymous.blob = BLOB {
                cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                pBlobData: params.as_ref() as *const _ as *mut u8,
            };
            prop
        };

        let operation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&prop),
            &handler,
        )
        .map_err(|e| format!("приложение {pid}: {e}"))?;

        let wait = WaitForSingleObject(event, ACTIVATE_MS);
        let _ = CloseHandle(event);
        if wait != WAIT_OBJECT_0 {
            return Err(format!("приложение {pid} не ответило за {ACTIVATE_MS} мс"));
        }
        let mut status = windows::core::HRESULT(0);
        let mut interface: Option<windows::core::IUnknown> = None;
        operation
            .GetActivateResult(&mut status, &mut interface)
            .map_err(|e| format!("приложение {pid}: {e}"))?;
        if status.is_err() {
            return Err(format!("приложение {pid}: {}", status.message()));
        }
        let interface = interface.ok_or_else(|| format!("приложение {pid}: нет звука"))?;
        interface
            .cast()
            .map_err(|e| format!("приложение {pid}: {e}"))
    }
}

/// The completion callback of the asynchronous activation: it only wakes the
/// waiting thread.
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct ActivateHandler {
    event: HANDLE,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivateHandler_Impl {
    fn ActivateCompleted(
        &self,
        _operation: Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        // SAFETY: `event` was created in `activate_process` and is closed only
        // after this callback has fired.
        unsafe { SetEvent(self.event) }.ok();
        Ok(())
    }
}

// ---------------------------------------------------------------- sessions

/// Every application with an audio session on the default output.
fn sessions() -> Vec<AudioApp> {
    with_com(read_sessions).unwrap_or_default()
}

fn read_sessions() -> Result<Vec<AudioApp>, String> {
    let mut out = Vec::new();
    // SAFETY: standard COM queries; every returned pointer is a COM reference
    // that is dropped at the end of its scope.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("аудиоустройства: {e}"))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| format!("нет устройства вывода: {e}"))?;
        let manager: IAudioSessionManager2 = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("сеансы звука: {e}"))?;
        let list = manager
            .GetSessionEnumerator()
            .map_err(|e| format!("сеансы звука: {e}"))?;
        let count = list.GetCount().unwrap_or(0);
        for index in 0..count {
            let Ok(control) = list.GetSession(index) else {
                continue;
            };
            let Ok(control) = control.cast::<IAudioSessionControl2>() else {
                continue;
            };
            if control
                .GetState()
                .map(|s| s != AudioSessionStateActive)
                .unwrap_or(true)
            {
                continue;
            }
            let Ok(pid) = control.GetProcessId() else {
                continue;
            };
            if pid == 0 {
                continue;
            }
            let binary = process_image(pid);
            if binary.is_empty() {
                continue;
            }
            let media = wide_string(control.GetDisplayName().unwrap_or_default());
            let name = std::path::Path::new(&binary)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| binary.clone());
            out.push(AudioApp {
                name,
                binary,
                index: pid,
                media,
            });
        }
    }
    Ok(out)
}

/// The file name of a running process, or an empty string when it cannot be
/// read (a protected system process).
fn process_image(pid: u32) -> String {
    // SAFETY: the handle is closed before returning; the buffer is sized by the
    // call itself.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or(path)
    }
}

// ---------------------------------------------------------------- mixing

trait Sink: Send + Sync {
    fn push_pcm(&self, bytes: &[u8]);
}

impl Sink for Buf {
    fn push_pcm(&self, bytes: &[u8]) {
        self.push(bytes);
    }
}

/// Samples of one application stream, pushed by its capture thread.
struct SourceBuf {
    data: Mutex<VecDeque<i16>>,
    alive: AtomicBool,
    cap: usize,
    error: Mutex<String>,
}

impl SourceBuf {
    fn new(rate: u32, channels: u8) -> Self {
        Self {
            data: Mutex::new(VecDeque::new()),
            alive: AtomicBool::new(true),
            cap: (rate as usize * channels as usize) * 2,
            error: Mutex::new(String::new()),
        }
    }

    fn fail(&self, text: String) {
        if let Ok(mut e) = self.error.lock() {
            *e = text;
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

impl Sink for SourceBuf {
    fn push_pcm(&self, bytes: &[u8]) {
        if let Ok(mut data) = self.data.lock() {
            for pair in bytes.chunks_exact(2) {
                data.push_back(i16::from_le_bytes([pair[0], pair[1]]));
            }
            while data.len() > self.cap {
                data.pop_front();
            }
        }
    }
}

/// Sum the per-application streams into one stereo PCM stream, exactly like the
/// Linux mixer: every 20 ms the available frames of each source are added.
fn mix_loop(buf: Arc<Buf>, sources: Vec<Arc<SourceBuf>>, stop: Arc<AtomicBool>, rate: u32) {
    let frames = (rate as usize / 50).max(64); // 20 ms
    let per_source = frames * 2;
    let mut scratch: Vec<i16> = Vec::with_capacity(per_source);
    let mut mix: Vec<i16> = vec![0; per_source];
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(20));
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if !sources.iter().any(|s| s.alive.load(Ordering::SeqCst)) {
            let message = sources
                .iter()
                .find_map(|s| {
                    let text = s.error.lock().map(|e| e.clone()).unwrap_or_default();
                    (!text.is_empty()).then_some(text)
                })
                .unwrap_or_else(|| "выбранные приложения перестали играть".into());
            buf.set_error(message);
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
    buf.close();
}

/// A background system loopback capture that behaves like a pipe: the deck's
/// visualizer asks for bytes and gets them as they arrive.
pub(crate) struct StreamReader {
    buf: Arc<Buf>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl StreamReader {
    pub(crate) fn system(rate: u32) -> Result<Self, String> {
        let buf = Arc::new(Buf::new());
        buf.reset();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_buf = buf.clone();
        let thread_stop = stop.clone();
        let worker = std::thread::Builder::new()
            .name("sdw-spectrum".into())
            .spawn(move || {
                let result = with_com(|| {
                    let mut capture = Capture::open_system()?;
                    pump(&*thread_buf, &mut capture, &thread_stop, rate)
                });
                if let Err(e) = result {
                    thread_buf.set_error(e);
                }
                thread_buf.close();
            })
            .map_err(|e| format!("поток звука: {e}"))?;
        Ok(Self {
            buf,
            stop,
            worker: Some(worker),
        })
    }

    /// The capture error, when the stream stopped on one.
    pub(crate) fn error(&self) -> String {
        self.buf.error()
    }

    /// Bytes if there are any, an empty result after `wait`, and `None` while
    /// the device is simply silent (loopback sends nothing then).
    pub(crate) fn chunk(
        &mut self,
        out: &mut [u8],
        wait: Duration,
    ) -> std::io::Result<Option<usize>> {
        if out.is_empty() {
            return Ok(Some(0));
        }
        let mut data = match self.buf.data.lock() {
            Ok(d) => d,
            Err(_) => return Ok(Some(0)),
        };
        if data.is_empty() && !self.buf.closed.load(Ordering::SeqCst) {
            let (guard, _) = match self.buf.cv.wait_timeout(data, wait) {
                Ok(pair) => pair,
                Err(_) => return Ok(Some(0)),
            };
            data = guard;
        }
        if !data.is_empty() {
            let n = out.len().min(data.len());
            for slot in out.iter_mut().take(n) {
                *slot = data.pop_front().unwrap_or(0);
            }
            return Ok(Some(n));
        }
        if self.buf.closed.load(Ordering::SeqCst) {
            return Ok(Some(0));
        }
        Ok(None)
    }
}

impl Drop for StreamReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
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
