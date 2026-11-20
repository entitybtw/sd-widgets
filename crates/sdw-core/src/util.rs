//! Small cross-platform helpers: paths, processes, network addresses.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sd-widgets")
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sd-widgets")
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sd-widgets")
}

pub fn ensure_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Write a file atomically and keep secrets private on Unix.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(|s| s.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn has(name: &str) -> bool {
    which(name).is_some()
}

pub fn run(name: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(name)
        .args(args)
        .output()
        .map_err(|e| format!("{name}: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("{name}: код {}", out.status.code().unwrap_or(-1))
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn run_shell(command: &str) -> Result<String, String> {
    #[cfg(windows)]
    let mut c = {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    };
    #[cfg(not(windows))]
    let mut c = {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };
    let out = c.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("код {}", out.status.code().unwrap_or(-1))
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn spawn_detached(name: &str, args: &[&str]) -> Result<u32, String> {
    let mut c = Command::new(name);
    c.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let child = c.spawn().map_err(|e| format!("{name}: {e}"))?;
    Ok(child.id())
}

pub fn hostname() -> String {
    #[cfg(windows)]
    {
        if let Ok(s) = std::env::var("COMPUTERNAME") {
            return s;
        }
    }
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "computer".into())
}

pub fn local_ipv4() -> Option<std::net::IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    sock.local_addr().ok().map(|a| a.ip())
}

pub fn addresses(port: u16) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(ip) = local_ipv4() {
        out.push(format!("http://{ip}:{port}"));
    }
    out.push(format!("http://127.0.0.1:{port}"));
    out
}

pub fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

pub fn is_steam_deck() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    for p in [
        "/sys/class/dmi/id/product_name",
        "/sys/class/dmi/id/board_name",
    ] {
        if let Ok(s) = std::fs::read_to_string(p) {
            let s = s.to_ascii_lowercase();
            if s.contains("jupiter") || s.contains("galileo") || s.contains("steam deck") {
                return true;
            }
        }
    }
    false
}

pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut c = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(unix)]
    let mut c = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn format_duration(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{d}д {h}ч")
    } else if h > 0 {
        format!("{h}ч {m}м")
    } else {
        format!("{m}м")
    }
}

/// Replace `{name}` in a template with the matching value.
///
/// Unknown names stay exactly as written, so a typo in an addon is visible in
/// the result instead of silently turning into an empty string. Braces that do
/// not form a short key are left alone as well (CSS and JSON pass through).
pub fn expand(template: &str, vars: &BTreeMap<String, String>) -> String {
    if !template.contains('{') || vars.is_empty() {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let key = &after[..close];
        if key.is_empty() || key.len() > 64 || key.contains('{') || key.contains('\n') {
            out.push('{');
            rest = after;
            continue;
        }
        match vars.get(key) {
            Some(v) => out.push_str(v),
            None => {
                out.push('{');
                out.push_str(key);
                out.push('}');
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Perform an HTTP request on behalf of an addon action.
///
/// The body of a successful answer is returned (trimmed to something a toast or
/// a log line can show). Only absolute `http(s)` URLs are accepted: an addon is
/// allowed to talk to an API, not to read local files through `file://`.
pub fn http_call(
    method: &str,
    url: &str,
    headers: &BTreeMap<String, String>,
    body: &str,
) -> Result<String, String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!(
            "{url}: адреса только http:// и https:// (проверьте подстановку {{...}})"
        ));
    }
    let method = {
        let m = method.trim().to_ascii_uppercase();
        if m.is_empty() {
            "GET".to_string()
        } else {
            m
        }
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .http_status_as_error(false)
        .user_agent(concat!("sd-widgets/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut req = match method.as_str() {
        "GET" => agent.get(url),
        "DELETE" => agent.delete(url),
        "HEAD" => agent.head(url),
        "POST" | "PUT" | "PATCH" => {
            let mut r = match method.as_str() {
                "POST" => agent.post(url),
                "PUT" => agent.put(url),
                _ => agent.patch(url),
            };
            let has_type = headers
                .keys()
                .any(|k| k.eq_ignore_ascii_case("content-type"));
            for (k, v) in headers {
                r = r.header(k, v);
            }
            if !body.is_empty() && !has_type {
                r = r.header("Content-Type", "application/json");
            }
            // A POST/PUT without a body still needs `send`, not `call`.
            let resp = r.send(body);
            return finish(&method, url, resp);
        }
        other => return Err(format!("метод {other} не поддерживается")),
    };
    for (k, v) in headers {
        req = req.header(k, v);
    }
    finish(&method, url, req.call())
}

fn finish(
    method: &str,
    url: &str,
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<String, String> {
    let mut resp = result.map_err(|e| format!("{method} {url}: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .with_config()
        .limit(1 << 20)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "{method} {url}: HTTP {status}: {}",
            truncate(text.trim(), 240)
        ));
    }
    Ok(truncate(text.trim(), 400))
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Bytes as a size a person reads: `84.2 KB`, `1.4 MB`, `812 B`.
pub fn human_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        format!("{kb:.1} KB")
    } else {
        format!("{:.1} MB", kb / 1024.0)
    }
}

pub fn mask_secret(s: &str) -> String {
    if s.len() <= 8 {
        "••••••••".into()
    } else {
        format!("{}…{}", &s[..4], &s[s.len() - 4..])
    }
}

// ---------------------------------------------------------------- base64

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding. Small enough to keep the dependency list
/// short; notification images travel this way between the daemon and the Deck.
pub fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for c in text.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> BTreeMap<String, String> {
        let mut v = BTreeMap::new();
        v.insert("name".into(), "deck".into());
        v.insert("settings.lamp".into(), "light.kitchen".into());
        v
    }

    #[test]
    fn templates_replace_known_names_only() {
        let v = vars();
        assert_eq!(expand("hi {name}", &v), "hi deck");
        assert_eq!(
            expand("{settings.lamp} {missing}", &v),
            "light.kitchen {missing}"
        );
        assert_eq!(expand("body { color: {name} }", &v), "body { color: deck }");
        assert_eq!(expand("no braces", &v), "no braces");
        assert_eq!(expand("{unclosed", &v), "{unclosed");
    }
}
