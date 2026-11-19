//! SD Widgets shared crate: protocol models, HTTP client, discovery, UDP gamepad packets.

pub mod api;
pub mod auth;
pub mod config;
pub mod discovery;
pub mod ha;
pub mod i18n;
pub mod model;
pub mod udp;
pub mod update;
pub mod util;

pub use i18n::{tr, Lang};
pub use model::*;

/// Convenience: the persisted Deck config path.
pub fn deck_config_path() -> std::path::PathBuf {
    config::path()
}
