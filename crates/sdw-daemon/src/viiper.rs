//! VIIPER lifecycle and virtual gamepad plumbing.
//!
//! The daemon owns the VIIPER server process when it starts it itself, and otherwise
//! talks to an already-running instance. Pads are real USB devices from the guest's
//! point of view; the Deck sends compact UDP state packets which we translate into
//! the device-specific wire format.

use crate::config::{self, DaemonConfig};
use crate::state::Logger;
use sdw_core::auth::token_matches_tag;
use sdw_core::model::*;
use sdw_core::udp::{self, GamepadState};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use viiper_client::devices::dualsense::input::DualsenseInput;
use viiper_client::devices::dualshock4::input::Dualshock4Input;
use viiper_client::devices::ns2pro::input::Ns2proInput;
use viiper_client::devices::xbox360::input::Xbox360Input;
use viiper_client::{DeviceCreateRequest, ViiperClient};

pub struct PadRuntime {
    pub id: String,
    pub name: String,
    pub kind: PadKind,
    pub bus_id: u32,
    pub dev_id: String,
    pub created_at: u64,
    pub secret: String,
    pub connected: AtomicBool,
    pub last_input: AtomicU64,
    pub rumble: Mutex<[u8; 2]>,
    pub error: Mutex<String>,
    pub stop: AtomicBool,
    tx: SyncSender<GamepadState>,
}

impl PadRuntime {
    pub fn info(&self) -> PadInfo {
        PadInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind,
            bus_id: self.bus_id,
            dev_id: self.dev_id.clone(),
            created_at: self.created_at,
            connected: self.connected.load(Ordering::Relaxed),
            last_input_ms: self.last_input.load(Ordering::Relaxed),
            rumble: *self.rumble.lock().unwrap_or_else(|e| e.into_inner()),
            error: self
                .error
                .lock()
                .map(|e| e.clone())
                .unwrap_or_default(),
        }
    }
}

pub struct ViiperManager {
    logger: Logger,
    pads: Mutex<HashMap<String, Arc<PadRuntime>>>,
    child: Mutex<Option<Child>>,
    last_error: Mutex<String>,
    started_by_us: AtomicBool,
    next_bus: Mutex<u32>,
}

impl ViiperManager {
    pub fn new(logger: Logger) -> Self {
        Self {
            logger,
            pads: Mutex::new(HashMap::new()),
            child: Mutex::new(None),
            last_error: Mutex::new(String::new()),
            started_by_us: AtomicBool::new(false),
            next_bus: Mutex::new(1),
        }
    }

    fn set_error(&self, e: impl Into<String>) {
        let e = e.into();
        if !e.is_empty() {
            self.logger.warn(format!("VIIPER: {e}"));
        }
        if let Ok(mut slot) = self.last_error.lock() {
            *slot = e;
        }
    }

    pub fn error(&self) -> String {
        self.last_error
            .lock()
            .map(|e| e.clone())
            .unwrap_or_default()
    }

    pub fn bin(&self, cfg: &DaemonConfig) -> Option<std::path::PathBuf> {
        if !cfg.viiper.bin.trim().is_empty() {
            let p = std::path::PathBuf::from(&cfg.viiper.bin);
            if p.is_file() {
                return Some(p);
            }
        }
        if let Some(p) = sdw_core::util::which("viiper") {
            return Some(p);
        }
        let local = config::viiper_bin_path();
        local.is_file().then_some(local)
    }

    pub fn key_path(&self) -> std::path::PathBuf {
        config::viiper_key_path()
    }

    /// Read the key that the VIIPER server uses for encrypted API connections.
    pub fn key(&self, cfg: &DaemonConfig) -> String {
        if !cfg.viiper.password.trim().is_empty() {
            return cfg.viiper.password.trim().to_string();
        }
        std::fs::read_to_string(self.key_path())
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    }

    pub fn write_key(&self, password: &str) -> Result<(), String> {
        let path = self.key_path();
        let password = password.trim();
        if password.is_empty() {
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            }
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            sdw_core::util::ensure_dir(parent).map_err(|e| e.to_string())?;
        }
        sdw_core::util::atomic_write(&path, password.as_bytes()).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    fn addr(cfg: &DaemonConfig) -> Result<SocketAddr, String> {
        cfg.viiper
            .api_addr
            .parse()
            .map_err(|_| format!("неверный адрес VIIPER API: {}", cfg.viiper.api_addr))
    }

    fn client(cfg: &DaemonConfig) -> Result<ViiperClient, String> {
        let addr = Self::addr(cfg)?;
        let password = if !cfg.viiper.password.trim().is_empty() {
            cfg.viiper.password.trim().to_string()
        } else {
            std::fs::read_to_string(config::viiper_key_path())
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        Ok(ViiperClient::new_with_password(addr, password))
    }

    fn ping(cfg: &DaemonConfig) -> Result<String, String> {
        Self::client(cfg)?
            .ping()
            .map(|p| p.version)
            .map_err(|e| e.to_string())
    }

    pub fn status(&self, cfg: &DaemonConfig) -> ViiperStatus {
        let bin = self.bin(cfg);
        let key_path = self.key_path();
        let password_set = !self.key(cfg).is_empty() || key_path.exists();
        let (running, version) = match Self::ping(cfg) {
            Ok(v) => (true, v),
            Err(_) => (false, String::new()),
        };
        ViiperStatus {
            installed: bin.is_some(),
            running,
            version,
            api_addr: cfg.viiper.api_addr.clone(),
            usb_addr: cfg.viiper.usb_addr.clone(),
            password_set,
            key_path: key_path.display().to_string(),
            usbip_installed: sdw_core::util::has("usbip"),
            auto_start: cfg.viiper.auto_start,
            error: self.error(),
        }
    }

    pub fn start(&self, cfg: &DaemonConfig) -> Result<(), String> {
        if Self::ping(cfg).is_ok() {
            self.set_error("");
            self.logger.info(format!(
                "VIIPER уже запущен на {}",
                cfg.viiper.api_addr
            ));
            return Ok(());
        }
        let bin = self
            .bin(cfg)
            .ok_or_else(|| "VIIPER не установлен: установите его в разделе «VIIPER»".to_string())?;

        // The key file must exist before the server starts.
        if !cfg.viiper.password.trim().is_empty() {
            self.write_key(&cfg.viiper.password)?;
        }

        let log_path = sdw_core::util::data_dir().join("viiper.log");
        if let Some(parent) = log_path.parent() {
            sdw_core::util::ensure_dir(parent).map_err(|e| e.to_string())?;
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| format!("журнал VIIPER: {e}"))?;
        let log2 = log.try_clone().map_err(|e| e.to_string())?;

        let mut cmd = Command::new(&bin);
        cmd.arg("server")
            .arg(format!("--api.addr={}", cfg.viiper.api_addr))
            .arg(format!("--usb.addr={}", cfg.viiper.usb_addr))
            .arg("--log.level=info")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log2));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let child = cmd
            .spawn()
            .map_err(|e| format!("запуск {}: {e}", bin.display()))?;
        self.logger
            .info(format!("VIIPER запущен: {} server", bin.display()));
        if let Ok(mut slot) = self.child.lock() {
            if let Some(mut old) = slot.take() {
                let _ = old.kill();
            }
            *slot = Some(child);
        }
        self.started_by_us.store(true, Ordering::Relaxed);

        let deadline = Instant::now() + Duration::from_secs(8);
        let mut last = String::new();
        while Instant::now() < deadline {
            match Self::ping(cfg) {
                Ok(v) => {
                    self.set_error("");
                    self.logger.info(format!("VIIPER готов, версия {v}"));
                    return Ok(());
                }
                Err(e) => last = e,
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let tail = std::fs::read_to_string(&log_path)
            .ok()
            .map(|s| {
                s.lines()
                    .rev()
                    .take(6)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let msg = if tail.trim().is_empty() {
            format!("VIIPER не ответил за 8 секунд: {last}")
        } else {
            format!("VIIPER не ответил за 8 секунд: {last}\n{tail}")
        };
        self.set_error(msg.clone());
        Err(msg)
    }

    pub fn stop(&self) {
        for (_, pad) in self.pads.lock().map(|p| p.clone()).unwrap_or_default() {
            pad.stop.store(true, Ordering::Relaxed);
        }
        if let Ok(mut pads) = self.pads.lock() {
            pads.clear();
        }
        if let Ok(mut slot) = self.child.lock() {
            if let Some(mut child) = slot.take() {
                self.logger.info("Останавливаю VIIPER");
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        self.started_by_us.store(false, Ordering::Relaxed);
    }

    pub fn restart(&self, cfg: &DaemonConfig) -> Result<(), String> {
        self.stop();
        std::thread::sleep(Duration::from_millis(400));
        self.start(cfg)
    }

    pub fn pads(&self) -> Vec<PadInfo> {
        let mut out: Vec<PadInfo> = self
            .pads
            .lock()
            .map(|p| p.values().map(|r| r.info()).collect())
            .unwrap_or_default();
        out.sort_by_key(|p| p.created_at);
        out
    }

    pub fn pad(&self, id: &str) -> Option<Arc<PadRuntime>> {
        self.pads.lock().ok().and_then(|p| p.get(id).cloned())
    }

    pub fn create_pad(&self, cfg: &DaemonConfig, name: &str, kind: PadKind) -> Result<PadCreated, String> {
        if Self::ping(cfg).is_err() {
            self.start(cfg)?;
        }
        let client = Self::client(cfg)?;
        let bus = client
            .bus_create(None)
            .map_err(|e| format!("создание USB-шины: {e}"))?
            .bus_id;
        let req = DeviceCreateRequest {
            r#type: Some(kind.viiper_type().to_string()),
            id_vendor: None,
            id_product: None,
            device_specific: None,
        };
        let dev = match client.bus_device_add(bus, &req) {
            Ok(d) => d,
            Err(e) => {
                let _ = client.bus_remove(Some(bus));
                return Err(format!("создание устройства: {e}"));
            }
        };
        let stream = match client.connect_device(bus, &dev.dev_id) {
            Ok(s) => s,
            Err(e) => {
                let _ = client.bus_device_remove(bus, Some(&dev.dev_id));
                let _ = client.bus_remove(Some(bus));
                return Err(format!("подключение к устройству: {e}"));
            }
        };

        // 32-bit so it fits the compact UDP packet without a lookup table.
        let id = random_id(4);
        let secret = sdw_core::auth::random_secret(32);
        let (tx, rx) = sync_channel(8);
        let runtime = Arc::new(PadRuntime {
            id: id.clone(),
            name: name.trim().to_string(),
            kind,
            bus_id: bus,
            dev_id: dev.dev_id.clone(),
            created_at: now_ms(),
            secret: secret.clone(),
            connected: AtomicBool::new(true),
            last_input: AtomicU64::new(0),
            rumble: Mutex::new([0, 0]),
            error: Mutex::new(String::new()),
            stop: AtomicBool::new(false),
            tx,
        });
        if let Ok(mut pads) = self.pads.lock() {
            pads.insert(id.clone(), runtime.clone());
        }
        if let Ok(mut n) = self.next_bus.lock() {
            *n = (*n).max(bus + 1);
        }

        let logger = self.logger.clone();
        let rt = runtime.clone();
        std::thread::spawn(move || {
            run_pad(stream, rt.clone(), rx, logger);
            rt.connected.store(false, Ordering::Relaxed);
        });

        self.logger.info(format!(
            "Создан виртуальный пад «{}» ({}, bus {bus})",
            runtime.name,
            kind.label()
        ));
        Ok(PadCreated {
            pad: runtime.info(),
            udp_secret: secret,
        })
    }

    pub fn delete_pad(&self, id: &str) -> Result<(), String> {
        let pad = self
            .pads
            .lock()
            .map_err(|_| "pads lock")?
            .remove(id);
        let Some(pad) = pad else {
            return Err("пад не найден".into());
        };
        pad.stop.store(true, Ordering::Relaxed);
        let cfg = crate::config::load();
        if let Ok(client) = Self::client(&cfg) {
            let _ = client.bus_device_remove(pad.bus_id, Some(&pad.dev_id));
            let _ = client.bus_remove(Some(pad.bus_id));
        }
        self.logger
            .info(format!("Удалён виртуальный пад «{}»", pad.name));
        Ok(())
    }

    /// Receives compact UDP packets from the Deck and forwards them to the matching pad.
    pub fn run_udp_listener(&self, port: u16, stop: Arc<AtomicBool>, logger: Logger) {
        let sock = match std::net::UdpSocket::bind(("0.0.0.0", port)) {
            Ok(s) => s,
            Err(e) => {
                logger.error(format!("UDP-порт {port} занят: {e}"));
                return;
            }
        };
        let _ = sock.set_read_timeout(Some(Duration::from_millis(500)));
        logger.info(format!("Приём геймпадов на UDP :{port}"));
        let mut buf = [0u8; 128];
        let mut last_cleanup = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            match sock.recv_from(&mut buf) {
                Ok((n, from)) => {
                    let Some(pkt) = udp::PadPacket::decode(&buf[..n]) else {
                        continue;
                    };
                    let Some(pad) = self.pad(&format!("{:08x}", pkt.pad_id)) else {
                        continue;
                    };
                    if !token_matches_tag(&pad.secret, &pkt.tag) {
                        continue;
                    }
                    pad.last_input.store(now_ms(), Ordering::Relaxed);
                    if !pad.connected.load(Ordering::Relaxed) {
                        pad.connected.store(true, Ordering::Relaxed);
                    }
                    // Keep the freshest state; a full queue means the sender is busy.
                    let _ = pad.tx.try_send(pkt.state);
                    let _ = from;
                }
                Err(_) => {}
            }
            if last_cleanup.elapsed() > Duration::from_secs(5) {
                last_cleanup = Instant::now();
                // Nothing to clean: pads live until deleted. Kept for future stale-pad handling.
            }
        }
    }
}

fn run_pad(
    mut stream: viiper_client::DeviceStream,
    rt: Arc<PadRuntime>,
    rx: Receiver<GamepadState>,
    logger: Logger,
) {
    let out_rt = rt.clone();
    let kind = rt.kind;
    let _ = stream.on_output(move |reader| {
        let size = match kind {
            PadKind::Xbox360 => 2,
            PadKind::DualShock4 => 8,
            PadKind::DualSense => 31,
            PadKind::SwitchPro => 34,
        };
        let mut buf = vec![0u8; size];
        reader.read_exact(&mut buf)?;
        let (left, right) = match kind {
            PadKind::Xbox360 => (buf[0], buf[1]),
            PadKind::DualShock4 => (buf[1], buf[2]),
            PadKind::DualSense => (buf[3], buf[4]),
            PadKind::SwitchPro => (0, 0),
        };
        if let Ok(mut r) = out_rt.rumble.lock() {
            *r = [left, right];
        }
        Ok(())
    });

    let mut last_sent = Instant::now();
    let mut neutral = false;
    while !rt.stop.load(Ordering::Relaxed) {
        match rx.recv_timeout(Duration::from_millis(150)) {
            Ok(state) => {
                if let Err(e) = send_state(&mut stream, rt.kind, &state) {
                    if let Ok(mut slot) = rt.error.lock() {
                        *slot = e.clone();
                    }
                    logger.warn(format!("пад «{}»: {e}", rt.name));
                    rt.connected.store(false, Ordering::Relaxed);
                    return;
                }
                last_sent = Instant::now();
                neutral = false;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if !neutral && last_sent.elapsed() > Duration::from_millis(900) {
                    let _ = send_state(&mut stream, rt.kind, &GamepadState::default());
                    neutral = true;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = send_state(&mut stream, rt.kind, &GamepadState::default());
}

fn send_state(
    stream: &mut viiper_client::DeviceStream,
    kind: PadKind,
    s: &GamepadState,
) -> Result<(), String> {
    let b = s.buttons;
    let has = |bit: u32| b & bit != 0;
    let axis = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0) as i16;
    let axis8 = |v: f32| (v.clamp(-1.0, 1.0) * 127.0) as i8;
    match kind {
        PadKind::Xbox360 => {
            let mut buttons: u32 = 0;
            if has(udp::BTN_DPAD_UP) {
                buttons |= 0x0001;
            }
            if has(udp::BTN_DPAD_DOWN) {
                buttons |= 0x0002;
            }
            if has(udp::BTN_DPAD_LEFT) {
                buttons |= 0x0004;
            }
            if has(udp::BTN_DPAD_RIGHT) {
                buttons |= 0x0008;
            }
            if has(udp::BTN_START) {
                buttons |= 0x0010;
            }
            if has(udp::BTN_BACK) {
                buttons |= 0x0020;
            }
            if has(udp::BTN_L3) {
                buttons |= 0x0040;
            }
            if has(udp::BTN_R3) {
                buttons |= 0x0080;
            }
            if has(udp::BTN_LB) {
                buttons |= 0x0100;
            }
            if has(udp::BTN_RB) {
                buttons |= 0x0200;
            }
            if has(udp::BTN_GUIDE) {
                buttons |= 0x0400;
            }
            if has(udp::BTN_A) {
                buttons |= 0x1000;
            }
            if has(udp::BTN_B) {
                buttons |= 0x2000;
            }
            if has(udp::BTN_X) {
                buttons |= 0x4000;
            }
            if has(udp::BTN_Y) {
                buttons |= 0x8000;
            }
            stream
                .send(&Xbox360Input {
                    buttons,
                    lt: (s.lt.clamp(0.0, 1.0) * 255.0) as u8,
                    rt: (s.rt.clamp(0.0, 1.0) * 255.0) as u8,
                    lx: axis(s.lx),
                    ly: axis(s.ly),
                    rx: axis(s.rx),
                    ry: axis(s.ry),
                    reserved: [0; 6],
                })
                .map_err(|e| e.to_string())
        }
        PadKind::DualShock4 | PadKind::DualSense => {
            let mut dpad: u8 = 8;
            let up = has(udp::BTN_DPAD_UP);
            let down = has(udp::BTN_DPAD_DOWN);
            let left = has(udp::BTN_DPAD_LEFT);
            let right = has(udp::BTN_DPAD_RIGHT);
            dpad = match (up, down, left, right) {
                (true, false, false, false) => 0,
                (true, false, false, true) => 1,
                (false, false, false, true) => 2,
                (false, true, false, true) => 3,
                (false, true, false, false) => 4,
                (false, true, true, false) => 5,
                (false, false, true, false) => 6,
                (true, false, true, false) => 7,
                _ => 8,
            };
            let accel = if s.accel == [0, 0, 0] {
                [0, 0, -8192]
            } else {
                s.accel
            };
            let (mut buttons, touch, create, options, ps, l2btn, r2btn) = if kind == PadKind::DualShock4 {
                let mut buttons: u16 = 0;
                if has(udp::BTN_X) {
                    buttons |= 0x0010;
                }
                if has(udp::BTN_A) {
                    buttons |= 0x0020;
                }
                if has(udp::BTN_B) {
                    buttons |= 0x0040;
                }
                if has(udp::BTN_Y) {
                    buttons |= 0x0080;
                }
                if has(udp::BTN_LB) {
                    buttons |= 0x0100;
                }
                if has(udp::BTN_RB) {
                    buttons |= 0x0200;
                }
                if has(udp::BTN_BACK) {
                    buttons |= 0x1000;
                }
                if has(udp::BTN_START) {
                    buttons |= 0x2000;
                }
                if has(udp::BTN_L3) {
                    buttons |= 0x4000;
                }
                if has(udp::BTN_R3) {
                    buttons |= 0x8000;
                }
                (buttons as u32, 0x0002u32, 0x1000u32, 0x2000u32, 0x0001u32, 0x0400u32, 0x0800u32)
            } else {
                let mut buttons: u32 = 0;
                if has(udp::BTN_X) {
                    buttons |= 0x0010;
                }
                if has(udp::BTN_A) {
                    buttons |= 0x0020;
                }
                if has(udp::BTN_B) {
                    buttons |= 0x0040;
                }
                if has(udp::BTN_Y) {
                    buttons |= 0x0080;
                }
                if has(udp::BTN_LB) {
                    buttons |= 0x0100;
                }
                if has(udp::BTN_RB) {
                    buttons |= 0x0200;
                }
                if has(udp::BTN_BACK) {
                    buttons |= 0x1000;
                }
                if has(udp::BTN_START) {
                    buttons |= 0x2000;
                }
                if has(udp::BTN_L3) {
                    buttons |= 0x4000;
                }
                if has(udp::BTN_R3) {
                    buttons |= 0x8000;
                }
                (buttons, 0x0002u32, 0x1000u32, 0x2000u32, 0x10000u32, 0x0400u32, 0x0800u32)
            };
            let _ = (touch, create, options);
            if s.lt > 0.5 {
                buttons |= l2btn;
            }
            if s.rt > 0.5 {
                buttons |= r2btn;
            }
            if has(udp::BTN_GUIDE) {
                buttons |= ps;
            }
            if kind == PadKind::DualShock4 {
                stream
                    .send(&Dualshock4Input {
                        stick_lx: axis8(s.lx),
                        stick_ly: axis8(-s.ly),
                        stick_rx: axis8(s.rx),
                        stick_ry: axis8(-s.ry),
                        buttons: buttons as u16,
                        dpad,
                        trigger_l2: (s.lt.clamp(0.0, 1.0) * 255.0) as u8,
                        trigger_r2: (s.rt.clamp(0.0, 1.0) * 255.0) as u8,
                        touch1x: 0,
                        touch1y: 0,
                        touch1_active: 0x80,
                        touch2x: 0,
                        touch2y: 0,
                        touch2_active: 0x80,
                        gyro_x: s.gyro[0],
                        gyro_y: s.gyro[1],
                        gyro_z: s.gyro[2],
                        accel_x: accel[0],
                        accel_y: accel[1],
                        accel_z: accel[2],
                    })
                    .map_err(|e| e.to_string())
            } else {
                stream
                    .send(&DualsenseInput {
                        stick_lx: axis8(s.lx),
                        stick_ly: axis8(-s.ly),
                        stick_rx: axis8(s.rx),
                        stick_ry: axis8(-s.ry),
                        buttons,
                        dpad,
                        trigger_l2: (s.lt.clamp(0.0, 1.0) * 255.0) as u8,
                        trigger_r2: (s.rt.clamp(0.0, 1.0) * 255.0) as u8,
                        touch1x: 0,
                        touch1y: 0,
                        touch1_active: 0x80,
                        touch2x: 0,
                        touch2y: 0,
                        touch2_active: 0x80,
                        gyro_x: s.gyro[0],
                        gyro_y: s.gyro[1],
                        gyro_z: s.gyro[2],
                        accel_x: accel[0],
                        accel_y: accel[1],
                        accel_z: accel[2],
                    })
                    .map_err(|e| e.to_string())
            }
        }
        PadKind::SwitchPro => {
            let mut buttons: u32 = 0;
            if has(udp::BTN_B) {
                buttons |= 1;
            }
            if has(udp::BTN_A) {
                buttons |= 2;
            }
            if has(udp::BTN_Y) {
                buttons |= 4;
            }
            if has(udp::BTN_X) {
                buttons |= 8;
            }
            if has(udp::BTN_RB) {
                buttons |= 16;
            }
            if s.rt > 0.5 {
                buttons |= 32;
            }
            if has(udp::BTN_START) {
                buttons |= 64;
            }
            if has(udp::BTN_R3) {
                buttons |= 128;
            }
            if has(udp::BTN_DPAD_DOWN) {
                buttons |= 256;
            }
            if has(udp::BTN_DPAD_RIGHT) {
                buttons |= 512;
            }
            if has(udp::BTN_DPAD_LEFT) {
                buttons |= 1024;
            }
            if has(udp::BTN_DPAD_UP) {
                buttons |= 2048;
            }
            if has(udp::BTN_LB) {
                buttons |= 4096;
            }
            if s.lt > 0.5 {
                buttons |= 8192;
            }
            if has(udp::BTN_BACK) {
                buttons |= 16384;
            }
            if has(udp::BTN_L3) {
                buttons |= 32768;
            }
            if has(udp::BTN_GUIDE) {
                buttons |= 65536;
            }
            let stick = |v: f32| ((v.clamp(-1.0, 1.0) * 32767.0) as i32 + 32768) as u16 >> 4;
            stream
                .send(&Ns2proInput {
                    buttons,
                    lx: stick(s.lx),
                    ly: stick(s.ly),
                    rx: stick(s.rx),
                    ry: stick(s.ry),
                    accel_x: s.accel[0],
                    accel_y: s.accel[1],
                    accel_z: s.accel[2],
                    gyro_x: s.gyro[0],
                    gyro_y: s.gyro[1],
                    gyro_z: s.gyro[2],
                })
                .map_err(|e| e.to_string())
        }
    }
}

/// Pad IDs travel as u32 on the wire. Our IDs are 6 random bytes rendered as hex.
pub fn pad_wire_id(id: &str) -> u32 {
    u32::from_str_radix(id, 16).unwrap_or(0)
}
