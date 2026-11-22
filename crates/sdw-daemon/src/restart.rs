//! One-click restart after an update: the file on disk is the new build, the
//! process is still the old one.
//!
//! Under the systemd user service the unit owns the process, so the restart is
//! asked of systemd and the new build comes back with the service. Anywhere
//! else the daemon starts the file it was launched from, detached, and leaves.
//! That replacement is started with `--restart`, so it waits for this one to
//! let go of the single-instance lock instead of finding "another daemon" and
//! bouncing off.

use sdw_core::{t, Lang};
use std::path::PathBuf;

/// How the daemon is going to come back.
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// `systemctl [--user] --no-block restart <unit>`: systemd stops this
    /// process and starts the unit again.
    Service { unit: String, user: bool },
    /// Start this file again, detached, and leave.
    Exec(PathBuf),
}

/// What the environment says about the way this process was started.
struct Started {
    /// The service that owns the process: its unit and whether it lives in the
    /// user manager.
    unit: Option<(String, bool)>,
    /// The AppImage behind the running process, if it is one.
    appimage: Option<PathBuf>,
    /// The running file itself.
    exe: Option<PathBuf>,
}

/// Restart the daemon, and describe what was started for the log.
pub fn restart(lang: Lang) -> Result<String, String> {
    match plan(lang)? {
        Plan::Service { unit, user } => {
            // `--no-block` matters: a blocking restart waits for the stop job,
            // which waits for this very process to end. The command still
            // answers whether the manager accepted the job, so a restart that
            // cannot happen says so instead of leaving the daemon gone.
            let mut cmd = std::process::Command::new("systemctl");
            cmd.arg("--no-block");
            if user {
                cmd.arg("--user");
            }
            let out = cmd
                .arg("restart")
                .arg(&unit)
                .output()
                .map_err(|e| format!("systemctl: {e}"))?;
            if !out.status.success() {
                let why = String::from_utf8_lossy(&out.stderr);
                return Err(format!("systemctl: {}", why.trim()));
            }
            Ok(format!("systemd: {unit}"))
        }
        Plan::Exec(file) => {
            let file = file.to_string_lossy().to_string();
            sdw_core::util::spawn_detached(&file, &["--restart"])?;
            Ok(file)
        }
    }
}

fn plan(lang: Lang) -> Result<Plan, String> {
    plan_for(
        Started {
            unit: std::env::var_os("INVOCATION_ID")
                .filter(|v| !v.is_empty())
                .and_then(|_| {
                    let text = std::fs::read_to_string("/proc/self/cgroup").ok()?;
                    service_from_cgroup(&text)
                }),
            appimage: std::env::var_os("APPIMAGE")
                .map(PathBuf::from)
                .filter(|p| p.is_file()),
            exe: std::env::current_exe().ok(),
        },
        lang,
    )
}

fn plan_for(started: Started, lang: Lang) -> Result<Plan, String> {
    if let Some((unit, user)) = started.unit {
        return Ok(Plan::Service { unit, user });
    }
    started
        .appimage
        .or(started.exe)
        .map(Plan::Exec)
        .ok_or_else(|| {
            t!(
                lang,
                "не нашёл файл демона — запустите его вручную",
                "could not find the daemon file — start it by hand"
            )
            .to_string()
        })
}

/// The service that owns this process, taken from `/proc/self/cgroup`. The
/// flag says whether the unit belongs to the user manager, which is what the
/// install script sets up.
fn service_from_cgroup(text: &str) -> Option<(String, bool)> {
    for line in text.lines() {
        let Some(path) = line.splitn(3, ':').nth(2) else {
            continue;
        };
        let Some(unit) = path.rsplit('/').next().filter(|u| u.ends_with(".service")) else {
            continue;
        };
        // A `.scope` is a process group the session started, not a unit that
        // can be restarted; those fall through to the file path.
        let user = path.contains("user.slice") || path.contains("user@");
        return Some((unit.to_string(), user));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_user_unit_that_owns_the_process() {
        let text =
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/sdw-daemon.service\n";
        assert_eq!(
            service_from_cgroup(text),
            Some(("sdw-daemon.service".to_string(), true))
        );
    }

    #[test]
    fn reads_the_older_cgroup_layout_too() {
        let text = "1:name=systemd:/system.slice/sdw-daemon.service\n";
        assert_eq!(
            service_from_cgroup(text),
            Some(("sdw-daemon.service".to_string(), false))
        );
    }

    #[test]
    fn a_scope_is_not_a_service() {
        assert_eq!(
            service_from_cgroup("0::/user.slice/user-1000.slice/session-2.scope\n"),
            None
        );
        assert_eq!(service_from_cgroup("0::/\n"), None);
    }

    #[test]
    fn a_service_wins_over_the_file() {
        let plan = plan_for(
            Started {
                unit: Some(("sdw-daemon.service".to_string(), true)),
                appimage: Some(PathBuf::from("/home/x/sdw-daemon.AppImage")),
                exe: Some(PathBuf::from("/home/x/sdw-daemon.AppImage")),
            },
            Lang::Ru,
        )
        .unwrap();
        assert_eq!(
            plan,
            Plan::Service {
                unit: "sdw-daemon.service".to_string(),
                user: true
            }
        );
    }

    #[test]
    fn without_a_service_the_running_file_is_started_again() {
        let plan = plan_for(
            Started {
                unit: None,
                appimage: Some(PathBuf::from("/home/x/sdw-daemon.AppImage")),
                exe: Some(PathBuf::from("/proc/self/exe")),
            },
            Lang::Ru,
        )
        .unwrap();
        assert_eq!(
            plan,
            Plan::Exec(PathBuf::from("/home/x/sdw-daemon.AppImage"))
        );

        let plan = plan_for(
            Started {
                unit: None,
                appimage: None,
                exe: Some(PathBuf::from("/home/x/sdw-daemon")),
            },
            Lang::Ru,
        )
        .unwrap();
        assert_eq!(plan, Plan::Exec(PathBuf::from("/home/x/sdw-daemon")));
    }

    #[test]
    fn without_a_file_it_says_so() {
        let err = plan_for(
            Started {
                unit: None,
                appimage: None,
                exe: None,
            },
            Lang::Ru,
        )
        .unwrap_err();
        assert!(err.contains("не нашёл файл демона"));
    }
}
