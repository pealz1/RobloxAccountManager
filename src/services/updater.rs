//! Self-update from the fork's GitHub releases only, verified by SHA-256, with a
//! rollback copy kept until the new build starts.

use super::Services;
use crate::error::{AppError, AppResult};
use crate::roblox::agent;
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

const TRUSTED_HOSTS: [&str; 2] = ["github.com", "objects.githubusercontent.com"];

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub exe_url: String,
    pub sha256_url: String,
    pub notes: String,
}

#[derive(Deserialize)]
struct Release {
    #[serde(rename = "tag_name", default)]
    tag: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize, Clone)]
struct Asset {
    #[serde(default)]
    name: String,
    #[serde(rename = "browser_download_url", default)]
    url: String,
}

/// Parses `v1.2.3` / `1.2.3` into comparable parts.
fn parts(version: &str) -> Vec<u64> {
    version.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).filter_map(|p| p.parse().ok()).collect()
}

/// True when `latest` is newer than `current`.
pub fn is_newer(current: &str, latest: &str) -> bool {
    parts(latest) > parts(current)
}

fn host_of(url: &str) -> &str {
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("")
}

fn trusted(url: &str) -> bool {
    let host = host_of(url);
    TRUSTED_HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

/// Queries the fork's releases and returns an update if one is newer than this build.
pub fn check(beta: bool) -> AppResult<Option<UpdateInfo>> {
    let url = format!("https://api.github.com/repos/{}/releases?per_page=10", crate::REPO);
    let mut response = agent().get(&url).header("Accept", "application/vnd.github+json").call().map_err(|_| {
        AppError::new("UPDATE_CHECK_FAILED", "Update Check Failed", "Could not reach GitHub to check for updates.").retryable()
    })?;
    if response.status().as_u16() != 200 {
        return Err(AppError::new("UPDATE_CHECK_FAILED", "Update Check Failed", "GitHub did not return the releases.").retryable());
    }
    let releases: Vec<Release> = response.body_mut().read_json().map_err(|e| AppError::unexpected("release list", e))?;
    let chosen = releases.into_iter().find(|r| beta || !r.prerelease);
    let Some(release) = chosen else { return Ok(None) };
    if !is_newer(crate::VERSION, &release.tag) {
        return Ok(None);
    }
    let exe = release.assets.iter().find(|a| a.name.ends_with(".exe"));
    let sha = release.assets.iter().find(|a| a.name.ends_with(".sha256"));
    match (exe, sha) {
        (Some(exe), Some(sha)) if trusted(&exe.url) && trusted(&sha.url) => Ok(Some(UpdateInfo {
            version: release.tag.trim_start_matches(['v', 'V']).to_owned(),
            exe_url: exe.url.clone(),
            sha256_url: sha.url.clone(),
            notes: if release.body.is_empty() { release.name } else { release.body },
        })),
        _ => Ok(None),
    }
}

/// One-shot background check that records any available update for the UI.
pub fn start_check(services: &Arc<Services>) {
    let stop = Arc::new(AtomicBool::new(false));
    let beta = services.core.settings().update_channel == crate::store::settings::UpdateChannel::Beta;
    let skipped = services.core.settings().skipped_version;
    let worker_services = Arc::clone(services);
    let worker = super::Worker::spawn("update-check", stop, move || {
        if let Ok(Some(info)) = check(beta)
            && info.version != skipped
        {
            worker_services.live.set_update_available(Some(info.version));
            worker_services.request_repaint();
        }
    });
    services.add_worker(worker);
}

fn download(url: &str, limit: usize) -> AppResult<Vec<u8>> {
    if !trusted(url) {
        return Err(AppError::new("UPDATE_UNTRUSTED", "Untrusted Download", "The update URL is not a GitHub release."));
    }
    let mut response = agent()
        .get(url)
        .call()
        .map_err(|_| AppError::new("UPDATE_DOWNLOAD_FAILED", "Download Failed", "The update could not be downloaded.").retryable())?;
    let bytes = response.body_mut().with_config().limit(limit as u64).read_to_vec().map_err(|e| {
        AppError::new("UPDATE_DOWNLOAD_FAILED", "Download Failed", "The update could not be downloaded.").with_detail(e.to_string())
    })?;
    Ok(bytes)
}

/// Downloads the update, checks its SHA-256, and swaps the running exe. On Windows
/// this writes a helper script that waits for exit, replaces the exe (keeping a
/// `.old` rollback copy) and relaunches. Returns once the helper has been started.
pub fn download_and_apply(info: &UpdateInfo) -> AppResult<()> {
    use sha2::{Digest, Sha256};
    let exe_bytes = download(&info.exe_url, 200 * 1024 * 1024)?;
    let expected = String::from_utf8_lossy(&download(&info.sha256_url, 4096)?).split_whitespace().next().unwrap_or_default().to_lowercase();
    if expected.len() != 64 {
        return Err(AppError::new("UPDATE_NO_CHECKSUM", "No Checksum", "The release did not publish a usable SHA-256."));
    }
    let actual = format!("{:x}", Sha256::digest(&exe_bytes));
    if actual != expected {
        return Err(AppError::new(
            "UPDATE_CHECKSUM_MISMATCH",
            "Update Rejected",
            "The downloaded update failed its checksum and was discarded.",
        ));
    }
    let current = crate::paths::exe_path();
    let staged = current.with_extension("new");
    std::fs::write(&staged, &exe_bytes).map_err(|e| AppError::io("Writing the update", &e))?;
    apply_swap(&current, &staged)
}

#[cfg(windows)]
fn apply_swap(current: &std::path::Path, staged: &std::path::Path) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let pid = std::process::id();
    let old = current.with_extension("old");
    let script = crate::paths::data_file("update.cmd");
    let body = format!(
        "@echo off\r\n\
         :wait\r\n\
         tasklist /FI \"PID eq {pid}\" | find \"{pid}\" >nul && (timeout /t 1 /nobreak >nul & goto wait)\r\n\
         del \"{old}\" >nul 2>&1\r\n\
         move /y \"{cur}\" \"{old}\" >nul\r\n\
         move /y \"{new}\" \"{cur}\" >nul\r\n\
         start \"\" \"{cur}\"\r\n\
         del \"%~f0\" >nul 2>&1\r\n",
        old = old.display(),
        cur = current.display(),
        new = staged.display(),
    );
    std::fs::write(&script, body).map_err(|e| AppError::io("Writing the update helper", &e))?;
    std::process::Command::new("cmd")
        .args(["/c", "start", "", "/min", &script.to_string_lossy()])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| {
            AppError::new("UPDATE_LAUNCH_FAILED", "Update Failed", "The update helper could not start.").with_detail(e.to_string())
        })?;
    crate::log_info!("Update staged; exit the app to finish installing");
    Ok(())
}

#[cfg(not(windows))]
fn apply_swap(_current: &std::path::Path, _staged: &std::path::Path) -> AppResult<()> {
    Err(AppError::new("WINDOWS_ONLY", "Windows Only", "Self-update is only supported on Windows."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert!(is_newer("3.0.0", "3.0.1"));
        assert!(is_newer("3.0.0", "v3.1.0"));
        assert!(!is_newer("3.0.0", "3.0.0"));
        assert!(!is_newer("3.1.0", "3.0.9"));
    }

    #[test]
    fn only_github_hosts_are_trusted() {
        assert!(trusted("https://github.com/x/y/releases/download/v1/NovaRAM.exe"));
        assert!(trusted("https://objects.githubusercontent.com/abc"));
        assert!(!trusted("https://evil.example.com/NovaRAM.exe"));
        assert!(!trusted("https://github.com.evil.com/x"));
    }
}
