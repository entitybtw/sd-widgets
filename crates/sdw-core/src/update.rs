//! Release checks and downloads for the self-hosted Gitea releases.
//!
//! Both binaries ask `https://git.entitybtw.ru` for the newest release, compare
//! the tag with their own version and can download the matching asset. Public
//! releases need no token, so the same code works on the Deck and on the PC.

use crate::model::VERSION;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const HOST: &str = "https://git.entitybtw.ru";
pub const REPO: &str = "entitybtw/sd-widgets";
pub const RELEASES_PAGE: &str = "https://git.entitybtw.ru/entitybtw/sd-widgets/releases";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateAsset {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub newer: bool,
    pub name: String,
    pub notes: String,
    pub published: String,
    pub page: String,
    pub assets: Vec<UpdateAsset>,
}

impl UpdateInfo {
    /// The asset for this application on this platform. `app` is `sdw-deck`
    /// or `sdw-daemon`.
    pub fn asset_for(&self, app: &str) -> Option<&UpdateAsset> {
        let platform = if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        let app = app.to_ascii_lowercase();
        let platform = platform.to_string();
        let fits = |name: &str| {
            let n = name.to_ascii_lowercase();
            if !n.contains(&app) {
                return false;
            }
            let by_ext = if platform == "windows" {
                n.ends_with(".exe") || n.ends_with(".msi") || n.ends_with(".zip")
            } else if platform == "macos" {
                n.ends_with(".dmg") || n.ends_with(".tar.gz")
            } else {
                n.ends_with(".appimage") || n.ends_with(".tar.gz") || n.ends_with(".deb")
            };
            by_ext || n.contains(&platform)
        };
        self.assets
            .iter()
            .find(|a| fits(&a.name))
            .or_else(|| {
                self.assets
                    .iter()
                    .find(|a| a.name.to_ascii_lowercase().contains(&app))
            })
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .user_agent(concat!("sd-widgets/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn rank(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<u64>().ok())
        .collect()
}

/// True when `latest` is a newer version than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let a = rank(latest);
    let b = rank(current);
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// Ask the release API for the newest published release.
pub fn check() -> Result<UpdateInfo, String> {
    let url = format!("{HOST}/api/v1/repos/{REPO}/releases?limit=10");
    let mut r = agent()
        .get(&url)
        .header("Accept", "application/json")
        .call()
        .map_err(|e| format!("git.entitybtw.ru: {e}"))?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if status != 200 {
        return Err(format!(
            "git.entitybtw.ru: HTTP {status}: {}",
            crate::util::truncate(&text, 200)
        ));
    }
    let list: Vec<serde_json::Value> =
        serde_json::from_str(&text).map_err(|e| format!("релизы: {e}"))?;
    let release = list
        .iter()
        .find(|r| !r["draft"].as_bool().unwrap_or(false))
        .ok_or_else(|| "на git.entitybtw.ru пока нет релизов".to_string())?;
    let tag = release["tag_name"].as_str().unwrap_or("").to_string();
    let latest = tag.trim_start_matches(['v', 'V']).to_string();
    if latest.is_empty() {
        return Err("релиз без tag_name".into());
    }
    let assets = release["assets"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?.to_string();
            let url = a["browser_download_url"].as_str().unwrap_or("").to_string();
            if name.is_empty() || url.is_empty() {
                return None;
            }
            Some(UpdateAsset {
                name,
                url,
                size: a["size"].as_u64().unwrap_or(0),
            })
        })
        .collect();
    Ok(UpdateInfo {
        current: VERSION.to_string(),
        latest: latest.clone(),
        newer: is_newer(&latest, VERSION),
        name: release["name"].as_str().unwrap_or(&tag).to_string(),
        notes: release["body"].as_str().unwrap_or("").to_string(),
        published: release["published_at"]
            .as_str()
            .or_else(|| release["created_at"].as_str())
            .unwrap_or("")
            .to_string(),
        page: release["html_url"]
            .as_str()
            .unwrap_or(RELEASES_PAGE)
            .to_string(),
        assets,
    })
}

pub fn updates_dir() -> PathBuf {
    crate::util::data_dir().join("updates")
}

/// Download an asset into `dest` and report received bytes through `progress`.
/// Executables get the +x bit on Unix, so an AppImage can be started directly.
pub fn download(
    asset: &UpdateAsset,
    dest: &Path,
    progress: Arc<AtomicU64>,
) -> Result<u64, String> {
    if let Some(parent) = dest.parent() {
        crate::util::ensure_dir(parent).map_err(|e| e.to_string())?;
    }
    progress.store(0, Ordering::Relaxed);
    let mut r = agent()
        .get(&asset.url)
        .call()
        .map_err(|e| format!("скачивание: {e}"))?;
    let status = r.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("скачивание: HTTP {status}"));
    }
    let total = r
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(asset.size);
    let tmp = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("bin")
    ));
    let mut file = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let mut reader = r.body_mut().with_config().limit(u64::MAX).reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut received = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("запись: {e}"))?;
        received += n as u64;
        progress.store(received, Ordering::Relaxed);
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if total > 0 && received != total {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("скачано {received} из {total} байт"));
    }
    if dest.exists() {
        let _ = std::fs::remove_file(dest);
    }
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755));
    }
    Ok(received)
}

/// Replace the running binary with a freshly downloaded one.
///
/// On Unix a running executable can be renamed and a new file can take its
/// place, which is exactly how an AppImage updates itself. Windows keeps the
/// file locked, so there we only report the downloaded path.
pub fn install(downloaded: &Path) -> Result<String, String> {
    // Inside an AppImage, current_exe() points into the read-only squashfs mount.
    // The AppImage runtime exposes the real file in $APPIMAGE, which is the file
    // we must replace.
    let current = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .map(Ok)
        .unwrap_or_else(|| std::env::current_exe().map_err(|e| format!("текущий файл: {e}")))?;
    #[cfg(unix)]
    {
        let backup = current.with_extension("old");
        let _ = std::fs::remove_file(&backup);
        std::fs::rename(&current, &backup).map_err(|e| format!("переименовать: {e}"))?;
        match std::fs::copy(downloaded, &current) {
            Ok(_) => {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    &current,
                    std::fs::Permissions::from_mode(0o755),
                );
                Ok(format!(
                    "обновление установлено: {} (перезапустите приложение)",
                    current.display()
                ))
            }
            Err(e) => {
                let _ = std::fs::rename(&backup, &current);
                Err(format!("скопировать: {e}"))
            }
        }
    }
    #[cfg(not(unix))]
    {
        Err(format!(
            "закройте приложение и замените файл вручную: {}",
            downloaded.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn picks_the_asset_for_the_app() {
        let info = UpdateInfo {
            assets: vec![
                UpdateAsset {
                    name: "sdw-deck-0.2.0-x86_64.AppImage".into(),
                    url: "a".into(),
                    size: 1,
                },
                UpdateAsset {
                    name: "sdw-daemon-0.2.0-x86_64.AppImage".into(),
                    url: "b".into(),
                    size: 1,
                },
                UpdateAsset {
                    name: "SHA256SUMS".into(),
                    url: "c".into(),
                    size: 1,
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            info.asset_for("sdw-deck").map(|a| a.url.as_str()),
            Some("a")
        );
        assert_eq!(
            info.asset_for("sdw-daemon").map(|a| a.url.as_str()),
            Some("b")
        );
        assert!(info.asset_for("nothing").is_none());
    }
}
