//! Release checks and downloads.
//!
//! By default both binaries ask `https://git.entitybtw.ru` for the newest
//! release, compare the tag with their own version and can download the
//! matching asset. The source is configurable: a Forgejo/Gitea instance or a
//! GitHub repository, written as the repository's web address. Public releases
//! need no token, so the same code works on the Deck and on the PC.

use crate::model::VERSION;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const HOST: &str = "https://git.entitybtw.ru";
pub const REPO: &str = "entitybtw/sd-widgets";

/// Where releases come from. GitHub and Forgejo/Gitea answer the same release
/// fields, so one parser reads both; only the API path and the page differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteKind {
    /// Forgejo or Gitea: `{base}/api/v1/repos/{repo}/releases`.
    Forgejo,
    /// github.com, or a GitHub Enterprise host with `/api/v3`.
    GitHub,
}

/// A release source: the instance's root and the `owner/name` repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub kind: RemoteKind,
    /// `https://host`, no trailing slash.
    pub base: String,
    /// `owner/name`.
    pub repo: String,
}

impl Default for Remote {
    fn default() -> Self {
        Self {
            kind: RemoteKind::Forgejo,
            base: HOST.into(),
            repo: REPO.into(),
        }
    }
}

impl Remote {
    /// Read the repository's web address.
    ///
    /// Accepted: `https://host/owner/name`, `host/owner/name`, a trailing
    /// `/releases`, `.git` or `/src/branch/main`, a GitHub API address and the
    /// `git@host:owner/name.git` form a `git remote` prints. An empty line
    /// means the built-in repository. The host decides the kind: anything with
    /// `github` in it speaks GitHub, everything else Forgejo. An instance under
    /// a sub-path (`https://host/git/owner/name`) is not supported: the first
    /// two path pieces are always owner and repository.
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw = text.trim();
        if raw.is_empty() {
            return Ok(Self::default());
        }
        // `git@github.com:owner/name.git` from `git remote -v`.
        let scp = !raw.contains("://") && raw.split('/').next().is_some_and(|h| h.contains('@'));
        let raw = if scp {
            let (user_host, path) = raw
                .split_once(':')
                .ok_or_else(|| format!("адрес без пути: {raw}"))?;
            let host = user_host.rsplit('@').next().unwrap_or(user_host);
            format!("https://{host}/{path}")
        } else {
            raw.to_string()
        };
        let (scheme, rest) = if let Some(r) = raw.strip_prefix("https://") {
            ("https", r)
        } else if let Some(r) = raw.strip_prefix("http://") {
            ("http", r)
        } else if let Some(r) = raw.strip_prefix("ssh://") {
            ("https", r)
        } else {
            ("https", raw.as_str())
        };
        // `ssh://git@host/...` keeps the user in the first path element.
        let rest = rest.trim_start_matches('/');
        let mut parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            return Err(format!("нет имени хоста: {raw}"));
        }
        let host = parts.remove(0).rsplit('@').next().unwrap_or("").to_string();
        if host.is_empty() {
            return Err(format!("нет имени хоста: {raw}"));
        }
        if host.eq_ignore_ascii_case("api.github.com") && parts.first() == Some(&"repos") {
            parts.remove(0);
        }
        // The API host is not the web host: the release page lives on
        // github.com either way.
        let host = if host.eq_ignore_ascii_case("api.github.com") {
            "github.com".to_string()
        } else {
            host
        };
        if parts.len() < 2 {
            return Err(format!(
                "нужен адрес вида https://хост/владелец/репозиторий: {raw}"
            ));
        }
        let owner = parts[0].trim();
        let name = parts[1].trim().trim_end_matches(".git").trim();
        if owner.is_empty() || name.is_empty() {
            return Err(format!("пустой владелец или репозиторий: {raw}"));
        }
        let repo = format!("{owner}/{name}");
        let host_lc = host.to_ascii_lowercase();
        let github = host_lc == "github.com" || host_lc.contains("github.");
        let kind = if github {
            RemoteKind::GitHub
        } else {
            RemoteKind::Forgejo
        };
        Ok(Self {
            kind,
            base: format!("{scheme}://{host}"),
            repo,
        })
    }

    /// `github.com · owner/name`, for a settings line.
    pub fn label(&self) -> String {
        let host = self
            .base
            .split("://")
            .nth(1)
            .unwrap_or(&self.base)
            .trim_end_matches('/');
        format!("{host} · {}", self.repo)
    }

    /// The release list endpoint of this kind of server.
    pub fn api_url(&self) -> String {
        match self.kind {
            RemoteKind::GitHub if self.base.contains("github.com") => {
                format!(
                    "https://api.github.com/repos/{}/releases?per_page=10",
                    self.repo
                )
            }
            RemoteKind::GitHub => format!(
                "{}/api/v3/repos/{}/releases?per_page=10",
                self.base, self.repo
            ),
            RemoteKind::Forgejo => {
                format!("{}/api/v1/repos/{}/releases?limit=10", self.base, self.repo)
            }
        }
    }

    /// The page a person opens in a browser.
    pub fn releases_page(&self) -> String {
        format!("{}/{}/releases", self.base, self.repo)
    }
}

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
        let platform = if cfg!(windows) { "windows" } else { "linux" };
        let app = app.to_ascii_lowercase();
        let platform = platform.to_string();
        let fits = |name: &str| {
            let n = name.to_ascii_lowercase();
            if !n.contains(&app) {
                return false;
            }
            let by_ext = if platform == "windows" {
                n.ends_with(".exe") || n.ends_with(".msi") || n.ends_with(".zip")
            } else {
                n.ends_with(".appimage") || n.ends_with(".tar.gz") || n.ends_with(".deb")
            };
            by_ext || n.contains(&platform)
        };
        self.assets.iter().find(|a| fits(&a.name)).or_else(|| {
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

/// Ask the built-in release source for the newest published release.
pub fn check() -> Result<UpdateInfo, String> {
    check_at(&Remote::default())
}

/// Ask `remote` for the newest published release.
pub fn check_at(remote: &Remote) -> Result<UpdateInfo, String> {
    let url = remote.api_url();
    let label = remote.label();
    let mut r = agent()
        .get(&url)
        .header("Accept", "application/json")
        .call()
        .map_err(|e| format!("{label}: {e}"))?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if status != 200 {
        return Err(format!(
            "{label}: HTTP {status}: {}",
            crate::util::truncate(&text, 200)
        ));
    }
    let list: Vec<serde_json::Value> =
        serde_json::from_str(&text).map_err(|e| format!("релизы: {e}"))?;
    let release = list
        .iter()
        .find(|r| !r["draft"].as_bool().unwrap_or(false))
        .ok_or_else(|| format!("на {label} пока нет релизов"))?;
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
        page: release["html_url"].as_str().unwrap_or("").to_string(),
        assets,
    }
    .with_page(remote))
}

impl UpdateInfo {
    /// Fill in the release page when the server did not send one; a mirrored
    /// instance may omit `html_url`.
    fn with_page(mut self, remote: &Remote) -> Self {
        if self.page.trim().is_empty() {
            self.page = remote.releases_page();
        }
        self
    }
}

pub fn updates_dir() -> PathBuf {
    crate::util::data_dir().join("updates")
}

/// Download an asset into `dest` and report received bytes through `progress`.
/// Executables get the +x bit on Unix, so an AppImage can be started directly.
pub fn download(asset: &UpdateAsset, dest: &Path, progress: Arc<AtomicU64>) -> Result<u64, String> {
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
                let _ = std::fs::set_permissions(&current, std::fs::Permissions::from_mode(0o755));
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

    #[test]
    fn an_empty_remote_is_the_built_in_one() {
        let r = Remote::parse("  ").unwrap();
        assert_eq!(r, Remote::default());
        assert_eq!(r.kind, RemoteKind::Forgejo);
        assert_eq!(
            r.api_url(),
            "https://git.entitybtw.ru/api/v1/repos/entitybtw/sd-widgets/releases?limit=10"
        );
        assert_eq!(
            r.releases_page(),
            "https://git.entitybtw.ru/entitybtw/sd-widgets/releases"
        );
        assert_eq!(r.label(), "git.entitybtw.ru · entitybtw/sd-widgets");
    }

    #[test]
    fn a_github_repository_speaks_github() {
        for text in [
            "https://github.com/owner/name",
            "github.com/owner/name",
            "https://github.com/owner/name/",
            "https://github.com/owner/name.git",
            "https://github.com/owner/name/releases",
            "https://api.github.com/repos/owner/name",
            "git@github.com:owner/name.git",
            "ssh://git@github.com/owner/name.git",
        ] {
            let r = Remote::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(r.kind, RemoteKind::GitHub, "{text}");
            assert_eq!(r.base, "https://github.com", "{text}");
            assert_eq!(r.repo, "owner/name", "{text}");
            assert_eq!(
                r.api_url(),
                "https://api.github.com/repos/owner/name/releases?per_page=10",
                "{text}"
            );
        }
    }

    #[test]
    fn a_forgejo_instance_keeps_its_scheme_and_port() {
        let r = Remote::parse("http://git.example.org:3000/team/tool").unwrap();
        assert_eq!(r.kind, RemoteKind::Forgejo);
        assert_eq!(r.base, "http://git.example.org:3000");
        assert_eq!(r.repo, "team/tool");
        assert_eq!(
            r.api_url(),
            "http://git.example.org:3000/api/v1/repos/team/tool/releases?limit=10"
        );
        assert_eq!(
            r.releases_page(),
            "http://git.example.org:3000/team/tool/releases"
        );
    }

    #[test]
    fn a_github_enterprise_host_uses_api_v3() {
        let r = Remote::parse("https://github.example.com/o/r").unwrap();
        assert_eq!(r.kind, RemoteKind::GitHub);
        assert_eq!(
            r.api_url(),
            "https://github.example.com/api/v3/repos/o/r/releases?per_page=10"
        );
    }

    #[test]
    fn a_broken_remote_is_refused() {
        assert!(Remote::parse("example.org").is_err());
        assert!(Remote::parse("https://example.org/only-owner").is_err());
        assert!(Remote::parse("https:///owner/name").is_err());
    }

    #[test]
    fn a_missing_page_falls_back_to_the_instance() {
        let info = UpdateInfo {
            page: String::new(),
            ..Default::default()
        }
        .with_page(&Remote::parse("https://github.com/o/r").unwrap());
        assert_eq!(info.page, "https://github.com/o/r/releases");
    }
}
