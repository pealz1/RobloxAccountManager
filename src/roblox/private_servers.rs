//! Private-server links: resolve share/VIP URLs, list an account's servers, refresh links.

use super::{agent, get_json, network_error, post_json, status_error};
use crate::error::{AppError, AppResult};
use crate::store::model::SavedPrivateServer;
use serde_json::Value;
use std::sync::LazyLock;

use regex::Regex;

static VIP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"roblox\.com/games/(\d+)/[^?#]*\?[^#]*privateServerLinkCode=([A-Za-z0-9]+)").unwrap());
static SHARE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"roblox\.com/share[^?#]*[?&]code=([A-Za-z0-9]+)").unwrap());

/// Turns a VIP URL, share URL or `games/<id>?privateServerLinkCode=` link into
/// `(place_id, link_code)`. A bare numeric code is treated as the link code.
pub fn resolve_share_link(input: &str, cookie: Option<&str>) -> AppResult<(Option<String>, String)> {
    let text = input.trim();
    if text.chars().all(|c| c.is_ascii_digit()) && !text.is_empty() {
        return Ok((None, text.to_owned()));
    }
    if let Some(m) = VIP_RE.captures(text) {
        return Ok((Some(m[1].to_owned()), m[2].to_owned()));
    }
    let Some(code) = SHARE_RE.captures(text).map(|m| m[1].to_owned()) else {
        return Ok((None, String::new()));
    };
    // Share links need resolving through Roblox.
    let cookie = cookie.unwrap_or_default();
    for payload in [serde_json::json!({ "linkId": code, "linkType": "Server" }), serde_json::json!({ "code": code, "type": "Server" })] {
        let Ok(value) = post_json("Share link", "https://apis.roblox.com/sharelinks/v1/resolve-link", cookie, &payload) else {
            continue;
        };
        if let Some((place, link)) = read_resolved(&value) {
            return Ok((Some(place), link));
        }
    }
    Ok((None, String::new()))
}

fn read_resolved(value: &Value) -> Option<(String, String)> {
    let text = value.to_string();
    let place = Regex::new(r#""placeId"\s*:\s*(\d+)"#).ok()?.captures(&text)?[1].to_owned();
    let link =
        Regex::new(r#""(?:linkCode|privateServerLinkCode|accessCode)"\s*:\s*"([A-Za-z0-9_\-]+)""#).ok()?.captures(&text)?[1].to_owned();
    Some((place, link))
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    data: Vec<Value>,
    #[serde(rename = "nextPageCursor", default)]
    next_cursor: Option<String>,
}

use serde::Deserialize;

/// Lists the private servers an account owns (optionally only for one place).
/// Returns saved-server records with the best join link we can build.
pub fn list_servers(cookie: &str, user_id: u64, place_id: Option<u64>) -> AppResult<Vec<SavedPrivateServer>> {
    if cookie.is_empty() {
        return Err(AppError::new("COOKIE_MISSING", "Account Cookie Missing", "This account has no saved cookie."));
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cursor = String::new();
    for _ in 0..100 {
        let url = match place_id {
            Some(pid) => format!("https://games.roblox.com/v1/games/{pid}/private-servers?limit=10&cursor={cursor}"),
            None => format!(
                "https://games.roblox.com/v1/private-servers/my-private-servers?itemsPerPage=10&privateServersTab=MyPrivateServers&cursor={cursor}"
            ),
        };
        let page: Page = get_json("Private servers", &url, Some(cookie))?;
        for entry in &page.data {
            if place_id.is_some() && entry.get("owner").and_then(|o| o.get("id")).and_then(super::super::import::as_u64) != Some(user_id) {
                continue;
            }
            let id_field = if place_id.is_some() { "vipServerId" } else { "privateServerId" };
            let Some(id) = entry.get(id_field).and_then(super::super::import::as_u64).filter(|v| *v > 0) else {
                continue;
            };
            if !seen.insert(id) {
                continue;
            }
            let detail = get_json::<Value>("Private server", &format!("https://games.roblox.com/v1/vip-servers/{id}"), Some(cookie)).ok();
            out.push(build_record(id, entry, detail.as_ref(), place_id));
        }
        match page.next_cursor {
            Some(next) if !next.is_empty() => cursor = next,
            _ => break,
        }
    }
    Ok(out)
}

fn build_record(id: u64, entry: &Value, detail: Option<&Value>, place_filter: Option<u64>) -> SavedPrivateServer {
    let detail = detail.cloned().unwrap_or(Value::Null);
    let game = detail.get("game").cloned().unwrap_or(Value::Null);
    let root = game.get("rootPlace").cloned().unwrap_or(Value::Null);
    let place_id = root
        .get("id")
        .and_then(super::super::import::as_u64)
        .or_else(|| entry.get("placeId").and_then(super::super::import::as_u64))
        .or(place_filter)
        .unwrap_or(0);
    SavedPrivateServer {
        id: id.to_string(),
        vip_server_id: id,
        place_id,
        game_name: game
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| root.get("name").and_then(Value::as_str))
            .unwrap_or("Unknown Game")
            .to_owned(),
        name: detail
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| entry.get("name").and_then(Value::as_str))
            .unwrap_or(&id.to_string())
            .to_owned(),
        link: build_link(&detail, entry, place_id),
        ..Default::default()
    }
}

fn build_link(detail: &Value, entry: &Value, place_id: u64) -> String {
    for source in [detail, entry] {
        for key in ["link", "privateServerLink", "shareLink"] {
            if let Some(link) = source.get(key).and_then(Value::as_str).filter(|l| l.contains("roblox.com")) {
                return normalize_link(link);
            }
        }
    }
    let code = [detail, entry].iter().find_map(|v| {
        ["joinCode", "linkCode", "privateServerLinkCode"].iter().find_map(|k| v.get(*k).and_then(Value::as_str).filter(|s| !s.is_empty()))
    });
    match code {
        Some(code) if place_id > 0 => format!("https://www.roblox.com/games/{place_id}?privateServerLinkCode={code}"),
        _ => String::new(),
    }
}

fn normalize_link(link: &str) -> String {
    let link = link.trim();
    if link.starts_with('/') {
        format!("https://www.roblox.com{link}")
    } else if link.starts_with("roblox.com") || link.starts_with("www.roblox.com") {
        format!("https://{link}")
    } else {
        link.to_owned()
    }
}

/// Regenerates the join code for a VIP server the account owns, returning the new link.
pub fn refresh_link(cookie: &str, vip_server_id: u64, place_id: u64) -> AppResult<String> {
    let value = patch_vip(cookie, vip_server_id)?;
    let link = build_link(&value, &Value::Null, place_id);
    if !link.is_empty() {
        return Ok(link);
    }
    let detail: Value = get_json("Private server", &format!("https://games.roblox.com/v1/vip-servers/{vip_server_id}"), Some(cookie))?;
    let link = build_link(&detail, &Value::Null, place_id);
    if link.is_empty() {
        Err(AppError::new("PRIVATE_SERVER_LINK_UNAVAILABLE", "No Link", "Roblox made a new code but did not return a usable link."))
    } else {
        Ok(link)
    }
}

fn patch_vip(cookie: &str, vip_server_id: u64) -> AppResult<Value> {
    let token = super::csrf_token(cookie)?;
    let response = agent()
        .patch(&format!("https://games.roblox.com/v1/vip-servers/{vip_server_id}"))
        .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
        .header("X-CSRF-TOKEN", &token)
        .config()
        .http_status_as_error(false)
        .build()
        .send_json(serde_json::json!({ "newJoinCode": true }))
        .map_err(|e| network_error("Private server link", &e))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(status_error("Private server link", status));
    }
    let mut response = response;
    response.body_mut().read_json().map_err(|e| AppError::unexpected("vip patch", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_code_passthrough() {
        assert_eq!(resolve_share_link("1234", None).unwrap(), (None, "1234".to_string()));
    }

    #[test]
    fn vip_url_parsed_without_network() {
        let url = "https://www.roblox.com/games/606849621/Jailbreak?privateServerLinkCode=ABC123";
        assert_eq!(resolve_share_link(url, None).unwrap(), (Some("606849621".to_string()), "ABC123".to_string()));
    }

    #[test]
    fn unknown_input_returns_empty_code() {
        assert_eq!(resolve_share_link("not a link", None).unwrap(), (None, String::new()));
    }

    #[test]
    fn link_building_prefers_place_and_code() {
        let entry = serde_json::json!({ "privateServerLinkCode": "xy", "placeId": 42 });
        assert_eq!(build_link(&Value::Null, &entry, 42), "https://www.roblox.com/games/42?privateServerLinkCode=xy");
    }
}
