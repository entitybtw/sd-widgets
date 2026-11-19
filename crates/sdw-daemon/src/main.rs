//! sd-widgets daemon: virtual gamepads, extra screen, notifications and Home Assistant.

mod audio;
mod config;
#[cfg(feature = "gui")]
mod gui;
mod install;
mod media;
mod monitor;
mod notifications;
mod server;
mod state;
mod tray;
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
    /// ask git.entitybtw.ru for the newest release, then exit
    #[arg(long)]
    check_updates: bool,
    /// download the newest daemon build from git.entitybtw.ru, then exit
    #[arg(long)]
    download_update: bool,
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
    if cli.check_updates || cli.download_update {
        match sdw_core::update::check() {
            Ok(info) => {
                println!(
                    "сейчас {} · на git.entitybtw.ru {}",
                    info.current, info.latest
                );
                if info.newer {
                    println!("доступно обновление: {}", info.name);
                } else {
                    println!("это последняя версия");
                }
                println!("страница: {}", info.page);
                for a in &info.assets {
                    println!("  файл {} ({} байт) {}", a.name, a.size, a.url);
                }
                if cli.download_update {
                    match info.asset_for("sdw-daemon") {
                        Some(asset) => {
                            let dest = sdw_core::update::updates_dir().join(&asset.name);
                            println!("скачиваю в {}", dest.display());
                            let progress = Arc::new(std::sync::atomic::AtomicU64::new(0));
                            match sdw_core::update::download(asset, &dest, progress) {
                                Ok(bytes) => println!("готово: {bytes} байт"),
                                Err(e) => {
                                    eprintln!("скачивание: {e}");
                                    std::process::exit(1);
                                }
                            }
                        }
                        None => {
                            eprintln!("в релизе нет файла для этого компьютера");
                            std::process::exit(1);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("проверка обновлений: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    if cli.print_config {
        println!("config: {}", config::path().display());
        println!(
            "api:    {}",
            sdw_core::util::addresses(cfg.api_port).join("  ")
        );
        println!("input:  udp :{}", cfg.input_port);
        println!("viiper: {}", cfg.viiper.api_addr);
        println!("setup:  {}", cfg.setup_done);
        return;
    }

    // One daemon per user: a second process would own the notification bus or
    // the API while the first one serves the deck, and notifications vanish
    // between them. Keep the lock file open for the whole run.
    let _instance_lock = match single_instance() {
        Ok(lock) => lock,
        Err(pid) => {
            eprintln!(
                "sdw-daemon уже запущен (pid {pid}). остановите его или перезагрузитесь: \
                 два демона делят D-Bus и API и уведомления теряются."
            );
            std::process::exit(1);
        }
    };

    let state = AppState::new();
    if !state.cfg().viiper.password.is_empty() {
        let _ = state.viiper.write_key(&state.cfg().viiper.password);
    }
    // The tray keeps the daemon reachable while every window is closed. It is
    // optional: some sessions have no StatusNotifierWatcher at all.
    let _tray = if state.cfg().tray {
        tray::start(state.clone())
    } else {
        None
    };

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

/// Keep one daemon per user. The lock file holds the pid for the message and
/// never leaves git or the config dir.
fn single_instance() -> Result<std::fs::File, u32> {
    let path = sdw_core::util::config_dir().join("daemon.lock");
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
    {
        Ok(f) => f,
        // A read-only or missing config dir must not block the daemon.
        Err(_) => {
            return Ok(std::fs::File::open(&path).unwrap_or_else(|_| {
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(std::env::temp_dir().join("sdw-daemon.lock"))
                    .expect("lock file")
            }))
        }
    };
    #[cfg(unix)]
    {
        use std::io::{Read, Seek, SeekFrom, Write};
        use std::os::unix::io::AsRawFd;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let mut text = String::new();
            let mut probe = file.try_clone().ok();
            if let Some(f) = probe.as_mut() {
                let _ = f.seek(SeekFrom::Start(0));
                let _ = f.read_to_string(&mut text);
            }
            return Err(text.trim().parse().unwrap_or(0));
        }
        let mut f = &file;
        let _ = f.set_len(0);
        let _ = writeln!(f, "{}", std::process::id());
        return Ok(file);
    }
    #[cfg(not(unix))]
    {
        Ok(file)
    }
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
