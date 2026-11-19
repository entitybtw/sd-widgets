//! Low-latency controller packets: Steam Deck → daemon over UDP, then into VIIPER.

use crate::auth::token_tag;
use crate::model::now_ms;

pub const MAGIC: &[u8; 4] = b"SDWI";
pub const VERSION: u8 = 1;
pub const PACKET_SIZE: usize = 64;

// Canonical buttons shared by the Deck input reader and the daemon's device mappers.
pub const BTN_A: u32 = 1 << 0;
pub const BTN_B: u32 = 1 << 1;
pub const BTN_X: u32 = 1 << 2;
pub const BTN_Y: u32 = 1 << 3;
pub const BTN_LB: u32 = 1 << 4;
pub const BTN_RB: u32 = 1 << 5;
pub const BTN_BACK: u32 = 1 << 6;
pub const BTN_START: u32 = 1 << 7;
pub const BTN_GUIDE: u32 = 1 << 8;
pub const BTN_L3: u32 = 1 << 9;
pub const BTN_R3: u32 = 1 << 10;
pub const BTN_DPAD_UP: u32 = 1 << 11;
pub const BTN_DPAD_DOWN: u32 = 1 << 12;
pub const BTN_DPAD_LEFT: u32 = 1 << 13;
pub const BTN_DPAD_RIGHT: u32 = 1 << 14;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GamepadState {
    pub buttons: u32,
    pub lt: f32,
    pub rt: f32,
    /// -1..1, positive right/up
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    /// rad/s, raw-ish
    pub gyro: [i16; 3],
    pub accel: [i16; 3],
}

impl GamepadState {
    pub fn set(&mut self, bit: u32, pressed: bool) {
        if pressed {
            self.buttons |= bit;
        } else {
            self.buttons &= !bit;
        }
    }

    pub fn axes_i16(&self) -> [i16; 4] {
        [
            (self.lx.clamp(-1.0, 1.0) * 32767.0) as i16,
            (self.ly.clamp(-1.0, 1.0) * 32767.0) as i16,
            (self.rx.clamp(-1.0, 1.0) * 32767.0) as i16,
            (self.ry.clamp(-1.0, 1.0) * 32767.0) as i16,
        ]
    }
}

#[derive(Debug, Clone, Default)]
pub struct PadPacket {
    pub pad_id: u32,
    pub seq: u32,
    pub tag: [u8; 8],
    pub state: GamepadState,
}

impl PadPacket {
    pub fn encode(&self, secret: &str) -> [u8; PACKET_SIZE] {
        let mut b = [0u8; PACKET_SIZE];
        b[0..4].copy_from_slice(MAGIC);
        b[4] = VERSION;
        b[8..12].copy_from_slice(&self.pad_id.to_le_bytes());
        b[12..16].copy_from_slice(&self.seq.to_le_bytes());
        b[16..24].copy_from_slice(&token_tag(secret));
        b[24..28].copy_from_slice(&self.state.buttons.to_le_bytes());
        b[28] = (self.state.lt.clamp(0.0, 1.0) * 255.0) as u8;
        b[29] = (self.state.rt.clamp(0.0, 1.0) * 255.0) as u8;
        let axes = self.state.axes_i16();
        b[32..34].copy_from_slice(&axes[0].to_le_bytes());
        b[34..36].copy_from_slice(&axes[1].to_le_bytes());
        b[36..38].copy_from_slice(&axes[2].to_le_bytes());
        b[38..40].copy_from_slice(&axes[3].to_le_bytes());
        for (i, v) in self.state.gyro.iter().enumerate() {
            b[40 + i * 2..42 + i * 2].copy_from_slice(&v.to_le_bytes());
        }
        for (i, v) in self.state.accel.iter().enumerate() {
            b[46 + i * 2..48 + i * 2].copy_from_slice(&v.to_le_bytes());
        }
        b
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < PACKET_SIZE || &bytes[0..4] != MAGIC || bytes[4] != VERSION {
            return None;
        }
        let mut tag = [0u8; 8];
        tag.copy_from_slice(&bytes[16..24]);
        let i16_at = |o: usize| i16::from_le_bytes([bytes[o], bytes[o + 1]]);
        Some(Self {
            pad_id: u32::from_le_bytes(bytes[8..12].try_into().ok()?),
            seq: u32::from_le_bytes(bytes[12..16].try_into().ok()?),
            tag,
            state: GamepadState {
                buttons: u32::from_le_bytes(bytes[24..28].try_into().ok()?),
                lt: bytes[28] as f32 / 255.0,
                rt: bytes[29] as f32 / 255.0,
                lx: i16_at(32) as f32 / 32767.0,
                ly: i16_at(34) as f32 / 32767.0,
                rx: i16_at(36) as f32 / 32767.0,
                ry: i16_at(38) as f32 / 32767.0,
                gyro: [i16_at(40), i16_at(42), i16_at(44)],
                accel: [i16_at(46), i16_at(48), i16_at(50)],
            },
        })
    }
}

/// Human-readable list of currently pressed buttons, used by the Deck UI.
pub fn pressed_labels(buttons: u32) -> Vec<&'static str> {
    const MAP: &[(u32, &str)] = &[
        (BTN_A, "A"),
        (BTN_B, "B"),
        (BTN_X, "X"),
        (BTN_Y, "Y"),
        (BTN_LB, "LB"),
        (BTN_RB, "RB"),
        (BTN_BACK, "View"),
        (BTN_START, "Menu"),
        (BTN_GUIDE, "Steam"),
        (BTN_L3, "L3"),
        (BTN_R3, "R3"),
        (BTN_DPAD_UP, "↑"),
        (BTN_DPAD_DOWN, "↓"),
        (BTN_DPAD_LEFT, "←"),
        (BTN_DPAD_RIGHT, "→"),
    ];
    MAP.iter()
        .filter(|(bit, _)| buttons & bit != 0)
        .map(|(_, name)| *name)
        .collect()
}

pub fn last_input_age_ms(last: u64) -> u64 {
    now_ms().saturating_sub(last)
}
