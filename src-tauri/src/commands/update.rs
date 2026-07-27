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
/// Hosts release assets may come from. GitHub serves the release page from
/// github.com and redirects the actual download to its object store, so both
/// are needed - and nothing else is.
const ALLOWED_HOSTS: [&str; 2] = ["github.com", "objects.githubusercontent.com"];

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

/// True when `url` is https and its authority is exactly one of
/// [`ALLOWED_HOSTS`]. Compares the whole authority, so userinfo tricks
/// ("https://github.com@evil.example/…") and odd ports are rejected too.
fn host_allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or_default();
    ALLOWED_HOSTS.contains(&host)
}

/// The release's `SHA256SUMS` asset, derived from the .deb URL rather than
/// looked up separately: both are assets of the same release, so they share a
/// download directory, and deriving it means the checksum can't be pointed at
/// a different release than the package.
fn sums_url(deb_url: &str) -> Option<String> {
    let (dir, _) = deb_url.rsplit_once('/')?;
    Some(format!("{dir}/SHA256SUMS"))
}

/// The expected digest for `basename` from a `sha256sum` listing.
fn expected_sha(sums: &str, basename: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        // sha256sum's format: digest, two spaces, name.
        let (sha, name) = line.split_once("  ")?;
        (name.trim() == basename).then(|| sha.trim().to_ascii_lowercase())
    })
}

/// The refusal gate in front of `pkexec apt-get`: the URL the UI passed must
/// match the one we resolved ourselves, that URL must be an amd64 `.deb` on an
/// allowed host, and the release must be strictly newer. Split out from
/// [`apply_blocking`] so the refusals can be exercised without a network call.
fn vet_asset(requested: &str, deb_url: &str, available: bool) -> Result<(), String> {
    if !requested.is_empty() && requested != deb_url {
        return Err("refusing to install an unexpected asset".into());
    }
    if !host_allowed(deb_url) || !deb_url.ends_with("_amd64.deb") {
        return Err("refusing to install an unexpected asset".into());
    }
    // Forward only. A stale or spoofed "latest" would otherwise be a way to
    // reinstall a known-bad older build over the running one.
    if !available {
        return Err("the latest release is not newer than the installed version".into());
    }
    Ok(())
}

fn file_sha256(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| format!("reading the download: {e}"))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("hashing the download: {e}"))?;
    Ok(format!("{:x}", hasher.finalize()))
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
            // Pin the scheme across redirects too, so a hijacked redirect can't
            // downgrade the check to plaintext, and cap the hang on a
            // black-holed network.
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "20",
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

fn apply_blocking(requested: String) -> Result<(), String> {
    // The URL the frontend hands us is never trusted: this ends in `apt-get`
    // as root, so the release is looked up again here and only what *we*
    // resolved gets installed. The argument is still accepted for
    // compatibility and must match byte for byte.
    let info = check_blocking()?;
    let deb_url = info
        .deb_url
        .clone()
        .ok_or_else(|| "the latest release ships no .deb package".to_string())?;
    vet_asset(&requested, &deb_url, info.available)?;

    // Fetch the release's checksum list before the package, so a mismatch
    // costs nothing but the listing.
    let basename = deb_url.rsplit('/').next().unwrap_or_default().to_string();
    let sums_url = sums_url(&deb_url).ok_or_else(|| "malformed asset url".to_string())?;
    let sums = Command::new("curl")
        .args([
            "-fsSL",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "30",
            &sums_url,
        ])
        .output()
        .map_err(|e| format!("curl is required to install updates: {e}"))?;
    if !sums.status.success() {
        return Err("could not fetch the release checksums".into());
    }
    let expected = expected_sha(&String::from_utf8_lossy(&sums.stdout), &basename)
        .ok_or_else(|| format!("the release publishes no checksum for {basename}"))?;

    let dir = std::env::temp_dir().join("inari-update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("temp dir: {e}"))?;
    let deb = dir.join("inari_latest_amd64.deb");

    let ok = Command::new("curl")
        .args([
            "-fL",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "600",
        ])
        .arg("-o")
        .arg(&deb)
        .arg(&deb_url)
        .status()
        .map_err(|e| format!("download failed to start: {e}"))?
        .success();
    if !ok {
        return Err("download failed".into());
    }

    // Nothing reaches root without matching the checksum the release workflow
    // published alongside the package.
    match file_sha256(&deb) {
        Ok(actual) if actual == expected => {}
        Ok(_) => {
            let _ = std::fs::remove_file(&deb);
            return Err("the download failed its checksum check; not installing".into());
        }
        Err(e) => {
            let _ = std::fs::remove_file(&deb);
            return Err(e);
        }
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
    // pulls in any new runtime deps and upgrades in place. No
    // `--allow-downgrades`: we only ever install a strictly newer version, and
    // apt refusing a downgrade is a last line of defence worth keeping.
    let status = Command::new("pkexec")
        .arg("sh")
        .arg("-c")
        .arg(r#"apt-get install -y "$1""#)
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

/// Download the latest `.deb`, verify it against the release's `SHA256SUMS`,
/// and install it with `pkexec apt-get`. The `deb_url` the UI passes is only
/// cross-checked, never trusted (see [`apply_blocking`]). The UI should offer
/// to restart once this returns Ok.
#[tauri::command]
pub async fn apply_update(deb_url: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || apply_blocking(deb_url))
        .await
        .map_err(|e| format!("update install failed to run: {e}"))?
}

/// The command handed to the detached relauncher: the stale `" (deleted)"`
/// suffix stripped, and the path single-quoted so spaces (or a quote) in it
/// survive `sh -c`.
fn relaunch_script(exe: &str) -> String {
    let real = exe.strip_suffix(" (deleted)").unwrap_or(exe);
    let quoted = format!("'{}'", real.replace('\'', r"'\''"));
    format!("sleep 2; exec {quoted}")
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
        let script = relaunch_script(&exe.to_string_lossy());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A well-formed asset URL, as the GitHub API hands it to us.
    const DEB: &str =
        "https://github.com/fbnlrz/Inari/releases/download/v1.0.9/inari_1.0.9_amd64.deb";

    // --- version comparison (decides whether a root install happens) -----

    #[test]
    fn semver_compares_numerically_not_lexically() {
        assert!(semver("1.0.10") > semver("1.0.9"), "lexically 10 < 9");
        assert!(semver("1.10.0") > semver("1.9.9"));
        assert_eq!(semver("v1.0.8"), (1, 0, 8));
        assert_eq!(semver("  1.0.8  "), (1, 0, 8));
        // Missing or unparsable components read as zero rather than failing.
        assert_eq!(semver("1.2"), (1, 2, 0));
        assert_eq!(semver(""), (0, 0, 0));
        assert_eq!(semver("not-a-version"), (0, 0, 0));
    }

    #[test]
    fn a_prerelease_of_the_running_version_is_not_an_update() {
        // Trailing text is dropped per component, and `available` is a strict
        // `>` - so v1.0.8-rc1 never offers itself to a 1.0.8 install.
        assert_eq!(semver("1.0.8-rc1"), semver("1.0.8"));
        assert!(semver("1.0.8-rc1") <= semver("1.0.8"));
    }

    // --- the gate in front of `pkexec apt-get` ---------------------------

    #[test]
    fn vet_asset_accepts_what_we_resolved_ourselves() {
        assert!(vet_asset(DEB, DEB, true).is_ok());
        // The url is only cross-checked, so an empty one from the UI is fine.
        assert!(vet_asset("", DEB, true).is_ok());
    }

    #[test]
    fn vet_asset_refuses_a_url_the_webview_made_up() {
        assert!(vet_asset("https://github.com/o/r/other_amd64.deb", DEB, true).is_err());
    }

    #[test]
    fn vet_asset_refuses_assets_off_the_allowlist_or_of_the_wrong_kind() {
        assert!(vet_asset("", "https://evil.example/inari_1.0.9_amd64.deb", true).is_err());
        assert!(
            vet_asset("", "https://github.com@evil.example/inari_1.0.9_amd64.deb", true).is_err(),
            "userinfo"
        );
        assert!(
            vet_asset("", "https://github.com/o/r/download/v1/install.sh", true).is_err(),
            "not a package"
        );
    }

    #[test]
    fn vet_asset_never_installs_backwards() {
        let err = vet_asset(DEB, DEB, false).expect_err("older release");
        assert!(err.contains("not newer"), "{err}");
    }

    // --- asset resolution -------------------------------------------------

    #[test]
    fn host_allowlist_rejects_lookalikes() {
        assert!(host_allowed("https://github.com/fbnlrz/Inari/releases/download/v1/a.deb"));
        assert!(host_allowed("https://objects.githubusercontent.com/x"));
        assert!(!host_allowed("http://github.com/x"), "plaintext");
        assert!(!host_allowed("https://github.com.evil.example/x"), "suffix");
        assert!(!host_allowed("https://github.com@evil.example/x"), "userinfo");
        assert!(!host_allowed("https://github.com:8443/x"), "port");
        assert!(!host_allowed("https://raw.githubusercontent.com/x"));
    }

    #[test]
    fn sums_url_sits_next_to_the_package() {
        assert_eq!(
            sums_url("https://github.com/o/r/releases/download/v1.0.7/inari_1.0.7_amd64.deb"),
            Some("https://github.com/o/r/releases/download/v1.0.7/SHA256SUMS".to_string())
        );
    }

    #[test]
    fn expected_sha_matches_by_basename_only() {
        let sums = "aaaa  inari_1.0.7_amd64.AppImage\nBBBB  inari_1.0.7_amd64.deb\n";
        assert_eq!(
            expected_sha(sums, "inari_1.0.7_amd64.deb").as_deref(),
            Some("bbbb")
        );
        assert!(expected_sha(sums, "inari_9.9.9_amd64.deb").is_none());
    }

    #[test]
    fn sums_url_needs_a_directory_to_derive_from() {
        assert_eq!(sums_url("inari_1.0.7_amd64.deb"), None);
    }

    #[test]
    fn expected_sha_tolerates_crlf_and_skips_junk_lines() {
        // A SHA256SUMS produced on, or served through, something that adds
        // CRs must still match - otherwise every install fails closed.
        let sums = "# a comment\r\nAAAA  inari_1.0.7_amd64.deb\r\n";
        assert_eq!(
            expected_sha(sums, "inari_1.0.7_amd64.deb").as_deref(),
            Some("aaaa")
        );
    }

    // --- the remaining refusal paths --------------------------------------

    #[test]
    fn open_url_refuses_non_https_schemes() {
        // These all return before anything is spawned.
        assert!(open_url("http://example.com".into()).is_err());
        assert!(open_url("file:///etc/shadow".into()).is_err());
        assert!(open_url("javascript:alert(1)".into()).is_err());
    }

    #[test]
    fn relaunch_script_drops_the_deleted_suffix_an_upgrade_leaves() {
        assert_eq!(
            relaunch_script("/usr/bin/inari (deleted)"),
            "sleep 2; exec '/usr/bin/inari'"
        );
    }

    #[test]
    fn relaunch_script_quotes_paths_that_would_otherwise_break_sh() {
        assert_eq!(
            relaunch_script("/opt/my apps/inari"),
            "sleep 2; exec '/opt/my apps/inari'"
        );
        // A quote closes and reopens the literal instead of ending it.
        assert_eq!(
            relaunch_script("/opt/it's/inari"),
            r"sleep 2; exec '/opt/it'\''s/inari'"
        );
    }
}
