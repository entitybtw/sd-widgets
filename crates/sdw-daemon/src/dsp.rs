//! Sample conversion for the Windows audio capture.
//!
//! WASAPI hands out whatever the audio engine mixes in: usually 32-bit float
//! at 48 kHz, sometimes 16-bit, and with any channel count the device has. The
//! deck wants stereo `s16le` at the rate its player was opened with, so this
//! module does the conversion (and a small linear resampler) here, where it is
//! plain arithmetic and can be tested on any platform.

/// How one sample is stored in the device's mix format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleKind {
    F32,
    I16,
    I32,
}

impl SampleKind {
    pub fn bytes(self) -> usize {
        match self {
            SampleKind::I16 => 2,
            SampleKind::F32 | SampleKind::I32 => 4,
        }
    }
}

/// Interleaved device samples → interleaved stereo `i16` at the requested rate.
pub struct Converter {
    kind: SampleKind,
    channels: usize,
    /// Output samples per input frame.
    step: f64,
    /// Where the next output sample sits between `prev` and the current frame.
    pos: f64,
    prev: [f32; 2],
    started: bool,
}

impl Converter {
    pub fn new(kind: SampleKind, channels: u16, in_rate: u32, out_rate: u32) -> Self {
        let in_rate = in_rate.max(1);
        let out_rate = out_rate.max(1);
        Self {
            kind,
            channels: channels.max(1) as usize,
            step: in_rate as f64 / out_rate as f64,
            pos: 1.0,
            prev: [0.0; 2],
            started: false,
        }
    }

    /// Convert one WSAPI packet. A partial trailing frame is kept out: WASAPI
    /// always delivers whole frames, and a half frame would shift the channels.
    pub fn push(&mut self, data: &[u8], out: &mut Vec<i16>) {
        let size = self.kind.bytes();
        let frame = size * self.channels;
        if frame == 0 {
            return;
        }
        for chunk in data.chunks_exact(frame) {
            let current = self.frame(chunk);
            if !self.started {
                self.prev = current;
                self.started = true;
                continue;
            }
            while self.pos <= 1.0 {
                let t = self.pos as f32;
                out.push(to_i16(self.prev[0] + (current[0] - self.prev[0]) * t));
                out.push(to_i16(self.prev[1] + (current[1] - self.prev[1]) * t));
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.prev = current;
        }
    }

    /// One device frame as a stereo pair. Mono is doubled, more than two
    /// channels keep the front pair — the deck has two speakers anyway.
    fn frame(&self, chunk: &[u8]) -> [f32; 2] {
        let sample = |i: usize| -> f32 {
            let at = i * self.kind.bytes();
            match self.kind {
                SampleKind::F32 => {
                    f32::from_le_bytes([chunk[at], chunk[at + 1], chunk[at + 2], chunk[at + 3]])
                }
                SampleKind::I16 => i16::from_le_bytes([chunk[at], chunk[at + 1]]) as f32 / 32768.0,
                SampleKind::I32 => {
                    i32::from_le_bytes([chunk[at], chunk[at + 1], chunk[at + 2], chunk[at + 3]])
                        as f32
                        / 2147483648.0
                }
            }
        };
        if self.channels == 1 {
            let mono = sample(0);
            [mono, mono]
        } else {
            [sample(0), sample(1)]
        }
    }
}

fn to_i16(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32_bytes(frames: &[[f32; 2]]) -> Vec<u8> {
        let mut out = Vec::new();
        for frame in frames {
            for value in frame {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        out
    }

    #[test]
    fn the_same_rate_passes_frames_through() {
        let mut converter = Converter::new(SampleKind::F32, 2, 48_000, 48_000);
        let mut out = Vec::new();
        converter.push(
            &f32_bytes(&[[0.0, 0.0], [0.5, -0.5], [1.0, -1.0]]),
            &mut out,
        );
        // One frame of delay, then every sample exactly as it came in.
        assert_eq!(out, vec![16384, -16384, 32767, -32767]);
    }

    #[test]
    fn mono_is_doubled() {
        let mut data = Vec::new();
        for value in [0.25f32, 0.5] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        let mut converter = Converter::new(SampleKind::F32, 1, 48_000, 48_000);
        let mut out = Vec::new();
        converter.push(&data, &mut out);
        assert_eq!(out, vec![16384, 16384]);
    }

    #[test]
    fn resampling_keeps_the_length() {
        // 96 kHz into 48 kHz: half the frames come out.
        let frames: Vec<[f32; 2]> = (0..4801).map(|i| [i as f32 / 4801.0, 0.0]).collect();
        let mut converter = Converter::new(SampleKind::F32, 2, 96_000, 48_000);
        let mut out = Vec::new();
        converter.push(&f32_bytes(&frames), &mut out);
        let produced = out.len() / 2;
        assert!(
            (2398..=2401).contains(&produced),
            "produced {produced} frames"
        );
    }

    #[test]
    fn sixteen_bit_input_is_scaled() {
        let mut data = Vec::new();
        for _ in 0..2 {
            data.extend_from_slice(&0i16.to_le_bytes());
            data.extend_from_slice(&i16::MAX.to_le_bytes());
        }
        let mut converter = Converter::new(SampleKind::I16, 2, 48_000, 48_000);
        let mut out = Vec::new();
        converter.push(&data, &mut out);
        assert_eq!(out, vec![0, 32766]);
    }

    #[test]
    fn a_partial_frame_is_ignored() {
        let mut converter = Converter::new(SampleKind::F32, 2, 48_000, 48_000);
        let mut out = Vec::new();
        converter.push(&[0u8; 6], &mut out);
        assert!(out.is_empty());
    }
}
