//! Account-facing Roblox calls: identity, validation, presence, auth tickets, quick sign-in.

use super::{agent, get_json, network_error, post_json, status_error, throttle};
use crate::error::{AppError, AppResult};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub user_id: u64,
    pub username: String,
    pub display_name: String,
}

#[derive(Deserialize)]
struct AuthenticatedUser {
    id: u64,
    name: String,
    #[serde(rename = "displayName", default)]
    display_name: String,
}

/// The account that owns a cookie. Also the cheapest validity check.
pub fn whoami(cookie: &str) -> AppResult<Identity> {
    let user: AuthenticatedUser = get_json("Account lookup", "https://users.roblox.com/v1/users/authenticated", Some(cookie))?;
    Ok(Identity { user_id: user.id, username: user.name, display_name: user.display_name })
}

/// Confirms a cookie is accepted by Roblox right now.
pub fn validate(cookie: &str) -> AppResult<()> {
    whoami(cookie).map(|_| ())
}

#[derive(Deserialize)]
struct UsernameLookup {
    data: Vec<UsernameEntry>,
}

#[derive(Deserialize)]
struct UsernameEntry {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(rename = "requestedUsername", default)]
    requested: String,
}

/// Resolves a username to a user id. Rate-limited, so calls are spaced apart.
pub fn user_id_from_username(username: &str) -> AppResult<u64> {
    throttle(Duration::from_secs(1));
    let body = serde_json::json!({ "usernames": [username], "excludeBannedUsers": false });
    let response = agent()
        .post("https://users.roblox.com/v1/usernames/users")
        .header("Content-Type", "application/json")
        .config()
        .http_status_as_error(false)
        .build()
        .send_json(&body)
        .map_err(|e| network_error("Username lookup", &e))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(status_error("Username lookup", status));
    }
    let mut response = response;
    let parsed: UsernameLookup = response.body_mut().read_json().map_err(|e| AppError::unexpected("username lookup", e))?;
    parsed
        .data
        .into_iter()
        .find(|e| e.name.eq_ignore_ascii_case(username) || e.requested.eq_ignore_ascii_case(username))
        .map(|e| e.id)
        .ok_or_else(|| AppError::not_found("Roblox User", username))
}

#[derive(Deserialize)]
struct UserProfile {
    name: String,
    #[serde(rename = "displayName", default)]
    display_name: String,
}

pub fn username_from_user_id(user_id: u64) -> AppResult<Identity> {
    let profile: UserProfile = get_json("User lookup", &format!("https://users.roblox.com/v1/users/{user_id}"), None)?;
    Ok(Identity { user_id, username: profile.name, display_name: profile.display_name })
}

#[derive(Debug, Clone, Default)]
pub struct Presence {
    pub online: bool,
    pub in_game: bool,
    pub place_id: Option<u64>,
    pub root_place_id: Option<u64>,
    pub universe_id: Option<u64>,
    pub job_id: String,
    pub last_location: String,
}

#[derive(Deserialize)]
struct PresenceResponse {
    #[serde(rename = "userPresences", default)]
    presences: Vec<PresenceEntry>,
}

#[derive(Deserialize, Default)]
struct PresenceEntry {
    #[serde(rename = "userPresenceType", default)]
    kind: u8,
    #[serde(rename = "lastLocation", default)]
    last_location: String,
    #[serde(rename = "placeId", default)]
    place_id: Option<u64>,
    #[serde(rename = "rootPlaceId", default)]
    root_place_id: Option<u64>,
    #[serde(rename = "universeId", default)]
    universe_id: Option<u64>,
    #[serde(rename = "gameId", default)]
    game_id: Option<String>,
}

/// Where a user is right now, as seen by `cookie`'s account.
pub fn presence(user_id: u64, cookie: &str) -> AppResult<Presence> {
    let body = serde_json::json!({ "userIds": [user_id] });
    let value = post_json("Presence", "https://presence.roblox.com/v1/presence/users", cookie, &body)?;
    let parsed: PresenceResponse = serde_json::from_value(value).map_err(|e| AppError::unexpected("presence", e))?;
    let entry = parsed.presences.into_iter().next().unwrap_or_default();
    Ok(Presence {
        online: entry.kind != 0,
        in_game: entry.kind == 2,
        place_id: entry.place_id,
        root_place_id: entry.root_place_id,
        universe_id: entry.universe_id,
        job_id: entry.game_id.unwrap_or_default(),
        last_location: entry.last_location,
    })
}

/// A one-time ticket used to launch a game as this account.
pub fn auth_ticket(cookie: &str) -> AppResult<String> {
    if cookie.trim().is_empty() {
        return Err(AppError::new("COOKIE_MISSING", "Account Cookie Missing", "This account has no saved cookie."));
    }
    let token = super::csrf_token(cookie)?;
    let response = agent()
        .post("https://auth.roblox.com/v1/authentication-ticket/")
        .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
        .header("X-CSRF-TOKEN", &token)
        .header("Referer", "https://www.roblox.com/")
        .header("RBX-For-Gameauth", "true")
        .config()
        .http_status_as_error(false)
        .build()
        .send_empty()
        .map_err(|e| network_error("Authentication ticket", &e))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(status_error("Authentication ticket", status));
    }
    response
        .headers()
        .get("rbx-authentication-ticket")
        .and_then(|v| v.to_str().ok())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            AppError::new(
                "AUTH_TICKET_MISSING",
                "Authentication Ticket Missing",
                "Roblox did not return a launch ticket. Try again shortly.",
            )
            .retryable()
        })
}

// ---------- Quick sign-in (cross-device login codes) ----------

/// A pending quick sign-in. The user approves `code` on roblox.com/login/enterCode
/// (or in the Roblox app) while Nova polls for the result.
#[derive(Debug, Clone)]
pub struct QuickLogin {
    pub code: String,
    pub private_key: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
struct CreateResponse {
    code: String,
    #[serde(rename = "privateKey")]
    private_key: String,
    #[serde(rename = "expirationTime")]
    expiration_time: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickStatus {
    Pending,
    /// The code was entered and the account approved; ready to finish sign-in.
    Validated(String),
    Cancelled,
}

/// Starts a quick sign-in and returns the code to show the user.
pub fn quick_login_start() -> AppResult<QuickLogin> {
    let response = agent()
        .post("https://apis.roblox.com/auth-token-service/v1/login/create")
        .header("Content-Type", "application/json")
        .send(b"{}".as_slice())
        .map_err(|e| network_error("Quick sign-in", &e))?;
    let mut response = response;
    let parsed: CreateResponse = response.body_mut().read_json().map_err(|e| AppError::unexpected("quick login create", e))?;
    Ok(QuickLogin { code: parsed.code, private_key: parsed.private_key, expires_at: parsed.expiration_time })
}

/// Polls the status of a pending quick sign-in. Poll roughly every 4 seconds.
pub fn quick_login_poll(login: &QuickLogin) -> AppResult<QuickStatus> {
    if chrono::Utc::now() > login.expires_at {
        return Ok(QuickStatus::Cancelled);
    }
    let body = serde_json::json!({ "code": login.code, "privateKey": login.private_key });
    let response = agent()
        .post("https://apis.roblox.com/auth-token-service/v1/login/status")
        .header("Content-Type", "application/json")
        .config()
        .http_status_as_error(false)
        .build()
        .send_json(&body)
        .map_err(|e| network_error("Quick sign-in", &e))?;
    let status = response.status().as_u16();
    if status == 400 {
        return Ok(QuickStatus::Cancelled);
    }
    if status == 403
        && let Some(token) = response.headers().get("x-csrf-token").and_then(|v| v.to_str().ok()).map(str::to_owned)
    {
        let retry = agent()
            .post("https://apis.roblox.com/auth-token-service/v1/login/status")
            .header("Content-Type", "application/json")
            .header("X-CSRF-TOKEN", &token)
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(&body)
            .map_err(|e| network_error("Quick sign-in", &e))?;
        return read_status(retry);
    }
    read_status(response)
}

fn read_status(mut response: ureq::http::Response<ureq::Body>) -> AppResult<QuickStatus> {
    #[derive(Deserialize)]
    struct StatusBody {
        #[serde(default)]
        status: String,
    }
    let body: StatusBody = response.body_mut().read_json().map_err(|e| AppError::unexpected("quick login status", e))?;
    Ok(match body.status.as_str() {
        "Validated" => QuickStatus::Validated(String::new()),
        "Cancelled" => QuickStatus::Cancelled,
        _ => QuickStatus::Pending,
    })
}

/// Finishes an approved quick sign-in and returns the account cookie.
pub fn quick_login_redeem(login: &QuickLogin) -> AppResult<String> {
    let body = serde_json::json!({ "ctype": "AuthToken", "cvalue": login.code, "password": login.private_key });
    let mut token = String::new();
    for attempt in 0..2 {
        let response = agent()
            .post("https://auth.roblox.com/v2/login")
            .header("Content-Type", "application/json")
            .header("X-CSRF-TOKEN", &token)
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(&body)
            .map_err(|e| network_error("Quick sign-in", &e))?;
        let status = response.status().as_u16();
        if (status == 403 || status == 400)
            && attempt == 0
            && let Some(fresh) = response.headers().get("x-csrf-token").and_then(|v| v.to_str().ok())
        {
            token = fresh.to_owned();
            continue;
        }
        if status != 200 {
            return Err(status_error("Quick sign-in", status));
        }
        return extract_cookie(&response);
    }
    Err(status_error("Quick sign-in", 403))
}

fn extract_cookie(response: &ureq::http::Response<ureq::Body>) -> AppResult<String> {
    for value in response.headers().get_all("set-cookie") {
        if let Ok(text) = value.to_str()
            && let Some(rest) = text.strip_prefix(".ROBLOSECURITY=")
        {
            let cookie = rest.split(';').next().unwrap_or_default();
            if !cookie.is_empty() {
                return Ok(cookie.to_owned());
            }
        }
    }
    Err(AppError::new("QUICK_LOGIN_NO_COOKIE", "Sign-In Incomplete", "Roblox approved the code but did not return a session."))
}
