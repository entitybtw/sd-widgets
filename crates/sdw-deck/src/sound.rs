//! UI sounds.
//!
//! Three built-in character packs are synthesised on demand into small WAV
//! files in the cache dir; any single event can also point at a custom file.
//! Playback is delegated to whatever the system already has (`pw-play`,
//! `paplay`, `aplay`, `ffplay` or `mpv`), so the app stays dependency-free.

use sdw_core::model::SoundCfg;
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cue {
    Click,
    Navigate,
    Open,
    Back,
    Notify,
    Error,
    Startup,
}

impl Cue {
    fn key(self) -> &'static str {
        match self {
            Cue::Click => "click",
            Cue::Navigate => "navigate",
            Cue::Open => "open",
            Cue::Back => "back",
            Cue::Notify => "notify",
            Cue::Error => "error",
            Cue::Startup => "startup",
        }
    }
}

fn custom(cfg: &SoundCfg, cue: Cue) -> &str {
    match cue {
        Cue::Click => &cfg.click,
        Cue::Navigate => &cfg.navigate,
        Cue::Open => &cfg.open,
        Cue::Back => &cfg.back,
        Cue::Notify => &cfg.notify,
        Cue::Error => &cfg.error,
        Cue::Startup => &cfg.startup,
    }
}

/// Fire a cue. Cheap enough to call from any UI action.
pub fn play(cfg: &SoundCfg, cue: Cue) {
    if !cfg.enabled {
        return;
    }
    let volume = cfg.volume.clamp(0, 100);
    if volume == 0 {
        return;
    }
    let path = {
        let file = custom(cfg, cue);
        if !file.trim().is_empty() {
            let p = PathBuf::from(file.trim());
            if p.is_file() {
                Some(p)
            } else {
                None
            }
        } else {
            builtin(&cfg.pack, cue, volume)
        }
    };
    let Some(path) = path else {
        return;
    };
    spawn_player(&path);
}

fn spawn_player(path: &std::path::Path) {
    use std::process::{Command, Stdio};
    let p = path.to_string_lossy().to_string();
    let candidates: Vec<(&str, Vec<String>)> = vec![
        ("pw-play", vec![p.clone()]),
        ("paplay", vec![p.clone()]),
        ("aplay", vec!["-q".into(), p.clone()]),
        (
            "ffplay",
            vec![
                "-nodisp".into(),
                "-autoexit".into(),
                "-loglevel".into(),
                "quiet".into(),
                p.clone(),
            ],
        ),
        (
            "mpv",
            vec![
                "--no-video".into(),
                "--really-quiet".into(),
                p.clone(),
            ],
        ),
    ];
    for (bin, args) in candidates {
        if sdw_core::util::which(bin).is_none() {
            continue;
        }
        let spawned = Command::new(bin)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if spawned.is_ok() {
            return;
        }
    }
}

/// Render (and cache) a built-in cue at the requested volume.
fn builtin(pack: &str, cue: Cue, volume: u8) -> Option<PathBuf> {
    let dir = sdw_core::util::cache_dir().join("sounds");
    sdw_core::util::ensure_dir(&dir).ok()?;
    let name = format!("{}-{}-{}.wav", sanitize(pack), cue.key(), volume);
    let path = dir.join(&name);
    if path.is_file() {
        return Some(path);
    }
    let samples = synth(pack, cue, volume as f32 / 100.0);
    let bytes = wav_bytes(&samples);
    let mut tmp = path.clone();
    tmp.set_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp).ok()?;
        f.write_all(&bytes).ok()?;
    }
    std::fs::rename(&tmp, &path).ok()?;
    Some(path)
}

fn sanitize(s: &str) -> String {
    let clean: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if clean.is_empty() {
        "grid".into()
    } else {
        clean
    }
}

const RATE: u32 = 44_100;

#[derive(Clone, Copy)]
enum Wave {
    Sine,
    Tri,
    Square,
}

struct Note {
    from: f32,
    to: f32,
    secs: f32,
    gain: f32,
    wave: Wave,
    /// Extra silence appended after the note, in seconds.
    tail: f32,
}

fn note(from: f32, to: f32, secs: f32, gain: f32, wave: Wave) -> Note {
    Note {
        from,
        to,
        secs,
        gain,
        wave,
        tail: 0.0,
    }
}

fn with_tail(mut n: Note, tail: f32) -> Note {
    n.tail = tail;
    n
}

fn recipe(pack: &str, cue: Cue) -> Vec<Note> {
    match pack {
        // Soft: rounded sines, gentle attack, lower register.
        "soft" => match cue {
            Cue::Click => vec![note(520.0, 500.0, 0.06, 0.22, Wave::Sine)],
            Cue::Navigate => vec![note(392.0, 440.0, 0.11, 0.20, Wave::Sine)],
            Cue::Open => vec![note(392.0, 587.0, 0.16, 0.20, Wave::Sine)],
            Cue::Back => vec![note(587.0, 392.0, 0.16, 0.20, Wave::Sine)],
            Cue::Notify => vec![
                note(523.0, 523.0, 0.10, 0.18, Wave::Sine),
                note(659.0, 659.0, 0.14, 0.18, Wave::Sine),
            ],
            Cue::Error => vec![
                note(311.0, 311.0, 0.12, 0.20, Wave::Sine),
                note(247.0, 247.0, 0.20, 0.20, Wave::Sine),
            ],
            Cue::Startup => vec![
                note(330.0, 330.0, 0.14, 0.18, Wave::Sine),
                note(440.0, 440.0, 0.14, 0.18, Wave::Sine),
                note(550.0, 550.0, 0.26, 0.18, Wave::Sine),
            ],
        },
        // Chip: square edges, quick decay, arcade feel.
        "chip" => match cue {
            Cue::Click => vec![note(1046.0, 1046.0, 0.035, 0.14, Wave::Square)],
            Cue::Navigate => vec![note(784.0, 988.0, 0.06, 0.13, Wave::Square)],
            Cue::Open => vec![note(523.0, 1046.0, 0.10, 0.13, Wave::Square)],
            Cue::Back => vec![note(988.0, 523.0, 0.10, 0.13, Wave::Square)],
            Cue::Notify => vec![
                note(880.0, 880.0, 0.05, 0.13, Wave::Square),
                with_tail(note(1318.0, 1318.0, 0.09, 0.13, Wave::Square), 0.02),
            ],
            Cue::Error => vec![
                note(220.0, 196.0, 0.09, 0.14, Wave::Square),
                note(165.0, 147.0, 0.16, 0.14, Wave::Square),
            ],
            Cue::Startup => vec![
                note(523.0, 523.0, 0.07, 0.12, Wave::Square),
                note(784.0, 784.0, 0.07, 0.12, Wave::Square),
                note(1046.0, 1046.0, 0.07, 0.12, Wave::Square),
                note(1318.0, 1318.0, 0.16, 0.12, Wave::Square),
            ],
        },
        // Grid: the default. Clean, quiet, glassy — a UI that clicks.
        _ => match cue {
            Cue::Click => vec![note(880.0, 880.0, 0.035, 0.16, Wave::Sine)],
            Cue::Navigate => vec![note(587.0, 740.0, 0.09, 0.16, Wave::Tri)],
            Cue::Open => vec![note(440.0, 880.0, 0.13, 0.16, Wave::Tri)],
            Cue::Back => vec![note(880.0, 440.0, 0.13, 0.16, Wave::Tri)],
            Cue::Notify => vec![
                note(740.0, 740.0, 0.08, 0.15, Wave::Sine),
                with_tail(note(988.0, 988.0, 0.16, 0.15, Wave::Sine), 0.02),
            ],
            Cue::Error => vec![
                note(247.0, 247.0, 0.10, 0.15, Wave::Tri),
                note(185.0, 185.0, 0.20, 0.15, Wave::Tri),
            ],
            Cue::Startup => vec![
                note(330.0, 330.0, 0.09, 0.14, Wave::Sine),
                note(494.0, 494.0, 0.09, 0.14, Wave::Sine),
                note(660.0, 660.0, 0.09, 0.14, Wave::Sine),
                note(988.0, 988.0, 0.24, 0.14, Wave::Sine),
            ],
        },
    }
}

fn synth(pack: &str, cue: Cue, volume: f32) -> Vec<i16> {
    let notes = recipe(pack, cue);
    let total: f32 = notes.iter().map(|n| n.secs + n.tail).sum();
    let mut out = Vec::with_capacity((total * RATE as f32) as usize + 1);
    let mut phase = 0.0f32;
    for n in notes {
        let count = (n.secs * RATE as f32).max(1.0) as usize;
        for i in 0..count {
            let t = i as f32 / RATE as f32;
            let k = i as f32 / count as f32;
            let freq = n.from + (n.to - n.from) * k;
            phase += freq / RATE as f32;
            let p = phase.fract();
            let raw = match n.wave {
                Wave::Sine => (p * std::f32::consts::TAU).sin(),
                Wave::Tri => 1.0 - 4.0 * (p - 0.5).abs(),
                Wave::Square => {
                    if p < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
            };
            // 6 ms attack, exponential decay; avoids clicks and sounds placed.
            let attack = (t / 0.006).min(1.0);
            let decay = (-t * 9.0).exp();
            let env = attack * decay;
            let v = raw * env * n.gain * volume * 0.9;
            out.push((v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
        }
        let pad = (n.tail * RATE as f32) as usize;
        out.extend(std::iter::repeat(0).take(pad));
    }
    out
}

fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    b.extend_from_slice(&2u16.to_le_bytes()); // block align
    b.extend_from_slice(&16u16.to_le_bytes()); // bits
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}
