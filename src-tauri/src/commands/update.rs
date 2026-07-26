//! In-app update check and self-install for the Debian/.deb build.
//!
//! Follows the app's "call the system tool" approach: the GitHub API is queried
//! with `curl` and the new .deb is installed with `pkexec apt-get`, so there is
//! no HTTP/TLS crate dependency. The one-click apply is only offered when the
//! app was installed from the .deb package and `pkexec` is present; source and
//! AppImage installs still get the "update available" notice.

use serde::Serialize;
use std::process::Command;

const REPO: &str = "fbnlrz/Inari";
const PKG: &str = "inari";

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    /// Currently running version (from `CARGO_PKG_VERSION`).
    pub current: String,
    /// Latest release version, without the leading `v`.
    pub latest: String,
    /// True when `latest` is newer than `current`.
    pub available: bool,
    /// The GitHub release page.
    pub url: String,
    /// Release notes (markdown), possibly empty.
    pub notes: String,
    /// The amd64 `.deb` asset URL, if the release ships one.
    pub deb_url: Option<String>,
    /// True when this install can update itself in place (installed from the
    /// `.deb` package and `pkexec` is available).
    pub can_self_install: bool,
}

/// Parse "X.Y.Z" (tolerating a leading `v` and trailing suffixes) into a tuple
/// so versions compare numerically rather than lexically.
fn semver(v: &str) -> (u64, u64, u64) {
    let mut it = v.trim().trim_start_matches('v').split('.').map(|p| {
        p.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0)
    });
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

/// True when a shell command is on PATH.
fn have(cmd: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {cmd} >/dev/null 2>&1"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// True when the running app is the installed `inari` .deb package.
fn installed_from_deb() -> bool {
    Command::new("dpkg-query")
        .args(["-W", "-f=${Version}", PKG])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

fn check_blocking() -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let api = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "-H",
            "Accept: application/vnd.github+json",
            "-A",
            "inari-updater",
            &api,
        ])
        .output()
        .map_err(|e| format!("curl is required to check for updates: {e}"))?;
    if !out.status.success() {
        return Err("could not reach the GitHub API".into());
    }
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("bad API response: {e}"))?;

    let tag = json["tag_name"].as_str().unwrap_or_default();
    let latest = tag.trim_start_matches('v').to_string();
    let url = json["html_url"].as_str().unwrap_or_default().to_string();
    let notes = json["body"].as_str().unwrap_or_default().to_string();
    let deb_url = json["assets"].as_array().and_then(|assets| {
        assets.iter().find_map(|a| {
            let name = a["name"].as_str()?;
            name.ends_with("_amd64.deb")
                .then(|| a["browser_download_url"].as_str().map(String::from))
                .flatten()
        })
    });

    let available = !latest.is_empty() && semver(&latest) > semver(&current);
    let can_self_install = deb_url.is_some() && installed_from_deb() && have("pkexec");

    Ok(UpdateInfo {
        current,
        latest,
        available,
        url,
        notes,
        deb_url,
        can_self_install,
    })
}

/// Query the latest release and compare it to the running version.
#[tauri::command]
pub async fn check_update() -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(check_blocking)
        .await
        .map_err(|e| format!("update check failed to run: {e}"))?
}

fn apply_blocking(deb_url: String) -> Result<(), String> {
    if !deb_url.ends_with("_amd64.deb") || !deb_url.starts_with("https://") {
        return Err("refusing to install an unexpected asset".into());
    }
    let dir = std::env::temp_dir().join("inari-update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("temp dir: {e}"))?;
    let deb = dir.join("inari_latest_amd64.deb");

    let ok = Command::new("curl")
        .arg("-fL")
        .arg("-o")
        .arg(&deb)
        .arg(&deb_url)
        .status()
        .map_err(|e| format!("download failed to start: {e}"))?
        .success();
    if !ok {
        return Err("download failed".into());
    }

    // Make the dir and file readable by apt's `_apt` sandbox user so the local
    // install runs sandboxed (no "Download is performed unsandboxed" notice).
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::set_permissions(&deb, std::fs::Permissions::from_mode(0o644));
    }

    // Install as root via polkit (a graphical password prompt). Running apt-get
    // through `sh -c` lets it resolve from PATH under pkexec's reset env; apt
    // pulls in any new runtime deps and upgrades in place.
    let status = Command::new("pkexec")
        .arg("sh")
        .arg("-c")
        .arg(r#"apt-get install -y --allow-downgrades "$1""#)
        .arg("inari-update")
        .arg(&deb)
        .status()
        .map_err(|e| format!("pkexec is required to install the update: {e}"))?;

    let _ = std::fs::remove_file(&deb);

    match status.code() {
        Some(0) => Ok(()),
        Some(126) | Some(127) => Err("authorization was cancelled".into()),
        Some(c) => Err(format!("the installer exited with code {c}")),
        None => Err("the installer was terminated".into()),
    }
}

/// Download the latest `.deb` and install it with `pkexec apt-get`. The UI
/// should offer to restart once this returns Ok.
#[tauri::command]
pub async fn apply_update(deb_url: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || apply_blocking(deb_url))
        .await
        .map_err(|e| format!("update install failed to run: {e}"))?
}

/// Relaunch the app so the freshly installed version takes over.
///
/// Tauri's own `restart()` re-execs `current_exe()`, but after an in-place
/// package upgrade the running binary's inode has been replaced, so on Linux
/// that path resolves to a stale `"…/inari (deleted)"` which cannot be exec'd
/// and the app would just quit. Instead: resolve the real path, hand a detached
/// helper the job of relaunching it after we've fully exited (so our virtual
/// sinks are torn down before the fresh instance recreates them), then exit.
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    if let Ok(exe) = std::env::current_exe() {
        let p = exe.to_string_lossy();
        let real = p.strip_suffix(" (deleted)").unwrap_or(&p);
        let quoted = format!("'{}'", real.replace('\'', r"'\''"));
        let script = format!("sleep 2; exec {quoted}");
        // setsid detaches the relauncher into its own session so it survives our
        // exit; fall back to a plain detached shell if setsid is unavailable.
        let spawned = Command::new("setsid")
            .args(["sh", "-c", &script])
            .spawn()
            .is_ok();
        if !spawned {
            let _ = Command::new("sh").args(["-c", &script]).spawn();
        }
    }
    app.exit(0);
}

/// Open a release page in the user's browser.
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("refusing to open a non-https url".into());
    }
    Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("xdg-open: {e}"))
}
