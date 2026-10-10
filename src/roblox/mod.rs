//! Roblox web API: authentication, user info, presence, games, private servers,
//! launching, quick sign-in and client-log parsing.

pub mod account;
pub mod games;
pub mod launch;
pub mod logs;
pub mod private_servers;

use crate::error::{AppError, AppResult};
use serde::de::DeserializeOwned;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

pub const USER_AGENT: &str = concat!("NovaRAM/", env!("CARGO_PKG_VERSION"));

static AGENT: LazyLock<ureq::Agent> =
    LazyLock::new(|| ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).user_agent(USER_AGENT).build().into());

/// Global floor between unauthenticated username lookups, which Roblox rate-limits hard.
static USERNAME_LOOKUP_GATE: LazyLock<Mutex<Option<Instant>>> = LazyLock::new(|| Mutex::new(None));

pub fn agent() -> &'static ureq::Agent {
    &AGENT
}

/// Spaces out calls to a shared endpoint by `min_gap`.
pub fn throttle(min_gap: Duration) {
    let mut last = USERNAME_LOOKUP_GATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(prev) = *last {
        let elapsed = prev.elapsed();
        if elapsed < min_gap {
            std::thread::sleep(min_gap - elapsed);
        }
    }
    *last = Some(Instant::now());
}

fn network_error(context: &str, err: &ureq::Error) -> AppError {
    use ureq::Error;
    match err {
        Error::Timeout(_) => {
            AppError::new("NETWORK_TIMEOUT", "Request Timed Out", "Roblox did not respond in time. Check your connection and try again.")
                .retryable()
        }
        Error::ConnectionFailed | Error::Io(_) => {
            AppError::new("NETWORK_UNAVAILABLE", "Roblox Could Not Be Reached", "Check your internet connection and try again.").retryable()
        }
        other => AppError::new("NETWORK_REQUEST_FAILED", "Request Failed", format!("{context} could not be completed."))
            .with_detail(other.to_string())
            .retryable(),
    }
}

fn status_error(context: &str, status: u16) -> AppError {
    let base = match status {
        401 | 403 => AppError::new(
            "COOKIE_INVALID",
            "Account Cookie Invalid",
            "Roblox rejected this account cookie. Re-add or re-import the account.",
        ),
        429 => {
            AppError::new("RATE_LIMITED", "Roblox Rate Limit", "Roblox is rate-limiting requests. Wait a moment and try again.").retryable()
        }
        500..=599 => AppError::new("ROBLOX_SERVER_ERROR", "Roblox Error", "Roblox returned a server error. Try again shortly.").retryable(),
        _ => AppError::new("ROBLOX_REQUEST_FAILED", "Roblox Request Failed", format!("{context} failed.")),
    };
    base.with_detail(format!("HTTP {status}"))
}

/// Cookie header value for an account token.
fn cookie_header(cookie: &str) -> String {
    format!(".ROBLOSECURITY={cookie}")
}

/// A GET returning parsed JSON. Authenticated when `cookie` is set.
pub fn get_json<T: DeserializeOwned>(context: &str, url: &str, cookie: Option<&str>) -> AppResult<T> {
    let mut request = agent().get(url).header("Accept", "application/json");
    if let Some(cookie) = cookie {
        request = request.header("Cookie", cookie_header(cookie));
    }
    let mut response = request.call().map_err(|e| network_error(context, &e))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(status_error(context, status));
    }
    response.body_mut().read_json().map_err(|e| {
        AppError::new("ROBLOX_BAD_RESPONSE", "Unexpected Response", format!("{context} returned data Nova could not read."))
            .with_detail(e.to_string())
    })
}

/// Fetches an `x-csrf-token` for authenticated POSTs by hitting the logout endpoint.
pub fn csrf_token(cookie: &str) -> AppResult<String> {
    let result = agent().post("https://auth.roblox.com/v2/logout").header("Cookie", cookie_header(cookie)).send_empty();
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(_)) => {
            // 403 carries the token in its headers; ureq surfaces it as an error we re-read.
            return retry_csrf(cookie);
        }
        Err(err) => return Err(network_error("CSRF token request", &err)),
    };
    response.headers().get("x-csrf-token").and_then(|v| v.to_str().ok()).map(str::to_owned).map(Ok).unwrap_or_else(|| retry_csrf(cookie))
}

fn retry_csrf(cookie: &str) -> AppResult<String> {
    let response = agent()
        .post("https://auth.roblox.com/v2/logout")
        .header("Cookie", cookie_header(cookie))
        .config()
        .http_status_as_error(false)
        .build()
        .send_empty()
        .map_err(|e| network_error("CSRF token request", &e))?;
    response.headers().get("x-csrf-token").and_then(|v| v.to_str().ok()).map(str::to_owned).ok_or_else(|| {
        AppError::new("CSRF_FAILED", "Could Not Authorize", "Roblox did not return a security token. Try again.").retryable()
    })
}

/// Authenticated POST with automatic CSRF retry. Returns the raw response body.
pub fn post_json<B: serde::Serialize>(context: &str, url: &str, cookie: &str, body: &B) -> AppResult<serde_json::Value> {
    let mut token = csrf_token(cookie).unwrap_or_default();
    for attempt in 0..2 {
        let response = agent()
            .post(url)
            .header("Cookie", cookie_header(cookie))
            .header("X-CSRF-TOKEN", &token)
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(body)
            .map_err(|e| network_error(context, &e))?;
        let status = response.status().as_u16();
        if (status == 403 || status == 400)
            && attempt == 0
            && let Some(fresh) = response.headers().get("x-csrf-token").and_then(|v| v.to_str().ok())
        {
            token = fresh.to_owned();
            continue;
        }
        if status != 200 {
            return Err(status_error(context, status));
        }
        let mut response = response;
        return response.body_mut().read_json().map_err(|e| {
            AppError::new("ROBLOX_BAD_RESPONSE", "Unexpected Response", format!("{context} returned data Nova could not read."))
                .with_detail(e.to_string())
        });
    }
    Err(status_error(context, 403))
}
