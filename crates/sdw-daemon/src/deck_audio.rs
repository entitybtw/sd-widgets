//! The deck's own sound on the computer, next to its picture.
//!
//! The deck records whatever it plays (`parec` on the monitor of its default
//! output) and posts raw `s16le` chunks to `POST /api/v1/deck/audio`. This
//! module hands them to one long-lived player process (`pw-cat` or `paplay` on
//! Linux, `ffplay` on Windows), so the sound starts quickly and stays in step
//! with the picture.
//!
//! Nothing is captured or played unless somebody on the computer is looking at
//! the deck page: the deck only sends while the mirror is watched, and the
//! player is closed as soon as chunks stop arriving.

use sdw_core::model::now_ms;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Close the player after this long without a chunk. A paused deck (or a
/// closed viewer) must not leave a live sink open on the computer.
const IDLE_MS: u64 = 1500;

struct Inner {
    player: Option<Child>,
    /// The rate/channels the running player was opened with.
    rate: u32,
    channels: u8,
}

pub struct DeckAudio {
    inner: Mutex<Inner>,
    last_ms: AtomicU64,
    bytes: AtomicU64,
    error: Mutex<String>,
}

impl Default for DeckAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl DeckAudio {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                player: None,
                rate: 0,
                channels: 0,
            }),
            last_ms: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            error: Mutex::new(String::new()),
        }
    }

    /// Play one chunk of PCM. The first chunk of a burst starts the player;
    /// the rest are written straight into its stdin.
    pub fn play(&self, rate: u32, channels: u8, pcm: &[u8]) -> Result<(), String> {
        let rate = rate.clamp(8_000, 192_000);
        let channels = channels.clamp(1, 8);
        let mut inner = self.inner.lock().map_err(|_| "audio lock".to_string())?;
        if inner.player.is_none() || inner.rate != rate || inner.channels != channels {
            inner.player = Some(spawn(rate, channels)?);
            inner.rate = rate;
            inner.channels = channels;
        }
        let Some(child) = inner.player.as_mut() else {
            return Err("плеер не запустился".into());
        };
        let Some(stdin) = child.stdin.as_mut() else {
            return Err("плеер закрыл вход".into());
        };
        if let Err(e) = stdin.write_all(pcm) {
            // The player died (a broken pipe): drop it so the next chunk opens
            // a fresh one instead of failing forever.
            inner.player = None;
            return Err(format!("звук не принят: {e}"));
        }
        self.last_ms.store(now_ms(), Ordering::Relaxed);
        self.bytes.fetch_add(pcm.len() as u64, Ordering::Relaxed);
        *self.error.lock().unwrap_or_else(|e| e.into_inner()) = String::new();
        Ok(())
    }

    /// Close the player once the deck has gone quiet. Called from a small
    /// background tick; returning `true` means the sink was closed.
    pub fn idle(&self) -> bool {
        let last = self.last_ms.load(Ordering::Relaxed);
        if last == 0 || now_ms().saturating_sub(last) < IDLE_MS {
            return false;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        if inner.player.is_none() {
            return false;
        }
        if let Some(mut child) = inner.player.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        true
    }

    pub fn playing(&self) -> bool {
        self.inner
            .lock()
            .map(|i| i.player.is_some())
            .unwrap_or(false)
    }

    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    pub fn error(&self) -> String {
        self.error.lock().map(|e| e.clone()).unwrap_or_default()
    }

    pub fn stop(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(mut child) = inner.player.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

/// One long-lived raw PCM player, reading the chunks from stdin.
///
/// The two tools disagree on the format name: `pw-cat` calls 16-bit
/// little-endian "s16", `paplay` calls it "s16le".
fn spawn(rate: u32, channels: u8) -> Result<Child, String> {
    #[cfg(windows)]
    {
        // Windows has no pw-cat/paplay; ffplay reads the same raw stream from
        // its standard input and goes straight to the default output.
        if sdw_core::util::has("ffplay") {
            return Command::new("ffplay")
                .args([
                    "-nodisp".to_string(),
                    "-autoexit".to_string(),
                    "-loglevel".to_string(),
                    "quiet".to_string(),
                    "-f".to_string(),
                    "s16le".to_string(),
                    "-ar".to_string(),
                    rate.to_string(),
                    "-ac".to_string(),
                    channels.to_string(),
                    "-".to_string(),
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("ffplay: {e}"));
        }
        return Err("нужен ffplay (входит в ffmpeg)".into());
    }
    #[cfg(not(windows))]
    {
        let mut candidates: Vec<(&str, Vec<String>)> = Vec::new();
        if sdw_core::util::has("pw-cat") {
            candidates.push((
                "pw-cat",
                vec![
                    "--playback".into(),
                    "--raw".into(),
                    "--format=s16".into(),
                    format!("--rate={rate}"),
                    format!("--channels={channels}"),
                    "-".into(),
                ],
            ));
        }
        if sdw_core::util::has("paplay") {
            candidates.push((
                "paplay",
                vec![
                    "--raw".into(),
                    "--format=s16le".into(),
                    format!("--rate={rate}"),
                    format!("--channels={channels}"),
                    "--".into(),
                ],
            ));
        }
        let mut last = String::from("нужен pw-cat или paplay (PipeWire или PulseAudio)");
        for (bin, args) in candidates {
            match Command::new(bin)
                .args(&args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(child) => return Ok(child),
                Err(e) => last = format!("{bin}: {e}"),
            }
        }
        Err(last)
    }
}

/// A background tick that closes an idle player and stops it at shutdown.
pub fn watch(
    store: std::sync::Arc<DeckAudio>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    std::thread::Builder::new()
        .name("sdw-deck-audio".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                store.idle();
                std::thread::sleep(Duration::from_millis(500));
            }
            store.stop();
        })
        .ok();
}
