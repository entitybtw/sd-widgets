//! sd-widgets daemon: virtual gamepads, extra screen, notifications and Home Assistant.

mod apps;
#[cfg(not(windows))]
mod audio;
#[cfg(windows)]
#[path = "win/audio.rs"]
mod audio;
mod autostart;
#[cfg(not(windows))]
mod bluetooth;
#[cfg(windows)]
#[path = "win/bluetooth.rs"]
mod bluetooth;
mod config;
mod deck_audio;
// Sample conversion for the Windows audio capture; tested on every platform.
#[cfg_attr(not(windows), allow(dead_code))]
mod dsp;
#[cfg(feature = "gui")]
mod gui;
mod install;
#[cfg(not(windows))]
mod media;
#[cfg(windows)]
#[path = "win/media.rs"]
mod media;
mod mirror;
mod monitor;
mod notifications;
mod restart;
mod server;
mod spectrum;
mod state;
mod tray;
mod viiper;
// The Virtual Display Driver settings editor is pure text handling; on Linux
// it is only compiled for its tests and to keep `cargo check` honest.
#[cfg_attr(not(windows), allow(dead_code))]
mod vdd;
#[cfg(windows)]
mod win;

use clap::Parser;
use sdw_core::model::VERSION;
use state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    /// start with the window hidden, as at login
    #[arg(long)]
    hidden: bool,
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
    /// bring back the window of a running daemon, then exit
    #[arg(long)]
    show: bool,
    /// stop a running daemon, then exit
    #[arg(long)]
    quit: bool,
    /// wait for a daemon that is shutting down to let go, then take its place
    /// (the restart button in the window uses this)
    #[arg(long)]
    restart: bool,
}

fn main() {
    let cli = Cli::parse();
    sdw_core::util::ensure_dir(&sdw_core::util::config_dir()).ok();
    // On Windows, tools downloaded into <data>/bin (ffmpeg) must be found by
    // the capture code, which just runs `ffmpeg` from PATH.
    #[cfg(windows)]
    install::prepend_bin_to_path();

    // Two small signals instead of a control socket: a second launch, the tray
    // and the CLI all talk to the running daemon through one-shot files.
    if cli.show {
        config::request("show");
        println!("окно демона открывается снова");
        return;
    }
    if cli.quit {
        config::request("quit");
        println!("демон останавливается");
        return;
    }

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
        let remote = sdw_core::update::Remote::parse(&cfg.update_remote).unwrap_or_default();
        match sdw_core::update::check_at(&remote) {
            Ok(info) => {
                println!(
                    "сейчас {} · на {} {}",
                    info.current,
                    remote.label(),
                    info.latest
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
    let _instance_lock = match single_instance(cli.restart) {
        Ok(lock) => lock,
        Err(pid) => {
            // Starting the daemon again is the natural way to ask for its
            // window. A headless copy has no window to show, and says so.
            if config::window_marker().exists() {
                config::request("show");
                eprintln!(
                    "sdw-daemon уже запущен (pid {pid}); окно открывается снова. \
                     остановить: sdw-daemon --quit"
                );
            } else {
                eprintln!(
                    "sdw-daemon уже работает в фоне (pid {pid}); остановить: \
                     sdw-daemon --quit"
                );
            }
            std::process::exit(0);
        }
    };

    // A marker from a previous run would claim a window that is not there.
    let _ = std::fs::remove_file(config::window_marker());

    let state = AppState::new();
    // Read Bluetooth once in the background, so the first deck poll after a
    // login already has a snapshot instead of an empty adapter row.
    bluetooth::warm(sdw_core::Lang::from_code(&state.cfg().lang));
    let _ = autostart::apply(state.cfg().autostart, &state.logger);
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

    catch_stop_signals();

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
        let result = gui::run(state.clone(), cli.hidden);
        if let Err(e) = result {
            eprintln!("gui: {e}");
            headless(state);
            return;
        }
        // The window only closes for real on an explicit quit; stopping here is
        // the clean path for that case.
        state.shutdown();
        return;
    }

    headless(state);
}

fn headless(state: Arc<AppState>) {
    // Without a window the daemon still stops on the same request file, so
    // `sdw-daemon --quit` and the tray keep working.
    let watch_state = state.clone();
    std::thread::Builder::new()
        .name("sdw-daemon-watch".into())
        .spawn(move || loop {
            if config::take_request("quit") || stop_requested() {
                watch_state.shutdown();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(400));
        })
        .ok();
    println!("sdw-widgets работает в фоне; остановить: sdw-daemon --quit");
    if let Err(e) = server::run(state.clone()) {
        eprintln!("http сервер не запустился: {e}");
        state.shutdown();
        std::process::exit(1);
    }
    state.shutdown();
}

/// Set when the process is asked to end by a signal: systemd sends SIGTERM on
/// a restart and at logout, a terminal sends SIGINT on Ctrl-C.
static STOP: AtomicBool = AtomicBool::new(false);

fn stop_requested() -> bool {
    STOP.load(Ordering::Relaxed)
}

#[cfg(unix)]
extern "C" fn on_stop_signal(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

/// Signals take the same clean road as `--quit`: streams, the monitor and the
/// virtual gamepads are stopped before the process ends. Left to the default
/// action they would be cut off mid-flight, and a restart would inherit them.
fn catch_stop_signals() {
    #[cfg(unix)]
    unsafe {
        let handler = on_stop_signal as *const () as libc::sighandler_t;
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    }
}

/// Keep one daemon per user. The lock file holds the pid for the message and
/// never leaves git or the config dir.
///
/// `wait` is the `--restart` hand-over: the replacement is started before the
/// old daemon has finished, so it waits for the lock instead of deciding that
/// another daemon owns it and bouncing off.
fn single_instance(wait: bool) -> Result<std::fs::File, u32> {
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
        // A hand-over waits a good while: the old daemon may still be stopping
        // its streams and helper processes when the replacement is spawned.
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if rc == 0 {
                break;
            }
            if !wait || Instant::now() >= deadline {
                let mut text = String::new();
                let mut probe = file.try_clone().ok();
                if let Some(f) = probe.as_mut() {
                    let _ = f.seek(SeekFrom::Start(0));
                    let _ = f.read_to_string(&mut text);
                }
                return Err(text.trim().parse().unwrap_or(0));
            }
            // The replacement is spawned while the old daemon is still alive.
            // The lock is let go at the end of `main`, a hair before the kernel
            // closes its sockets and its notification bus name: binding those
            // first would leave the new daemon without LAN discovery or the
            // notification service. So wait for the process itself to go — a
            // zombie already holds nothing — and only then take the lock.
            let mut probe = file.try_clone().ok();
            let old = probe.as_mut().and_then(|f| {
                let mut text = String::new();
                f.seek(SeekFrom::Start(0)).ok()?;
                f.read_to_string(&mut text).ok()?;
                text.trim().parse::<u32>().ok()
            });
            match old {
                Some(pid) if pid != std::process::id() => {
                    while process_alive(pid) && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
                _ => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        let mut f = &file;
        let _ = f.set_len(0);
        let _ = f.seek(SeekFrom::Start(0));
        let _ = writeln!(f, "{}", std::process::id());
        return Ok(file);
    }
    #[cfg(not(unix))]
    {
        Ok(file)
    }
}

/// Whether a process is still there, with a zombie counting as gone: it holds
/// no sockets and no locks any more, only a line in the process table.
#[cfg(target_os = "linux")]
fn process_alive(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        // `pid (name that may hold spaces) state …`
        Ok(stat) => !stat
            .rsplit(')')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with('Z'),
        Err(_) => false,
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_alive(pid: u32) -> bool {
    // Signal 0 asks the kernel whether the process exists; EPERM still means
    // somebody is there.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

fn has_display() -> bool {
    #[cfg(windows)]
    {
        true
    }
    #[cfg(unix)]
    {
        std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
    }
}
