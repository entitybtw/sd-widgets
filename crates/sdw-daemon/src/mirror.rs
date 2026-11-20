//! What the deck is showing right now.
//!
//! The deck pushes its own screen as JPEG frames to `POST /api/v1/deck/frame`;
//! this module keeps the newest one plus a small list of the decks that are
//! sending, so the daemon window can show the deck and say who is connected.
//! Nothing here talks to the network or blocks a frame: the store is a mutex
//! over a few kilobytes and a clone on read.
//!
//! A frame is a whole deck screen, and moving a few of them a second on top of
//! the stream and the status poll is not free. So the daemon says out loud
//! when the picture is on screen: `watching` is held only while the window or
//! the API is actually showing it, and the deck shares more slowly when it
//! lapses.

use sdw_core::model::{now_ms, DeckMirrorClient, DeckMirrorState};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// A deck counts as online while frames keep coming: this is the grace after
/// the last one. Short enough that a closed deck disappears quickly, long
/// enough that a hiccup in the Wi-Fi does not make it flicker.
pub const ONLINE_MS: u64 = 12_000;

struct Frame {
    bytes: Vec<u8>,
    at_ms: u64,
    width: u32,
    height: u32,
    page: String,
    seq: u64,
}

struct Client {
    info: DeckMirrorClient,
}

#[derive(Default)]
struct Inner {
    frame: Option<Frame>,
    clients: HashMap<String, Client>,
    seq: u64,
}

pub struct Mirror {
    inner: Mutex<Inner>,
    /// Somebody is showing the picture right now (the window, an API poll).
    watching: AtomicBool,
    /// Unix ms until which watching stays on: every look pushes it forward,
    /// so two viewers do not cancel each other out.
    watch_until: AtomicU64,
}

impl Mirror {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            watching: AtomicBool::new(false),
            watch_until: AtomicU64::new(0),
        }
    }

    /// Record that somebody is looking at the deck right now. A deck shares
    /// at full rate while this holds and slows down when it lapses.
    pub fn watching(&self) {
        self.watch_until
            .store(now_ms().saturating_add(8_000), Ordering::Relaxed);
        self.watching.store(true, Ordering::Relaxed);
    }

    pub fn is_watched(&self) -> bool {
        self.watching.load(Ordering::Relaxed) && now_ms() < self.watch_until.load(Ordering::Relaxed)
    }

    /// Store one frame from a deck.
    ///
    /// The id comes from the deck itself (`client_id` in its config), so a
    /// reconnecting deck updates its old row instead of adding a second one.
    /// Frames are kept only as the latest: this is a live view, not a history.
    #[allow(clippy::too_many_arguments)]
    pub fn ingest(
        &self,
        id: &str,
        name: &str,
        page: &str,
        version: &str,
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    ) {
        if bytes.is_empty() {
            return;
        }
        let id = if id.trim().is_empty() {
            "deck".to_string()
        } else {
            id.trim().to_string()
        };
        let at = now_ms();
        let mut inner = match self.inner.lock() {
            Ok(i) => i,
            Err(e) => e.into_inner(),
        };
        inner.seq = inner.seq.wrapping_add(1);
        let client = inner.clients.entry(id.clone()).or_insert_with(|| Client {
            info: DeckMirrorClient {
                id: id.clone(),
                ..DeckMirrorClient::default()
            },
        });
        if !name.trim().is_empty() {
            client.info.name = name.trim().to_string();
        }
        client.info.page = page.trim().to_string();
        client.info.version = version.trim().to_string();
        client.info.width = width;
        client.info.height = height;
        client.info.last_ms = at;
        client.info.frames = client.info.frames.saturating_add(1);
        client.info.bytes = client.info.bytes.saturating_add(bytes.len() as u64);
        client.info.online = true;
        if client.info.name.is_empty() {
            client.info.name = id.clone();
        }
        inner.frame = Some(Frame {
            bytes,
            at_ms: at,
            width,
            height,
            page: page.trim().to_string(),
            seq: inner.seq,
        });
    }

    /// The newest frame and the sequence number that names it, so a viewer can
    /// skip decoding a frame it has already shown.
    pub fn frame(&self) -> Option<(Vec<u8>, u64)> {
        let inner = self.inner.lock().ok()?;
        inner.frame.as_ref().map(|f| (f.bytes.clone(), f.seq))
    }

    pub fn seq(&self) -> u64 {
        self.inner.lock().map(|i| i.seq).unwrap_or(0)
    }

    /// One snapshot for the window and the API: whether a frame is recent
    /// enough to call live, and who is sending.
    pub fn state(&self) -> DeckMirrorState {
        let now = now_ms();
        let mut out = DeckMirrorState::default();
        let inner = match self.inner.lock() {
            Ok(i) => i,
            Err(e) => e.into_inner(),
        };
        if let Some(f) = &inner.frame {
            out.frame_ms = f.at_ms;
            out.width = f.width;
            out.height = f.height;
            out.page = f.page.clone();
            out.seq = f.seq;
            out.live = now.saturating_sub(f.at_ms) < ONLINE_MS;
        }
        let mut clients: Vec<DeckMirrorClient> = inner
            .clients
            .values()
            .map(|c| {
                let mut info = c.info.clone();
                info.online = now.saturating_sub(info.last_ms) < ONLINE_MS;
                info
            })
            .collect();
        // The deck that sent the newest frame is the one being watched.
        clients.sort_by(|a, b| {
            b.online
                .cmp(&a.online)
                .then(b.last_ms.cmp(&a.last_ms))
                .then(a.name.cmp(&b.name))
        });
        out.clients = clients;
        out.watched = self.is_watched();
        out
    }
}

impl Default for Mirror {
    fn default() -> Self {
        Self::new()
    }
}
