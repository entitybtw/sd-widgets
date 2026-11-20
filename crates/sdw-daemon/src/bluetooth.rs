//! Bluetooth over BlueZ.
//!
//! The daemon talks to `bluetoothctl` instead of linking a D-Bus client: BlueZ
//! already owns the adapter, pairing agents and policy, and its CLI is present
//! wherever Bluetooth works on Linux. Battery comes from BlueZ's own report and,
//! when the device is silent, from UPower, which is what shows the percentage in
//! the desktop panel.

use sdw_core::model::{BluetoothDevice, BluetoothStatus};
use sdw_core::{t, Lang};
use std::process::Command;

pub fn status(lang: Lang) -> BluetoothStatus {
    let mut out = BluetoothStatus::default();
    if !sdw_core::util::has("bluetoothctl") {
        out.error = t!(
            lang,
            "нужен bluetoothctl (пакет bluez)",
            "bluetoothctl is missing (bluez package)"
        )
        .into();
        return out;
    }
    // One `bluetoothctl` answers `show` and `devices` together, so a status
    // refresh is a pair of processes instead of one per device: the page can be
    // reloaded after every press without a visible pause.
    let listing = match script(&["show".into(), "devices".into()]) {
        Ok(text) => text,
        Err(e) => {
            out.error = e;
            return out;
        }
    };
    let mut adapter = String::new();
    let mut listed: Vec<(String, String)> = Vec::new();
    for raw in listing.lines() {
        let line = raw.trim_end_matches('\r').trim();
        if let Some(rest) = line.strip_prefix("Controller ") {
            adapter = rest.split_whitespace().next().unwrap_or("").to_string();
        } else if let Some(value) = line.strip_prefix("Powered:") {
            out.powered = value.trim().eq_ignore_ascii_case("yes");
        } else if let Some(value) = line.strip_prefix("Discovering:") {
            out.discovering = value.trim().eq_ignore_ascii_case("yes");
        } else if let Some(rest) = line.strip_prefix("Device ") {
            let mut parts = rest.splitn(2, ' ');
            let address = parts.next().unwrap_or("").trim();
            let name = parts.next().unwrap_or("").trim();
            if valid_address(address) {
                listed.push((address.to_string(), name.to_string()));
            }
        }
    }
    if adapter.is_empty() {
        out.error = t!(
            lang,
            "bluetooth-адаптер не найден",
            "no bluetooth adapter found"
        )
        .into();
        return out;
    }
    out.available = true;
    out.adapter = adapter;

    if listed.is_empty() {
        return out;
    }
    // Every `info` in the same process; the blocks are split back apart below.
    let commands: Vec<String> = listed.iter().map(|(a, _)| format!("info {a}")).collect();
    let details = script(&commands).unwrap_or_default();
    let mut blocks: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut current: Option<&str> = None;
    for raw in details.lines() {
        let line = raw.trim_end_matches('\r').trim();
        if let Some(rest) = line.strip_prefix("Device ") {
            if let Some(address) = rest.split_whitespace().next() {
                if valid_address(address) && !blocks.contains_key(address) {
                    blocks.insert(address.to_string(), String::new());
                    current = Some(address);
                    continue;
                }
            }
        }
        if let Some(address) = current {
            if let Some(block) = blocks.get_mut(address) {
                block.push_str(line);
                block.push('\n');
            }
        }
    }
    for (address, name) in &listed {
        let info = blocks.get(address).map(String::as_str).unwrap_or("");
        let mut device = parse_info(address, name, info);
        if device.battery.is_none() && device.connected {
            device.battery = upower_battery(address);
        }
        out.devices.push(device);
    }
    // Connected first, then paired, then by name: the useful devices are on top.
    out.devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.paired.cmp(&a.paired))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// Feed a list of commands to one `bluetoothctl` and take its output. The CLI
/// prints prompts with ANSI escapes between commands; nothing here matches a
/// prompt line, so the callers read the value lines and ignore the rest.
fn script(commands: &[String]) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("bluetoothctl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("bluetoothctl: {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        for command in commands {
            let _ = writeln!(stdin, "{command}");
        }
        let _ = writeln!(stdin, "quit");
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("bluetoothctl: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() && text.trim().is_empty() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("bluetoothctl: код {}", out.status.code().unwrap_or(-1))
        } else {
            err
        });
    }
    Ok(text)
}

fn parse_info(address: &str, listed_name: &str, info: &str) -> BluetoothDevice {
    let mut d = BluetoothDevice {
        address: address.to_string(),
        name: listed_name.to_string(),
        ..Default::default()
    };
    for raw in info.lines() {
        let line = raw.trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Name" | "Alias" if !value.is_empty() => d.name = value.to_string(),
            "Icon" => d.icon = value.to_string(),
            "Paired" => d.paired = value.eq_ignore_ascii_case("yes"),
            "Connected" => d.connected = value.eq_ignore_ascii_case("yes"),
            "Trusted" => d.trusted = value.eq_ignore_ascii_case("yes"),
            "Blocked" => d.blocked = value.eq_ignore_ascii_case("yes"),
            "RSSI" => d.rssi = value.parse().ok(),
            "Battery Percentage" => d.battery = parse_battery(value),
            _ => {}
        }
    }
    d
}

/// BlueZ prints `0x64 (100)`; older builds print `100`.
fn parse_battery(value: &str) -> Option<u8> {
    if let Some(open) = value.find('(') {
        if let Some(close) = value[open + 1..].find(')') {
            if let Ok(pct) = value[open + 1..open + 1 + close].trim().parse::<u8>() {
                return Some(pct.min(100));
            }
        }
    }
    value
        .trim_end_matches('%')
        .parse::<u8>()
        .ok()
        .map(|p| p.min(100))
}

fn upower_battery(address: &str) -> Option<u8> {
    if !sdw_core::util::has("upower") {
        return None;
    }
    let wanted = format!("dev_{}", address.replace(':', "_"));
    let list = run_other("upower", &["-e"])?;
    let path = list.lines().find(|line| line.contains(&wanted))?;
    let info = run_other("upower", &["-i", path.trim()])?;
    for line in info.lines() {
        if let Some(value) = line.trim().strip_prefix("percentage:") {
            return value.trim().trim_end_matches('%').parse::<u8>().ok();
        }
    }
    None
}

/// Run one declared Bluetooth action. Addresses are checked here, so the deck
/// cannot use the CLI as a free-form command channel.
pub fn action(action: &str, address: &str, lang: Lang) -> Result<String, String> {
    let action = action.trim();
    let invalid = || t!(lang, "неверный адрес устройства", "invalid device address").to_string();
    match action {
        "scan" => {
            // BlueZ scans until told to stop; a detached process ends by itself.
            sdw_core::util::spawn_detached("bluetoothctl", &["--timeout", "12", "scan", "on"])
                .map(|_| t!(lang, "поиск устройств", "scanning for devices").to_string())
        }
        "scan_off" => run(&["scan", "off"])
            .map(|_| t!(lang, "поиск остановлен", "scanning stopped").to_string()),
        "power_on" => run(&["power", "on"])
            .map(|_| t!(lang, "bluetooth включён", "bluetooth is on").to_string()),
        "power_off" => run(&["power", "off"])
            .map(|_| t!(lang, "bluetooth выключен", "bluetooth is off").to_string()),
        // One press from the deck has to be enough: BlueZ keeps pairing and
        // connecting as two separate steps, so the daemon runs both and only
        // reports success when the device is actually connected.
        "pair_connect" => {
            if !valid_address(address) {
                return Err(invalid());
            }
            let pair = run(&["--timeout", "25", "pair", address]);
            if let Err(e) = &pair {
                // A device may already be paired; only a failed connect after
                // that is worth reporting as an error.
                let info = run(&["info", address]).unwrap_or_default();
                if !info.lines().any(|l| {
                    l.trim()
                        .strip_prefix("Paired:")
                        .is_some_and(|v| v.trim().eq_ignore_ascii_case("yes"))
                }) {
                    return Err(e.clone());
                }
            }
            // Remembering the device makes the next visit connect by itself.
            let _ = run(&["trust", address]);
            run(&["--timeout", "25", "connect", address]).map(|_| message(lang, "connect", address))
        }
        "connect" | "disconnect" | "pair" | "trust" | "untrust" | "remove" | "info" => {
            if !valid_address(address) {
                return Err(invalid());
            }
            run(&["--timeout", "20", action, address]).map(|_| message(lang, action, address))
        }
        other => Err(format!(
            "{} {other}",
            t!(lang, "неизвестное действие:", "unknown action:")
        )),
    }
}

fn message(lang: Lang, verb: &str, address: &str) -> String {
    let what = match verb {
        "connect" => t!(lang, "подключено", "connected"),
        "disconnect" => t!(lang, "отключено", "disconnected"),
        "pair" => t!(lang, "спарено", "paired"),
        "trust" => t!(lang, "доверенное устройство", "trusted"),
        "untrust" => t!(lang, "больше не доверенное", "no longer trusted"),
        "remove" => t!(lang, "удалено", "removed"),
        _ => t!(lang, "готово", "done"),
    };
    format!("{address}: {what}")
}

pub fn valid_address(address: &str) -> bool {
    address.len() == 17
        && address.chars().enumerate().all(|(i, c)| {
            if i % 3 == 2 {
                c == ':'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn run(args: &[&str]) -> Result<String, String> {
    let out = Command::new("bluetoothctl")
        .args(args)
        .output()
        .map_err(|e| format!("bluetoothctl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let detail = if !err.is_empty() { err } else { text };
        return Err(if detail.is_empty() {
            format!("bluetoothctl: код {}", out.status.code().unwrap_or(-1))
        } else {
            detail.lines().last().unwrap_or(&detail).to_string()
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn run_other(name: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(name).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_checked() {
        assert!(valid_address("B8:17:43:1C:AD:28"));
        assert!(!valid_address("B8:17:43:1C:AD"));
        assert!(!valid_address("zz:17:43:1C:AD:28"));
        assert!(!valid_address("B8-17-43-1C-AD-28"));
    }

    #[test]
    fn info_is_parsed() {
        let info = "Device B8:17:43:1C:AD:28 (public)\n\tName: JBL Tune\n\tIcon: audio-headphones\n\
                    \tPaired: yes\n\tConnected: yes\n\tTrusted: yes\n\tBattery Percentage: 0x64 (100)\n";
        let d = parse_info("B8:17:43:1C:AD:28", "", info);
        assert_eq!(d.name, "JBL Tune");
        assert!(d.connected && d.paired);
        assert_eq!(d.battery, Some(100));
        assert!(d.is_audio());
    }

    #[test]
    fn battery_formats_are_read() {
        assert_eq!(parse_battery("0x50 (80)"), Some(80));
        assert_eq!(parse_battery("42%"), Some(42));
        assert_eq!(parse_battery("nonsense"), None);
    }
}
