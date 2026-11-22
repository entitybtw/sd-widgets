//! The frequency bands behind the visualizer.
//!
//! One Goertzel filter per band: no FFT and no extra dependency. The deck uses
//! it for the PCM it plays, the daemon uses the very same code for the
//! computer's sound, so the two meters cannot drift apart. Levels are
//! normalized to 0..=1 over the useful -60 dB..0 dB range.

/// How many bands the analyzer produces. The visible bar count is a setting
/// between 4 and this; bands are grouped when fewer are shown.
pub const BANDS: usize = 24;

/// Centre frequencies from bass to treble, roughly logarithmic.
pub const FREQS: [f32; BANDS] = [
    40.0, 55.0, 75.0, 100.0, 130.0, 170.0, 220.0, 290.0, 380.0, 500.0, 650.0, 850.0, 1100.0,
    1450.0, 1900.0, 2500.0, 3300.0, 4300.0, 5600.0, 7300.0, 9500.0, 12000.0, 15000.0, 18000.0,
];

/// Analyze the newest frames of an s16le PCM chunk. `channels` is 1 or 2.
pub fn analyze(pcm: &[u8], channels: usize, rate: u32) -> [f32; BANDS] {
    let channels = channels.clamp(1, 2);
    let frame = 2 * channels;
    if pcm.len() < frame || rate < 8000 {
        return [0.0; BANDS];
    }
    // 2048 frames is about 46 ms at 44.1 kHz: long enough for the bass bins,
    // short enough that the bars still follow the beat.
    let frames = (pcm.len() / frame).min(2048);
    if frames < 64 {
        return [0.0; BANDS];
    }
    let start = pcm.len() - frames * frame;
    let mut mono = Vec::with_capacity(frames);
    for i in 0..frames {
        let base = start + i * frame;
        let mut sum = 0.0f32;
        for c in 0..channels {
            sum += i16::from_le_bytes([pcm[base + c * 2], pcm[base + c * 2 + 1]]) as f32;
        }
        mono.push(sum / channels as f32 / 32768.0);
    }
    // A power of two for the FFT, using the newest samples.
    let n = frames.next_power_of_two().min(2048);
    let from = mono.len().saturating_sub(n);
    let mut buf = vec![(0.0f32, 0.0f32); n];
    for (i, value) in mono[from..].iter().enumerate() {
        // Hann keeps one loud tone from smearing across every band.
        let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos();
        buf[i].0 = value * w;
    }
    fft(&mut buf);
    let bin = rate as f32 / n as f32;
    let mut out = [0.0f32; BANDS];
    for band in 0..BANDS {
        // Geometric edges between the centre frequencies: each band owns the
        // stretch of spectrum between its neighbours.
        let low = if band == 0 {
            20.0
        } else {
            (FREQS[band - 1] * FREQS[band]).sqrt()
        };
        let high = if band + 1 == BANDS {
            rate as f32 * 0.5
        } else {
            (FREQS[band] * FREQS[band + 1]).sqrt()
        };
        let lo = ((low / bin).floor() as usize).max(1);
        let hi = ((high / bin).ceil() as usize).clamp(lo, n / 2);
        let mut power = 0.0f32;
        for (re, im) in &buf[lo..=hi] {
            power += re * re + im * im;
        }
        // Hann's coherent gain is 0.5, and a real sine needs the one-sided
        // doubling: amplitude = 4·|X| / N.
        let amp = ((power.sqrt() * 4.0 / n as f32) as f32).max(1e-6);
        // -60 dB..0 dB is the useful range for a little meter.
        let db = 20.0 * amp.log10();
        out[band] = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    }
    out
}

/// In-place radix-2 FFT: the bands need real spectrum energy, and 2048 points
/// are cheap enough to do here without pulling in an FFT crate.
fn fft(buf: &mut [(f32, f32)]) {
    let n = buf.len();
    if n < 2 || !n.is_power_of_two() {
        return;
    }
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            buf.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let angle = -2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (angle.cos(), angle.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (ur, ui) = buf[start + k];
                let (vr, vi) = buf[start + k + len / 2];
                let tr = vr * cr - vi * ci;
                let ti = vr * ci + vi * cr;
                buf[start + k] = (ur + tr, ui + ti);
                buf[start + k + len / 2] = (ur - tr, ui - ti);
                let next = (cr * wr - ci * wi, cr * wi + ci * wr);
                cr = next.0;
                ci = next.1;
            }
        }
        len <<= 1;
    }
}

/// Follow a new frame with a fast attack and a slow release, so a hit jumps
/// and the bar falls back smoothly instead of strobing.
pub fn follow(prev: &[f32; BANDS], next: &[f32; BANDS], release: f32) -> [f32; BANDS] {
    let release = release.clamp(0.4, 0.98);
    let mut out = *prev;
    for (i, value) in next.iter().enumerate() {
        out[i] = if *value > prev[i] {
            *value
        } else {
            prev[i] * release + value * (1.0 - release)
        };
    }
    out
}

/// Group `BANDS` analyzer values into `n` visible bars by taking the loudest
/// member, so a narrow bar never loses a peak between two bands.
pub fn group(bands: &[f32; BANDS], n: usize) -> Vec<f32> {
    let n = n.clamp(1, BANDS);
    (0..n)
        .map(|i| {
            let from = i * BANDS / n;
            let to = ((i + 1) * BANDS / n).max(from + 1).min(BANDS);
            bands[from..to].iter().cloned().fold(0.0f32, f32::max)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, amp: f32, frames: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(frames * 4);
        for i in 0..frames {
            let v = ((i as f32 * 2.0 * std::f32::consts::PI * freq / 48000.0).sin() * amp * 32767.0)
                as i16;
            out.extend_from_slice(&v.to_le_bytes());
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    #[test]
    fn a_tone_lights_its_own_band() {
        let pcm = sine(440.0, 0.5, 4096);
        let bands = analyze(&pcm, 2, 48000);
        let loudest = bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap();
        // 440 Hz sits between the 380 and 500 Hz bands; either may win.
        assert!((8..=9).contains(&loudest), "loudest band was {loudest}");
        assert!(bands[loudest] > 0.6, "band was {:.2}", bands[loudest]);
        assert!(bands[0] < 0.5, "bass leaked: {:.2}", bands[0]);
    }

    #[test]
    fn silence_stays_dark() {
        let pcm = vec![0u8; 8192];
        assert!(analyze(&pcm, 2, 48000).iter().all(|v| *v < 0.02));
    }

    #[test]
    fn grouping_keeps_the_peak() {
        let mut bands = [0.0f32; BANDS];
        bands[10] = 0.9;
        let bars = group(&bands, 12);
        assert!(bars.iter().cloned().fold(0.0, f32::max) >= 0.9);
    }
}
