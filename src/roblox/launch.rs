//! Builds the `roblox-player:` launch URL and starts it through the chosen launcher.

use super::account;
use crate::error::{AppError, AppResult};
use crate::store::settings::Launcher;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

static PLACE_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]{1,20}$").unwrap());
static JOB_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9A-Za-z\-]{1,64}$").unwrap());
static LINK_CODE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9A-Za-z_\-]{1,64}$").unwrap());

/// What to join. Any of the fields may be empty.
#[derive(Debug, Clone, Default)]
pub struct LaunchRequest {
    pub place_id: String,
    /// A numeric VIP link code, a VIP URL, or a share URL.
    pub private_server: String,
    pub job_id: String,
}

/// Evidence returned so the caller can match the launch to a process later.
#[derive(Debug, Clone)]
pub struct Launched {
    pub browser_tracker_id: u64,
    pub launch_time_ms: i64,
    pub place_id: String,
}

fn validate_ids(request: &LaunchRequest) -> AppResult<()> {
    if !request.place_id.is_empty() && !PLACE_ID_RE.is_match(&request.place_id) {
        return Err(AppError::invalid("PLACE_ID_INVALID", "The Place ID can only contain digits."));
    }
    if !request.job_id.is_empty() && !JOB_ID_RE.is_match(&request.job_id) {
        return Err(AppError::invalid("JOB_ID_INVALID", "The Job ID can only contain letters, digits and dashes."));
    }
    Ok(())
}

/// Builds the launch URL. `resolve_share` turns a share/VIP link into (place_id, link_code).
pub fn build_url(
    ticket: &str,
    request: &LaunchRequest,
    resolve_share: impl FnOnce(&str) -> AppResult<(Option<String>, String)>,
) -> AppResult<(String, Launched)> {
    validate_ids(request)?;
    let tracker = rand::random::<u64>() % 8_000_000_000_000_000 + 1_000_000_000_000_000;
    let launch_time = chrono::Utc::now().timestamp_millis();
    let mut place_id = request.place_id.clone();

    if request.place_id.is_empty() && request.private_server.is_empty() {
        let url = format!(
            "roblox-player:1+launchmode:play+gameinfo:{ticket}+launchtime:{launch_time}+browsertrackerid:{tracker}+robloxLocale:en_us+gameLocale:en_us"
        );
        return Ok((url, Launched { browser_tracker_id: tracker, launch_time_ms: launch_time, place_id }));
    }

    let mut link_code = String::new();
    if !request.private_server.is_empty() {
        let ps = request.private_server.trim();
        if ps.chars().all(|c| c.is_ascii_digit()) {
            link_code = ps.to_owned();
        } else {
            let (resolved_place, resolved_code) = resolve_share(ps)?;
            if resolved_code.is_empty() {
                return Err(AppError::invalid(
                    "PRIVATE_SERVER_INVALID",
                    "Could not read that private server. Use a numeric code, a VIP URL or a share link.",
                ));
            }
            if place_id.is_empty() {
                place_id = resolved_place.unwrap_or_default();
            }
            link_code = resolved_code;
        }
    }

    if !link_code.is_empty() && !LINK_CODE_RE.is_match(&link_code) {
        return Err(AppError::invalid("PRIVATE_SERVER_INVALID", "The private server link contains unexpected characters."));
    }
    if place_id.is_empty() {
        return Err(AppError::invalid("PLACE_ID_MISSING", "Enter a Place ID or a valid private-server link."));
    }
    if !PLACE_ID_RE.is_match(&place_id) {
        return Err(AppError::invalid("PLACE_ID_INVALID", "The Place ID can only contain digits."));
    }

    let mut url = format!(
        "roblox-player:1+launchmode:play+gameinfo:{ticket}+launchtime:{launch_time}\
         +placelauncherurl:https://assetgame.roblox.com/game/PlaceLauncher.ashx?request=RequestGameJob\
         &browserTrackerId={tracker}&placeId={place_id}&isPlayTogetherGame=false"
    );
    if !link_code.is_empty() {
        url.push_str(&format!("&linkCode={link_code}"));
    } else if !request.job_id.is_empty() {
        url.push_str(&format!("&gameId={}", request.job_id));
    }
    url.push_str(&format!("+browsertrackerid:{tracker}+robloxLocale:en_us+gameLocale:en_us"));
    Ok((url, Launched { browser_tracker_id: tracker, launch_time_ms: launch_time, place_id }))
}

/// Full launch: fetch a ticket, build the URL and start it. Returns launch evidence.
pub fn launch(cookie: &str, request: &LaunchRequest, launcher: Launcher, custom_path: &str) -> AppResult<Launched> {
    let ticket = account::auth_ticket(cookie)?;
    let (url, launched) = build_url(&ticket, request, |share| super::private_servers::resolve_share_link(share, Some(cookie)))?;
    start(&url, launcher, custom_path)?;
    Ok(launched)
}

/// Starts a prepared `roblox-player:` URL with the selected launcher.
pub fn start(url: &str, launcher: Launcher, custom_path: &str) -> AppResult<()> {
    match launcher {
        Launcher::Default => open_default(url),
        Launcher::Client => spawn(&find_roblox_player()?, &[url]),
        Launcher::Custom => {
            let path = PathBuf::from(custom_path.trim());
            if custom_path.trim().is_empty() {
                return Err(AppError::new("CUSTOM_LAUNCHER_UNSET", "Custom Launcher Not Set", "Choose a custom launcher in Settings."));
            }
            if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("exe")) != Some(true) {
                return Err(AppError::invalid("CUSTOM_LAUNCHER_INVALID", "The custom launcher must be an .exe file."));
            }
            if !path.exists() {
                return Err(AppError::not_found("Custom Launcher", custom_path));
            }
            spawn(&path, &[url])
        }
        strap => {
            let (folder, exe) = strap_paths(strap);
            let path = local_appdata_join(&[folder, exe])?;
            if !path.exists() {
                return Err(AppError::not_found(folder, &path.to_string_lossy()));
            }
            spawn(&path, &["-player", url])
        }
    }
}

fn strap_paths(launcher: Launcher) -> (&'static str, &'static str) {
    match launcher {
        Launcher::Bloxstrap => ("Bloxstrap", "Bloxstrap.exe"),
        Launcher::Fishstrap => ("Fishstrap", "Fishstrap.exe"),
        Launcher::Froststrap => ("Froststrap", "Froststrap.exe"),
        Launcher::Voidstrap => ("Voidstrap", "Voidstrap.exe"),
        _ => ("Bloxstrap", "Bloxstrap.exe"),
    }
}

fn local_appdata_join(parts: &[&str]) -> AppResult<PathBuf> {
    let mut path = crate::paths::local_appdata().ok_or_else(|| {
        AppError::new("LOCALAPPDATA_MISSING", "Windows App Data Missing", "The LOCALAPPDATA folder could not be located.")
    })?;
    for part in parts {
        path.push(part);
    }
    Ok(path)
}

/// Newest installed RobloxPlayerBeta.exe.
pub fn find_roblox_player() -> AppResult<PathBuf> {
    let versions = local_appdata_join(&["Roblox", "Versions"])?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&versions).into_iter().flatten().flatten() {
        if !entry.file_name().to_string_lossy().starts_with("version-") {
            continue;
        }
        let exe = entry.path().join("RobloxPlayerBeta.exe");
        if exe.exists() {
            let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().is_none_or(|(t, _)| modified > *t) {
                best = Some((modified, exe));
            }
        }
    }
    best.map(|(_, p)| p).ok_or_else(|| {
        AppError::new(
            "ROBLOX_NOT_INSTALLED",
            "Roblox Client Not Found",
            "No installed Roblox Player was found. Install Roblox or pick another launcher.",
        )
    })
}

#[cfg(windows)]
fn spawn(program: &Path, args: &[&str]) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new(program).args(args).creation_flags(CREATE_NO_WINDOW).spawn().map(|_| ()).map_err(|e| {
        AppError::new("LAUNCH_FAILED", "Roblox Could Not Start", "Windows could not start the launcher.").with_detail(e.to_string())
    })
}

#[cfg(not(windows))]
fn spawn(program: &Path, args: &[&str]) -> AppResult<()> {
    std::process::Command::new(program).args(args).spawn().map(|_| ()).map_err(|e| AppError::io("launch", &e))
}

#[cfg(windows)]
fn open_default(url: &str) -> AppResult<()> {
    // ShellExecute the protocol URL the way `os.startfile` does.
    spawn_shell(url)
}

#[cfg(not(windows))]
fn open_default(_url: &str) -> AppResult<()> {
    Err(AppError::new("WINDOWS_ONLY", "Windows Only", "Launching Roblox is only supported on Windows."))
}

#[cfg(windows)]
fn spawn_shell(url: &str) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // `cmd /c start "" <url>` hands the protocol URL to the registered handler.
    std::process::Command::new("cmd").args(["/c", "start", "", url]).creation_flags(CREATE_NO_WINDOW).spawn().map(|_| ()).map_err(|e| {
        AppError::new("LAUNCH_FAILED", "Roblox Could Not Start", "Windows could not open the Roblox link.").with_detail(e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_share(_: &str) -> AppResult<(Option<String>, String)> {
        Ok((None, String::new()))
    }

    #[test]
    fn home_launch_has_no_place() {
        let (url, launched) = build_url("TICKET", &LaunchRequest::default(), no_share).unwrap();
        assert!(url.contains("gameinfo:TICKET"));
        assert!(!url.contains("placeId"));
        assert!(launched.browser_tracker_id >= 1_000_000_000_000_000);
    }

    #[test]
    fn place_launch_includes_place_and_tracker() {
        let req = LaunchRequest { place_id: "606849621".into(), ..Default::default() };
        let (url, _) = build_url("T", &req, no_share).unwrap();
        assert!(url.contains("placeId=606849621"));
        assert!(url.contains("browserTrackerId="));
    }

    #[test]
    fn numeric_private_server_becomes_link_code() {
        let req = LaunchRequest { place_id: "1".into(), private_server: "123456".into(), ..Default::default() };
        let (url, _) = build_url("T", &req, |_| panic!("should not resolve numeric codes")).unwrap();
        assert!(url.contains("&linkCode=123456"));
    }

    #[test]
    fn share_link_is_resolved_for_place_and_code() {
        let req = LaunchRequest { private_server: "https://roblox.com/share?code=abc".into(), ..Default::default() };
        let (url, launched) = build_url("T", &req, |_| Ok((Some("99".into()), "code9".into()))).unwrap();
        assert!(url.contains("placeId=99") && url.contains("&linkCode=code9"));
        assert_eq!(launched.place_id, "99");
    }

    #[test]
    fn job_id_is_used_when_no_private_server() {
        let req = LaunchRequest { place_id: "5".into(), job_id: "abc-123".into(), ..Default::default() };
        let (url, _) = build_url("T", &req, no_share).unwrap();
        assert!(url.contains("&gameId=abc-123"));
    }

    #[test]
    fn invalid_ids_are_rejected() {
        let req = LaunchRequest { place_id: "12x".into(), ..Default::default() };
        assert_eq!(build_url("T", &req, no_share).err().unwrap().code, "PLACE_ID_INVALID");
    }
}
