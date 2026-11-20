//! reads the steam deck's own controls through evdev.
//!
//! one thread per device, one coordinator that turns raw events into a gamepad state,
//! gestures and button events. the coordinator also owns the udp socket that feeds
//! the virtual pad on the computer.

use sdw_core::model::GestureSpec;
use sdw_core::model::{GestureKind, SwipeDir};
use sdw_core::udp::{self, GamepadState};
use std::collections::HashSet;
use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc::{channel, Receiver, Sender};
use sdw_core::{t, Lang};
use std::time::{Duration, Instant};

pub const EV_KEY: u16 = 1;
pub const EV_ABS: u16 = 3;

pub const BTN_SOUTH: u16 = 0x130;
pub const BTN_EAST: u16 = 0x131;
pub const BTN_NORTH: u16 = 0x133;
pub const BTN_WEST: u16 = 0x134;
pub const BTN_TL: u16 = 0x136;
pub const BTN_TR: u16 = 0x137;
pub const BTN_TL2: u16 = 0x138;
pub const BTN_TR2: u16 = 0x139;
pub const BTN_SELECT: u16 = 0x13a;
pub const BTN_START: u16 = 0x13b;
pub const BTN_MODE: u16 = 0x13c;
pub const BTN_THUMBL: u16 = 0x13d;
pub const BTN_THUMBR: u16 = 0x13e;
pub const BTN_DPAD_UP: u16 = 0x220;
pub const BTN_DPAD_DOWN: u16 = 0x221;
pub const BTN_DPAD_LEFT: u16 = 0x222;
pub const BTN_DPAD_RIGHT: u16 = 0x223;
/// Steam Deck back buttons (hid-steam reports them as trigger-happy keys).
pub const BTN_L4: u16 = 0x2c0;
pub const BTN_L5: u16 = 0x2c1;
pub const BTN_R4: u16 = 0x2c2;
pub const BTN_R5: u16 = 0x2c3;
pub const BTN_LEFT_CLICK: u16 = 0x110;
pub const BTN_RIGHT_CLICK: u16 = 0x111;

const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const ABS_Z: u16 = 2;
const ABS_RX: u16 = 3;
const ABS_RY: u16 = 4;
const ABS_RZ: u16 = 5;
const ABS_GAS: u16 = 9;
const ABS_BRAKE: u16 = 10;
const ABS_HAT0X: u16 = 16;
const ABS_HAT0Y: u16 = 17;
const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;
pub const BTN_TOUCH: u16 = 0x14a;

/// Touchscreen housekeeping codes. They arrive on every finger and must never
/// be recorded as a bindable button.
pub fn is_touch_event(code: u16) -> bool {
    matches!(
        code,
        0x145..=0x14a // BTN_TOOL_FINGER … BTN_TOUCH
    ) || matches!(code, 0x14b..=0x14f)
}

#[derive(Debug, Clone)]
pub struct UdpTarget {
    pub addr: SocketAddr,
    pub pad_id: u32,
    pub secret: String,
}

pub enum InputCmd {
    Target(Option<UdpTarget>),
    Enabled(bool),
}

#[derive(Debug, Clone)]
pub enum DeckEvent {
    Pad(GamepadState),
    Gesture(GestureSpec),
    Button {
        code: u16,
        label: String,
        device: String,
        pressed: bool,
    },
    Devices(Vec<String>, bool),
    Error(String),
}

pub struct InputHandle {
    pub cmd_tx: Sender<InputCmd>,
    pub evt_rx: Receiver<DeckEvent>,
    pub devices: Vec<String>,
    pub errors: Vec<String>,
    pub has_gamepad: bool,
}

impl InputHandle {
    pub fn set_target(&self, target: Option<UdpTarget>) {
        let _ = self.cmd_tx.send(InputCmd::Target(target));
    }
    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.cmd_tx.send(InputCmd::Enabled(enabled));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Gamepad,
    Motion,
    Touch,
    Keys,
}

enum Raw {
    Key {
        role: Role,
        code: u16,
        value: i32,
        device: String,
    },
    Axis {
        role: Role,
        code: u16,
        value: f32,
    },
    Touch {
        slot: u8,
        x: Option<f32>,
        y: Option<f32>,
        tracking: Option<i32>,
    },
    TouchDown(bool),
}

/// Stable codes for the two failures the UI can translate itself.
pub const ERROR_LINUX_ONLY: &str = "linux-only";
pub const ERROR_NO_DEVICES: &str = "no-devices";

/// Turn an input error code into a line of the current language. Unknown
/// strings pass through, so a future code never disappears silently.
pub fn error_text(lang: Lang, code: &str) -> String {
    match code {
        ERROR_LINUX_ONLY => t!(
            lang,
            "чтение контроллера steam deck доступно только в linux",
            "steam deck controller input is linux only"
        )
        .to_string(),
        ERROR_NO_DEVICES => t!(
            lang,
            "не найдено доступных устройств ввода. добавьте пользователя в группу input.",
            "no readable input devices. add the user to the input group."
        )
        .to_string(),
        other => other.to_string(),
    }
}

pub fn start() -> InputHandle {
    let (cmd_tx, cmd_rx) = channel();
    let (evt_tx, evt_rx) = channel();
    let (raw_tx, raw_rx) = channel();

    #[cfg(target_os = "linux")]
    let mut scan = ScanState::default();
    #[cfg(target_os = "linux")]
    let (devices, has_gamepad, errors) = scan_into(&raw_tx, &mut scan);

    #[cfg(not(target_os = "linux"))]
    let (devices, has_gamepad, errors): (Vec<String>, bool, Vec<String>) = (
        Vec::new(),
        false,
        vec![ERROR_LINUX_ONLY.into()],
    );

    let evt_tx2 = evt_tx.clone();
    let device_list = devices.clone();
    std::thread::Builder::new()
        .name("sdw-input-coordinator".into())
        .spawn(move || {
            let _ = evt_tx2.send(DeckEvent::Devices(device_list, has_gamepad));
            coordinate(raw_rx, cmd_rx, evt_tx2);
        })
        .ok();

    // Steam Input and USB pads can appear long after the app started, and a
    // pad that is unplugged comes back on a new event node. Re-scan forever so
    // the Deck controls never need a restart to work.
    #[cfg(target_os = "linux")]
    {
        let evt = evt_tx.clone();
        let tx = raw_tx.clone();
        std::thread::Builder::new()
            .name("sdw-input-scan".into())
            .spawn(move || {
                let mut last: Option<Vec<String>> = None;
                loop {
                    std::thread::sleep(Duration::from_secs(3));
                    let (list, pad, _errors) = scan_into(&tx, &mut scan);
                    if last.as_ref() != Some(&list) {
                        let _ = evt.send(DeckEvent::Devices(list.clone(), pad));
                        last = Some(list);
                    }
                }
            })
            .ok();
    }

    InputHandle {
        cmd_tx,
        evt_rx,
        devices,
        errors,
        has_gamepad,
    }
}

/// What the rescan thread remembers: which event nodes already have a reader.
#[cfg(target_os = "linux")]
#[derive(Default)]
struct ScanState {
    seen: HashSet<std::path::PathBuf>,
    touch_taken: bool,
    motion_taken: bool,
}

#[cfg(target_os = "linux")]
fn scan_into(raw_tx: &Sender<Raw>, st: &mut ScanState) -> (Vec<String>, bool, Vec<String>) {
    let mut devices = Vec::new();
    let mut errors = Vec::new();
    let mut has_gamepad = false;
    let mut current = HashSet::new();
    for (path, dev) in evdev::enumerate() {
        current.insert(path.clone());
        let name = dev.name().unwrap_or("input").to_string();
        let keys = dev.supported_keys();
        let axes = dev.supported_absolute_axes();
        let has = |k: u16| keys.map(|s| s.contains(evdev::KeyCode(k))).unwrap_or(false);
        let has_ax = |a: u16| {
            axes.map(|s| s.contains(evdev::AbsoluteAxisCode(a)))
                .unwrap_or(false)
        };

        let is_touch = has_ax(ABS_MT_POSITION_X) || (has(BTN_TOUCH) && has_ax(ABS_X));
        let is_gamepad = has(BTN_SOUTH) || has(BTN_EAST) || has(BTN_MODE);
        let lower = name.to_lowercase();
        let is_motion = !is_gamepad
            && !is_touch
            && (lower.contains("motion") || lower.contains("gyro") || lower.contains("accel"));

        let role = if is_touch && !st.touch_taken {
            st.touch_taken = true;
            Role::Touch
        } else if is_gamepad {
            has_gamepad = true;
            Role::Gamepad
        } else if is_motion && !st.motion_taken {
            st.motion_taken = true;
            Role::Motion
        } else {
            Role::Keys
        };

        // ignore devices that cannot produce anything we understand
        if role == Role::Keys && !keys.map(|k| !k.iter().next().is_none()).unwrap_or(false) {
            continue;
        }

        devices.push(format!("{} · {}", name, path.display()));
        if st.seen.contains(&path) {
            continue;
        }
        st.seen.insert(path.clone());
        let label = name.clone();
        let tx = raw_tx.clone();
        std::thread::Builder::new()
            .name(format!("sdw-input-{}", path.display()))
            .spawn(move || {
                if let Err(e) = read_device(dev, role, label, tx) {
                    // device removal or permission loss: the app keeps running
                    let _ = e;
                }
            })
            .ok();
    }
    st.seen.retain(|p| current.contains(p));
    if devices.is_empty() {
        errors.push(ERROR_NO_DEVICES.to_string());
    }
    (devices, has_gamepad, errors)
}

#[cfg(target_os = "linux")]
fn read_device(
    mut dev: evdev::Device,
    role: Role,
    device: String,
    tx: Sender<Raw>,
) -> Result<(), String> {
    let mut ranges = std::collections::HashMap::<u16, (i32, i32)>::new();
    if let Ok(info) = dev.get_absinfo() {
        for (code, abs) in info {
            ranges.insert(code.0, (abs.minimum(), abs.maximum()));
        }
    }
    let norm = |code: u16, v: i32| -> f32 {
        let Some((min, max)) = ranges.get(&code).copied() else {
            return v as f32;
        };
        if max <= min {
            return 0.0;
        }
        (v - min) as f32 / (max - min) as f32
    };
    let norm_signed = |code: u16, v: i32| -> f32 {
        let Some((min, max)) = ranges.get(&code).copied() else {
            return (v as f32 / 32767.0).clamp(-1.0, 1.0);
        };
        if max <= min {
            return 0.0;
        }
        let mid = (max + min) as f32 / 2.0;
        let half = (max - min) as f32 / 2.0;
        ((v as f32 - mid) / half).clamp(-1.0, 1.0)
    };

    let is_mt = ranges.contains_key(&ABS_MT_POSITION_X);
    let mut slot: u8 = 0;
    loop {
        let events = match dev.fetch_events() {
            Ok(e) => e,
            Err(e) => {
                let _ = tx.send(Raw::Key {
                    role: Role::Keys,
                    code: 0,
                    value: -1,
                    device: format!("{device}: {e}"),
                });
                return Err(e.to_string());
            }
        };
        for ev in events {
            let code = ev.code();
            let value = ev.value();
            match ev.event_type().0 {
                EV_KEY => {
                    let pressed = value != 0;
                    match role {
                        Role::Touch => {
                            if code == BTN_TOUCH {
                                let _ = tx.send(Raw::TouchDown(pressed));
                            } else {
                                let _ = tx.send(Raw::Key {
                                    role,
                                    code,
                                    value,
                                    device: device.clone(),
                                });
                            }
                        }
                        _ => {
                            let _ = tx.send(Raw::Key {
                                role,
                                code,
                                value,
                                device: device.clone(),
                            });
                        }
                    }
                }
                EV_ABS => match role {
                    Role::Touch => match code {
                        ABS_MT_SLOT => slot = value.clamp(0, 9) as u8,
                        ABS_MT_TRACKING_ID => {
                            let _ = tx.send(Raw::Touch {
                                slot,
                                x: None,
                                y: None,
                                tracking: Some(value),
                            });
                        }
                        ABS_MT_POSITION_X => {
                            let _ = tx.send(Raw::Touch {
                                slot,
                                x: Some(norm(code, value)),
                                y: None,
                                tracking: None,
                            });
                        }
                        ABS_MT_POSITION_Y => {
                            let _ = tx.send(Raw::Touch {
                                slot,
                                x: None,
                                y: Some(norm(code, value)),
                                tracking: None,
                            });
                        }
                        ABS_X | ABS_Y => {
                            let _ = tx.send(Raw::Touch {
                                slot: 0,
                                x: (code == ABS_X).then(|| norm(code, value)),
                                y: (code == ABS_Y).then(|| norm(code, value)),
                                tracking: None,
                            });
                        }
                        _ => {}
                    },
                    Role::Gamepad => {
                        let v = match code {
                            ABS_X | ABS_Y | ABS_RX | ABS_RY => norm_signed(code, value),
                            ABS_HAT0X | ABS_HAT0Y => value as f32,
                            ABS_Z | ABS_RZ | ABS_GAS | ABS_BRAKE => norm(code, value),
                            _ => continue,
                        };
                        let _ = tx.send(Raw::Axis {
                            role,
                            code,
                            value: v,
                        });
                    }
                    Role::Motion => {
                        let _ = tx.send(Raw::Axis {
                            role,
                            code,
                            value: value as f32,
                        });
                    }
                    Role::Keys => {}
                },
                _ => {}
            }
        }
        if !is_mt && role == Role::Touch {
            // single-touch touchpads send BTN_TOUCH + ABS_X/Y, handled above
        }
    }
}

struct SlotState {
    active: bool,
    idx: Option<usize>,
}

struct Tracker {
    slots: [SlotState; 10],
    active: usize,
    max_fingers: usize,
    started: Option<Instant>,
    start: Vec<(f32, f32)>,
    last: Vec<(f32, f32)>,
    single_down: bool,
    /// Waiting for the first ABS_X/Y after BTN_TOUCH: the press position must
    /// not be taken from the previous touch, or every swipe looks the same.
    single_await: bool,
    single_started: Option<Instant>,
    single_start: (f32, f32),
    single_last: (f32, f32),
}

impl Tracker {
    fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| SlotState {
                active: false,
                idx: None,
            }),
            active: 0,
            max_fingers: 0,
            started: None,
            start: Vec::new(),
            last: Vec::new(),
            single_down: false,
            single_await: false,
            single_started: None,
            single_start: (0.0, 0.0),
            single_last: (0.0, 0.0),
        }
    }

    fn touch(
        &mut self,
        slot: u8,
        x: Option<f32>,
        y: Option<f32>,
        tracking: Option<i32>,
        now: Instant,
    ) {
        let i = slot as usize % 10;
        if let Some(id) = tracking {
            if id < 0 {
                if self.slots[i].active {
                    self.slots[i].active = false;
                    self.slots[i].idx = None;
                    self.active = self.active.saturating_sub(1);
                }
            } else if !self.slots[i].active {
                self.slots[i].active = true;
                self.active += 1;
                self.max_fingers = self.max_fingers.max(self.active);
                if self.started.is_none() {
                    self.started = Some(now);
                }
            }
            return;
        }
        if !self.slots[i].active {
            return;
        }
        let (Some(lx), Some(ly)) = (x, y) else {
            // one axis at a time
            return;
        };
        self.update_pos(i, lx, ly);
    }

    fn axis(&mut self, slot: u8, x: Option<f32>, y: Option<f32>, now: Instant) {
        let i = slot as usize % 10;
        if !self.slots[i].active {
            // single touch devices have no tracking id: start on first position while down
            if !self.single_down {
                return;
            }
            self.slots[i].active = true;
            self.active = 1;
            self.max_fingers = 1;
            if self.started.is_none() {
                self.started = Some(now);
            }
        }
        let (cx, cy) = match (x, y) {
            (Some(x), Some(y)) => (x, y),
            (Some(x), None) => (x, self.last.first().map(|p| p.1).unwrap_or(0.5)),
            (None, Some(y)) => (self.last.first().map(|p| p.0).unwrap_or(0.5), y),
            (None, None) => return,
        };
        self.update_pos(i, cx, cy);
    }

    fn update_pos(&mut self, i: usize, x: f32, y: f32) {
        match self.slots[i].idx {
            Some(idx) => {
                if let Some(p) = self.last.get_mut(idx) {
                    p.0 = x;
                    p.1 = y;
                }
            }
            None => {
                let idx = self.start.len();
                self.slots[i].idx = Some(idx);
                self.start.push((x, y));
                self.last.push((x, y));
                self.max_fingers = self.max_fingers.max(self.start.len());
            }
        }
    }

    fn down(&mut self, down: bool, now: Instant) {
        if down && !self.single_down {
            self.single_down = true;
            self.single_await = true;
            self.single_started = Some(now);
            self.start.clear();
            self.last.clear();
            self.active = 0;
            self.max_fingers = 1;
            self.started = Some(now);
            for s in &mut self.slots {
                s.active = false;
                s.idx = None;
            }
        } else if !down && self.single_down {
            self.single_down = false;
            self.single_await = false;
            if self.slots[0].idx.is_none() {
                self.start.push(self.single_start);
                self.last.push(self.single_last);
            }
            self.finish(now);
        }
    }

    fn single_pos(&mut self, x: Option<f32>, y: Option<f32>) {
        if let Some(x) = x {
            self.single_last.0 = x;
        }
        if let Some(y) = y {
            self.single_last.1 = y;
        }
        if self.single_down {
            if self.single_await {
                // First coordinates of this touch: this is where it started.
                self.single_start = self.single_last;
                self.single_await = false;
            }
            self.slots[0].active = true;
            self.update_pos(0, self.single_last.0, self.single_last.1);
        }
    }

    fn poll(&mut self, now: Instant) -> Option<GestureSpec> {
        if self.active == 0 && !self.single_down && self.started.is_some() {
            return self.finish(now);
        }
        None
    }

    fn finish(&mut self, now: Instant) -> Option<GestureSpec> {
        let started = self.started.take()?;
        let fingers = self.max_fingers.clamp(1, 4) as u8;
        let dur = now.saturating_duration_since(started);
        let start = centroid(&self.start);
        let end = centroid(&self.last);
        let pinch = if self.start.len() >= 2 {
            Some((
                distance(self.start[0], self.start[1]),
                distance(self.last[0], self.last[1]),
            ))
        } else {
            None
        };
        let result = gesture_from(fingers, dur, start, end, pinch);
        self.start.clear();
        self.last.clear();
        self.active = 0;
        self.max_fingers = 0;
        for s in &mut self.slots {
            s.active = false;
            s.idx = None;
        }
        result
    }
}

/// Build a gesture from a completed touch/trackpad movement. Coordinates are
/// normalised to 0..1 so the same thresholds work on any panel size.
pub(crate) fn gesture_from(
    fingers: u8,
    dur: Duration,
    start: (f32, f32),
    end: (f32, f32),
    pinch: Option<(f32, f32)>,
) -> Option<GestureSpec> {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let dist = (dx * dx + dy * dy).sqrt();
    if let Some((d0, d1)) = pinch {
        if d0 > 0.01 && (d1 - d0).abs() / d0 > 0.30 && (d1 - d0).abs() > 0.05 {
            return Some(GestureSpec {
                kind: if d1 > d0 {
                    GestureKind::PinchOut
                } else {
                    GestureKind::PinchIn
                },
                fingers,
                dir: None,
            });
        }
    }
    swipe_or_tap(fingers, dur, dist, dx, dy)
}

fn centroid(points: &[(f32, f32)]) -> (f32, f32) {
    if points.is_empty() {
        return (0.0, 0.0);
    }
    let (mut x, mut y) = (0.0, 0.0);
    for (px, py) in points {
        x += px;
        y += py;
    }
    (x / points.len() as f32, y / points.len() as f32)
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    (dx * dx + dy * dy).sqrt()
}

fn swipe_or_tap(fingers: u8, dur: Duration, dist: f32, dx: f32, dy: f32) -> Option<GestureSpec> {
    if dist > 0.055 {
        let dir = if dx.abs() > dy.abs() {
            if dx > 0.0 {
                SwipeDir::Right
            } else {
                SwipeDir::Left
            }
        } else if dy > 0.0 {
            SwipeDir::Down
        } else {
            SwipeDir::Up
        };
        return Some(GestureSpec {
            kind: GestureKind::Swipe,
            fingers,
            dir: Some(dir),
        });
    }
    if dur > Duration::from_millis(620) {
        return Some(GestureSpec {
            kind: GestureKind::LongPress,
            fingers,
            dir: None,
        });
    }
    Some(GestureSpec {
        kind: GestureKind::Tap,
        fingers,
        dir: None,
    })
}

fn coordinate(raw_rx: Receiver<Raw>, cmd_rx: Receiver<InputCmd>, evt_tx: Sender<DeckEvent>) {
    let socket = UdpSocket::bind("0.0.0.0:0").ok();
    let mut target: Option<UdpTarget> = None;
    let mut enabled = false;
    let mut state = GamepadState::default();
    let mut gyro = [0i16; 3];
    let mut accel = [0i16; 3];
    let mut tracker = Tracker::new();
    let mut seq = 0u32;
    let mut last_udp = Instant::now() - Duration::from_secs(1);
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut last_sent = GamepadState::default();
    let mut pressed: HashSet<u16> = HashSet::new();

    loop {
        let mut got = false;
        match raw_rx.recv_timeout(Duration::from_millis(6)) {
            Ok(first) => {
                handle_raw(
                    first,
                    &mut state,
                    &mut gyro,
                    &mut accel,
                    &mut tracker,
                    &evt_tx,
                    &mut pressed,
                );
                got = true;
                for _ in 0..128 {
                    match raw_rx.try_recv() {
                        Ok(raw) => handle_raw(
                            raw,
                            &mut state,
                            &mut gyro,
                            &mut accel,
                            &mut tracker,
                            &evt_tx,
                            &mut pressed,
                        ),
                        Err(_) => break,
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                InputCmd::Target(t) => {
                    target = t;
                    last_udp = Instant::now() - Duration::from_secs(1);
                }
                InputCmd::Enabled(on) => {
                    enabled = on;
                    last_udp = Instant::now() - Duration::from_secs(1);
                }
            }
        }

        let now = Instant::now();
        state.gyro = gyro;
        state.accel = accel;

        if enabled {
            if let (Some(t), Some(sock)) = (&target, &socket) {
                if last_udp.elapsed() >= Duration::from_millis(16) {
                    seq = seq.wrapping_add(1);
                    let packet = udp::PadPacket {
                        pad_id: t.pad_id,
                        seq,
                        tag: [0; 8],
                        state: state.clone(),
                    };
                    let bytes = packet.encode(&t.secret);
                    let _ = sock.send_to(&bytes, t.addr);
                    last_udp = now;
                    last_sent = state.clone();
                }
            }
        }

        if got || state != last_sent || last_emit.elapsed() >= Duration::from_millis(50) {
            if last_emit.elapsed() >= Duration::from_millis(16) || state != last_sent {
                let _ = evt_tx.send(DeckEvent::Pad(state.clone()));
                last_emit = now;
                last_sent = state.clone();
            }
        }

        if let Some(gesture) = tracker.poll(now) {
            let _ = evt_tx.send(DeckEvent::Gesture(gesture));
        }
    }
}

fn handle_raw(
    raw: Raw,
    state: &mut GamepadState,
    gyro: &mut [i16; 3],
    accel: &mut [i16; 3],
    tracker: &mut Tracker,
    evt_tx: &Sender<DeckEvent>,
    pressed: &mut HashSet<u16>,
) {
    match raw {
        Raw::Key {
            role,
            code,
            value,
            device,
        } => {
            let down = value != 0;
            match role {
                Role::Gamepad => {
                    if let Some(bit) = gamepad_bit(code) {
                        state.set(bit, down);
                    }
                    match code {
                        BTN_TL2 => state.lt = if down { 1.0 } else { 0.0 },
                        BTN_TR2 => state.rt = if down { 1.0 } else { 0.0 },
                        _ => {}
                    }
                    if code != 0 {
                        let _ = evt_tx.send(DeckEvent::Button {
                            code,
                            label: key_name(code),
                            device: device.clone(),
                            pressed: down,
                        });
                    }
                }
                Role::Touch => {
                    if down {
                        pressed.insert(code);
                    } else {
                        pressed.remove(&code);
                    }
                    let _ = evt_tx.send(DeckEvent::Button {
                        code,
                        label: key_name(code),
                        device,
                        pressed: down,
                    });
                }
                Role::Keys => {
                    if code == 0 {
                        let _ = evt_tx.send(DeckEvent::Error(device));
                        return;
                    }
                    if down {
                        pressed.insert(code);
                    } else {
                        pressed.remove(&code);
                    }
                    let _ = evt_tx.send(DeckEvent::Button {
                        code,
                        label: key_name(code),
                        device,
                        pressed: down,
                    });
                }
                Role::Motion => {}
            }
        }
        Raw::Axis { role, code, value } => match role {
            Role::Gamepad => match code {
                ABS_X => state.lx = value,
                ABS_Y => state.ly = -value,
                ABS_RX => state.rx = value,
                ABS_RY => state.ry = -value,
                ABS_Z | ABS_GAS => state.lt = value,
                ABS_RZ | ABS_BRAKE => state.rt = value,
                ABS_HAT0X => {
                    state.set(udp::BTN_DPAD_LEFT, value < -0.5);
                    state.set(udp::BTN_DPAD_RIGHT, value > 0.5);
                }
                ABS_HAT0Y => {
                    state.set(udp::BTN_DPAD_UP, value < -0.5);
                    state.set(udp::BTN_DPAD_DOWN, value > 0.5);
                }
                _ => {}
            },
            Role::Motion => match code {
                ABS_X => accel[0] = value as i16,
                ABS_Y => accel[1] = value as i16,
                ABS_Z => accel[2] = value as i16,
                ABS_RX => gyro[0] = value as i16,
                ABS_RY => gyro[1] = value as i16,
                ABS_RZ => gyro[2] = value as i16,
                _ => {}
            },
            _ => {}
        },
        Raw::Touch {
            slot,
            x,
            y,
            tracking,
        } => {
            let now = Instant::now();
            if tracking.is_some() {
                tracker.touch(slot, x, y, tracking, now);
            } else if x.is_some() || y.is_some() {
                tracker.axis(slot, x, y, now);
                if tracker.slots[slot as usize % 10].active {
                    tracker.single_pos(x, y);
                }
            }
        }
        Raw::TouchDown(down) => tracker.down(down, Instant::now()),
    }
}

fn gamepad_bit(code: u16) -> Option<u32> {
    Some(match code {
        BTN_SOUTH => udp::BTN_A,
        BTN_EAST => udp::BTN_B,
        BTN_NORTH => udp::BTN_Y,
        BTN_WEST => udp::BTN_X,
        BTN_TL => udp::BTN_LB,
        BTN_TR => udp::BTN_RB,
        BTN_SELECT => udp::BTN_BACK,
        BTN_START => udp::BTN_START,
        BTN_MODE => udp::BTN_GUIDE,
        BTN_THUMBL => udp::BTN_L3,
        BTN_THUMBR => udp::BTN_R3,
        BTN_DPAD_UP => udp::BTN_DPAD_UP,
        BTN_DPAD_DOWN => udp::BTN_DPAD_DOWN,
        BTN_DPAD_LEFT => udp::BTN_DPAD_LEFT,
        BTN_DPAD_RIGHT => udp::BTN_DPAD_RIGHT,
        _ => return None,
    })
}

pub fn key_name(code: u16) -> String {
    let name = match code {
        BTN_SOUTH => "A",
        BTN_EAST => "B",
        BTN_NORTH => "Y",
        BTN_WEST => "X",
        BTN_TL => "LB",
        BTN_TR => "RB",
        BTN_TL2 => "LT",
        BTN_TR2 => "RT",
        BTN_SELECT => "View",
        BTN_START => "Menu",
        BTN_MODE => "Steam",
        BTN_THUMBL => "L3",
        BTN_THUMBR => "R3",
        BTN_DPAD_UP => "D-pad ↑",
        BTN_DPAD_DOWN => "D-pad ↓",
        BTN_DPAD_LEFT => "D-pad ←",
        BTN_DPAD_RIGHT => "D-pad →",
        BTN_L4 => "L4",
        BTN_L5 => "L5",
        BTN_R4 => "R4",
        BTN_R5 => "R5",
        BTN_TOUCH => "Touch",
        0x110 => "Left click",
        0x111 => "Right click",
        0x113 => "Mute",
        0x114 => "Volume −",
        0x115 => "Volume +",
        0x116 => "Power",
        1 => "Esc",
        14 => "Backspace",
        15 => "Tab",
        28 => "Enter",
        57 => "Space",
        103 => "↑",
        105 => "←",
        106 => "→",
        108 => "↓",
        125 => "Super",
        c => {
            return format!("KEY_{c}");
        }
    };
    name.to_string()
}
