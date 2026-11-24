//! Extra-screen streaming: a virtual output on the computer, captured as JPEG
//! for the deck to look at.
//!
//! There is no single portable way to add a monitor, so the daemon detects the
//! compositor and picks the least surprising mechanism:
//!
//! * niri with virtual outputs (biri or the upstream patch): an output drawn by
//!   the compositor itself + `grim`
//! * niri / wlroots without them: VKMS output + `grim` (or a mirror of an existing output)
//! * Hyprland: native headless output + `grim`
//! * KDE Plasma: `krfb-virtualmonitor` + a tiny built-in RFB client
//! * X11: `ffmpeg -f x11grab`
//! * Windows: `ffmpeg -f gdigrab`
//!
//! Every path ends in the same place: the newest JPEG frame in shared memory,
//! served by `GET /api/v1/monitor/frame`.

use crate::config::DaemonConfig;
use crate::state::Logger;
use sdw_core::model::*;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Name of the output this daemon creates for streaming. A fixed name makes it
/// easy to find, reuse and remove, and shows up in the Deck's output list.
const NIRI_VIRTUAL_NAME: &str = "sdw-deck";

#[derive(Debug, Clone, Default)]
pub struct SharedFrame {
    pub bytes: Vec<u8>,
    pub ts: u64,
    pub width: u32,
    pub height: u32,
}

struct Inner {
    running: bool,
    mode: MonitorMode,
    backend: MonitorBackend,
    position: MonitorPosition,
    compositor: String,
    output: String,
    width: u32,
    height: u32,
    fps: u32,
    quality: u8,
    outputs: Vec<DisplayOutput>,
    tools: Vec<ToolStatus>,
    config_hint: String,
    config_file: String,
    last_error: String,
    note: String,
    started_at: u64,
    output_rect: (i32, i32, u32, u32),
    created_output: Option<String>,
    /// The running niri can create virtual outputs (`niri msg
    /// create-virtual-output`), which is how biri and patched builds behave.
    niri_virtual: bool,
    /// Output this daemon enabled for streaming, and the compositor config
    /// text from before its layout was written. Both are restored on stop, so
    /// a finished stream never leaves a phantom monitor behind.
    layout_output: Option<String>,
    layout_restore: Option<(std::path::PathBuf, String)>,
    rfb: bool,
}

pub struct MonitorManager {
    inner: Arc<Mutex<Inner>>,
    frame: Arc<Mutex<SharedFrame>>,
    stop: Mutex<Arc<AtomicBool>>,
    child: Arc<Mutex<Option<Child>>>,
    frames: Arc<AtomicU64>,
    logger: Logger,
}

impl MonitorManager {
    pub fn new(logger: Logger) -> Self {
        let mut inner = Inner {
            running: false,
            mode: MonitorMode::Extend,
            backend: MonitorBackend::Auto,
            position: MonitorPosition::Auto,
            compositor: String::new(),
            output: String::new(),
            width: 1280,
            height: 800,
            fps: 15,
            quality: 75,
            outputs: Vec::new(),
            tools: Vec::new(),
            config_hint: String::new(),
            config_file: String::new(),
            last_error: String::new(),
            note: String::new(),
            started_at: 0,
            output_rect: (0, 0, 0, 0),
            created_output: None,
            niri_virtual: niri_virtual_available(),
            layout_output: None,
            layout_restore: None,
            rfb: false,
        };
        let (compositor, outputs) = detect_all();
        inner.compositor = compositor.clone();
        inner.outputs = outputs;
        inner.tools = detect_tools();
        inner.config_hint = config_hint(
            &compositor,
            &inner.output,
            1280,
            800,
            60,
            inner.position,
            &inner.outputs,
        );
        inner.config_file = config_file_for(&compositor);
        Self {
            inner: Arc::new(Mutex::new(inner)),
            frame: Arc::new(Mutex::new(SharedFrame::default())),
            stop: Mutex::new(Arc::new(AtomicBool::new(true))),
            child: Arc::new(Mutex::new(None)),
            frames: Arc::new(AtomicU64::new(0)),
            logger,
        }
    }

    pub fn status(&self) -> MonitorStatus {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let frame = self.frame.lock().unwrap_or_else(|e| e.into_inner());
        let age = if frame.ts == 0 {
            0
        } else {
            now_ms().saturating_sub(frame.ts)
        };
        MonitorStatus {
            running: inner.running,
            mode: inner.mode,
            position: inner.position,
            backend: inner.backend,
            backend_label: inner.backend.label().to_string(),
            compositor: inner.compositor.clone(),
            output: inner.output.clone(),
            width: inner.width,
            height: inner.height,
            fps: inner.fps,
            quality: inner.quality,
            frame_age_ms: age,
            frames: self.frames.load(Ordering::Relaxed),
            outputs: inner.outputs.clone(),
            tools: inner.tools.clone(),
            config_hint: inner.config_hint.clone(),
            config_file: inner.config_file.clone(),
            last_error: inner.last_error.clone(),
            note: inner.note.clone(),
            vm: vm_kind(),
            vkms: vkms_available(),
            vkms_loaded: vkms_loaded(),
            niri_virtual: inner.niri_virtual,
            started_at: inner.started_at,
        }
    }

    pub fn latest_frame(&self) -> Option<Vec<u8>> {
        let f = self.frame.lock().ok()?;
        (!f.bytes.is_empty()).then(|| f.bytes.clone())
    }

    /// One button in the GUI: load VKMS and make it survive a reboot. This is
    /// the whole "make the extra screen work on niri or in a VM" path.
    pub fn prepare_vkms(&self, sudo_password: &str) -> Result<String, String> {
        run_vkms(sudo_password)?;
        install_vkms_autoload(sudo_password)?;
        self.refresh();
        Ok("vkms загружен и включён в автозагрузку".into())
    }

    pub fn refresh(&self) {
        let (compositor, outputs) = detect_all();
        let tools = detect_tools();
        let niri_virtual = niri_virtual_available();
        if let Ok(mut inner) = self.inner.lock() {
            inner.compositor = compositor;
            inner.outputs = outputs;
            inner.tools = tools;
            inner.niri_virtual = niri_virtual;
            inner.config_hint = config_hint(
                &inner.compositor,
                &inner.output,
                inner.width,
                inner.height,
                60,
                inner.position,
                &inner.outputs,
            );
            inner.config_file = config_file_for(&inner.compositor);
        }
    }

    pub fn stop(&self) {
        let old = {
            let mut slot = self.stop.lock().unwrap_or_else(|e| e.into_inner());
            let old = slot.clone();
            old.store(true, Ordering::Relaxed);
            *slot = Arc::new(AtomicBool::new(true));
            old
        };
        let _ = old;
        if let Ok(mut child) = self.child.lock() {
            if let Some(mut c) = child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        let (created, layout_output, layout_restore, compositor) = self
            .inner
            .lock()
            .map(|i| {
                (
                    i.created_output.clone(),
                    i.layout_output.clone(),
                    i.layout_restore.clone(),
                    i.compositor.clone(),
                )
            })
            .unwrap_or((None, None, None, String::new()));
        // A virtual screen that stays enabled after the stream ends leaves
        // windows on a display nobody can see, so turn it off explicitly.
        if let Some(name) = layout_output {
            disable_output(&compositor, &name);
        }
        // And remove an output this daemon created: nothing should be left in
        // `niri msg outputs` or `hyprctl monitors` after the stream stops.
        if let Some(name) = created {
            match compositor.as_str() {
                "niri" => {
                    let _ = sdw_core::util::run("niri", &["msg", "remove-virtual-output", &name]);
                }
                "hyprland" => {
                    if sdw_core::util::has("hyprctl") {
                        let _ = sdw_core::util::run("hyprctl", &["output", "remove", &name]);
                    }
                }
                _ => {}
            }
        }
        // And give the compositor its original config back: the layout block
        // was written for the stream, not for the next login.
        if let Some((path, text)) = layout_restore {
            if let Err(e) = write_config(&path, &text) {
                self.logger
                    .warn(format!("не удалось вернуть конфиг {}: {e}", path.display()));
            }
        }
        std::thread::sleep(Duration::from_millis(250));
        if let Ok(mut inner) = self.inner.lock() {
            inner.running = false;
            inner.created_output = None;
            inner.layout_output = None;
            inner.layout_restore = None;
            inner.rfb = false;
            inner.note.clear();
        }
    }

    pub fn start(&self, req: &MonitorStart, cfg: &DaemonConfig) -> Result<MonitorStatus, String> {
        self.stop();
        self.refresh();
        let (compositor, outputs) = {
            let inner = self.inner.lock().map_err(|_| "monitor lock")?;
            (inner.compositor.clone(), inner.outputs.clone())
        };
        let _ = cfg;
        let mut mode = req.mode;
        let mut backend = req.backend;
        if backend == MonitorBackend::Auto {
            backend = default_backend(&compositor, mode);
        }
        let width = req.width.clamp(320, 3840);
        let height = req.height.clamp(240, 2400);
        let fps = req.fps.clamp(1, 60);
        let quality = req.quality.clamp(20, 95);
        let position = req.position;
        let mut output = if req.output.trim().is_empty() {
            pick_output(&outputs, mode)
        } else {
            req.output.trim().to_string()
        };
        // A real, already enabled monitor is part of the desktop. Extending
        // onto it would move the user's own screens around instead of adding
        // one, which is exactly what a stale saved output caused.
        if mode == MonitorMode::Extend
            && outputs
                .iter()
                .any(|o| o.name == output && o.enabled && !o.virtual_output)
        {
            output = pick_output(&outputs, mode);
        }
        let mut rect = output_rect(&outputs, &output);
        let mut created = None;
        let mut rfb = false;

        let mut note = String::new();
        // A virtual monitor that the driver already prepared is not laid out
        // by the compositor path below: Windows has no config file to patch,
        // and `prepare_virtual` has already applied the exact rectangle.
        let mut vdd_prepared = false;
        let setup: Result<(), String> = (|| {
            match backend {
                MonitorBackend::Hyprland => {
                    if mode == MonitorMode::Extend {
                        let before: Vec<String> = outputs.iter().map(|o| o.name.clone()).collect();
                        if !sdw_core::util::has("hyprctl") {
                            return Err(
                                "hyprctl не найден: Hyprland не запущен или пакет не установлен"
                                    .into(),
                            );
                        }
                        let _ = sdw_core::util::run("hyprctl", &["output", "create", "headless"]);
                        std::thread::sleep(Duration::from_millis(800));
                        self.refresh();
                        let now = self.outputs();
                        let new = now.iter().find(|o| !before.contains(&o.name)).or_else(|| {
                            now.iter()
                                .find(|o| o.name.to_ascii_lowercase().contains("headless"))
                        });
                        let Some(new) = new else {
                            return Err("Hyprland не создал headless-выход. Проверьте `hyprctl output create headless`".into());
                        };
                        output = new.name.clone();
                        std::thread::sleep(Duration::from_millis(700));
                        created = Some(output.clone());
                        self.refresh();
                        rect = output_rect(&self.outputs(), &output);
                    }
                }
                MonitorBackend::NiriVirtual => {
                    if compositor != "niri" {
                        return Err(
                            "Виртуальный выход niri доступен только в сессии niri или biri".into(),
                        );
                    }
                    if !niri_virtual_available() {
                        return Err("Запущенный niri не умеет создавать виртуальные выходы. \
                             Установите biri (`paru -S biri-altinstall-git`) и войдите в сессию \
                             biri: он рисует выход тем же GPU и не зависит от VKMS."
                            .into());
                    }
                    // A previous run may have been killed before it removed its
                    // output; a stale same-name output would make creation fail.
                    let _ = sdw_core::util::run(
                        "niri",
                        &["msg", "remove-virtual-output", NIRI_VIRTUAL_NAME],
                    );
                    let (ws, hs, rs) = (
                        width.to_string(),
                        height.to_string(),
                        fps.max(30).to_string(),
                    );
                    sdw_core::util::run(
                        "niri",
                        &[
                            "msg",
                            "create-virtual-output",
                            "--name",
                            NIRI_VIRTUAL_NAME,
                            "--width",
                            &ws,
                            "--height",
                            &hs,
                            "--refresh-rate",
                            &rs,
                        ],
                    )
                    .map_err(|e| format!("niri не создал виртуальный выход: {e}"))?;
                    output = NIRI_VIRTUAL_NAME.to_string();
                    created = Some(output.clone());
                    std::thread::sleep(Duration::from_millis(500));
                    self.refresh();
                    rect = output_rect(&self.outputs(), &output);
                }
                MonitorBackend::Vkms => {
                    if !cfg!(target_os = "linux") {
                        return Err("VKMS доступен только на Linux".into());
                    }
                    let before: Vec<String> = outputs.iter().map(|o| o.name.clone()).collect();
                    run_vkms(&req.sudo_password)?;
                    // A VM can take a few seconds to enumerate a fresh DRM
                    // connector; poll instead of trusting one short sleep.
                    let mut found = None;
                    for _ in 0..24 {
                        std::thread::sleep(Duration::from_millis(300));
                        self.refresh();
                        let now = self.outputs();
                        found = now
                            .iter()
                            .find(|o| !before.contains(&o.name) && !o.name.starts_with("eDP"))
                            .or_else(|| now.iter().find(|o| o.virtual_output))
                            .or_else(|| now.iter().find(|o| !before.contains(&o.name)))
                            .map(|o| o.name.clone());
                        if found.is_some() {
                            break;
                        }
                    }
                    let Some(name) = found else {
                        let hint = self
                            .inner
                            .lock()
                            .map(|i| i.config_hint.clone())
                            .unwrap_or_default();
                        return Err(format!(
                            "VKMS загружен, но новый выход не появился. Проверьте, что модуль \
                             виден системе (`ls /sys/class/drm`), и подсказку по конфигу:\n{hint}"
                        ));
                    };
                    output = name;
                    std::thread::sleep(Duration::from_millis(700));
                    self.refresh();
                    rect = output_rect(&self.outputs(), &output);
                }
                MonitorBackend::Krfb => {
                    if mode == MonitorMode::Extend {
                        if !sdw_core::util::has("krfb-virtualmonitor") {
                            return Err(
                                "krfb-virtualmonitor не найден. Установите krfb (KDE) и повторите."
                                    .into(),
                            );
                        }
                        let child = Command::new("krfb-virtualmonitor")
                            .args([
                                "--resolution",
                                &format!("{width}x{height}"),
                                "--name",
                                "SD Widgets",
                            ])
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn()
                            .map_err(|e| format!("krfb-virtualmonitor: {e}"))?;
                        if let Ok(mut slot) = self.child.lock() {
                            *slot = Some(child);
                        }
                        rfb = true;
                        output = "krfb".into();
                        rect = (0, 0, width, height);
                    }
                }
                MonitorBackend::Vdd => {
                    #[cfg(windows)]
                    {
                        let (x, y) = layout_xy(&outputs, "", width, height, position);
                        let refresh = fps.max(30);
                        let (name, _aw, _ah, how) = crate::win::display::prepare_virtual(
                            width,
                            height,
                            refresh,
                            x,
                            y,
                            &self.logger,
                        )?;
                        output = name;
                        if !how.is_empty() {
                            note = how;
                        }
                        vdd_prepared = true;
                        if let Ok(mut inner) = self.inner.lock() {
                            inner.layout_output = Some(output.clone());
                        }
                        std::thread::sleep(Duration::from_millis(600));
                        self.refresh();
                        rect = output_rect(&self.outputs(), &output);
                    }
                    #[cfg(not(windows))]
                    {
                        return Err("Virtual Display Driver доступен только на Windows".into());
                    }
                }
                _ => {}
            }

            // Put the new output where the user asked: right, left, above or below
            // the primary display. This is applied live *and* written into the
            // compositor config, so the layout survives a restart.
            if !vdd_prepared
                && !rfb
                && mode == MonitorMode::Extend
                && !output.is_empty()
                && output != "весь экран"
            {
                let ephemeral = created.is_some()
                    || self
                        .outputs()
                        .iter()
                        .any(|o| o.name == output && o.virtual_output);
                match apply_output_layout(
                    &compositor,
                    &output,
                    width,
                    height,
                    fps,
                    position,
                    &self.outputs(),
                    &self.logger,
                ) {
                    Ok(restore) => {
                        if let Ok(mut inner) = self.inner.lock() {
                            if ephemeral {
                                inner.layout_output = Some(output.clone());
                            }
                            inner.layout_restore = restore;
                        }
                    }
                    Err(e) => {
                        self.logger
                            .warn(format!("не удалось применить раскладку монитора: {e}"));
                    }
                }
                std::thread::sleep(Duration::from_millis(550));
                self.refresh();
                let now = self.outputs();
                let active = now.iter().any(|o| o.name == output && o.enabled);
                if !active {
                    if compositor == "niri"
                        && backend != MonitorBackend::NiriVirtual
                        && vkms_loaded()
                    {
                        // niri asks the render GPU to draw into the extra
                        // scanner's buffers; with the NVIDIA driver it cannot
                        // produce a linear buffer, so the compositor logs
                        // "No supported renderer buffer format found" and
                        // leaves the connector off. Say so instead of only
                        // reporting a dead output.
                        return Err(format!(
                            "niri не включает {output}: он не умеет выводить изображение на DRM-устройство второй видеокарты (VKMS) с драйвером NVIDIA"
                        ));
                    }
                    return Err(format!("выход {output} остался выключенным"));
                }
                rect = output_rect(&now, &output);
            }
            Ok(())
        })();

        // A virtual output is a bonus, not a requirement: if the compositor
        // refuses it (niri + VKMS is a common case), stream a mirror of the main
        // screen instead of failing with a dead end.
        if let Err(e) = setup {
            if mode == MonitorMode::Mirror {
                return Err(e);
            }
            note = format!("{e}; включено зеркало основного экрана");
            self.logger.warn(format!(
                "Экран: {e} — переключаюсь на зеркало основного экрана"
            ));
            if backend == MonitorBackend::Hyprland {
                if let Some(name) = created.take() {
                    let _ = sdw_core::util::run("hyprctl", &["output", "remove", &name]);
                }
            }
            if compositor == "niri" {
                if let Some(name) = created.take() {
                    let _ = sdw_core::util::run("niri", &["msg", "remove-virtual-output", &name]);
                } else {
                    let _ = sdw_core::util::run("niri", &["msg", "output", &output, "off"]);
                }
            }
            mode = MonitorMode::Mirror;
            backend = MonitorBackend::Mirror;
            rfb = false;
            self.refresh();
            let outputs = self.outputs();
            output = pick_output(&outputs, MonitorMode::Mirror);
            rect = output_rect(&outputs, &output);
        }

        // Choose the actual capture mechanism.
        let capture = if rfb {
            Capture::Rfb
        } else if backend == MonitorBackend::X11 || compositor == "x11" {
            if !sdw_core::util::has("ffmpeg") {
                return Err("Для X11 нужен ffmpeg. Установите его и повторите.".into());
            }
            Capture::FfmpegX11
        } else if backend == MonitorBackend::Gdigrab
            || backend == MonitorBackend::Vdd
            || compositor == "windows"
        {
            if !sdw_core::util::has("ffmpeg") {
                return Err("Для Windows-трансляции нужен ffmpeg в PATH.".into());
            }
            Capture::FfmpegWindows
        } else {
            // Wayland screenshot path: grim, spectacle or gnome-screenshot.
            if sdw_core::util::has("grim") && !output.is_empty() {
                Capture::Grim
            } else if sdw_core::util::has("spectacle") {
                Capture::Spectacle
            } else if sdw_core::util::has("gnome-screenshot") {
                Capture::Gnome
            } else if sdw_core::util::has("ffmpeg") && cfg!(target_os = "linux") {
                Capture::FfmpegX11
            } else {
                return Err(
                    "Не найден инструмент захвата: установите grim (Wayland) или ffmpeg.".into(),
                );
            }
        };

        if output.is_empty() {
            output = "весь экран".into();
        }
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut slot = self.stop.lock().unwrap_or_else(|e| e.into_inner());
            *slot = stop.clone();
        }
        self.frames.store(0, Ordering::Relaxed);
        if let Ok(mut f) = self.frame.lock() {
            *f = SharedFrame::default();
        }
        {
            let mut inner = self.inner.lock().map_err(|_| "monitor lock")?;
            inner.running = true;
            inner.mode = mode;
            inner.backend = backend;
            inner.position = position;
            inner.output = output.clone();
            inner.width = width;
            inner.height = height;
            inner.fps = fps;
            inner.quality = quality;
            inner.last_error.clear();
            inner.note = note.clone();
            inner.started_at = now_ms();
            inner.output_rect = rect;
            inner.created_output = created.clone();
            inner.rfb = rfb;
            let outputs_now = inner.outputs.clone();
            inner.config_hint = config_hint(
                &compositor,
                &output,
                width,
                height,
                fps,
                position,
                &outputs_now,
            );
            inner.config_file = config_file_for(&compositor);
        }
        self.logger.info(format!(
            "Экран: {} · {} · {}x{}@{} · {}",
            mode.label(),
            backend.label(),
            width,
            height,
            fps,
            output
        ));

        let inner = self.inner.clone();
        let frame = self.frame.clone();
        let frames = self.frames.clone();
        let logger = self.logger.clone();
        let child_slot = self.child.clone();
        let stop2 = stop.clone();
        let quality2 = quality;
        std::thread::spawn(move || {
            // A fullscreen app or a mode switch can take the output away for
            // a moment. The loop keeps the last frame and tries again instead
            // of stopping the monitor and blanking the deck.
            while !stop2.load(Ordering::Relaxed) {
                let result = match capture {
                    Capture::Grim => capture_grim(
                        &output,
                        width,
                        height,
                        fps,
                        quality2,
                        stop2.clone(),
                        frame.clone(),
                        frames.clone(),
                    ),
                    Capture::Spectacle => capture_screenshot(
                        "spectacle",
                        width,
                        height,
                        fps,
                        quality2,
                        stop2.clone(),
                        frame.clone(),
                        frames.clone(),
                    ),
                    Capture::Gnome => capture_screenshot(
                        "gnome-screenshot",
                        width,
                        height,
                        fps,
                        quality2,
                        stop2.clone(),
                        frame.clone(),
                        frames.clone(),
                    ),
                    Capture::FfmpegX11 => capture_ffmpeg(
                        FfmpegKind::X11,
                        rect,
                        width,
                        height,
                        fps,
                        quality2,
                        stop2.clone(),
                        frame.clone(),
                        frames.clone(),
                        child_slot.clone(),
                    ),
                    Capture::FfmpegWindows => capture_ffmpeg(
                        FfmpegKind::Windows,
                        rect,
                        width,
                        height,
                        fps,
                        quality2,
                        stop2.clone(),
                        frame.clone(),
                        frames.clone(),
                        child_slot.clone(),
                    ),
                    Capture::Rfb => rfb_capture(
                        "127.0.0.1:5900",
                        width,
                        height,
                        fps,
                        quality2,
                        stop2.clone(),
                        frame.clone(),
                        frames.clone(),
                    ),
                };
                match result {
                    Ok(()) => break,
                    Err(e) => {
                        if stop2.load(Ordering::Relaxed) {
                            break;
                        }
                        logger.warn(format!("Экран: {e}"));
                        if let Ok(mut i) = inner.lock() {
                            i.last_error = e;
                        }
                        std::thread::sleep(Duration::from_millis(900));
                    }
                }
            }
        });

        Ok(self.status())
    }

    fn outputs(&self) -> Vec<DisplayOutput> {
        self.inner
            .lock()
            .map(|i| i.outputs.clone())
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy)]
enum Capture {
    Grim,
    Spectacle,
    Gnome,
    FfmpegX11,
    FfmpegWindows,
    Rfb,
}

fn default_backend(compositor: &str, mode: MonitorMode) -> MonitorBackend {
    if mode == MonitorMode::Mirror {
        return MonitorBackend::Mirror;
    }
    match compositor {
        "hyprland" => MonitorBackend::Hyprland,
        // biri (or a patched niri) draws the extra output on the same GPU, so
        // prefer it over a VKMS device the compositor often refuses to scan
        // out, especially with the NVIDIA driver.
        "niri" => {
            if niri_virtual_available() {
                MonitorBackend::NiriVirtual
            } else {
                MonitorBackend::Vkms
            }
        }
        "sway" | "wayland" => MonitorBackend::Vkms,
        "kde" => MonitorBackend::Krfb,
        // GNOME and an unidentified desktop have no output API the daemon can
        // drive, but VKMS still adds a real connector the compositor picks up.
        "gnome" if cfg!(target_os = "linux") && vkms_available() => MonitorBackend::Vkms,
        "x11" => MonitorBackend::X11,
        // A real virtual monitor needs the Virtual Display Driver; without it
        // the request falls back to mirroring the main screen.
        "windows" => {
            #[cfg(windows)]
            {
                if crate::win::display::vdd_ready() {
                    MonitorBackend::Vdd
                } else {
                    MonitorBackend::Gdigrab
                }
            }
            #[cfg(not(windows))]
            {
                MonitorBackend::Gdigrab
            }
        }
        _ if cfg!(target_os = "linux") && vkms_available() => MonitorBackend::Vkms,
        _ => MonitorBackend::Mirror,
    }
}

fn pick_output(outputs: &[DisplayOutput], mode: MonitorMode) -> String {
    if outputs.is_empty() {
        return String::new();
    }
    if mode == MonitorMode::Extend {
        if let Some(v) = outputs.iter().find(|o| o.virtual_output && o.enabled) {
            return v.name.clone();
        }
    }
    outputs
        .iter()
        .find(|o| o.enabled && o.primary && !o.virtual_output)
        .or_else(|| outputs.iter().find(|o| o.enabled && o.primary))
        .or_else(|| outputs.iter().find(|o| o.enabled))
        .or_else(|| outputs.iter().find(|o| o.primary))
        .or_else(|| outputs.first())
        .map(|o| o.name.clone())
        .unwrap_or_default()
}

fn output_rect(outputs: &[DisplayOutput], name: &str) -> (i32, i32, u32, u32) {
    if let Some(o) = outputs.iter().find(|o| o.name == name) {
        (o.x, o.y, o.width.max(1), o.height.max(1))
    } else if let Some(o) = outputs.first() {
        (o.x, o.y, o.width.max(1), o.height.max(1))
    } else {
        (0, 0, 1280, 800)
    }
}

/// Top-left corner for a new output of `w`×`h`, placed relative to the
/// desktop's real edges. Anchoring only on the primary display put a "right"
/// monitor on top of a second real screen; using the outermost enabled edge
/// keeps every monitor side by side.
fn layout_xy(
    outputs: &[DisplayOutput],
    output: &str,
    w: u32,
    h: u32,
    position: MonitorPosition,
) -> (i32, i32) {
    let others: Vec<&DisplayOutput> = outputs
        .iter()
        .filter(|o| o.name != output && o.enabled && !o.virtual_output)
        .collect();
    let others = if others.is_empty() {
        outputs.iter().filter(|o| o.name != output).collect()
    } else {
        others
    };
    let primary = others.iter().find(|o| o.primary).or_else(|| others.first());
    let (px, py) = primary.map(|o| (o.x, o.y)).unwrap_or((0, 0));
    if others.is_empty() {
        return match position {
            MonitorPosition::Auto | MonitorPosition::Right => (w as i32, 0),
            MonitorPosition::Left => (-(w as i32), 0),
            MonitorPosition::Above => (0, -(h as i32)),
            MonitorPosition::Below => (0, h as i32),
        };
    }
    let left = others.iter().map(|o| o.x).min().unwrap_or(px);
    let top = others.iter().map(|o| o.y).min().unwrap_or(py);
    let right = others
        .iter()
        .map(|o| o.x + o.width.max(1) as i32)
        .max()
        .unwrap_or(px);
    let bottom = others
        .iter()
        .map(|o| o.y + o.height.max(1) as i32)
        .max()
        .unwrap_or(py);
    match position {
        MonitorPosition::Auto | MonitorPosition::Right => (right, py),
        MonitorPosition::Left => (left - w as i32, py),
        MonitorPosition::Above => (px, top - h as i32),
        MonitorPosition::Below => (px, bottom),
    }
}

/// Exact niri mode string for `width`×`height`, using the output's own mode list.
/// niri rejects a mode that the output does not advertise, so the capture frame
/// rate (often 15) must never leak into this string.
fn niri_mode_string(output: &str, width: u32, height: u32) -> Option<String> {
    let text = sdw_core::util::run("niri", &["msg", "--json", "outputs"]).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let modes = v.get(output)?.get("modes")?.as_array()?;
    let mut best: Option<(i64, i64, u32, u32, u64)> = None;
    for m in modes {
        let w = m["width"].as_u64().unwrap_or(0) as u32;
        let h = m["height"].as_u64().unwrap_or(0) as u32;
        if w == 0 || h == 0 {
            continue;
        }
        let r = m["refresh_rate"].as_u64().unwrap_or(60000);
        let area = (w as i64 - width as i64).abs() * height.max(1) as i64
            + (h as i64 - height as i64).abs() * width.max(1) as i64;
        let score = if w == width && h == height {
            0
        } else {
            1_000_000 + area
        };
        let dist = (r as i64 - 60_000).abs();
        if best
            .map(|(bs, bd, _, _, _)| (score, dist) < (bs, bd))
            .unwrap_or(true)
        {
            best = Some((score, dist, w, h, r));
        }
    }
    let (_, _, w, h, r) = best?;
    Some(format!("{w}x{h}@{:.3}", r as f64 / 1000.0))
}

fn niri_output_active(output: &str) -> bool {
    let text = sdw_core::util::run("niri", &["msg", "--json", "outputs"]).unwrap_or_default();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    v.get(output)
        .map(|o| o["logical"].is_object() && o["logical"]["width"].as_u64().unwrap_or(0) > 0)
        .unwrap_or(false)
}

/// Put the freshly created output in place and remember the layout.
///
/// The live change makes the monitor usable immediately; patching the compositor
/// config makes the same layout survive a reboot. Every step is best-effort: a
/// compositor that cannot do one of them still gets the others.
fn apply_output_layout(
    compositor: &str,
    output: &str,
    width: u32,
    height: u32,
    fps: u32,
    position: MonitorPosition,
    outputs: &[DisplayOutput],
    logger: &Logger,
) -> Result<Option<(std::path::PathBuf, String)>, String> {
    let (x, y) = layout_xy(outputs, output, width, height, position);
    logger.info(format!(
        "Экран: раскладка {output} → {x},{y} ({})",
        position.label()
    ));
    let mode = format!("{width}x{height}@{}", fps.max(30));

    match compositor {
        "niri" => {
            let ours = output.starts_with("sdw-");
            let mode = if ours {
                // A virtual output accepts any mode; never borrow a physical
                // output's mode list here.
                format!("{width}x{height}@{}", fps.max(30))
            } else {
                niri_mode_string(output, width, height)
                    .unwrap_or_else(|| format!("{width}x{height}@{}", fps.max(30)))
            };
            let xs = x.to_string();
            let ys = y.to_string();
            for attempt in 0..2 {
                let _ = sdw_core::util::run("niri", &["msg", "output", output, "on"]);
                if ours {
                    let _ = sdw_core::util::run(
                        "niri",
                        &["msg", "output", output, "custom-mode", &mode],
                    );
                } else {
                    let _ = sdw_core::util::run("niri", &["msg", "output", output, "mode", &mode]);
                }
                let _ = sdw_core::util::run("niri", &["msg", "output", output, "scale", "1.0"]);
                let _ = sdw_core::util::run(
                    "niri",
                    &["msg", "output", output, "position", "set", &xs, &ys],
                );
                if niri_output_active(output) {
                    break;
                }
                if attempt == 0 {
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
            if !niri_output_active(output) {
                return Err(format!(
                    "niri не смог включить выход {output} в режиме {mode}"
                ));
            }
            let path = crate::config::niri_config_path();
            let previous = std::fs::read_to_string(&path).ok();
            patch_niri_config(&path, output, &mode, x, y)?;
            if let Some(previous) = previous {
                return Ok(Some((path, previous)));
            }
        }
        "hyprland" => {
            let spec = format!("{output},{width}x{height}@{fps},{x}x{y},1");
            if sdw_core::util::has("hyprctl") {
                let _ = sdw_core::util::run("hyprctl", &["keyword", "monitor", &spec]);
            }
            let path = crate::config::hypr_config_path();
            patch_hypr_config(&path, output, &spec)?;
        }
        "sway" | "wayland" => {
            if sdw_core::util::has("swaymsg") {
                let _ = sdw_core::util::run(
                    "swaymsg",
                    &[
                        "output",
                        output,
                        "mode",
                        &mode,
                        "position",
                        &x.to_string(),
                        &y.to_string(),
                        "scale",
                        "1",
                    ],
                );
            }
            let path = dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("sway/config");
            let previous = std::fs::read_to_string(&path).ok();
            patch_sway_config(&path, output, width, height, fps, x, y)?;
            if let Some(previous) = previous {
                return Ok(Some((path, previous)));
            }
        }
        "kde" => {
            if sdw_core::util::has("kscreen-doctor") {
                let _ = sdw_core::util::run(
                    "kscreen-doctor",
                    &[
                        &format!("output.{output}.enable"),
                        &format!("output.{output}.mode.{width}x{height}@{fps}"),
                        &format!("output.{output}.position.{x},{y}"),
                        &format!("output.{output}.scale.1"),
                    ],
                );
            }
        }
        "x11" => {
            let _ = sdw_core::util::run(
                "xrandr",
                &[
                    "--output",
                    output,
                    "--mode",
                    &format!("{width}x{height}"),
                    "--pos",
                    &format!("{x},{y}"),
                    "--scale",
                    "1x1",
                ],
            );
        }
        _ => {}
    }
    Ok(None)
}

/// Turn a streaming-only output off in the running compositor.
fn disable_output(compositor: &str, name: &str) {
    match compositor {
        "niri" => {
            let _ = sdw_core::util::run("niri", &["msg", "output", name, "off"]);
        }
        "hyprland" => {
            if sdw_core::util::has("hyprctl") {
                let _ = sdw_core::util::run(
                    "hyprctl",
                    &["keyword", "monitor", &format!("{name},disable")],
                );
            }
        }
        "sway" | "wayland" => {
            if sdw_core::util::has("swaymsg") {
                let _ = sdw_core::util::run("swaymsg", &["output", name, "disable"]);
            }
        }
        "kde" => {
            if sdw_core::util::has("kscreen-doctor") {
                let _ = sdw_core::util::run("kscreen-doctor", &[&format!("output.{name}.disable")]);
            }
        }
        "x11" => {
            let _ = sdw_core::util::run("xrandr", &["--output", name, "--off"]);
        }
        "windows" => {
            #[cfg(windows)]
            crate::win::display::disable_virtual(name);
        }
        _ => {}
    }
}

/// Replace the `output "name" { … }` block in niri's config, or append one.
fn patch_niri_config(
    path: &std::path::Path,
    output: &str,
    mode: &str,
    x: i32,
    y: i32,
) -> Result<(), String> {
    let block = format!(
        "output \"{output}\" {{\n    mode \"{mode}\"\n    scale 1.0\n    position x={x} y={y}\n}}"
    );
    let mut text = std::fs::read_to_string(path).unwrap_or_default();
    match find_kdl_block(&text, output) {
        Some((start, end)) => {
            text.replace_range(start..end, &block);
        }
        None => {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&block);
            text.push('\n');
        }
    }
    write_config(path, &text)
}

/// Locate a KDL `output "name" { … }` block, matching braces and ignoring strings/comments.
fn find_kdl_block(text: &str, output: &str) -> Option<(usize, usize)> {
    let needle = format!("output \"{output}\"");
    let start = text.find(&needle)?;
    let open = start + text[start..].find('{')?;
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    let mut in_string = false;
    let mut in_comment = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_comment {
            if c == b'\n' {
                in_comment = false;
            }
        } else if in_string {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                in_string = false;
            }
        } else {
            match c {
                b'"' => in_string = true,
                b'/' if bytes.get(i + 1) == Some(&b'/') => {
                    in_comment = true;
                    i += 1;
                }
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some((start, i + 1));
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Replace every `monitor=<name>,…` line in Hyprland's config, or append one.
fn patch_hypr_config(path: &std::path::Path, output: &str, spec: &str) -> Result<(), String> {
    let mut text = std::fs::read_to_string(path).unwrap_or_default();
    let line = format!("monitor={spec}");
    let mut found = false;
    let mut out = String::with_capacity(text.len() + line.len() + 2);
    for old in text.lines() {
        let trimmed = old.trim_start();
        let is_ours = trimmed
            .strip_prefix("monitor=")
            .and_then(|rest| rest.split(',').next())
            .map(|name| name.trim() == output)
            .unwrap_or(false);
        if is_ours {
            out.push_str(&line);
            out.push('\n');
            found = true;
        } else {
            out.push_str(old);
            out.push('\n');
        }
    }
    if !found {
        out.push_str("# sd-widgets: extra monitor\n");
        out.push_str(&line);
        out.push('\n');
    }
    text.clear();
    text.push_str(&out);
    write_config(path, &text)
}

/// Replace every `output <name> …` line in sway's config, or append one.
fn patch_sway_config(
    path: &std::path::Path,
    output: &str,
    width: u32,
    height: u32,
    fps: u32,
    x: i32,
    y: i32,
) -> Result<(), String> {
    let mut text = std::fs::read_to_string(path).unwrap_or_default();
    let line = format!(
        "output {output} mode {width}x{height}@{} position {x} {y} scale 1",
        fps.max(30)
    );
    let mut found = false;
    let mut out = String::with_capacity(text.len() + line.len() + 2);
    for old in text.lines() {
        let trimmed = old.trim_start();
        let is_ours = trimmed
            .strip_prefix("output ")
            .map(|rest| {
                let rest = rest.trim_start();
                rest.starts_with(output)
                    && rest[output.len()..]
                        .chars()
                        .next()
                        .map(|c| c.is_whitespace())
                        .unwrap_or(true)
            })
            .unwrap_or(false);
        if is_ours {
            if !found {
                out.push_str(&line);
                out.push('\n');
                found = true;
            }
        } else {
            out.push_str(old);
            out.push('\n');
        }
    }
    if !found {
        out.push_str("# sd-widgets: extra monitor\n");
        out.push_str(&line);
        out.push('\n');
    }
    text.clear();
    text.push_str(&out);
    write_config(path, &text)
}

fn write_config(path: &std::path::Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        sdw_core::util::ensure_dir(parent).map_err(|e| e.to_string())?;
    }
    sdw_core::util::atomic_write(path, text.as_bytes()).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- detection

pub fn detect_compositor() -> String {
    if cfg!(windows) {
        return "windows".into();
    }
    if std::env::var_os("NIRI_SOCKET").is_some() || sdw_core::util::has("niri") && niri_works() {
        return "niri".into();
    }
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        || (sdw_core::util::has("hyprctl") && sdw_core::util::run("hyprctl", &["version"]).is_ok())
    {
        return "hyprland".into();
    }
    if std::env::var_os("SWAYSOCK").is_some() {
        return "sway".into();
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if desktop.contains("kde") || std::env::var_os("KDE_FULL_SESSION").is_some() {
        return "kde".into();
    }
    if desktop.contains("gnome") {
        return "gnome".into();
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return "wayland".into();
    }
    if std::env::var_os("DISPLAY").is_some() {
        return "x11".into();
    }
    "unknown".into()
}

fn niri_works() -> bool {
    sdw_core::util::run("niri", &["msg", "version"]).is_ok()
}

/// The running niri exposes `create-virtual-output`. Stock niri does not; biri
/// (a fork carrying the virtual-output work) and patched builds do.
fn niri_virtual_available() -> bool {
    if !cfg!(target_os = "linux") || !sdw_core::util::has("niri") {
        return false;
    }
    sdw_core::util::run("niri", &["msg", "--help"])
        .map(|help| help.contains("create-virtual-output"))
        .unwrap_or(false)
}

fn detect_all() -> (String, Vec<DisplayOutput>) {
    let compositor = detect_compositor();
    let mut outputs = match compositor.as_str() {
        "niri" => niri_outputs().unwrap_or_default(),
        "hyprland" => hypr_outputs().unwrap_or_default(),
        "kde" => kde_outputs().unwrap_or_default(),
        "sway" => sway_outputs().unwrap_or_default(),
        "x11" => xrandr_outputs().unwrap_or_default(),
        "windows" => windows_outputs().unwrap_or_default(),
        _ => Vec::new(),
    };
    if outputs.is_empty() {
        if let Some(o) = niri_outputs().ok().filter(|v| !v.is_empty()) {
            outputs = o;
        } else if let Some(o) = hypr_outputs().ok().filter(|v| !v.is_empty()) {
            outputs = o;
        } else if let Some(o) = xrandr_outputs().ok().filter(|v| !v.is_empty()) {
            outputs = o;
        }
    }
    if outputs.is_empty() {
        outputs.push(DisplayOutput {
            name: "screen".into(),
            description: "Основной экран".into(),
            width: 1280,
            height: 800,
            refresh: 60.0,
            x: 0,
            y: 0,
            scale: 1.0,
            primary: true,
            virtual_output: false,
            enabled: true,
        });
    }
    // Mark the first output at the origin as primary when the compositor is silent.
    if !outputs.iter().any(|o| o.primary) {
        if let Some(o) = outputs.iter_mut().find(|o| o.x == 0 && o.y == 0) {
            o.primary = true;
        } else if let Some(o) = outputs.first_mut() {
            o.primary = true;
        }
    }
    (compositor, outputs)
}

fn niri_outputs() -> Result<Vec<DisplayOutput>, String> {
    let text = sdw_core::util::run("niri", &["msg", "--json", "outputs"])?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let obj = v.as_object().ok_or("niri: неожиданный JSON")?;
    let mut out = Vec::new();
    for (name, o) in obj {
        let logical = &o["logical"];
        let enabled = logical.is_object() && logical["width"].as_u64().unwrap_or(0) > 0;
        let modes = o["modes"].as_array().cloned().unwrap_or_default();
        let current = o["current_mode"].as_u64().unwrap_or(0) as usize;
        let mode = modes.get(current).or_else(|| modes.first());
        let (w, h, refresh) = mode
            .map(|m| {
                (
                    m["width"].as_u64().unwrap_or(0) as u32,
                    m["height"].as_u64().unwrap_or(0) as u32,
                    m["refresh_rate"].as_u64().unwrap_or(60000) as f32 / 1000.0,
                )
            })
            .unwrap_or((0, 0, 60.0));
        let logical_w = logical["width"].as_u64().unwrap_or(w as u64) as u32;
        let logical_h = logical["height"].as_u64().unwrap_or(h as u64) as u32;
        let make = o["make"].as_str().unwrap_or("");
        let model = o["model"].as_str().unwrap_or("");
        out.push(DisplayOutput {
            name: name.clone(),
            description: format!("{make} {model}").trim().to_string(),
            width: if enabled && logical_w > 0 {
                logical_w
            } else {
                w
            },
            height: if enabled && logical_h > 0 {
                logical_h
            } else {
                h
            },
            refresh,
            x: logical["x"].as_i64().unwrap_or(0) as i32,
            y: logical["y"].as_i64().unwrap_or(0) as i32,
            scale: logical["scale"].as_f64().unwrap_or(1.0) as f32,
            primary: enabled
                && logical["x"].as_i64().unwrap_or(-1) == 0
                && logical["y"].as_i64().unwrap_or(-1) == 0,
            virtual_output: is_virtual_name(name),
            enabled,
        });
    }
    out.sort_by_key(|o| (!o.enabled, o.x, o.y));
    Ok(out)
}

fn hypr_outputs() -> Result<Vec<DisplayOutput>, String> {
    let text = sdw_core::util::run("hyprctl", &["-j", "monitors"])?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let arr = v.as_array().ok_or("hyprctl: неожиданный JSON")?;
    let mut out = Vec::new();
    for m in arr {
        if m["disabled"].as_bool().unwrap_or(false) {
            continue;
        }
        let name = m["name"].as_str().unwrap_or("").to_string();
        let w = m["width"].as_u64().unwrap_or(0) as u32;
        let h = m["height"].as_u64().unwrap_or(0) as u32;
        out.push(DisplayOutput {
            name: name.clone(),
            description: m["description"].as_str().unwrap_or("").to_string(),
            width: w,
            height: h,
            refresh: m["refreshRate"].as_f64().unwrap_or(60.0) as f32,
            x: m["x"].as_i64().unwrap_or(0) as i32,
            y: m["y"].as_i64().unwrap_or(0) as i32,
            scale: m["scale"].as_f64().unwrap_or(1.0) as f32,
            primary: m["focused"].as_bool().unwrap_or(false) && m["x"].as_i64().unwrap_or(-1) == 0,
            virtual_output: is_virtual_name(&name),
            enabled: true,
        });
    }
    Ok(out)
}

fn kde_outputs() -> Result<Vec<DisplayOutput>, String> {
    let text = sdw_core::util::run("kscreen-doctor", &["-j"])?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let arr = v["outputs"]
        .as_array()
        .or_else(|| v.as_array())
        .ok_or("kscreen-doctor: неожиданный JSON")?;
    let mut out = Vec::new();
    for o in arr {
        let name = o["name"].as_str().unwrap_or("").to_string();
        if name.is_empty() || o["enabled"].as_bool() == Some(false) {
            continue;
        }
        let g = &o["geometry"];
        let (w, h) = (
            g["width"].as_u64().unwrap_or(0) as u32,
            g["height"].as_u64().unwrap_or(0) as u32,
        );
        let mut refresh = 60.0;
        if let Some(modes) = o["modes"].as_array() {
            if let Some(m) = modes
                .iter()
                .find(|m| m["current"].as_bool().unwrap_or(false))
            {
                if let Some(r) = m["refreshRate"].as_f64() {
                    refresh = r as f32;
                }
            }
        }
        out.push(DisplayOutput {
            name: name.clone(),
            description: o["description"].as_str().unwrap_or("").to_string(),
            width: w,
            height: h,
            refresh,
            x: g["x"].as_i64().unwrap_or(0) as i32,
            y: g["y"].as_i64().unwrap_or(0) as i32,
            scale: o["scale"].as_f64().unwrap_or(1.0) as f32,
            primary: o["primary"].as_bool().unwrap_or(false),
            virtual_output: is_virtual_name(&name),
            enabled: true,
        });
    }
    Ok(out)
}

fn sway_outputs() -> Result<Vec<DisplayOutput>, String> {
    let text = sdw_core::util::run("swaymsg", &["-t", "get_outputs", "-r"])?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let arr = v.as_array().ok_or("swaymsg: неожиданный JSON")?;
    let mut out = Vec::new();
    for o in arr {
        if !o["active"].as_bool().unwrap_or(false) {
            continue;
        }
        let name = o["name"].as_str().unwrap_or("").to_string();
        let rect = &o["rect"];
        out.push(DisplayOutput {
            name: name.clone(),
            description: o["make"].as_str().unwrap_or("").to_string(),
            width: rect["width"].as_u64().unwrap_or(0) as u32,
            height: rect["height"].as_u64().unwrap_or(0) as u32,
            refresh: o["current_mode"]["refresh"].as_f64().unwrap_or(60000.0) as f32 / 1000.0,
            x: rect["x"].as_i64().unwrap_or(0) as i32,
            y: rect["y"].as_i64().unwrap_or(0) as i32,
            scale: o["scale"].as_f64().unwrap_or(1.0) as f32,
            primary: rect["x"].as_i64().unwrap_or(-1) == 0 && rect["y"].as_i64().unwrap_or(-1) == 0,
            virtual_output: is_virtual_name(&name),
            enabled: true,
        });
    }
    Ok(out)
}

fn xrandr_outputs() -> Result<Vec<DisplayOutput>, String> {
    let text = sdw_core::util::run("xrandr", &["--query"])?;
    let mut out = Vec::new();
    for line in text.lines() {
        if !line.contains(" connected") {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else { continue };
        let mut primary = false;
        let mut geom = None;
        for p in parts {
            if p == "primary" {
                primary = true;
            } else if let Some((w, h, x, y)) = parse_geometry(p) {
                geom = Some((w, h, x, y));
                break;
            }
        }
        if let Some((w, h, x, y)) = geom {
            out.push(DisplayOutput {
                name: name.to_string(),
                description: String::new(),
                width: w,
                height: h,
                refresh: 60.0,
                x,
                y,
                scale: 1.0,
                primary,
                virtual_output: is_virtual_name(name),
                enabled: true,
            });
        }
    }
    Ok(out)
}

#[cfg(windows)]
fn windows_outputs() -> Result<Vec<DisplayOutput>, String> {
    crate::win::display::outputs()
}

#[cfg(not(windows))]
fn windows_outputs() -> Result<Vec<DisplayOutput>, String> {
    Ok(Vec::new())
}

fn parse_resolution(s: &str) -> Option<(u32, u32)> {
    let mut nums = s
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<u32>().ok());
    let w = nums.next()?;
    let h = nums.next()?;
    (w > 100 && h > 100).then_some((w, h))
}

fn parse_geometry(s: &str) -> Option<(u32, u32, i32, i32)> {
    // 2560x1440+0+0 or 1920x1080+-100+50
    let (size, rest) = s.split_once('+').or_else(|| s.split_once('-'))?;
    let (w, h) = size.split_once('x')?;
    let w: u32 = w.parse().ok()?;
    let h: u32 = h.parse().ok()?;
    let (xs, ys) = if let Some(r) = rest.strip_prefix('-') {
        let (a, b) = r.split_once(['+', '-'])?;
        (format!("-{a}"), b.to_string())
    } else {
        let (a, b) = rest.split_once(['+', '-'])?;
        (a.to_string(), b.to_string())
    };
    Some((w, h, xs.parse().ok()?, ys.parse().ok()?))
}

fn is_virtual_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("virtual")
        || n.contains("headless")
        || n.contains("vkms")
        || n.starts_with("wl-")
        || n.starts_with("sdw-")
}

// ---------------------------------------------------------------- tools

fn tool(name: &str, purpose: &str, hint: &str) -> ToolStatus {
    let path = sdw_core::util::which(name);
    ToolStatus {
        name: name.to_string(),
        installed: path.is_some(),
        path: path.map(|p| p.display().to_string()).unwrap_or_default(),
        purpose: purpose.to_string(),
        install_hint: hint.to_string(),
    }
}

fn detect_tools() -> Vec<ToolStatus> {
    let mut v = vec![
        tool("grim", "Снимки Wayland-экрана", "sudo pacman -S grim"),
        tool(
            "ffmpeg",
            "Поток X11/Windows и сжатие",
            "sudo pacman -S ffmpeg",
        ),
        tool("niri", "Управление выходами niri", "sudo pacman -S niri"),
        tool(
            "biri",
            "niri с виртуальными выходами (доп. монитор без VKMS)",
            "paru -S biri-altinstall-git",
        ),
        tool(
            "hyprctl",
            "Управление выходами Hyprland",
            "sudo pacman -S hyprland",
        ),
        tool("kscreen-doctor", "Экраны KDE", "sudo pacman -S libkscreen"),
        tool(
            "krfb-virtualmonitor",
            "Виртуальный монитор KDE",
            "sudo pacman -S krfb",
        ),
        tool("spectacle", "Снимки KDE", "sudo pacman -S spectacle"),
        tool(
            "gnome-screenshot",
            "Снимки GNOME",
            "sudo pacman -S gnome-screenshot",
        ),
        tool("wlr-randr", "Режимы wlroots", "sudo pacman -S wlr-randr"),
        tool("xrandr", "Экраны X11", "sudo pacman -S xorg-xrandr"),
        tool("xdotool", "Мышь X11", "sudo pacman -S xdotool"),
        tool("ydotool", "Мышь Wayland", "sudo pacman -S ydotool"),
        tool("wtype", "Клавиатура Wayland", "sudo pacman -S wtype"),
        tool(
            "usbip",
            "Подключение USB-устройств VIIPER",
            "sudo pacman -S usbip",
        ),
        tool(
            "viiper",
            "Сервер виртуальных USB-устройств",
            "см. раздел VIIPER",
        ),
    ];
    v.retain(|t| {
        // Keep the full list on Linux; hide irrelevant platform tools elsewhere.
        if cfg!(windows) {
            t.name == "ffmpeg" || t.name == "viiper" || t.name == "usbip"
        } else {
            true
        }
    });
    v
}

// ---------------------------------------------------------------- hints

fn config_hint(
    compositor: &str,
    output: &str,
    w: u32,
    h: u32,
    fps: u32,
    position: MonitorPosition,
    outputs: &[DisplayOutput],
) -> String {
    let out = if output.is_empty() || output == "весь экран" || output == "krfb" {
        "Virtual-1"
    } else {
        output
    };
    let (hx, hy) = layout_xy(outputs, out, w, h, position);
    let niri_mode = if compositor == "niri" {
        niri_mode_string(out, w, h).unwrap_or_else(|| format!("{w}x{h}@{fps}.000"))
    } else {
        format!("{w}x{h}@{}", fps.max(30))
    };
    let fps = fps.max(30);
    match compositor {
        "niri" if niri_virtual_available() => format!(
            "# niri (biri): виртуальный выход\nniri msg create-virtual-output --name {out} --width {w} --height {h} --refresh-rate {fps}\n\n# Убрать выход:\nniri msg remove-virtual-output {out}\n\n# Демон делает это сам при запуске и остановке экрана;\n# вручную достаточно одной команды выше."
        ),
        "niri" => format!(
            "# niri: ~/.config/niri/config.kdl\noutput \"{out}\" {{\n    mode \"{niri_mode}\"\n    scale 1.0\n    position x={hx} y={hy}\n}}\n\n# Один раз загрузить VKMS:\nsudo modprobe vkms num_outputs=1\n\n# Чтобы виртуальный экран появлялся после перезагрузки:\n# /etc/modules-load.d/sd-widgets.conf → vkms\n# /etc/modprobe.d/sd-widgets.conf → options vkms num_outputs=1\n\n# Настоящий доп. монитор без VKMS даёт biri:\n# paru -S biri-altinstall-git, затем войти в сессию biri"
        ),
        "hyprland" => format!(
            "# Hyprland: ~/.config/hypr/hyprland.conf\nmonitor={out},{w}x{h}@{fps},{hx}x{hy},1\n\n# Или одной командой:\nhyprctl output create headless\nhyprctl keyword monitor {out},{w}x{h}@{fps},{hx}x{hy},1"
        ),
        "kde" => format!(
            "# KDE Plasma\n# 1. Установите krfb:\nsudo pacman -S krfb\n\n# 2. Виртуальный монитор {w}x{h} создаётся автоматически при запуске\n#    «Дополнительного монитора» в SD Widgets.\n# 3. В «Параметрах системы → Экран» расположите его справа от основного."
        ),
        "gnome" => format!(
            "# GNOME Wayland\n# Дополнительный монитор создаётся через VKMS:\nsudo modprobe vkms num_outputs=1\n# Затем добавьте выход {w}x{h} в «Настройки → Дисплеи»."
        ),
        "x11" => format!(
            "# X11\nxrandr --newmode \"{w}x{h}_{fps}\" ...\nxrandr --addmode {out} \"{w}x{h}_{fps}\"\nxrandr --output {out} --mode \"{w}x{h}_{fps}\" --right-of HDMI-1"
        ),
        "windows" => format!(
            "# Windows\nПараметры → Система → Дисплей → Несколько дисплеев → Расширить.\n# Для трансляции нужен ffmpeg в PATH."
        ),
        _ => format!(
            "# Универсальный вариант\n# Создайте выход {w}x{h} средствами композитора и выберите его здесь.\n# Для зеркала достаточно grim или spectacle."
        ),
    }
}

fn config_file_for(compositor: &str) -> String {
    match compositor {
        "niri" => crate::config::niri_config_path().display().to_string(),
        "hyprland" => crate::config::hypr_config_path().display().to_string(),
        "kde" => dirs::config_dir()
            .map(|p| p.join("kdeglobals").display().to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------- privileged helper

/// Where this machine runs: "kvm", "vmware", "hyperv", … or empty on metal.
/// A virtual machine is why VKMS is the reliable route for an extra screen.
fn vm_kind() -> String {
    static CACHE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CACHE.get_or_init(vm_kind_probe).clone()
}

fn vm_kind_probe() -> String {
    #[cfg(target_os = "linux")]
    {
        if sdw_core::util::has("systemd-detect-virt") {
            if let Ok(out) = sdw_core::util::run("systemd-detect-virt", &["--vm"]) {
                let out = out.trim();
                if !out.is_empty() && out != "none" {
                    return out.to_string();
                }
            }
        }
        if let Ok(cpu) = std::fs::read_to_string("/proc/cpuinfo") {
            if cpu
                .lines()
                .any(|l| l.starts_with("flags") && l.contains("hypervisor"))
            {
                if let Ok(model) = std::fs::read_to_string("/sys/class/dmi/id/product_name") {
                    let model = model.trim().to_ascii_lowercase();
                    if !model.is_empty() {
                        return format!("vm · {model}");
                    }
                }
                return "vm".into();
            }
        }
    }
    String::new()
}

fn vkms_loaded() -> bool {
    std::path::Path::new("/sys/module/vkms").exists()
}

/// Whether this kernel ships the module at all. On a VM without a DRM output
/// this is the one thing worth checking before blaming the compositor.
fn vkms_available() -> bool {
    if vkms_loaded() {
        return true;
    }
    // The module does not appear or vanish during a run; probing once keeps the
    // 900 ms status poll free of extra processes.
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(vkms_available_probe)
}

fn vkms_available_probe() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    if sdw_core::util::has("modinfo")
        && sdw_core::util::run("modinfo", &["-n", "vkms"])
            .map(|p| !p.trim().is_empty())
            .unwrap_or(false)
    {
        return true;
    }
    sdw_core::util::has("modprobe") && sdw_core::util::run("modprobe", &["-n", "vkms"]).is_ok()
}

/// Load VKMS as directly as the system allows. Passwordless paths go first so a
/// desktop session does not get a password box when it is not needed; `pkexec`
/// is next, and the typed password only last.
fn run_vkms(sudo_password: &str) -> Result<(), String> {
    if vkms_loaded() {
        return Ok(());
    }
    if !vkms_available() {
        let vm = vm_kind();
        return Err(if vm.is_empty() {
            "в ядре нет модуля vkms: установите linux-modules-extra для своего ядра и повторите"
                .into()
        } else {
            format!(
                "в ядре этой машины ({vm}) нет модуля vkms: установите linux-modules-extra \
                 или ядро с vkms и повторите"
            )
        });
    }
    let args = ["vkms", "num_outputs=1"];
    let direct = sdw_core::util::run("modprobe", &args).is_ok() && vkms_loaded();
    if direct {
        return Ok(());
    }
    // A sudo rule or a session that is already root needs no prompting.
    if sdw_core::util::run("sudo", &["-n", "modprobe", "vkms", "num_outputs=1"]).is_ok()
        && vkms_loaded()
    {
        return Ok(());
    }
    let mut last = String::new();
    if sdw_core::util::has("pkexec") {
        match Command::new("pkexec").args(args).output() {
            Ok(out) if out.status.success() && vkms_loaded() => return Ok(()),
            Ok(out) => {
                last = String::from_utf8_lossy(&out.stderr).trim().to_string();
            }
            Err(e) => last = format!("pkexec: {e}"),
        }
    }
    if !sudo_password.is_empty() {
        let mut child = Command::new("sudo")
            .args(["-S", "-p", "", "modprobe", "vkms", "num_outputs=1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("sudo: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "{sudo_password}");
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if out.status.success() && vkms_loaded() {
            return Ok(());
        }
        last = String::from_utf8_lossy(&out.stderr).trim().to_string();
    }
    Err(if last.is_empty() {
        "для vkms нужен root: введите пароль sudo в мастере или выполните \
         `sudo modprobe vkms num_outputs=1`"
            .into()
    } else {
        format!("не удалось загрузить vkms: {last}")
    })
}

/// Write the two small files that keep VKMS loaded after a reboot. Called from
/// the GUI button, never behind the user's back.
fn install_vkms_autoload(sudo_password: &str) -> Result<(), String> {
    let script = "mkdir -p /etc/modules-load.d /etc/modprobe.d && \
         printf 'vkms\\n' > /etc/modules-load.d/sd-widgets.conf && \
         printf 'options vkms num_outputs=1\\n' > /etc/modprobe.d/sd-widgets.conf";
    if !sudo_password.is_empty() {
        let mut child = Command::new("sudo")
            .args(["-S", "-p", "", "sh", "-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("sudo: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "{sudo_password}");
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        return if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        };
    }
    let mut cmd = if sdw_core::util::has("pkexec") {
        Command::new("pkexec")
    } else {
        Command::new("sudo")
    };
    if cmd.get_program() == "sudo" {
        cmd.arg("-n");
    }
    cmd.args(["sh", "-c", script]);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() {
            "не удалось записать автозагрузку vkms: нужен root".into()
        } else {
            err
        })
    }
}

// ---------------------------------------------------------------- capture loops

fn store_frame(
    frame: &Arc<Mutex<SharedFrame>>,
    frames: &Arc<AtomicU64>,
    bytes: Vec<u8>,
    width: u32,
    height: u32,
) {
    if bytes.is_empty() {
        return;
    }
    if let Ok(mut f) = frame.lock() {
        f.bytes = bytes;
        f.ts = now_ms();
        f.width = width;
        f.height = height;
    }
    frames.fetch_add(1, Ordering::Relaxed);
}

fn encode_jpeg(
    img: &image::DynamicImage,
    width: u32,
    height: u32,
    quality: u8,
) -> Result<Vec<u8>, String> {
    let resized = if img.width() != width || img.height() != height {
        img.resize_exact(width, height, image::imageops::FilterType::Triangle)
    } else {
        img.clone()
    };
    let mut out = Vec::new();
    let encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(20, 95));
    resized
        .to_rgb8()
        .write_with_encoder(encoder)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn capture_grim(
    output: &str,
    width: u32,
    height: u32,
    fps: u32,
    quality: u8,
    stop: Arc<AtomicBool>,
    frame: Arc<Mutex<SharedFrame>>,
    frames: Arc<AtomicU64>,
) -> Result<(), String> {
    let interval = Duration::from_millis((1000 / fps.max(1)).max(16) as u64);
    let mut failures = 0u32;
    // Paint the computer's own cursor into the frame, so the person can follow
    // what happens while looking. Older builds do not know `-c`, so fall back
    // to a cursorless capture on the first failure and stay there.
    let mut with_cursor = true;
    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        let q = quality.to_string();
        let mut args: Vec<String> = vec!["-o".into(), output.into()];
        if with_cursor {
            args.push("-c".into());
        }
        args.extend([
            "-t".into(),
            "jpeg".into(),
            "-q".into(),
            q.clone(),
            "-".into(),
        ]);
        let out = Command::new("grim").args(&args).output();
        let (ok, bytes) = match out {
            Ok(o) if o.status.success() && !o.stdout.is_empty() => (true, o.stdout),
            _ => {
                if with_cursor {
                    // Retry once without `-c` before dropping to PNG.
                    with_cursor = false;
                    continue;
                }
                // Older grim builds or PNG-only fallback.
                args = vec![
                    "-o".into(),
                    output.into(),
                    "-t".into(),
                    "png".into(),
                    "-".into(),
                ];
                match Command::new("grim").args(&args).output() {
                    Ok(o) if o.status.success() && !o.stdout.is_empty() => {
                        match image::load_from_memory(&o.stdout)
                            .map_err(|e| e.to_string())
                            .and_then(|img| encode_jpeg(&img, width, height, quality))
                        {
                            Ok(j) => (true, j),
                            Err(_) => (false, Vec::new()),
                        }
                    }
                    _ => (false, Vec::new()),
                }
            }
        };
        if ok {
            store_frame(&frame, &frames, bytes, width, height);
            failures = 0;
        } else {
            failures += 1;
            if failures > 30 {
                return Err(format!("grim не может захватить выход {output}"));
            }
        }
        let spent = started.elapsed();
        if spent < interval {
            std::thread::sleep(interval - spent);
        }
    }
    Ok(())
}

fn capture_screenshot(
    tool: &str,
    width: u32,
    height: u32,
    fps: u32,
    quality: u8,
    stop: Arc<AtomicBool>,
    frame: Arc<Mutex<SharedFrame>>,
    frames: Arc<AtomicU64>,
) -> Result<(), String> {
    let interval = Duration::from_millis((1000 / fps.max(1)).max(50) as u64);
    let path = std::env::temp_dir().join(format!("sdw-frame-{}.png", std::process::id()));
    let mut failures = 0u32;
    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        let args: Vec<String> = if tool == "gnome-screenshot" {
            vec!["-f".into(), path.display().to_string()]
        } else {
            vec![
                "-b".into(),
                "-n".into(),
                "-o".into(),
                path.display().to_string(),
            ]
        };
        let status = Command::new(tool)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let mut ok = false;
        if status.map(|s| s.success()).unwrap_or(false) {
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    if let Ok(j) = encode_jpeg(&img, width, height, quality) {
                        store_frame(&frame, &frames, j, width, height);
                        ok = true;
                    }
                }
            }
            let _ = std::fs::remove_file(&path);
        }
        if ok {
            failures = 0;
        } else {
            failures += 1;
            if failures > 15 {
                return Err(format!("{tool} не делает снимки экрана"));
            }
        }
        let spent = started.elapsed();
        if spent < interval {
            std::thread::sleep(interval - spent);
        }
    }
    Ok(())
}

enum FfmpegKind {
    X11,
    Windows,
}

#[allow(clippy::too_many_arguments)]
fn capture_ffmpeg(
    kind: FfmpegKind,
    rect: (i32, i32, u32, u32),
    width: u32,
    height: u32,
    fps: u32,
    quality: u8,
    stop: Arc<AtomicBool>,
    frame: Arc<Mutex<SharedFrame>>,
    frames: Arc<AtomicU64>,
    child_slot: Arc<Mutex<Option<Child>>>,
) -> Result<(), String> {
    let (x, y, w, h) = rect;
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-loglevel", "error", "-nostdin"]);
    match kind {
        FfmpegKind::X11 => {
            let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
            cmd.args([
                "-f",
                "x11grab",
                "-framerate",
                &fps.to_string(),
                "-video_size",
                &format!("{w}x{h}"),
                "-draw_mouse",
                "1",
                "-i",
                &format!("{display}+{x},{y}"),
            ]);
        }
        FfmpegKind::Windows => {
            cmd.args([
                "-f",
                "gdigrab",
                "-framerate",
                &fps.to_string(),
                "-offset_x",
                &x.to_string(),
                "-offset_y",
                &y.to_string(),
                "-video_size",
                &format!("{w}x{h}"),
                "-draw_mouse",
                "1",
                "-i",
                "desktop",
            ]);
        }
    }
    cmd.args([
        "-vf",
        &format!("scale={width}:{height}"),
        "-f",
        "mjpeg",
        "-q:v",
        &((100 - quality as u32) / 4 + 2).to_string(),
        "-",
    ]);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| format!("ffmpeg: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("ffmpeg: нет stdout")?;
    if let Ok(mut slot) = child_slot.lock() {
        *slot = Some(child);
    }
    let mut buf = vec![0u8; 64 * 1024];
    let mut acc: Vec<u8> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        match stdout.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                acc.extend_from_slice(&buf[..n]);
                // Split concatenated JPEGs.
                loop {
                    let Some(start) = find_marker(&acc, &[0xFF, 0xD8]) else {
                        acc.clear();
                        break;
                    };
                    let Some(end) = find_marker(&acc[start..], &[0xFF, 0xD9]) else {
                        if start > 0 {
                            acc.drain(..start);
                        }
                        break;
                    };
                    let jpeg = acc[start..start + end + 2].to_vec();
                    acc.drain(..start + end + 2);
                    store_frame(&frame, &frames, jpeg, width, height);
                }
                if acc.len() > 8 * 1024 * 1024 {
                    acc.clear();
                }
            }
            Err(_) => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    Ok(())
}

fn find_marker(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn rfb_capture(
    addr: &str,
    width: u32,
    height: u32,
    fps: u32,
    quality: u8,
    stop: Arc<AtomicBool>,
    frame: Arc<Mutex<SharedFrame>>,
    frames: Arc<AtomicU64>,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut rfb = loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        match rfb_connect(addr) {
            Ok(r) => break r,
            Err(e) => {
                if Instant::now() >= deadline {
                    return Err(format!("Krfb не отвечает на {addr}: {e}"));
                }
                std::thread::sleep(Duration::from_millis(300));
            }
        }
    };
    rfb.request(false)?;
    let interval = Duration::from_millis((1000 / fps.max(1)).max(30) as u64);
    let mut failures = 0u32;
    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        match rfb.read_update() {
            Ok(rgb) => {
                failures = 0;
                if let Some(img) = image::RgbImage::from_raw(rfb.width, rfb.height, rgb) {
                    let dynimg = image::DynamicImage::ImageRgb8(img);
                    if let Ok(jpeg) = encode_jpeg(&dynimg, width, height, quality) {
                        store_frame(&frame, &frames, jpeg, width, height);
                    }
                }
                rfb.request(true)?;
            }
            Err(e) => {
                failures += 1;
                if failures > 8 {
                    return Err(format!("RFB: {e}"));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        let spent = started.elapsed();
        if spent < interval {
            std::thread::sleep(interval - spent);
        }
    }
    Ok(())
}

/// Tiny RFB (VNC) client used for KDE's `krfb-virtualmonitor`.
/// Only raw encoding is requested, which every VNC server supports.
pub fn rfb_connect(addr: &str) -> Result<RfbStream, String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("RFB {addr}: {e}"))?;
    stream.set_nodelay(true).ok();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .map_err(|e| e.to_string())?;
    let mut version = [0u8; 12];
    stream.read_exact(&mut version).map_err(|e| e.to_string())?;
    let version_str = String::from_utf8_lossy(&version);
    let minor: u32 = version_str
        .trim()
        .strip_prefix("RFB ")
        .and_then(|v| v.split('.').nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let reply = if minor >= 8 {
        "RFB 003.008\n"
    } else if minor >= 7 {
        "RFB 003.007\n"
    } else {
        "RFB 003.003\n"
    };
    stream
        .write_all(reply.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut chosen = 1u8;
    if minor >= 7 {
        let mut n = [0u8; 1];
        stream.read_exact(&mut n).map_err(|e| e.to_string())?;
        if n[0] == 0 {
            let mut len = [0u8; 4];
            let _ = stream.read_exact(&mut len);
            let mut reason = vec![0u8; u32::from_be_bytes(len).min(4096) as usize];
            let _ = stream.read_exact(&mut reason);
            return Err(format!(
                "RFB отклонил подключение: {}",
                String::from_utf8_lossy(&reason)
            ));
        }
        let mut types = vec![0u8; n[0] as usize];
        stream.read_exact(&mut types).map_err(|e| e.to_string())?;
        if types.contains(&1) {
            chosen = 1;
        } else if types.contains(&2) {
            return Err("Krfb требует VNC-пароль. Отключите пароль у krfb-virtualmonitor.".into());
        } else {
            return Err(format!("RFB: неподдерживаемые типы безопасности {types:?}"));
        }
        stream.write_all(&[chosen]).map_err(|e| e.to_string())?;
    }
    if minor >= 8 || chosen != 1 {
        let mut result = [0u8; 4];
        stream.read_exact(&mut result).map_err(|e| e.to_string())?;
        if u32::from_be_bytes(result) != 0 {
            return Err("RFB: ошибка аутентификации".into());
        }
    }
    // shared session
    stream.write_all(&[1]).map_err(|e| e.to_string())?;
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).map_err(|e| e.to_string())?;
    let width = u16::from_be_bytes([head[0], head[1]]) as u32;
    let height = u16::from_be_bytes([head[2], head[3]]) as u32;
    let mut pf = [0u8; 16];
    stream.read_exact(&mut pf).map_err(|e| e.to_string())?;
    let mut name_len = [0u8; 4];
    stream
        .read_exact(&mut name_len)
        .map_err(|e| e.to_string())?;
    let mut name = vec![0u8; u32::from_be_bytes(name_len).min(4096) as usize];
    stream.read_exact(&mut name).map_err(|e| e.to_string())?;

    // SetPixelFormat: 32bpp little-endian true colour, BGRX in memory.
    let mut msg = vec![0u8, 0, 0, 0];
    msg.extend_from_slice(&[32, 24, 0, 1]);
    msg.extend_from_slice(&255u16.to_be_bytes());
    msg.extend_from_slice(&255u16.to_be_bytes());
    msg.extend_from_slice(&255u16.to_be_bytes());
    msg.extend_from_slice(&[16, 8, 0, 0, 0, 0]);
    stream.write_all(&msg).map_err(|e| e.to_string())?;
    // SetEncodings: raw only
    stream
        .write_all(&[2, 0, 0, 1, 0, 0, 0, 0])
        .map_err(|e| e.to_string())?;
    Ok(RfbStream {
        stream,
        width,
        height,
        name: String::from_utf8_lossy(&name).to_string(),
    })
}

pub struct RfbStream {
    stream: TcpStream,
    pub width: u32,
    pub height: u32,
    pub name: String,
}

impl RfbStream {
    pub fn request(&mut self, incremental: bool) -> Result<(), String> {
        let mut msg = [3u8, incremental as u8, 0, 0, 0, 0, 0, 0, 0, 0];
        msg[2..4].copy_from_slice(&0u16.to_be_bytes());
        msg[4..6].copy_from_slice(&0u16.to_be_bytes());
        msg[6..8].copy_from_slice(&(self.width as u16).to_be_bytes());
        msg[8..10].copy_from_slice(&(self.height as u16).to_be_bytes());
        self.stream.write_all(&msg).map_err(|e| e.to_string())
    }

    pub fn read_update(&mut self) -> Result<Vec<u8>, String> {
        loop {
            let mut t = [0u8; 1];
            self.stream.read_exact(&mut t).map_err(|e| e.to_string())?;
            match t[0] {
                0 => {
                    let mut pad = [0u8; 1];
                    self.stream
                        .read_exact(&mut pad)
                        .map_err(|e| e.to_string())?;
                    let mut nrects = [0u8; 2];
                    self.stream
                        .read_exact(&mut nrects)
                        .map_err(|e| e.to_string())?;
                    let n = u16::from_be_bytes(nrects) as usize;
                    let mut pixels: Option<Vec<u8>> = None;
                    for _ in 0..n {
                        let mut r = [0u8; 12];
                        self.stream.read_exact(&mut r).map_err(|e| e.to_string())?;
                        let x = u16::from_be_bytes([r[0], r[1]]) as usize;
                        let y = u16::from_be_bytes([r[2], r[3]]) as usize;
                        let w = u16::from_be_bytes([r[4], r[5]]) as usize;
                        let h = u16::from_be_bytes([r[6], r[7]]) as usize;
                        let enc = i32::from_be_bytes([r[8], r[9], r[10], r[11]]);
                        if enc == 0 {
                            let mut data = vec![0u8; w * h * 4];
                            self.stream
                                .read_exact(&mut data)
                                .map_err(|e| e.to_string())?;
                            if pixels.is_none() {
                                pixels =
                                    Some(vec![0u8; self.width as usize * self.height as usize * 3]);
                            }
                            let canvas = pixels.as_mut().unwrap();
                            for row in 0..h {
                                let sy = y + row;
                                if sy >= self.height as usize {
                                    break;
                                }
                                for col in 0..w {
                                    let sx = x + col;
                                    if sx >= self.width as usize {
                                        break;
                                    }
                                    let si = (row * w + col) * 4;
                                    let di = (sy * self.width as usize + sx) * 3;
                                    // BGRX
                                    canvas[di] = data[si + 2];
                                    canvas[di + 1] = data[si + 1];
                                    canvas[di + 2] = data[si];
                                }
                            }
                        } else if enc == -223 {
                            // DesktopSize pseudo-encoding: no payload.
                            if let Some(canvas) = pixels.as_mut() {
                                let _ = canvas;
                            }
                        } else {
                            return Err(format!("RFB: неподдерживаемое кодирование {enc}"));
                        }
                    }
                    if let Some(p) = pixels {
                        return Ok(p);
                    }
                }
                1 => {
                    let mut hdr = [0u8; 5];
                    self.stream
                        .read_exact(&mut hdr)
                        .map_err(|e| e.to_string())?;
                    let count = u16::from_be_bytes([hdr[3], hdr[4]]) as usize;
                    let mut skip = vec![0u8; count * 6];
                    self.stream
                        .read_exact(&mut skip)
                        .map_err(|e| e.to_string())?;
                }
                2 => {}
                3 => {
                    let mut hdr = [0u8; 7];
                    self.stream
                        .read_exact(&mut hdr)
                        .map_err(|e| e.to_string())?;
                    let len = u32::from_be_bytes([hdr[3], hdr[4], hdr[5], hdr[6]]) as usize;
                    let mut skip = vec![0u8; len.min(1 << 20)];
                    self.stream
                        .read_exact(&mut skip)
                        .map_err(|e| e.to_string())?;
                }
                other => return Err(format!("RFB: неизвестное сообщение {other}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry() {
        assert_eq!(parse_geometry("2560x1440+0+0"), Some((2560, 1440, 0, 0)));
        assert_eq!(
            parse_geometry("1920x1080+-100+50"),
            Some((1920, 1080, -100, 50))
        );
    }

    #[test]
    fn resolutions() {
        assert_eq!(parse_resolution("2560 x 1440"), Some((2560, 1440)));
        assert_eq!(parse_resolution("1920 x 1080 (Retina)"), Some((1920, 1080)));
    }
}
