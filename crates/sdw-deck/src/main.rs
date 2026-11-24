//! sd-widgets for the steam deck.
//!
//! a touch-first control surface: drawn gamepad, binds, extra screen, notifications,
//! home assistant and pairing with the computer daemon.

mod actions;
mod addons_ui;
mod app;
mod audio_ui;
mod binds;
mod clock;
mod components;
mod deck_view;
mod ha_link;
mod input;
mod pages;
mod share_audio;
mod sound;
mod sys;
mod theme;
mod timers;
mod viz;
mod viz_stage;

use clap::Parser;
use eframe::egui;
use sdw_core::model::DeckConfig;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "sdw-deck",
    version,
    about = "sd-widgets for the Steam Deck",
    long_about = None
)]
struct Cli {
    /// start in fullscreen
    #[arg(long)]
    fullscreen: bool,
    /// force a window, even on a handheld
    #[arg(long)]
    windowed: bool,
    /// pair with or switch to a computer by name, id or address
    #[arg(long)]
    host: Option<String>,
    /// open a page: home, computers, gamepad, screen, notifications, ha, binds, settings
    #[arg(long)]
    page: Option<String>,
    /// forget the local configuration and start over
    #[arg(long)]
    reset: bool,
    /// print the config path and exit
    #[arg(long)]
    print_config: bool,
    /// write the config to this path instead of the default
    #[arg(long)]
    config: Option<PathBuf>,
}

fn main() -> eframe::Result<()> {
    let cli = Cli::parse();

    let mut cfg = match &cli.config {
        Some(path) => std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<DeckConfig>(&text).ok())
            .unwrap_or_default(),
        None => sdw_core::config::load(),
    };
    // A file given with `--config` skips `load`, so the migration runs here for
    // both paths; it is idempotent.
    cfg = sdw_core::config::migrate(cfg);

    if cli.reset {
        cfg = DeckConfig::default();
    }
    if let Some(page) = &cli.page {
        cfg.last_page = page.clone();
    }
    if let Some(needle) = &cli.host {
        let needle = needle.trim();
        let found = cfg
            .hosts
            .iter()
            .find(|h| {
                h.id == needle || h.name.eq_ignore_ascii_case(needle) || h.url.contains(needle)
            })
            .map(|h| h.id.clone());
        match found {
            Some(id) => cfg.active_host = Some(id),
            None => eprintln!("sd-widgets: computer «{needle}» is not paired yet"),
        }
    }

    if cli.print_config {
        let path = cli.config.clone().unwrap_or_else(sdw_core::config::path);
        println!("config: {}", path.display());
        println!("device: {}", cfg.device_name);
        println!("hosts:  {}", cfg.hosts.len());
        for host in &cfg.hosts {
            println!("  {}  {}", host.name, host.url);
        }
        return Ok(());
    }

    sdw_core::util::ensure_dir(&sdw_core::util::config_dir()).ok();
    if let Some(path) = &cli.config {
        if let Some(parent) = path.parent() {
            sdw_core::util::ensure_dir(parent).ok();
        }
        let json = serde_json::to_vec_pretty(&cfg).unwrap_or_default();
        let _ = sdw_core::util::atomic_write(path, &json);
    } else if let Err(e) = sdw_core::config::save(&cfg) {
        eprintln!("sd-widgets: cannot save the config: {e}");
    }

    let fullscreen = if cli.windowed {
        false
    } else {
        cli.fullscreen || default_fullscreen()
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("sd-widgets")
        .with_app_id("sd-widgets")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([880.0, 620.0])
        .with_fullscreen(fullscreen);
    if let Some(icon) = theme::window_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "sd-widgets",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, cfg)))),
    )
}

fn default_fullscreen() -> bool {
    if std::env::var_os("SDW_WINDOWED").is_some() {
        return false;
    }
    if std::env::var_os("SDW_FULLSCREEN").is_some() {
        return true;
    }
    if std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some() {
        return true;
    }
    sdw_core::util::is_steam_deck()
}
