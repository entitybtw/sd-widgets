//! The deck's own sound going to the computer, next to its picture.
//!
//! While somebody on the computer watches the deck page, `parec` records what
//! the deck plays on its default output and this module posts small raw
//! `s16le` chunks to `POST /api/v1/deck/audio`. The daemon plays them with
//! `pw-cat`/`paplay`, so no codec is linked into either side.
//!
//! Capture is a separate process and a separate thread: a quiet deck costs
//! nothing, and a failed capture leaves the picture alone.

use sdw_core::api::Client;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// One chunk is 100 ms of stereo 48 kHz s16: small enough to stay in step with
/// the picture, big enough that the HTTP overhead disappears.
const CHUNK: usize = 19_200;
const RATE: u32 = 48_000;
const CHANNELS: u8 = 2;

pub struct ShareAudio {
    stop: Arc<AtomicBool>,
    /// Kept so the process can be stopped if the thread is wedged.
    child: Option<Arc<std::sync::Mutex<Child>>>,
}

impl ShareAudio {
    /// Start recording the deck's default output. Fails at once when there is
    /// no `parec`, so the settings page can say why instead of staying silent.
    pub fn start(client: Client) -> Result<Self, String> {
        if !sdw_core::util::has("parec") {
            return Err("на деке нет parec (нужен pipewire-pulse или pulseaudio-utils)".into());
        }
        let mut child = Command::new("parec")
            .args([
                "--device=@DEFAULT_MONITOR@",
                "--format=s16le",
                &format!("--rate={RATE}"),
                &format!("--channels={CHANNELS}"),
                "--latency-msec=40",
                "--raw",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("parec: {e}"))?;
        let Some(mut out) = child.stdout.take() else {
            let _ = child.kill();
            return Err("parec не отдал поток".into());
        };
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::Builder::new()
            .name("sdw-share-audio".into())
            .spawn(move || {
                let mut buf = vec![0u8; CHUNK];
                let mut failures = 0u32;
                while !flag.load(Ordering::Relaxed) {
                    match out.read_exact(&mut buf) {
                        Ok(()) => {}
                        Err(_) => break,
                    }
                    if flag.load(Ordering::Relaxed) {
                        break;
                    }
                    match client.push_deck_audio(RATE, CHANNELS, &buf) {
                        Ok(_) => failures = 0,
                        Err(_) => {
                            // A closed page is normal; three misses in a row
                            // mean the lane is dead, so let it end.
                            failures += 1;
                            if failures >= 3 {
                                break;
                            }
                        }
                    }
                }
            })
            .ok();
        Ok(Self {
            stop,
            child: Some(Arc::new(std::sync::Mutex::new(child))),
        })
    }
}

impl Drop for ShareAudio {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(child) = &self.child {
            if let Ok(mut c) = child.lock() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
}

/// Wait a moment between a page opening and capture starting, so the first
/// second of a burst is not fought over by the screenshot and `parec`.
pub fn settle() {
    std::thread::sleep(Duration::from_millis(150));
}
