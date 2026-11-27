//! reads the steam deck's own controls through evdev.
//!
//! one thread per device, one coordinator that turns raw events into a gamepad state,
//! gestures and button events. the coordinator also owns the udp socket that feeds
//! the virtual pad on the computer.

use sdw_core::model::GestureSpec;
use sdw_core::model::{GestureKind, PadSide, PadZone, SwipeDir};
use sdw_core::udp::{self, GamepadState};
use sdw_core::{t, Lang};
use std::collections::HashSet;
#[cfg(target_os = "linux")]
use std::io::Read;
use std::net::{SocketAddr, UdpSocket};
#[cfg(target_os = "linux")]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
#[cfg(target_os = "linux")]
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const EV_KEY: u16 = 1;
pub const EV_ABS: u16 = 3;
pub const EV_SYN: u16 = 0;

pub const BTN_SOUTH: u16 = 0x130;
pub const BTN_EAST: u16 = 0x131;
/// Kernel alias of `BTN_X`: both hid-steam and xpad send the physical X here.
pub const BTN_NORTH: u16 = 0x133;
/// Kernel alias of `BTN_Y`: both hid-steam and xpad send the physical Y here.
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
/// Touchpad clicks: the left pad and the right pad respectively.
pub const BTN_THUMB: u16 = 0x121;
pub const BTN_THUMB2: u16 = 0x122;
pub const BTN_DPAD_UP: u16 = 0x220;
pub const BTN_DPAD_DOWN: u16 = 0x221;
pub const BTN_DPAD_LEFT: u16 = 0x222;
pub const BTN_DPAD_RIGHT: u16 = 0x223;
/// Steam Deck back buttons. hid-steam reports them as `BTN_TRIGGER_HAPPY1..4`
/// in the order left top, right top, left bottom, right bottom; the Deck calls
/// the upper pair L4/R4 and the lower pair L5/R5.
pub const BTN_L4: u16 = 0x2c0;
pub const BTN_R4: u16 = 0x2c1;
pub const BTN_L5: u16 = 0x2c2;
pub const BTN_R5: u16 = 0x2c3;
/// The quick access button, `BTN_BASE` in the kernel.
pub const BTN_QAM: u16 = 0x126;
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
const ABS_HAT1X: u16 = 18;
const ABS_HAT1Y: u16 = 19;
const ABS_HAT2X: u16 = 20;
const ABS_HAT2Y: u16 = 21;

/// Valve's USB vendor id; Steam Input's emulated Xbox pad uses product 0x11ff.
const STEAM_VENDOR: u16 = 0x28de;
const STEAM_VIRTUAL_PAD: u16 = 0x11ff;
const STEAM_DECK_PRODUCT: u16 = 0x1205;

/// The Deck sends its whole state as report 9 of the raw HID stream. Steam
/// Input's userspace driver takes the hidraw node, and the kernel hid-steam
/// driver then removes its evdev device, so in Game Mode these reports are the
/// only place where the touchpads exist.
#[cfg(target_os = "linux")]
const DECK_REPORT_ID: u8 = 9;
#[cfg(target_os = "linux")]
const DECK_REPORT_SIZE: usize = 64;
#[cfg(target_os = "linux")]
const O_NONBLOCK: i32 = 0o4000;
#[cfg(target_os = "linux")]
const DECK_BUTTON_COUNT: usize = 24;

/// One bit of the Deck report and the evdev code it stands for. The order is
/// only used to index `DeckReport::buttons`.
#[cfg(target_os = "linux")]
const DECK_BUTTONS: [(usize, u8, u16); DECK_BUTTON_COUNT] = [
    (8, 0, BTN_TR2),
    (8, 1, BTN_TL2),
    (8, 2, BTN_TR),
    (8, 3, BTN_TL),
    // The kernel names the face buttons by the SNES diamond: these are the
    // physical Y, B, X and A.
    (8, 4, BTN_WEST),
    (8, 5, BTN_EAST),
    (8, 6, BTN_NORTH),
    (8, 7, BTN_SOUTH),
    (9, 0, BTN_DPAD_UP),
    (9, 1, BTN_DPAD_RIGHT),
    (9, 2, BTN_DPAD_LEFT),
    (9, 3, BTN_DPAD_DOWN),
    (9, 4, BTN_SELECT),
    (9, 5, BTN_MODE),
    (9, 6, BTN_START),
    (9, 7, BTN_L5),
    (10, 0, BTN_R5),
    (10, 1, BTN_THUMB),
    (10, 2, BTN_THUMB2),
    (10, 6, BTN_THUMBL),
    (11, 2, BTN_THUMBR),
    (13, 1, BTN_L4),
    (13, 2, BTN_R4),
    (14, 2, BTN_QAM),
];

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
    /// One raw touchpad sample. `touched` is what the tracker decided (a
    /// finger is on the pad); `bit` is the raw hardware touch bit and
    /// `pressure` the pad's pressure reading, kept for the input card so a
    /// light touch can be diagnosed on the Deck itself.
    PadTouch {
        side: PadSide,
        x: f32,
        y: f32,
        touched: bool,
        bit: bool,
        pressure: u16,
    },
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
    /// A Steam Deck pad node: the sticks and buttons of a normal gamepad plus
    /// both touchpads (HAT0, HAT1) and the analog triggers (HAT2).
    SteamDeck,
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
    /// One touchpad sample from either the hidraw report or evdev. `touch` is
    /// the hardware touch bit: the kernel and SDL both treat it as the only
    /// authority on whether a coordinate is a finger, and `false` means the
    /// axes were left behind by the last touch. `pressure` is reported for
    /// diagnostics only (evdev has none, so it is 0 there).
    Pad {
        side: PadSide,
        x: f32,
        y: f32,
        touch: bool,
        pressure: u16,
    },
}

/// Stable codes for the two failures the UI can translate itself.
pub const ERROR_LINUX_ONLY: &str = "linux-only";
pub const ERROR_NO_DEVICES: &str = "no-devices";
pub const ERROR_DECK_HIDRAW: &str = "deck-hidraw";

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
        ERROR_DECK_HIDRAW => t!(
            lang,
            "тачпады недоступны: steam input держит контроллер, а сырой hidraw-узел не читается. проверьте доступ к /dev/hidraw* (группа input).",
            "touchpads are unavailable: steam input holds the controller and its raw HID node cannot be read. check access to /dev/hidraw* (input group)."
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
    let (devices, has_gamepad, errors): (Vec<String>, bool, Vec<String>) =
        (Vec::new(), false, vec![ERROR_LINUX_ONLY.into()]);

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
    hidraw: Option<HidrawReader>,
}

/// A reader of the Deck's raw report. Steam keeps the hidraw node open while
/// it runs, so the kernel's evdev device is gone; the reader stops as soon as
/// Steam Input's virtual pad disappears and evdev can be used again.
#[cfg(target_os = "linux")]
struct HidrawReader {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
}

/// The Deck node is the one with HAT1 (its pads) and gamepad keys.
#[cfg(target_os = "linux")]
fn is_deck_node(dev: &evdev::Device) -> bool {
    let gamepad = dev
        .supported_keys()
        .map(|keys| {
            keys.contains(evdev::KeyCode(BTN_SOUTH))
                || keys.contains(evdev::KeyCode(BTN_EAST))
                || keys.contains(evdev::KeyCode(BTN_MODE))
        })
        .unwrap_or(false);
    let pads = dev
        .supported_absolute_axes()
        .map(|axes| axes.contains(evdev::AbsoluteAxisCode(ABS_HAT1X)))
        .unwrap_or(false);
    gamepad && pads
}

/// Steam Input's emulated pad repeats the Deck's buttons and sticks on a
/// uinput device. It has no touchpad axes, so when the real Deck node is
/// present it is pure duplication: ignore it.
fn is_steam_virtual_pad(vendor: u16, product: u16) -> bool {
    vendor == STEAM_VENDOR && product == STEAM_VIRTUAL_PAD
}

#[cfg(target_os = "linux")]
fn scan_into(raw_tx: &Sender<Raw>, st: &mut ScanState) -> (Vec<String>, bool, Vec<String>) {
    let mut devices = Vec::new();
    let mut errors = Vec::new();
    let mut has_gamepad = false;
    let mut current = HashSet::new();

    // Enumerate once: opening every event node twice was wasteful and made the
    // virtual-pad decision race with the loop below.
    let all: Vec<(PathBuf, evdev::Device)> = evdev::enumerate().collect();
    let virtual_pad_present = all.iter().any(|(_, dev)| {
        let id = dev.input_id();
        is_steam_virtual_pad(id.vendor(), id.product())
    });
    let deck_evdev_present = all.iter().any(|(_, dev)| is_deck_node(dev));

    // Steam Input's userspace driver takes the Deck's hidraw node and the
    // kernel hid-steam driver removes its evdev device, so in Game Mode the
    // pads exist only in the raw report. Read it while the virtual pad says
    // Steam Input is active, and give the hidraw node back when it is not.
    let hidraw_path = if virtual_pad_present {
        steam_deck_hidraw()
    } else {
        None
    };
    let use_hidraw = hidraw_path.is_some();

    let mut stop_reader = false;
    if let Some(reader) = &st.hidraw {
        let dead = !reader.alive.load(Ordering::Relaxed);
        let wrong = hidraw_path.as_ref() != Some(&reader.path);
        if dead || wrong {
            stop_reader = true;
        }
    }
    if stop_reader {
        if let Some(reader) = st.hidraw.take() {
            reader.stop.store(true, Ordering::Relaxed);
        }
    }
    if st.hidraw.is_none() {
        if let Some(path) = hidraw_path.clone() {
            let stop = Arc::new(AtomicBool::new(false));
            let alive = Arc::new(AtomicBool::new(true));
            let tx = raw_tx.clone();
            let thread_stop = stop.clone();
            let thread_alive = alive.clone();
            let thread_path = path.clone();
            std::thread::Builder::new()
                .name(format!("sdw-input-hidraw-{}", path.display()))
                .spawn(move || {
                    read_deck_hidraw(
                        thread_path,
                        "Steam Deck".into(),
                        tx,
                        thread_stop,
                        thread_alive,
                    )
                })
                .ok();
            st.hidraw = Some(HidrawReader { path, stop, alive });
        }
    }
    if let Some(path) = &hidraw_path {
        devices.push(format!("Steam Deck (hidraw) · {}", path.display()));
        has_gamepad = true;
    } else if virtual_pad_present && !deck_evdev_present {
        errors.push(ERROR_DECK_HIDRAW.to_string());
    }

    for (path, dev) in all {
        let id = dev.input_id();
        // With the raw report we own the whole Deck state: skip both the real
        // node (it may be a dead leftover) and Steam's duplicate virtual pad.
        let skip = if use_hidraw {
            is_deck_node(&dev) || is_steam_virtual_pad(id.vendor(), id.product())
        } else {
            deck_evdev_present && is_steam_virtual_pad(id.vendor(), id.product())
        };
        // Keep the path in `current` even when skipping, so a reader that was
        // already spawned before the Deck node appeared is not spawned twice.
        current.insert(path.clone());
        if skip {
            continue;
        }
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
            // The Steam Deck puts its two touchpads on HAT0/HAT1 and its analog
            // triggers on HAT2 of the same node as the sticks. A generic pad
            // only has HAT0 (the d-pad), so the presence of HAT1 says "Deck".
            if has_ax(ABS_HAT1X) {
                Role::SteamDeck
            } else {
                Role::Gamepad
            }
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

/// The Deck's whole state, parsed from one raw report.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, Default)]
struct DeckReport {
    buttons: [bool; DECK_BUTTON_COUNT],
    lx: f32,
    ly: f32,
    rx: f32,
    ry: f32,
    lt: f32,
    rt: f32,
    lpad: (f32, f32),
    rpad: (f32, f32),
    /// The hardware touch bits. Unlike the zeroed axes they survive a finger
    /// resting exactly on the centre of a pad.
    lpad_touched: bool,
    rpad_touched: bool,
    /// Capacitive/pressure readings, bytes 56..60. A finger on the pad is
    /// above the rest level; 0 means nothing is there.
    lpad_pressure: u16,
    rpad_pressure: u16,
    accel: [i16; 3],
    gyro: [i16; 3],
}

#[cfg(target_os = "linux")]
fn parse_deck_report(data: &[u8]) -> Option<DeckReport> {
    if data.len() < DECK_REPORT_SIZE || data[0] != 1 || data[1] != 0 || data[2] != DECK_REPORT_ID {
        return None;
    }
    let s16 = |i: usize| i16::from_le_bytes([data[i], data[i + 1]]);
    let u16v = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]);
    let norm = |v: i16| (v as f32 / 32767.0).clamp(-1.0, 1.0);
    let lpad_touched = data[10] & 0x08 != 0;
    let rpad_touched = data[10] & 0x10 != 0;

    let mut buttons = [false; DECK_BUTTON_COUNT];
    for (slot, (byte, bit, _)) in DECK_BUTTONS.iter().enumerate() {
        buttons[slot] = data[*byte] & (1u8 << *bit) != 0;
    }

    Some(DeckReport {
        buttons,
        lx: norm(s16(48)),
        // hid-steam reports the sticks' Y already negated; hand the coordinator
        // the same evdev-style value it would read from the node.
        ly: -norm(s16(50)),
        rx: norm(s16(52)),
        ry: -norm(s16(54)),
        lt: (u16v(44) as f32 / 32767.0).clamp(0.0, 1.0),
        rt: (u16v(46) as f32 / 32767.0).clamp(0.0, 1.0),
        // SDL negates the pad's Y: the hardware reports positive as "up",
        // while the rest of this code (and the screen) calls down positive.
        // Without this a swipe up would fire "down".
        lpad: (norm(s16(16)), -norm(s16(18))),
        rpad: (norm(s16(20)), -norm(s16(22))),
        lpad_touched,
        rpad_touched,
        lpad_pressure: u16v(56),
        rpad_pressure: u16v(58),
        accel: [s16(24), s16(28), -s16(26)],
        gyro: [s16(30), s16(34), -s16(32)],
    })
}

#[cfg(target_os = "linux")]
fn send_axis(tx: &Sender<Raw>, code: u16, value: f32, old: &mut f32) {
    if (value - *old).abs() > 0.0005 {
        *old = value;
        let _ = tx.send(Raw::Axis {
            role: Role::SteamDeck,
            code,
            value,
        });
    }
}

#[cfg(target_os = "linux")]
fn send_motion(tx: &Sender<Raw>, code: u16, value: i16, old: &mut i16) {
    if value != *old {
        *old = value;
        let _ = tx.send(Raw::Axis {
            role: Role::Motion,
            code,
            value: value as f32,
        });
    }
}

/// Turn one report into the same events hid-steam would have emitted. Axes go
/// first and buttons last, so a fully pressed trigger's button wins over its
/// slightly lower analog value.
#[cfg(target_os = "linux")]
fn emit_deck_report(report: &DeckReport, prev: &mut DeckReport, tx: &Sender<Raw>, device: &str) {
    // Every report carries both pads, with the hardware touch bit and the
    // position. The bit is the truth (`hid-steam` clears the axes when it is
    // gone), and the position is sent unchanged, so a dead-centre touch is a
    // real (0, 0) sample rather than a release. The pressure goes along so the
    // input card can show what the pad really reports.
    let _ = tx.send(Raw::Pad {
        side: PadSide::Left,
        x: report.lpad.0,
        y: report.lpad.1,
        touch: report.lpad_touched,
        pressure: report.lpad_pressure,
    });
    let _ = tx.send(Raw::Pad {
        side: PadSide::Right,
        x: report.rpad.0,
        y: report.rpad.1,
        touch: report.rpad_touched,
        pressure: report.rpad_pressure,
    });
    send_axis(tx, ABS_X, report.lx, &mut prev.lx);
    send_axis(tx, ABS_Y, report.ly, &mut prev.ly);
    send_axis(tx, ABS_RX, report.rx, &mut prev.rx);
    send_axis(tx, ABS_RY, report.ry, &mut prev.ry);
    send_axis(tx, ABS_HAT2Y, report.lt, &mut prev.lt);
    send_axis(tx, ABS_HAT2X, report.rt, &mut prev.rt);
    for (i, code) in [ABS_X, ABS_Y, ABS_Z].iter().enumerate() {
        send_motion(tx, *code, report.accel[i], &mut prev.accel[i]);
    }
    for (i, code) in [ABS_RX, ABS_RY, ABS_RZ].iter().enumerate() {
        send_motion(tx, *code, report.gyro[i], &mut prev.gyro[i]);
    }
    for (slot, (_, _, code)) in DECK_BUTTONS.iter().enumerate() {
        if report.buttons[slot] != prev.buttons[slot] {
            let _ = tx.send(Raw::Key {
                role: Role::SteamDeck,
                code: *code,
                value: i32::from(report.buttons[slot]),
                device: device.to_string(),
            });
            prev.buttons[slot] = report.buttons[slot];
        }
    }
}

#[cfg(target_os = "linux")]
fn read_deck_hidraw(
    path: PathBuf,
    device: String,
    tx: Sender<Raw>,
    stop: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
) {
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) => {
            let _ = tx.send(Raw::Key {
                role: Role::Keys,
                code: 0,
                value: -1,
                device: format!("{}: {error}", path.display()),
            });
            alive.store(false, Ordering::Relaxed);
            return;
        }
    };
    let mut buf = [0u8; DECK_REPORT_SIZE];
    let mut prev = DeckReport::default();
    // A node that exists but never yields report 9 is the failure the Deck
    // test needs to see: silence would look like a dead touchpad. The first
    // unexpected bytes are kept, so the format is visible in the error line.
    let mut unparsed = 0u32;
    let mut sample: Option<Vec<u8>> = None;
    while !stop.load(Ordering::Relaxed) {
        match file.read(&mut buf) {
            Ok(0) => std::thread::sleep(Duration::from_millis(4)),
            Ok(n) => {
                if let Some(report) = parse_deck_report(&buf[..n]) {
                    unparsed = 0;
                    sample = None;
                    emit_deck_report(&report, &mut prev, &tx, &device);
                } else {
                    unparsed = unparsed.saturating_add(1);
                    if sample.is_none() {
                        sample = Some(buf[..n].to_vec());
                    }
                    if unparsed == 50 {
                        let head = sample
                            .as_deref()
                            .unwrap_or(&[])
                            .iter()
                            .take(16)
                            .map(|b| format!("{b:02x}"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        let _ = tx.send(Raw::Key {
                            role: Role::Keys,
                            code: 0,
                            value: -1,
                            device: format!("{}: unexpected report {head}", path.display()),
                        });
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(4));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                let _ = tx.send(Raw::Key {
                    role: Role::Keys,
                    code: 0,
                    value: -1,
                    device: format!("{}: {error}", path.display()),
                });
                break;
            }
        }
    }
    alive.store(false, Ordering::Relaxed);
}

/// Find the hidraw node of the Deck's gamepad interface. The client HID device
/// Steam opens has no input device of its own; the emulated mouse and keyboard
/// interfaces do, so that is what tells them apart.
#[cfg(target_os = "linux")]
fn steam_deck_hidraw() -> Option<PathBuf> {
    let entries = std::fs::read_dir("/sys/class/hidraw").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let uevent = entry.path().join("device/uevent");
        let Ok(text) = std::fs::read_to_string(&uevent) else {
            continue;
        };
        if !hidraw_matches(&text) {
            continue;
        }
        let node = PathBuf::from("/dev").join(std::path::Path::new(&name));
        if !node.exists() {
            continue;
        }
        // Only the layered client device that Steam opens is the gamepad
        // stream. The emulated mouse and keyboard interfaces have an input
        // device of their own; opening one of those would read nothing.
        if !entry.path().join("device/input").exists() {
            return Some(node);
        }
    }
    None
}

/// The `HID_ID` line is `bus:vendor:product` in hex.
#[cfg(target_os = "linux")]
fn hidraw_matches(uevent: &str) -> bool {
    uevent.lines().any(|line| {
        let Some(rest) = line.strip_prefix("HID_ID=") else {
            return false;
        };
        let mut parts = rest.split(':');
        let _bus = parts.next();
        let vendor = parts.next().and_then(|v| u32::from_str_radix(v, 16).ok());
        let product = parts.next().and_then(|v| u32::from_str_radix(v, 16).ok());
        vendor == Some(u32::from(STEAM_VENDOR)) && product == Some(u32::from(STEAM_DECK_PRODUCT))
    })
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
    // evdev has no touch bit for the pads, so the last seen axes are sampled
    // at every SYN_REPORT and zero means "lifted". The raw hidraw path uses
    // the real touch bits instead.
    let mut lpad = (0.0f32, 0.0f32);
    let mut rpad = (0.0f32, 0.0f32);
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
                    Role::SteamDeck => match code {
                        // Both touchpads, raw around a centre. Keep the last
                        // value and publish it at the report boundary.
                        ABS_HAT0X => lpad.0 = norm_signed(code, value),
                        // hid-steam passes the pad's Y through unnegated, so
                        // it is flipped here for the same screen convention
                        // as the hidraw report.
                        ABS_HAT0Y => lpad.1 = -norm_signed(code, value),
                        ABS_HAT1X => rpad.0 = norm_signed(code, value),
                        ABS_HAT1Y => rpad.1 = -norm_signed(code, value),
                        _ => {
                            let v = match code {
                                ABS_X | ABS_Y | ABS_RX | ABS_RY => norm_signed(code, value),
                                // Analog triggers: HAT2Y is the left one,
                                // HAT2X the right one, 0..max.
                                ABS_HAT2Y | ABS_HAT2X => norm(code, value),
                                ABS_Z | ABS_RZ | ABS_GAS | ABS_BRAKE => norm(code, value),
                                _ => continue,
                            };
                            let _ = tx.send(Raw::Axis {
                                role,
                                code,
                                value: v,
                            });
                        }
                    },
                    Role::Motion => {
                        let _ = tx.send(Raw::Axis {
                            role,
                            code,
                            value: value as f32,
                        });
                    }
                    Role::Keys => {}
                },
                EV_SYN => {
                    // One report ends here. evdev has no touch bit, so the
                    // report boundary is the only place to tell "finger still
                    // down" from "finger just lifted".
                    if code == 0 && role == Role::SteamDeck {
                        let _ = tx.send(Raw::Pad {
                            side: PadSide::Left,
                            x: lpad.0,
                            y: lpad.1,
                            // evdev has no touch bit and no pressure: the
                            // coordinate itself is all there is.
                            touch: false,
                            pressure: 0,
                        });
                        let _ = tx.send(Raw::Pad {
                            side: PadSide::Right,
                            x: rpad.0,
                            y: rpad.1,
                            touch: false,
                            pressure: 0,
                        });
                    }
                }
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

/// One touchpad's state across the reports of a single touch.
#[derive(Default)]
struct PadState {
    active: bool,
    /// The pad click is a separate bind; it must not also produce the tap it
    /// would otherwise make on release.
    clicked: bool,
    click_seen: bool,
    /// The click already fired its part's bind, so a second sample and the
    /// release must not fire it again.
    click_sent: bool,
    /// A swipe or a long press already fired during this touch, so the
    /// release must not fire a second gesture.
    fired: bool,
    started: Option<Instant>,
    start: (f32, f32),
    last: (f32, f32),
    /// The part of the pad this touch is in, from where it started. A click
    /// resolves through it too, so the same press means one thing in each
    /// quarter of the pad.
    zone: Option<PadZone>,
    /// Farthest point from `start` and how far it is. Using the farthest point
    /// instead of the last one means a curved swipe or a swipe that comes back
    /// still counts.
    reach: f32,
    reach_at: (f32, f32),
    /// Total length of the path, so a circle that returns near the start is
    /// still movement rather than a tap.
    travel: f32,
    /// Whether this touch was reported by the hardware bit at least once.
    /// Such a touch ends with the bit, because SDL shows that the coordinates
    /// can outlive the finger. A touch the bit never mentioned is read from
    /// the coordinate instead, so a firmware that raises the bit only on a
    /// firm press cannot make light touches stop working.
    bit_this_touch: bool,
    /// Where the last touch ended. A leftover coordinate that never moves must
    /// not start a new touch, or a lifted finger would look like a press held
    /// forever; a real new touch begins once the coordinate leaves this point.
    release_at: Option<(f32, f32)>,
}

/// How far a finger must travel on a pad before it is a swipe, in the 0..1
/// space the gesture code speaks (a full pad is 1.0 across). The live
/// threshold is a little larger than the release one so a gesture fires once
/// the direction is clear, not on the first jittery sample.
const PAD_SWIPE: f32 = 0.055;
const PAD_SWIPE_LIVE: f32 = 0.08;
const PAD_TRAVEL_LIVE: f32 = 0.14;
const PAD_LONG: Duration = Duration::from_millis(620);
/// How far from the middle a coordinate must be to count as a finger on a pad
/// that sends no touch bit at all.
const PAD_OFF_CENTRE: f32 = 0.02;

/// Turns the two touchpads' samples into gestures, one per touch.
///
/// In Game Mode report 9 carries an explicit touch bit for each pad, so a
/// finger on the exact centre is a real touch and a release is a bit, not a
/// guess: the coordinates outlive the finger there, so only the bit can end a
/// touch. On the evdev node there is no such bit; hid-steam zeroes the axes
/// when the finger lifts, so there the middle is the release and an offset is
/// the finger.
///
/// A movement gesture does not wait for the release: the swipe fires as soon
/// as the finger has clearly travelled, and a long press fires while the
/// finger is still down. That way a pad gesture cannot be lost to a release
/// bit that never arrives, and the answer is felt under the finger. The tap
/// is the only gesture that needs the release, because it is defined by one.
struct PadTracker {
    left: PadState,
    right: PadState,
}

impl PadTracker {
    fn new() -> Self {
        Self {
            left: PadState::default(),
            right: PadState::default(),
        }
    }

    fn state(&mut self, side: PadSide) -> &mut PadState {
        match side {
            PadSide::Left => &mut self.left,
            PadSide::Right => &mut self.right,
        }
    }

    /// Whether a finger is currently on that pad, as the tracker understands
    /// it. The UI uses this, not the raw coordinate.
    fn is_touched(&self, side: PadSide) -> bool {
        match side {
            PadSide::Left => self.left.active,
            PadSide::Right => self.right.active,
        }
    }

    /// Remember a pad click. The click bind is the action; a short touch that
    /// happens to include a click must not fire a second, tap bind.
    fn click(&mut self, side: PadSide, down: bool) {
        let st = self.state(side);
        st.clicked = down;
        if down {
            st.click_seen = true;
        } else if !st.active {
            st.click_seen = false;
            st.click_sent = false;
        }
    }

    /// Feed one sample. Returns a gesture when this sample completes one:
    /// a swipe or a long press as soon as it is clear, a tap on release.
    ///
    /// `signal` is the hardware touch bit. Once a touch has been reported by
    /// the bit, only the bit can end it: SDL keeps the last coordinates across
    /// a release, so a nonzero position proves nothing there. A touch the bit
    /// never reported (evdev, or firmware that raises it late) is read from
    /// the coordinate: a clear offset from the middle is a finger, and the
    /// middle is the release.
    fn feed(
        &mut self,
        side: PadSide,
        x: f32,
        y: f32,
        signal: bool,
        now: Instant,
    ) -> Option<GestureSpec> {
        let st = self.state(side);
        // -1..1 around a centre -> the 0..1 the gesture code speaks.
        let point = (
            ((x + 1.0) * 0.5).clamp(0.0, 1.0),
            ((y + 1.0) * 0.5).clamp(0.0, 1.0),
        );
        let off_centre = distance(point, (0.5, 0.5)) > PAD_OFF_CENTRE;
        // A coordinate the firmware left behind after a release sits exactly
        // where the last touch ended, so it cannot start a new one.
        let away_from_release = st
            .release_at
            .map(|at| distance(at, point) > PAD_OFF_CENTRE)
            .unwrap_or(true);
        let touched = if signal {
            st.bit_this_touch = true;
            true
        } else if st.bit_this_touch {
            // This touch came from the bit and this sample has none: the
            // finger is up, whatever the axes still say.
            false
        } else if st.active {
            // A position-only touch continues while the coordinate stays off
            // the middle.
            off_centre
        } else {
            off_centre && away_from_release
        };
        if touched {
            if !st.active {
                st.active = true;
                st.fired = false;
                st.started = Some(now);
                st.start = point;
                st.last = point;
                st.reach = 0.0;
                st.reach_at = point;
                st.travel = 0.0;
                // The part is fixed by where the finger landed, not by where
                // it ends: dragging out of a quarter must not move the tap.
                st.zone = Some(PadZone::from_point(point.0, point.1));
                st.click_seen = st.clicked;
                st.click_sent = false;
            }
            // A hard press is a tap on the part it went down in, so pressing
            // an edge of the pad runs that edge's bind. It is resolved here,
            // on the pad sample, because the zone is only known once the
            // finger's position has arrived.
            if st.clicked && !st.click_sent {
                st.click_sent = true;
                // The press is the action: the same touch must not also turn
                // into a swipe or a tap under the finger.
                st.fired = true;
                return Some(pad_press(side, st.zone));
            }
            st.travel += distance(st.last, point);
            let reach = distance(st.start, point);
            if reach > st.reach {
                st.reach = reach;
                st.reach_at = point;
            }
            st.last = point;
            if st.fired {
                return None;
            }
            // The direction is taken from the farthest point, not the newest
            // one: a curve or a swipe back still has one clear direction.
            if st.reach >= PAD_SWIPE_LIVE || st.travel >= PAD_TRAVEL_LIVE {
                st.fired = true;
                return Some(pad_gesture(side, st, GestureKind::Swipe));
            }
            if let Some(started) = st.started {
                if now.saturating_duration_since(started) >= PAD_LONG {
                    st.fired = true;
                    let zone = st.zone;
                    return Some(GestureSpec {
                        kind: GestureKind::LongPress,
                        fingers: 1,
                        dir: None,
                        pad: Some(side),
                        zone,
                    });
                }
            }
            return None;
        }
        if !st.active {
            // The click may arrive with no usable position (the evdev node
            // zeroes the axes on a lift). It still has to run its bind, so it
            // goes to the whole pad rather than nowhere.
            if st.clicked && !st.click_sent {
                st.click_sent = true;
                return Some(pad_press(side, None));
            }
            return None;
        }
        st.active = false;
        st.release_at = Some(point);
        st.bit_this_touch = false;
        let started = st.started.take()?;
        let dur = now.saturating_duration_since(started);
        let fired = st.fired;
        let click_seen = st.click_seen;
        let click_sent = st.click_sent;
        st.fired = false;
        st.click_seen = false;
        st.click_sent = false;
        if fired {
            return None;
        }
        if click_seen && !click_sent {
            // The press and the lift landed in the same sample: the part is
            // still the action, and losing it would make a quick press dead.
            return Some(pad_press(side, st.zone));
        }
        let mut g = if st.reach > PAD_SWIPE || st.travel > PAD_TRAVEL_LIVE {
            pad_gesture(side, st, GestureKind::Swipe)
        } else if dur >= PAD_LONG {
            GestureSpec {
                kind: GestureKind::LongPress,
                fingers: 1,
                dir: None,
                pad: Some(side),
                zone: st.zone,
            }
        } else if click_seen {
            // The click was the action; the touch that carried it is not a
            // second, separate tap.
            return None;
        } else {
            GestureSpec {
                kind: GestureKind::Tap,
                fingers: 1,
                dir: None,
                pad: Some(side),
                zone: st.zone,
            }
        };
        if g.kind == GestureKind::Swipe && g.dir.is_none() {
            g.dir = Some(SwipeDir::Right);
        }
        Some(g)
    }
}

/// A pad press: the part the finger went down in, or the whole pad when no
/// position has arrived yet, so a press is never simply lost.
fn pad_press(side: PadSide, zone: Option<PadZone>) -> GestureSpec {
    match zone {
        Some(zone) => GestureSpec::pad_part(side, zone),
        None => GestureSpec {
            kind: GestureKind::Tap,
            fingers: 1,
            dir: None,
            pad: Some(side),
            zone: None,
        },
    }
}

/// A swipe in the direction of the farthest travelled point.
fn pad_gesture(side: PadSide, st: &PadState, kind: GestureKind) -> GestureSpec {
    let (dx, dy) = if st.reach > 0.02 {
        (st.reach_at.0 - st.start.0, st.reach_at.1 - st.start.1)
    } else {
        (st.last.0 - st.start.0, st.last.1 - st.start.1)
    };
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
    GestureSpec {
        kind,
        fingers: 1,
        dir: Some(dir),
        pad: Some(side),
        zone: None,
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
                pad: None,
                zone: None,
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
            pad: None,
            zone: None,
        });
    }
    if dur > Duration::from_millis(620) {
        return Some(GestureSpec {
            kind: GestureKind::LongPress,
            fingers,
            dir: None,
            pad: None,
            zone: None,
        });
    }
    Some(GestureSpec {
        kind: GestureKind::Tap,
        fingers,
        dir: None,
        pad: None,
        zone: None,
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
    let mut pads = PadTracker::new();
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
                    &mut pads,
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
                            &mut pads,
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
    pads: &mut PadTracker,
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
                Role::Gamepad | Role::SteamDeck => {
                    if let Some(bit) = gamepad_bit(code) {
                        state.set(bit, down);
                    }
                    match code {
                        BTN_TL2 => state.lt = if down { 1.0 } else { 0.0 },
                        BTN_TR2 => state.rt = if down { 1.0 } else { 0.0 },
                        _ => {}
                    }
                    if role == Role::SteamDeck {
                        match code {
                            BTN_THUMB => pads.click(PadSide::Left, down),
                            BTN_THUMB2 => pads.click(PadSide::Right, down),
                            _ => {}
                        }
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
            Role::SteamDeck => match code {
                ABS_X => state.lx = value,
                ABS_Y => state.ly = -value,
                ABS_RX => state.rx = value,
                ABS_RY => state.ry = -value,
                // Analog triggers arrive on HAT2, not Z/RZ like a generic pad.
                ABS_HAT2Y => state.lt = value,
                ABS_HAT2X => state.rt = value,
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
        Raw::Pad {
            side,
            x,
            y,
            touch,
            pressure,
        } => {
            let now = Instant::now();
            // The UI shows a touch only when the tracker says the pad is
            // really occupied; a stale coordinate is not a finger.
            let present = pads.feed(side, x, y, touch, now);
            let _ = evt_tx.send(DeckEvent::PadTouch {
                side,
                x,
                y,
                touched: pads.is_touched(side),
                bit: touch,
                pressure,
            });
            if let Some(gesture) = present {
                let _ = evt_tx.send(DeckEvent::Gesture(gesture));
            }
        }
    }
}

fn gamepad_bit(code: u16) -> Option<u32> {
    Some(match code {
        BTN_SOUTH => udp::BTN_A,
        BTN_EAST => udp::BTN_B,
        // `BTN_NORTH` is `BTN_X` and `BTN_WEST` is `BTN_Y` in the kernel:
        // the north/west names describe the SNES diamond, not the Xbox one.
        BTN_NORTH => udp::BTN_X,
        BTN_WEST => udp::BTN_Y,
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
        BTN_NORTH => "X",
        BTN_WEST => "Y",
        BTN_TL => "LB",
        BTN_TR => "RB",
        BTN_TL2 => "LT",
        BTN_TR2 => "RT",
        BTN_SELECT => "View",
        BTN_START => "Menu",
        BTN_MODE => "Steam",
        BTN_THUMBL => "L3",
        BTN_THUMBR => "R3",
        BTN_THUMB => "L-pad",
        BTN_THUMB2 => "R-pad",
        BTN_QAM => "QAM",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_buttons_follow_the_kernel_alias() {
        // The kernel names these by the SNES diamond: BTN_NORTH is BTN_X and
        // BTN_WEST is BTN_Y. hid-steam (Steam Deck) and xpad both report the
        // physical X there, so the label and the forwarded bit must follow the
        // alias, not the compass direction.
        assert_eq!(key_name(BTN_NORTH), "X");
        assert_eq!(key_name(BTN_WEST), "Y");
        assert_eq!(gamepad_bit(BTN_NORTH), Some(udp::BTN_X));
        assert_eq!(gamepad_bit(BTN_WEST), Some(udp::BTN_Y));
    }

    #[test]
    fn steam_virtual_pad_is_recognised() {
        assert!(is_steam_virtual_pad(0x28de, 0x11ff));
        // The Deck node itself and a plain Xbox pad are real controllers.
        assert!(!is_steam_virtual_pad(0x28de, 0x1205));
        assert!(!is_steam_virtual_pad(0x045e, 0x028e));
    }

    #[test]
    fn pad_click_has_a_name() {
        assert_eq!(key_name(BTN_THUMB), "L-pad");
        assert_eq!(key_name(BTN_THUMB2), "R-pad");
    }

    #[test]
    fn pad_swipe_is_tagged_with_its_side() {
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        // Finger lands on the left half of the left pad.
        assert!(pads.feed(PadSide::Left, -0.5, 0.0, true, t0).is_none());
        // Moves to the right half: the swipe fires under the finger, without
        // waiting for a release bit that may never arrive.
        let g = pads
            .feed(
                PadSide::Left,
                0.5,
                0.0,
                true,
                t0 + Duration::from_millis(120),
            )
            .expect("a swipe");
        assert_eq!(g.kind, GestureKind::Swipe);
        assert_eq!(g.dir, Some(SwipeDir::Right));
        assert_eq!(g.fingers, 1);
        assert_eq!(g.pad, Some(PadSide::Left));
        // The release does not repeat it.
        assert!(pads
            .feed(
                PadSide::Left,
                0.0,
                0.0,
                false,
                t0 + Duration::from_millis(160),
            )
            .is_none());
    }

    #[test]
    fn pad_tap_is_a_tap_and_an_untouched_pad_is_silent() {
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        // Nothing happens while the pad is untouched.
        assert!(pads.feed(PadSide::Right, 0.0, 0.0, false, t0).is_none());
        // A finger resting in place is not a gesture yet.
        assert!(pads.feed(PadSide::Right, 0.2, 0.1, true, t0).is_none());
        let g = pads
            .feed(
                PadSide::Right,
                0.2,
                0.1,
                false,
                t0 + Duration::from_millis(90),
            )
            .expect("a tap");
        assert_eq!(g.kind, GestureKind::Tap);
        assert_eq!(g.pad, Some(PadSide::Right));
    }

    #[test]
    fn pad_centre_touch_is_a_tap_not_a_miss() {
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        // The hardware touch bit is set while both axes are exactly zero.
        assert!(pads.feed(PadSide::Left, 0.0, 0.0, true, t0).is_none());
        let g = pads
            .feed(
                PadSide::Left,
                0.0,
                0.0,
                false,
                t0 + Duration::from_millis(70),
            )
            .expect("a centre tap");
        assert_eq!(g.kind, GestureKind::Tap);
        assert_eq!(g.pad, Some(PadSide::Left));
    }

    #[test]
    fn pad_full_path_keeps_a_swipe_that_returns() {
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        assert!(pads.feed(PadSide::Left, -0.5, 0.0, true, t0).is_none());
        // The swipe fires on the farthest point of the path.
        let g = pads
            .feed(
                PadSide::Left,
                0.6,
                0.0,
                true,
                t0 + Duration::from_millis(100),
            )
            .expect("a swipe");
        assert_eq!(g.kind, GestureKind::Swipe);
        assert_eq!(g.dir, Some(SwipeDir::Right));
        // Coming back to the start must not produce a tap on release.
        assert!(pads
            .feed(
                PadSide::Left,
                -0.5,
                0.0,
                true,
                t0 + Duration::from_millis(180),
            )
            .is_none());
        assert!(pads
            .feed(
                PadSide::Left,
                0.0,
                0.0,
                false,
                t0 + Duration::from_millis(220),
            )
            .is_none());
    }

    #[test]
    fn pad_finds_all_four_swipe_directions() {
        for (x, y, want) in [
            (0.7, 0.0, SwipeDir::Right),
            (-0.7, 0.0, SwipeDir::Left),
            (0.0, 0.7, SwipeDir::Down),
            (0.0, -0.7, SwipeDir::Up),
        ] {
            let mut pads = PadTracker::new();
            let t0 = Instant::now();
            assert!(pads.feed(PadSide::Right, 0.0, 0.0, true, t0).is_none());
            let g = pads
                .feed(PadSide::Right, x, y, true, t0 + Duration::from_millis(100))
                .expect("a swipe");
            assert_eq!(g.kind, GestureKind::Swipe);
            assert_eq!(g.dir, Some(want), "raw ({x}, {y})");
        }
    }

    #[test]
    fn pad_long_press_fires_while_the_finger_stays() {
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        assert!(pads.feed(PadSide::Right, 0.1, 0.1, true, t0).is_none());
        // Still down, but past the long-press mark: the gesture fires now,
        // so a release bit that never comes cannot swallow it.
        let g = pads
            .feed(
                PadSide::Right,
                0.1,
                0.1,
                true,
                t0 + Duration::from_millis(700),
            )
            .expect("a long press");
        assert_eq!(g.kind, GestureKind::LongPress);
        assert_eq!(g.pad, Some(PadSide::Right));
        // The release is not a second gesture.
        assert!(pads
            .feed(
                PadSide::Right,
                0.1,
                0.1,
                false,
                t0 + Duration::from_millis(1000),
            )
            .is_none());
    }

    #[test]
    fn a_pad_click_fires_its_part_and_not_a_tap() {
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        // The click bit and the touch bit arrive together; the press is the
        // action, and it goes to the part the finger is in. The release must
        // not fire a second, whole-pad tap on top of it.
        pads.click(PadSide::Left, true);
        let g = pads
            .feed(PadSide::Left, -0.5, -0.5, true, t0)
            .expect("the press resolves a part");
        assert_eq!(g.kind, GestureKind::Tap);
        assert_eq!(g.pad, Some(PadSide::Left));
        assert_eq!(g.zone, Some(PadZone::TopLeft));
        assert!(pads
            .feed(
                PadSide::Left,
                -0.5,
                -0.5,
                false,
                t0 + Duration::from_millis(90),
            )
            .is_none());
        pads.click(PadSide::Left, false);
        // A later light touch is its own part, with its own bind.
        assert!(pads
            .feed(
                PadSide::Left,
                -0.5,
                -0.6,
                true,
                t0 + Duration::from_millis(500)
            )
            .is_none());
        let g = pads
            .feed(
                PadSide::Left,
                -0.5,
                -0.6,
                false,
                t0 + Duration::from_millis(560),
            )
            .expect("a later tap");
        assert_eq!(g.kind, GestureKind::Tap);
        assert_eq!(g.zone, Some(PadZone::TopLeft));
    }

    #[test]
    fn a_press_without_a_position_still_fires_the_whole_pad() {
        // The evdev node zeroes the pad axes when the finger lifts, so a
        // click can arrive with no usable coordinate. It must still run a
        // bind, not vanish.
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        pads.click(PadSide::Left, true);
        let g = pads
            .feed(PadSide::Left, 0.0, 0.0, false, t0)
            .expect("the press fires");
        assert_eq!(g.kind, GestureKind::Tap);
        assert_eq!(g.pad, Some(PadSide::Left));
        assert_eq!(g.zone, None);
        // One press is one action.
        assert!(pads
            .feed(
                PadSide::Left,
                0.0,
                0.0,
                false,
                t0 + Duration::from_millis(40)
            )
            .is_none());
    }

    #[test]
    fn a_press_is_not_also_a_swipe() {
        // Holding the pad and sliding used to produce the press and then a
        // swipe from the same finger. The press is the action.
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        pads.click(PadSide::Right, true);
        let press = pads
            .feed(PadSide::Right, -0.7, 0.7, true, t0)
            .expect("the press");
        assert_eq!(press.zone, Some(PadZone::BottomLeft));
        // A long slide after the press stays quiet.
        for step in 1..12 {
            let x = -0.7 + step as f32 * 0.12;
            assert!(pads
                .feed(
                    PadSide::Right,
                    x,
                    0.7,
                    true,
                    t0 + Duration::from_millis(step * 12)
                )
                .is_none());
        }
        assert!(pads
            .feed(
                PadSide::Right,
                0.7,
                0.7,
                false,
                t0 + Duration::from_millis(200)
            )
            .is_none());
    }

    #[test]
    fn a_gesture_id_and_label_say_which_pad() {
        let glass = GestureSpec {
            kind: GestureKind::Swipe,
            fingers: 1,
            dir: Some(SwipeDir::Left),
            pad: None,
            zone: None,
        };
        let left = GestureSpec {
            pad: Some(PadSide::Left),
            ..glass
        };
        // The glass keeps the old id, so existing binds still match.
        assert_eq!(glass.id(), "1f-swipe-left");
        assert_eq!(left.id(), "1f-swipe-left@left");
        assert_ne!(glass.id(), left.id());
        assert!(left.label().contains("левая панель"));
    }

    #[test]
    fn a_pad_part_keeps_the_whole_pad_id_as_its_prefix() {
        // A bind for the whole pad must stay an id of its own, so a part's id
        // can fall back to it without ever matching it directly.
        let whole = GestureSpec {
            kind: GestureKind::Tap,
            fingers: 1,
            dir: None,
            pad: Some(PadSide::Left),
            zone: None,
        };
        assert_eq!(whole.id(), "1f-tap@left");
        let part = GestureSpec::pad_part(PadSide::Left, PadZone::TopRight);
        assert_eq!(part.id(), "1f-tap@left:tr");
        assert_ne!(part.id(), whole.id());
        // The fallback drops only the part.
        assert_eq!(part.whole_pad().id(), whole.id());
        assert!(part.label().contains("верхний правый"));
        assert!(!whole.label().contains("верхний правый"));
    }

    #[test]
    fn every_part_of_a_pad_gets_its_own_id() {
        let mut ids: Vec<String> = PadZone::ALL
            .iter()
            .map(|z| GestureSpec::pad_part(PadSide::Right, *z).id())
            .collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "each part is a different bind");
    }

    #[test]
    fn a_part_is_taken_from_where_the_finger_lands() {
        // The pad's own coordinates, so the halves are the visual ones.
        assert_eq!(PadZone::from_point(0.1, 0.1), PadZone::TopLeft);
        assert_eq!(PadZone::from_point(0.9, 0.1), PadZone::TopRight);
        assert_eq!(PadZone::from_point(0.1, 0.9), PadZone::BottomLeft);
        assert_eq!(PadZone::from_point(0.9, 0.9), PadZone::BottomRight);
    }

    #[test]
    fn a_press_and_a_light_tap_on_the_same_part_agree() {
        // Both ways of hitting a pad must resolve to the same part, otherwise
        // a bind would work with a press and not with a touch.
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        pads.click(PadSide::Right, true);
        let press = pads
            .feed(PadSide::Right, 0.6, 0.6, true, t0)
            .expect("a press");
        pads.click(PadSide::Right, false);
        pads.feed(
            PadSide::Right,
            0.6,
            0.6,
            false,
            t0 + Duration::from_millis(80),
        );

        assert!(pads
            .feed(
                PadSide::Right,
                0.6,
                0.6,
                true,
                t0 + Duration::from_millis(400),
            )
            .is_none());
        let tap = pads
            .feed(
                PadSide::Right,
                0.6,
                0.6,
                false,
                t0 + Duration::from_millis(460),
            )
            .expect("a tap");
        assert_eq!(tap.kind, GestureKind::Tap);
        assert_eq!(tap.zone, press.zone);
        assert_eq!(tap.pad, press.pad);
        assert_eq!(tap.id(), press.id());
    }

    #[test]
    fn dragging_into_another_part_keeps_the_one_it_started_in() {
        // A finger that lands in one quarter and moves is still that quarter's
        // press when it lifts; the movement is a swipe, not a moved tap.
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        assert!(pads.feed(PadSide::Left, -0.6, -0.6, true, t0).is_none());
        let g = pads
            .feed(
                PadSide::Left,
                -0.6,
                -0.6,
                false,
                t0 + Duration::from_millis(70),
            )
            .expect("a tap");
        assert_eq!(g.zone, Some(PadZone::TopLeft));
    }

    #[cfg(target_os = "linux")]
    fn empty_deck_report() -> [u8; DECK_REPORT_SIZE] {
        let mut data = [0u8; DECK_REPORT_SIZE];
        data[0] = 1;
        data[1] = 0;
        data[2] = DECK_REPORT_ID;
        data
    }

    #[cfg(target_os = "linux")]
    fn deck_button(report: &DeckReport, code: u16) -> bool {
        DECK_BUTTONS
            .iter()
            .position(|(_, _, c)| *c == code)
            .map(|i| report.buttons[i])
            .unwrap_or(false)
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn deck_report_reads_pads_sticks_and_triggers() {
        let mut data = empty_deck_report();
        // Both pads report their touch bits and a position.
        data[10] = 0x08 | 0x10;
        data[16..18].copy_from_slice(&(-16384i16).to_le_bytes());
        data[18..20].copy_from_slice(&(16384i16).to_le_bytes());
        data[20..22].copy_from_slice(&(8192i16).to_le_bytes());
        data[22..24].copy_from_slice(&(-8192i16).to_le_bytes());
        // Sticks: hid-steam negates Y, so the raw positive Y must come out
        // negative here as well.
        data[48..50].copy_from_slice(&(32767i16).to_le_bytes());
        data[50..52].copy_from_slice(&(32767i16).to_le_bytes());
        data[52..54].copy_from_slice(&(-32767i16).to_le_bytes());
        data[54..56].copy_from_slice(&(-32767i16).to_le_bytes());
        // Triggers: left on HAT2Y, right on HAT2X.
        data[44..46].copy_from_slice(&(16384u16).to_le_bytes());
        data[46..48].copy_from_slice(&(32767u16).to_le_bytes());

        let report = parse_deck_report(&data).expect("report 9");
        assert!(report.lpad_touched);
        assert!(report.rpad_touched);
        assert!((report.lpad.0 + 0.5).abs() < 0.001);
        // Raw Y is +0.5 (up), which is negative in screen terms.
        assert!((report.lpad.1 + 0.5).abs() < 0.001);
        assert!((report.rpad.0 - 0.25).abs() < 0.001);
        assert!((report.rpad.1 - 0.25).abs() < 0.001);
        assert!((report.lx - 1.0).abs() < 0.001);
        assert!((report.ly + 1.0).abs() < 0.001);
        assert!((report.rx + 1.0).abs() < 0.001);
        assert!((report.ry - 1.0).abs() < 0.001);
        assert!((report.lt - 0.5).abs() < 0.001);
        assert!((report.rt - 1.0).abs() < 0.001);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn deck_report_keeps_a_centre_touch_alive() {
        let mut data = empty_deck_report();
        // The touch bit is set while both coordinates are exactly zero.
        data[10] = 0x08;
        let report = parse_deck_report(&data).expect("report 9");
        assert!(report.lpad_touched);
        assert!(!report.rpad_touched);
        assert_eq!(report.lpad, (0.0, 0.0));

        let (tx, rx) = channel();
        let mut prev = DeckReport::default();
        emit_deck_report(&report, &mut prev, &tx, "test");
        drop(tx);
        let mut seen = false;
        for raw in rx {
            if let Raw::Pad {
                side: PadSide::Left,
                x,
                y,
                touch: true,
                ..
            } = raw
            {
                assert_eq!((x, y), (0.0, 0.0));
                seen = true;
            }
        }
        assert!(seen, "the centre touch must reach the tracker");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn the_position_reaches_the_tracker_and_the_bit_is_not_faked() {
        let mut data = empty_deck_report();
        // No touch bit, but the axes clearly left the centre. The position must
        // arrive unchanged; the bit must stay honest so the tracker keeps
        // trusting it over the coordinates on a pad that does send it.
        data[16..18].copy_from_slice(&(16384i16).to_le_bytes());
        data[20..22].copy_from_slice(&(-16384i16).to_le_bytes());
        let report = parse_deck_report(&data).expect("report 9");
        assert!(!report.lpad_touched);
        assert!(!report.rpad_touched);

        let (tx, rx) = channel();
        let mut prev = DeckReport::default();
        emit_deck_report(&report, &mut prev, &tx, "test");
        drop(tx);
        let samples: Vec<(PadSide, f32, f32, bool)> = rx
            .into_iter()
            .filter_map(|raw| match raw {
                Raw::Pad {
                    side, x, y, touch, ..
                } => Some((side, x, y, touch)),
                _ => None,
            })
            .collect();
        assert_eq!(samples.len(), 2);
        assert!(!samples[0].3);
        assert_eq!(samples[0].0, PadSide::Left);
        assert!((samples[0].1 - 0.5).abs() < 0.01);
        assert_eq!(samples[1].0, PadSide::Right);
        assert!((samples[1].1 + 0.5).abs() < 0.01);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_raw_upward_move_becomes_an_up_swipe() {
        // End to end through the report: the hardware's positive Y is up, and
        // the tracker must call it "up", not "down". This is the sign SDL
        // uses, and the one the Deck's pads need.
        let mut data = empty_deck_report();
        data[10] = 0x08;
        data[16..18].copy_from_slice(&(0i16).to_le_bytes());
        data[18..20].copy_from_slice(&(0i16).to_le_bytes());
        let start = parse_deck_report(&data).expect("report 9");

        let mut moved = data;
        // Slide the finger up: the raw Y grows positive.
        moved[18..20].copy_from_slice(&(22000i16).to_le_bytes());
        let end = parse_deck_report(&moved).expect("report 9");

        let (tx, rx) = channel();
        let mut prev = DeckReport::default();
        emit_deck_report(&start, &mut prev, &tx, "test");
        emit_deck_report(&end, &mut prev, &tx, "test");
        drop(tx);

        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        let mut gestures = Vec::new();
        for raw in rx {
            if let Raw::Pad {
                side: PadSide::Left,
                x,
                y,
                touch,
                ..
            } = raw
            {
                if let Some(g) = pads.feed(PadSide::Left, x, y, touch, t0) {
                    gestures.push(g);
                }
            }
        }
        assert_eq!(gestures.len(), 1, "one swipe");
        assert_eq!(gestures[0].kind, GestureKind::Swipe);
        assert_eq!(gestures[0].dir, Some(SwipeDir::Up));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn pad_pressure_is_parsed_for_diagnostics() {
        let mut data = empty_deck_report();
        data[56..58].copy_from_slice(&(120u16).to_le_bytes());
        data[58..60].copy_from_slice(&(7u16).to_le_bytes());
        let report = parse_deck_report(&data).expect("report 9");
        assert_eq!(report.lpad_pressure, 120);
        assert_eq!(report.rpad_pressure, 7);
        // Pressure is a reading, not the touch signal: the axes stayed at the
        // centre without the bit, so nothing is reported as touched.
        assert!(!report.lpad_touched);

        let (tx, rx) = channel();
        let mut prev = DeckReport::default();
        emit_deck_report(&report, &mut prev, &tx, "test");
        drop(tx);
        let pressures: Vec<u16> = rx
            .into_iter()
            .filter_map(|raw| match raw {
                Raw::Pad { pressure, .. } => Some(pressure),
                _ => None,
            })
            .collect();
        assert_eq!(pressures, vec![120, 7]);
    }

    #[test]
    fn a_pad_that_sends_the_bit_ignores_a_leftover_coordinate() {
        // The bit arrives once, then stops while the axes stay put. SDL keeps
        // the last position like this, so the coordinates must not keep the
        // touch alive or a lifted finger would look like a press forever.
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        assert!(pads.feed(PadSide::Left, 0.4, 0.2, true, t0).is_none());
        assert!(pads.is_touched(PadSide::Left));
        // The very next sample without the bit ends the touch, however soon
        // after the first one it arrives.
        let g = pads.feed(
            PadSide::Left,
            0.4,
            0.2,
            false,
            t0 + Duration::from_millis(8),
        );
        assert!(!pads.is_touched(PadSide::Left));
        // A short touch that ends before it travelled is a tap, not nothing.
        assert_eq!(g.expect("a tap").kind, GestureKind::Tap);
    }

    #[test]
    fn a_firm_press_does_not_switch_light_touches_off() {
        // The reported symptom: a firm press works, a light touch does not.
        // A touch the bit reported must not make every later coordinate
        // useless — a light touch that the bit misses is still a touch.
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        // A firm press: the bit is set.
        assert!(pads.feed(PadSide::Left, 0.3, 0.3, true, t0).is_none());
        assert!(pads.is_touched(PadSide::Left));
        // Lifting is a tap, and the stale coordinate that stays behind must
        // not restart the touch.
        let g = pads.feed(
            PadSide::Left,
            0.3,
            0.3,
            false,
            t0 + Duration::from_millis(60),
        );
        assert_eq!(g.expect("a tap").kind, GestureKind::Tap);
        assert!(!pads.is_touched(PadSide::Left));
        assert!(pads
            .feed(
                PadSide::Left,
                0.3,
                0.3,
                false,
                t0 + Duration::from_millis(80)
            )
            .is_none());
        assert!(!pads.is_touched(PadSide::Left));

        // A light touch elsewhere, with no bit at all, is still a finger.
        assert!(pads
            .feed(
                PadSide::Left,
                -0.4,
                0.2,
                false,
                t0 + Duration::from_millis(120)
            )
            .is_none());
        assert!(pads.is_touched(PadSide::Left));
    }

    #[test]
    fn a_position_only_pad_reads_the_coordinate() {
        // evdev shape: no bit ever arrives, so the middle is the release and
        // an offset is the finger.
        let mut pads = PadTracker::new();
        let t0 = Instant::now();
        // The middle starts nothing.
        assert!(pads.feed(PadSide::Right, 0.0, 0.0, false, t0).is_none());
        assert!(!pads.is_touched(PadSide::Right));
        // An offset is a finger.
        assert!(pads
            .feed(
                PadSide::Right,
                0.5,
                -0.3,
                false,
                t0 + Duration::from_millis(8),
            )
            .is_none());
        assert!(pads.is_touched(PadSide::Right));
        // Back to the middle ends it.
        assert!(pads
            .feed(
                PadSide::Right,
                0.0,
                0.0,
                false,
                t0 + Duration::from_millis(16),
            )
            .is_some());
        assert!(!pads.is_touched(PadSide::Right));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn deck_report_reads_buttons_and_imu() {
        let mut data = empty_deck_report();
        data[13] |= 1 << 1; // left top grip: L4
        data[10] |= 1 << 1; // left pad click
        data[14] |= 1 << 2; // quick access
        data[24..26].copy_from_slice(&(1000i16).to_le_bytes());
        data[26..28].copy_from_slice(&(2000i16).to_le_bytes());
        data[28..30].copy_from_slice(&(3000i16).to_le_bytes());
        data[30..32].copy_from_slice(&(4000i16).to_le_bytes());
        data[32..34].copy_from_slice(&(5000i16).to_le_bytes());
        data[34..36].copy_from_slice(&(6000i16).to_le_bytes());

        let report = parse_deck_report(&data).expect("report 9");
        assert!(deck_button(&report, BTN_L4));
        assert!(deck_button(&report, BTN_THUMB));
        assert!(deck_button(&report, BTN_QAM));
        assert!(!deck_button(&report, BTN_R4));
        // The kernel maps accel X/Z/Y and gyro X/Z/Y, negating both Zs.
        assert_eq!(report.accel, [1000, 3000, -2000]);
        assert_eq!(report.gyro, [4000, 6000, -5000]);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn deck_report_rejects_other_messages() {
        let mut data = empty_deck_report();
        data[2] = 1; // ordinary controller state
        assert!(parse_deck_report(&data).is_none());
        data[2] = DECK_REPORT_ID;
        assert!(parse_deck_report(&data[..DECK_REPORT_SIZE - 1]).is_none());
        data[0] = 0;
        assert!(parse_deck_report(&data).is_none());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn deck_report_edges_fire_once() {
        let (tx, rx) = channel();
        let mut prev = DeckReport::default();
        let mut report = DeckReport::default();
        let slot = DECK_BUTTONS
            .iter()
            .position(|(_, _, c)| *c == BTN_L4)
            .expect("L4");
        report.buttons[slot] = true;
        report.lpad = (0.5, -0.5);

        emit_deck_report(&report, &mut prev, &tx, "Steam Deck");
        let first: Vec<Raw> = rx.try_iter().collect();
        assert_eq!(
            first
                .iter()
                .filter(|r| matches!(r, Raw::Key { .. }))
                .count(),
            1
        );
        assert_eq!(
            first
                .iter()
                .filter(|r| matches!(r, Raw::Axis { .. }))
                .count(),
            0
        );
        // Both pads are published on every report, so the live position and
        // the touch bit never depend on the axes having changed.
        assert_eq!(
            first
                .iter()
                .filter(|r| matches!(r, Raw::Pad { .. }))
                .count(),
            2
        );

        // The same state again repeats the pad samples but nothing else: no
        // second press and no axis chatter.
        emit_deck_report(&report, &mut prev, &tx, "Steam Deck");
        let again: Vec<Raw> = rx.try_iter().collect();
        assert_eq!(
            again
                .iter()
                .filter(|r| matches!(r, Raw::Key { .. }))
                .count(),
            0
        );
        assert_eq!(
            again
                .iter()
                .filter(|r| matches!(r, Raw::Pad { .. }))
                .count(),
            2
        );

        report.buttons[slot] = false;
        report.lpad = (0.0, 0.0);
        report.lpad_touched = false;
        emit_deck_report(&report, &mut prev, &tx, "Steam Deck");
        let released: Vec<Raw> = rx.try_iter().collect();
        assert_eq!(
            released
                .iter()
                .filter(|r| matches!(r, Raw::Key { value: 0, .. }))
                .count(),
            1
        );
        // The release is a touch bit going down, not a position guess.
        assert!(released.iter().any(|r| matches!(
            r,
            Raw::Pad {
                side: PadSide::Left,
                touch: false,
                ..
            }
        )));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn grip_bits_use_the_evdev_codes() {
        // hid-steam's report 9 bits and the evdev keys they become. L4/R4 are
        // the upper pair, L5/R5 the lower pair.
        assert_eq!(
            (BTN_L4, BTN_R4, BTN_L5, BTN_R5),
            (0x2c0, 0x2c1, 0x2c2, 0x2c3)
        );
        let has = |byte: usize, bit: u8, code: u16| {
            DECK_BUTTONS
                .iter()
                .any(|&(b, bi, c)| b == byte && bi == bit && c == code)
        };
        assert!(has(13, 1, BTN_L4));
        assert!(has(13, 2, BTN_R4));
        assert!(has(9, 7, BTN_L5));
        assert!(has(10, 0, BTN_R5));
        assert!(has(10, 1, BTN_THUMB));
        assert!(has(10, 2, BTN_THUMB2));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn hidraw_uevent_finds_the_deck_client() {
        assert!(hidraw_matches(
            "HID_NAME=Steam Deck\nHID_ID=0003:000028DE:00001205\n"
        ));
        assert!(!hidraw_matches("HID_ID=0003:000028DE:000011FF\n"));
        assert!(!hidraw_matches("HID_ID=0003:0000045E:0000028E\n"));
        assert!(!hidraw_matches("HID_NAME=Steam Deck\n"));
    }
}
