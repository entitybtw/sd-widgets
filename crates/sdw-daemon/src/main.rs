//! sd-widgets daemon: virtual gamepads, extra screen, notifications and Home Assistant.

mod config;
#[cfg(feature = "gui")]
mod gui;
mod install;
mod monitor;
mod notifications;
mod server;
mod state;
mod viiper;

use clap::Parser;
use sdw_core::model::VERSION;
use state::AppState;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(
    name = "sdw-daemon",
    version,
    about = "sd-widgets daemon for the computer",
    long_about = None
)]
struct Cli {
    /// run without a window (server only)
    #[arg(long)]
    headless: bool,
    /// override the api port (also moves the udp input port)
    #[arg(long)]
    port: Option<u16>,
    /// forget the pairing password and start setup again
    #[arg(long)]
    reset_password: bool,
    /// print the config path and addresses, then exit
    #[arg(long)]
    print_config: bool,
}

fn main() {
    let cli = Cli::parse();
    sdw_core::util::ensure_dir(&sdw_core::util::config_dir()).ok();

    let mut cfg = config::load();
    if let Some(port) = cli.port {
        cfg.api_port = port;
        cfg.input_port = port.saturating_add(1);
        let _ = config::save(&cfg);
    }
    if cli.reset_password {
        cfg.api_password_hash.clear();
        cfg.api_token = sdw_core::model::random_id(24);
        cfg.setup_done = false;
        let _ = config::save(&cfg);
        println!("пароль сброшен; при следующем запуске откроется настройка");
    }
    if cli.print_config {
        println!("config: {}", config::path().display());
        println!("api:    {}", sdw_core::util::addresses(cfg.api_port).join("  "));
        println!("input:  udp :{}", cfg.input_port);
        println!("viiper: {}", cfg.viiper.api_addr);
        println!("setup:  {}", cfg.setup_done);
        return;
    }

    let state = AppState::new();
    if !state.cfg().viiper.password.is_empty() {
        let _ = state.viiper.write_key(&state.cfg().viiper.password);
    }

    let want_window = !cli.headless && has_display();
    #[cfg(not(feature = "gui"))]
    let want_window = false;

    println!(
        "sd-widgets {} · {} · {}",
        VERSION,
        sdw_core::util::hostname(),
        sdw_core::model::platform()
    );
    for addr in sdw_core::util::addresses(state.cfg().api_port) {
        println!("  api   {addr}");
    }
    println!("  input udp :{}", state.cfg().input_port);
    println!("  config {}", config::path().display());

    #[cfg(feature = "gui")]
    if want_window {
        let server_state = state.clone();
        std::thread::spawn(move || {
            if let Err(e) = server::run(server_state.clone()) {
                server_state
                    .logger
                    .error(format!("http сервер не запустился: {e}"));
            }
        });
        let result = gui::run(state.clone());
        state.shutdown();
        if let Err(e) = result {
            eprintln!("gui: {e}");
            headless(state);
        }
        return;
    }

    headless(state);
}

fn headless(state: Arc<AppState>) {
    if let Err(e) = server::run(state.clone()) {
        eprintln!("http сервер не запустился: {e}");
        state.shutdown();
        std::process::exit(1);
    }
    state.shutdown();
}

fn has_display() -> bool {
    #[cfg(windows)]
    {
        true
    }
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
    }
}
