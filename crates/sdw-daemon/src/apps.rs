//! Installed applications and power actions.
//!
//! The deck may start an application on the computer, but only one that the
//! daemon itself discovered: the API speaks ids, never a command line. Power
//! actions are a fixed list as well, so a lost deck cannot do anything the
//! user did not put in this file.

use sdw_core::model::{AppEntry, AppKind, PowerAction};
use std::path::{Path, PathBuf};

/// An application plus what is needed to start it. Only the sanitized
/// [`AppEntry`] crosses the API.
#[derive(Debug, Clone)]
pub struct App {
    pub entry: AppEntry,
    program: String,
    args: Vec<String>,
    /// A Windows shortcut is opened through the shell, not executed.
    shortcut: bool,
}

impl App {
    fn shell(entry: AppEntry, program: String, args: Vec<String>, shortcut: bool) -> Self {
        Self {
            entry,
            program,
            args,
            shortcut,
        }
    }
}

/// Scan the applications installed for this user.
pub fn scan() -> Vec<App> {
    #[cfg(windows)]
    {
        scan_windows()
    }
    #[cfg(not(windows))]
    {
        scan_linux()
    }
}

/// Start an application by the id returned from [`scan`].
pub fn launch(id: &str) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty() || id.len() > 200 {
        return Err("Нет такого приложения".into());
    }
    let found = scan().into_iter().find(|a| a.entry.id == id);
    let Some(app) = found else {
        return Err("Приложение не найдено; обновите список".into());
    };
    let mut args: Vec<&str> = app.args.iter().map(String::as_str).collect();
    let program = if cfg!(windows) && app.shortcut {
        args.insert(0, "/C");
        args.insert(1, "start");
        args.insert(2, "");
        args.push(app.program.as_str());
        "cmd"
    } else {
        app.program.as_str()
    };
    sdw_core::util::spawn_detached(program, &args)
        .map(|_| format!("запущено: {}", app.entry.name))?;
    Ok(format!("запущено: {}", app.entry.name))
}

// ---------------------------------------------------------------- power

/// Run one declared power action. On Linux the session manager does the work;
/// on Windows the native commands are used.
pub fn power(action: PowerAction) -> Result<String, String> {
    #[cfg(windows)]
    {
        match action {
            PowerAction::Lock => {
                sdw_core::util::run("rundll32.exe", &["user32.dll,LockWorkStation"])
            }
            PowerAction::Sleep => {
                sdw_core::util::run("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"])
            }
            PowerAction::Reboot => sdw_core::util::run("shutdown", &["/r", "/t", "0"]),
            PowerAction::Shutdown => sdw_core::util::run("shutdown", &["/s", "/t", "0"]),
            PowerAction::ScreenOff => screen_off_windows(),
        }
        .map(|_| format!("{}: готово", action.key()))
    }
    #[cfg(not(windows))]
    {
        match action {
            PowerAction::Lock => lock_linux(),
            PowerAction::Sleep => {
                if sdw_core::util::has("systemctl") {
                    sdw_core::util::run("systemctl", &["suspend"]).map(|_| "сон".into())
                } else if sdw_core::util::has("loginctl") {
                    sdw_core::util::run("loginctl", &["suspend"]).map(|_| "сон".into())
                } else {
                    Err("Нужен systemctl или loginctl".into())
                }
            }
            PowerAction::Reboot => {
                if sdw_core::util::has("systemctl") {
                    sdw_core::util::run("systemctl", &["reboot"]).map(|_| "перезагрузка".into())
                } else {
                    Err("Нужен systemctl".into())
                }
            }
            PowerAction::Shutdown => {
                if sdw_core::util::has("systemctl") {
                    sdw_core::util::run("systemctl", &["poweroff"]).map(|_| "выключение".into())
                } else {
                    Err("Нужен systemctl".into())
                }
            }
            PowerAction::ScreenOff => screen_off_linux(),
        }
    }
}

#[cfg(not(windows))]
fn lock_linux() -> Result<String, String> {
    // loginctl reaches the compositor's own lock screen (niri, sway, GNOME,
    // KDE); the xdg-screensaver fallback covers X11 sessions without one.
    if sdw_core::util::has("loginctl") {
        if sdw_core::util::run("loginctl", &["lock-session"]).is_ok() {
            return Ok("заблокировано".into());
        }
    }
    if sdw_core::util::has("xdg-screensaver") {
        return sdw_core::util::run("xdg-screensaver", &["lock"]).map(|_| "заблокировано".into());
    }
    Err("Нет loginctl или xdg-screensaver".into())
}

#[cfg(not(windows))]
fn screen_off_linux() -> Result<String, String> {
    // niri, sway and Hyprland can blank their own outputs. Anything else falls
    // back to the first locker that can be told to blank now.
    for (cmd, args) in [
        ("niri", vec!["msg", "action", "power-off-monitors"]),
        ("swaymsg", vec!["output", "*", "power", "off"]),
        ("hyprctl", vec!["dispatch", "dpms", "off"]),
    ] {
        if sdw_core::util::has(cmd) && sdw_core::util::run(cmd, &args).is_ok() {
            return Ok("экран выключен".into());
        }
    }
    if sdw_core::util::has("xset") {
        return sdw_core::util::run("xset", &["dpms", "force", "off"])
            .map(|_| "экран выключен".into());
    }
    Err("Композитор не умеет выключать экран".into())
}

#[cfg(windows)]
fn screen_off_windows() -> Result<String, String> {
    // Ask the monitor to sleep through the shell, which works for every
    // Windows version the daemon supports.
    let script = r#"
Add-Type -Namespace W -Name S -MemberDefinition '[DllImport("user32.dll")] public static extern System.IntPtr SendMessage(System.IntPtr hWnd, uint Msg, System.IntPtr wParam, System.IntPtr lParam);';
[W.S]::SendMessage([System.IntPtr]0xffff, 0x0112, [System.IntPtr]0xF170, [System.IntPtr]2) | Out-Null
"#;
    sdw_core::util::run("powershell", &["-NoProfile", "-Command", script])
        .map(|_| "экран выключен".into())
}

// ---------------------------------------------------------------- linux apps

#[cfg(not(windows))]
fn scan_linux() -> Vec<App> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::data_dir() {
        dirs.push(home.join("applications"));
    }
    let data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for dir in data_dirs.split(':').filter(|d| !d.trim().is_empty()) {
        dirs.push(Path::new(dir).join("applications"));
    }

    let mut seen = std::collections::HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Some(id) = desktop_id(&dir, &path) else {
                continue;
            };
            // A user file shadows the system one with the same desktop id.
            if !seen.insert(id.clone()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(app) = parse_desktop(&id, &text, &path) {
                apps.push(app);
            }
        }
    }
    apps.sort_by(|a, b| {
        a.entry
            .name
            .to_lowercase()
            .cmp(&b.entry.name.to_lowercase())
    });
    apps
}

/// The desktop id is the path under `applications/` with `/` replaced by `-`,
/// which is exactly how the desktop environment names the file.
#[cfg(not(windows))]
fn desktop_id(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let mut id = String::new();
    for part in rel.components() {
        let text = part.as_os_str().to_str()?;
        if !id.is_empty() {
            id.push('-');
        }
        id.push_str(text.trim_end_matches(".desktop"));
    }
    (!id.is_empty()).then_some(id)
}

#[cfg(not(windows))]
fn parse_desktop(id: &str, text: &str, path: &Path) -> Option<App> {
    let mut section = "";
    let mut name = String::new();
    let mut comment = String::new();
    let mut exec = String::new();
    let mut categories = String::new();
    let mut kind = AppKind::Gui;
    let mut hidden = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            section = line;
            continue;
        }
        if section != "[Desktop Entry]" {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Name" => name = unescape_desktop(value),
            "Comment" => comment = unescape_desktop(value),
            "Exec" => exec = unescape_desktop(value),
            "Categories" => categories = value.to_string(),
            "Terminal" if value.eq_ignore_ascii_case("true") => kind = AppKind::Terminal,
            "NoDisplay" | "Hidden" if value.eq_ignore_ascii_case("true") => hidden = true,
            "Type" if !value.eq_ignore_ascii_case("application") => return None,
            "TryExec" => {
                let first = value.split_whitespace().next().unwrap_or("");
                if !first.is_empty() && !sdw_core::util::has(first) && !Path::new(first).is_file() {
                    return None;
                }
            }
            _ => {}
        }
    }
    if hidden || name.trim().is_empty() {
        return None;
    }
    let (program, args) = split_exec(&exec)?;
    let source = {
        // The install location makes two "Settings" entries distinguishable.
        let parent = path.parent().unwrap_or(Path::new(""));
        parent
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("applications")
            .to_string()
    };
    let source = if categories.is_empty() {
        source
    } else {
        format!("{source} · {}", categories.split(';').next().unwrap_or(""))
    };
    Some(App::shell(
        AppEntry {
            id: id.to_string(),
            name,
            comment,
            kind,
            source,
        },
        program,
        args,
        false,
    ))
}

#[cfg(not(windows))]
/// Split a desktop `Exec` line into a program and arguments, dropping field
/// codes. Desktop files use double quotes and backslash escapes; there is no
/// shell, so nothing else is interpreted.
fn split_exec(text: &str) -> Option<(String, Vec<String>)> {
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            }
            ' ' | '\t' if !quoted => {
                if any || !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            '%' => match chars.peek().copied() {
                // %% is a literal percent.
                Some('%') => {
                    chars.next();
                    cur.push('%');
                }
                // A field code stands for a file or URL the deck does not
                // pass; drop it together with the percent.
                Some(c) if "fFuUdDnNickvm".contains(c) => {
                    chars.next();
                }
                _ => cur.push('%'),
            },
            other => cur.push(other),
        }
    }
    if any || !cur.is_empty() {
        tokens.push(cur);
    }
    let mut tokens = tokens.into_iter().filter(|t| !t.is_empty());
    let program = tokens.next()?;
    Some((program, tokens.collect()))
}

#[cfg(not(windows))]
fn unescape_desktop(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

// ---------------------------------------------------------------- windows apps

#[cfg(windows)]
fn scan_windows() -> Vec<App> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(program_data) = std::env::var_os("ProgramData") {
        dirs.push(PathBuf::from(program_data).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(app_data) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(app_data).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    let mut apps = Vec::new();
    for dir in dirs {
        walk_shortcuts(&dir, &dir, &mut apps);
    }
    apps.sort_by(|a, b| {
        a.entry
            .name
            .to_lowercase()
            .cmp(&b.entry.name.to_lowercase())
    });
    apps.dedup_by(|a, b| a.entry.id == b.entry.id);
    apps
}

#[cfg(windows)]
fn walk_shortcuts(root: &Path, dir: &Path, out: &mut Vec<App>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_shortcuts(root, &path, out);
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext != "lnk" && ext != "url" {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.trim().is_empty() || name.starts_with('~') {
            continue;
        }
        let Some(path_text) = path.to_str() else {
            continue;
        };
        let id = format!("win-{}", short_hash(path_text));
        let source = path
            .strip_prefix(root)
            .ok()
            .and_then(|p| p.parent())
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();
        out.push(App::shell(
            AppEntry {
                id,
                name: name.to_string(),
                comment: String::new(),
                kind: AppKind::Gui,
                source,
            },
            path_text.to_string(),
            Vec::new(),
            true,
        ));
    }
}

#[cfg(windows)]
fn short_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(text.as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn exec_is_split_without_a_shell() {
        let (program, args) = split_exec(r#""/opt/My App/run" --flag "two words" %U"#).unwrap();
        assert_eq!(program, "/opt/My App/run");
        assert_eq!(args, vec!["--flag", "two words"]);
        assert!(split_exec("").is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn desktop_manifest_is_read() {
        let sample = "[Desktop Entry]\nType=Application\nName=Files\nComment=Browse files\n\
                      Exec=nautilus --new-window %U\nTerminal=false\nCategories=System;Utility;\n";
        let app = parse_desktop(
            "org.gnome.Nautilus",
            sample,
            Path::new("/usr/share/applications/x.desktop"),
        )
        .unwrap();
        assert_eq!(app.entry.name, "Files");
        assert_eq!(app.entry.kind, AppKind::Gui);
        assert_eq!(app.program, "nautilus");
        assert_eq!(app.args, vec!["--new-window"]);
    }

    #[cfg(not(windows))]
    #[test]
    fn hidden_and_terminal_apps_are_handled() {
        let hidden = "[Desktop Entry]\nType=Application\nName=No\nNoDisplay=true\nExec=x\n";
        assert!(parse_desktop("no", hidden, Path::new("/x.desktop")).is_none());
        let term = "[Desktop Entry]\nType=Application\nName=Term\nTerminal=true\nExec=htop\n";
        let app = parse_desktop("term", term, Path::new("/x.desktop")).unwrap();
        assert_eq!(app.entry.kind, AppKind::Terminal);
    }

    #[test]
    fn power_names_round_trip() {
        for action in PowerAction::ALL {
            assert_eq!(PowerAction::parse(action.key()), Some(action));
        }
        assert!(PowerAction::Shutdown.destructive());
        assert!(!PowerAction::Lock.destructive());
    }
}
