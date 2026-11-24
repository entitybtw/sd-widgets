//! Start the daemon together with the desktop session.
//!
//! On Linux two mechanisms, because no single one covers it: an XDG autostart
//! entry (GNOME, KDE, Xfce and anything that honours it) and a line in the
//! compositor's own config, since niri and Hyprland ignore XDG autostart. On
//! Windows it is the per-user `Run` key, which every desktop session reads.
//! Turning the switch off removes whatever was written.
//!
//! The command always carries `--hidden`: at login the daemon should serve the
//! Deck and the tray, not open a window over the desktop. The tray, `--show`
//! and a second launch still bring the window back.

use crate::state::Logger;
use std::path::{Path, PathBuf};

#[cfg(not(windows))]
const NIRI_MARK: &str = "// sd-widgets: daemon autostart";
#[cfg(not(windows))]
const HYPR_MARK: &str = "# sd-widgets: daemon autostart";

/// What [`apply`] did, so the window can say it instead of guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Written,
    Removed,
    /// A debug build: writing it into the session would point the login at
    /// `target/debug`, so nothing was touched.
    Skipped,
}

/// The command the session should run, quoted for a desktop entry and for
/// KDL/Hyprland configs. A `target/debug` binary is a development build and
/// must never write itself into the maintainer's session config; a release
/// build is what `make release` produces, and it is fine to autostart.
#[cfg(not(windows))]
fn command() -> Option<(String, String, String)> {
    let exe = release_exe()?;
    let path = exe.display().to_string();
    let sh = shell_quote(&path);
    let kdl = kdl_quote(&path);
    Some((
        format!("{sh} --hidden"),
        format!("spawn-at-startup {kdl} \"--hidden\""),
        format!("exec-once = {sh} --hidden"),
    ))
}

/// The binary a session should start: the AppImage when this runs from one,
/// otherwise the release executable. A `target/debug` build is a development
/// build and must never write itself into the user's session config.
fn release_exe() -> Option<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(appimage));
    }
    let exe = std::env::current_exe().ok()?;
    let debug = exe
        .components()
        .zip(exe.components().skip(1))
        .any(|(a, b)| a.as_os_str() == "target" && b.as_os_str() == "debug");
    if debug {
        return None;
    }
    Some(exe)
}

/// The `Run` value on Windows. `CreateProcess` splits the command line itself,
/// so only a path with spaces needs quotes and no backslash is escaped.
#[cfg(windows)]
fn windows_command() -> Option<String> {
    let path = release_exe()?.display().to_string();
    let quoted = if path.contains(' ') {
        format!("\"{path}\"")
    } else {
        path
    };
    Some(format!("{quoted} --hidden"))
}

#[cfg(not(windows))]
fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".into();
    }
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+:@".contains(c))
    {
        return s.to_string();
    }
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(not(windows))]
fn kdl_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// True when this line is one of ours: a spawn/exec for this daemon, including
/// entries written by an older version or the installer.
#[cfg(not(windows))]
fn ours(line: &str) -> bool {
    let t = line.trim_start();
    let spawn = t.starts_with("spawn-at-startup") || t.starts_with("exec-once");
    spawn
        && (t.contains("sdw-daemon")
            || t.contains("sdwidgets")
            || t.contains("sd-widgets")
            || t.contains("sdw_deck"))
}

#[cfg(not(windows))]
fn without_ours(text: &str) -> String {
    let mut out: Vec<&str> = Vec::with_capacity(text.lines().count());
    let mut skip_pending = false;
    for line in text.lines() {
        let t = line.trim();
        if t == NIRI_MARK.trim() || t == HYPR_MARK.trim() {
            // The blank line above the block belongs to it; without this the
            // user's config would gain a blank line on every on/off pair.
            if out.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
                out.pop();
            }
            // The marker owns exactly the next non-empty line, whatever the
            // binary is called: an AppImage can have any file name.
            skip_pending = true;
            continue;
        }
        if skip_pending {
            if t.is_empty() {
                continue;
            }
            skip_pending = false;
            continue;
        }
        if ours(line) {
            continue;
        }
        out.push(line);
    }
    // A marker at the very top leaves its separator blank line behind.
    while out.first().map(|l| l.trim().is_empty()).unwrap_or(false) {
        out.remove(0);
    }
    let mut text = out.join("\n").trim_end_matches('\n').to_string();
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

#[cfg(not(windows))]
fn write_if_changed(path: &Path, text: &str, logger: &Logger) -> bool {
    match std::fs::read_to_string(path) {
        Ok(old) if old == text => return false,
        _ => {}
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(path, text) {
        Ok(()) => {
            logger.info(format!("автозапуск: {}", path.display()));
            true
        }
        Err(e) => {
            logger.warn(format!(
                "автозапуск: не удалось записать {}: {e}",
                path.display()
            ));
            false
        }
    }
}

#[cfg(not(windows))]
fn remove_if_present(path: &Path, logger: &Logger) {
    if path.exists() {
        match std::fs::remove_file(path) {
            Ok(()) => logger.info(format!("автозапуск выключен: {}", path.display())),
            Err(e) => logger.warn(format!(
                "автозапуск: не удалось удалить {}: {e}",
                path.display()
            )),
        }
    }
}

/// The body of a systemd user unit. The service is only useful while the
/// user's session exists (sound, notifications and the session bus live
/// there), so it is bound to `graphical-session.target`. The command already
/// carries `--hidden`: systemd starts it in the tray, and `sdw-daemon --show`
/// brings the window back.
#[cfg(not(windows))]
fn systemd_unit(cmd: &str) -> String {
    format!(
        "[Unit]\n\
         Description=sd-widgets daemon\n\
         Documentation=https://git.entitybtw.ru/entitybtw/sd-widgets\n\
         PartOf=graphical-session.target\n\
         After=graphical-session.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={cmd}\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         Environment=RUST_BACKTRACE=1\n\
         \n\
         [Install]\n\
         WantedBy=graphical-session.target\n"
    )
}

/// `systemctl --user` works only on a real user systemd. A machine without it
/// (a session started by hand, a container, OpenRC) reports failure here, and
/// the caller then says so plainly instead of claiming the service is on.
#[cfg(not(windows))]
fn user_systemd() -> bool {
    sdw_core::util::has("systemctl")
        && sdw_core::util::run("systemctl", &["--user", "show-environment"]).is_ok()
}

#[cfg(not(windows))]
fn xdg_entry(cmd: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=sd-widgets daemon\n\
         Comment=sd-widgets daemon in the background\n\
         Exec={cmd}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n\
         X-sd-widgets-autostart=1\n"
    )
}

/// Put the session in the requested state. Safe to call on every start: it
/// only writes when the content actually changes.
pub fn apply(enabled: bool, logger: &Logger) -> Outcome {
    #[cfg(windows)]
    return apply_windows(enabled, logger);
    #[cfg(not(windows))]
    return apply_unix(enabled, logger);
}

/// Windows: the per-user `Run` key, which every interactive session reads.
/// No service and no window: `--hidden` keeps the daemon in the tray.
#[cfg(windows)]
fn apply_windows(enabled: bool, logger: &Logger) -> Outcome {
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "sd-widgets";
    if !enabled {
        match reg(&["delete", KEY, "/v", NAME, "/f"]) {
            Ok(_) => logger.info("автозапуск выключен"),
            Err(e) => logger.warn(format!("автозапуск: {e}")),
        }
        return Outcome::Removed;
    }
    let Some(command) = windows_command() else {
        logger.info(
            "автозапуск: пропущен для отладочной сборки (запустите установленный \
             демон или AppImage, чтобы включить его)",
        );
        return Outcome::Skipped;
    };
    match reg(&["add", KEY, "/v", NAME, "/t", "REG_SZ", "/d", &command, "/f"]) {
        Ok(_) => {
            logger.info(format!("автозапуск: {command}"));
            Outcome::Written
        }
        Err(e) => {
            logger.warn(format!("автозапуск: {e}"));
            Outcome::Removed
        }
    }
}

/// Run `reg.exe` without flashing a console window.
#[cfg(windows)]
fn reg(args: &[&str]) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW`
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("reg")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("reg: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let text = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if text.is_empty() {
            format!("reg завершился с {}", output.status)
        } else {
            text
        })
    }
}

#[cfg(not(windows))]
fn apply_unix(enabled: bool, logger: &Logger) -> Outcome {
    // `$XDG_CONFIG_HOME` when the session sets it, `~/.config` otherwise, so
    // the entry and the compositor lines land where the desktop looks.
    let entry = crate::config::session_config_dir().join("autostart/sdw-daemon.desktop");
    if !enabled {
        remove_if_present(&entry, logger);
        systemd_off(logger);
        for (path, _mark) in compositor_configs() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let next = without_ours(&text);
                if next != text {
                    write_if_changed(&path, &next, logger);
                }
            }
        }
        return Outcome::Removed;
    }

    let Some((xdg_cmd, niri_line, hypr_line)) = command() else {
        logger.info(
            "автозапуск: пропущен для отладочной сборки (запустите установленный \
             демон или AppImage, чтобы включить его)",
        );
        return Outcome::Skipped;
    };
    write_if_changed(&entry, &xdg_entry(&xdg_cmd), logger);
    systemd_on(&xdg_cmd, logger);

    let niri = crate::config::niri_config_path();
    if niri.exists() {
        if let Ok(text) = std::fs::read_to_string(&niri) {
            let body = without_ours(&text);
            let next = format!(
                "{NIRI_MARK}\n{niri_line}\n\n{}",
                body.trim_start_matches('\n')
            );
            write_if_changed(&niri, &next, logger);
        }
    }
    let hypr = crate::config::hypr_config_path();
    if hypr.exists() {
        if let Ok(text) = std::fs::read_to_string(&hypr) {
            // Exactly one blank line separates the user's config from the
            // block: `without_ours` removes it together with the block.
            let mut body = without_ours(&text).trim_matches('\n').to_string();
            if !body.is_empty() {
                body.push('\n');
                body.push('\n');
            }
            let next = format!("{body}{HYPR_MARK}\n{hypr_line}\n");
            write_if_changed(&hypr, &next, logger);
        }
    }
    Outcome::Written
}

/// Where the user unit lives: the path `systemctl --user` reads.
#[cfg(not(windows))]
fn unit_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/systemd/user/sdw-daemon.service")
}

/// Write and enable the user unit. The XDG entry stays as the fallback for a
/// machine with no user systemd at all; with both, systemd starts the daemon
/// and the desktop entry finds it already running and exits.
#[cfg(not(windows))]
fn systemd_on(cmd: &str, logger: &Logger) {
    let unit = unit_path();
    if !user_systemd() {
        if unit_path().exists() {
            // No user systemd any more: the unit would sit there dead and
            // make the switch look on. The desktop entry is the live fallback.
            remove_if_present(&unit_path(), logger);
            logger.info(
                "автозапуск: systemd --user недоступен, служба убрана; работает \
                 запись автозапуска рабочего стола",
            );
        }
        return;
    }
    let written = write_if_changed(&unit, &systemd_unit(cmd), logger);
    if written {
        let _ = sdw_core::util::run("systemctl", &["--user", "daemon-reload"]);
    }
    let enabled = sdw_core::util::run("systemctl", &["--user", "is-enabled", "sdw-daemon.service"])
        .map(|out| out.trim() == "enabled")
        .unwrap_or(false);
    if enabled {
        return;
    }
    // `enable` only: the daemon is already running (this is its own switch),
    // and `--now` would start a second copy that cannot bind the port.
    match sdw_core::util::run("systemctl", &["--user", "enable", "sdw-daemon.service"]) {
        Ok(_) => logger.info("автозапуск: служба systemd --user sdw-daemon включена"),
        Err(e) => logger.warn(format!("автозапуск: systemctl --user enable: {e}")),
    }
}

#[cfg(not(windows))]
fn systemd_off(logger: &Logger) {
    let unit = unit_path();
    if !unit.exists() {
        return;
    }
    let _ = sdw_core::util::run(
        "systemctl",
        &["--user", "disable", "--now", "sdw-daemon.service"],
    );
    remove_if_present(&unit, logger);
    let _ = sdw_core::util::run("systemctl", &["--user", "daemon-reload"]);
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    /// The unit is valid systemd syntax, starts the daemon with its path
    /// quoted and does not start a second window.
    #[test]
    fn unit_is_a_valid_user_service() {
        // The unit receives the same command as the desktop entry, which is
        // `"<path>" --hidden`.
        let unit = systemd_unit("\"/home/x/Apps/sdw-daemon.AppImage\" --hidden");
        assert!(unit.starts_with("[Unit]\n"));
        assert!(unit.contains("ExecStart=\"/home/x/Apps/sdw-daemon.AppImage\" --hidden\n"));
        // The session command already ends with the flag; repeating it is
        // harmless to systemd but reads as a bug in every unit file.
        assert!(!unit.contains("--hidden --hidden"));
        assert!(unit.contains("WantedBy=graphical-session.target"));
        assert!(unit.contains("PartOf=graphical-session.target"));
        for section in ["[Unit]", "[Service]", "[Install]"] {
            assert!(unit.contains(section), "{section} missing");
        }
        // Every line is either a section, empty or key=value.
        for line in unit.lines().filter(|l| !l.trim().is_empty()) {
            assert!(
                line.starts_with('[') && line.ends_with(']') || line.contains('='),
                "bad line: {line}"
            );
        }
    }

    /// Adding our line and taking it away again must leave the user's config
    /// exactly as it was, with no gap growing at the top on every login.
    #[test]
    fn compositor_config_round_trips() {
        let original = "// user config\ninput {\n}\n";
        let added = format!(
            "{NIRI_MARK}\nspawn-at-startup \"/opt/x/sdw-daemon\" \"--hidden\"\n\n{original}"
        );
        assert_eq!(without_ours(&added), original);
        // Every write leaves one blank line between the user's config and our
        // block, so removal sees the same shape either way.
        let hypr = format!("{original}\n{HYPR_MARK}\nexec-once = /opt/x/sdw-daemon --hidden\n");
        let added = format!("{original}\nadded = 1\n\n{HYPR_MARK}\nexec-once = /x\n");
        assert_eq!(without_ours(&hypr), original);
        assert_eq!(without_ours(&added), format!("{original}\nadded = 1\n"));
        // A file that never had our line is untouched.
        assert_eq!(without_ours(original), original);
    }

    #[test]
    fn no_debug_build_ever_autostarts() {
        // A path under target/debug is a development build.
        let exe = std::env::current_exe().unwrap_or_default();
        if exe.to_string_lossy().contains("target/debug") {
            assert!(command().is_none());
        }
    }
}

#[cfg(not(windows))]
fn compositor_configs() -> Vec<(PathBuf, &'static str)> {
    vec![
        (crate::config::niri_config_path(), NIRI_MARK),
        (crate::config::hypr_config_path(), HYPR_MARK),
    ]
}
