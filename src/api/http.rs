//! Loopback HTTP API. Binds 127.0.0.1 only, requires a bearer token, refuses any
//! request carrying a browser `Origin`, and locks out after repeated bad tokens.

use super::commands;
use crate::core::Core;
use crate::error::{AppError, AppResult};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Request, Response, Server};

const AUTH_WINDOW: Duration = Duration::from_secs(60);

pub struct ApiHandle {
    stop: Arc<AtomicBool>,
}

impl Drop for ApiHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

struct Lockout {
    failures: Vec<Instant>,
    max: u32,
}

impl Lockout {
    fn locked(&mut self) -> bool {
        let cutoff = Instant::now() - AUTH_WINDOW;
        self.failures.retain(|t| *t > cutoff);
        self.max > 0 && self.failures.len() as u32 >= self.max
    }
}

/// Starts the server on a background thread.
pub fn start(core: Arc<Core>, port: u16, token: String, max_auth_failures: u32) -> AppResult<ApiHandle> {
    let server = Server::http(("127.0.0.1", port)).map_err(|e| {
        AppError::new("API_BIND_FAILED", "API Port In Use", format!("Could not bind 127.0.0.1:{port}.")).with_detail(e.to_string())
    })?;
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let lockout = Mutex::new(Lockout { failures: Vec::new(), max: max_auth_failures });
    std::thread::Builder::new()
        .name("api-http".into())
        .spawn(move || {
            crate::log_info!("Local API listening on http://127.0.0.1:{port}");
            while !worker_stop.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(400)) {
                    Ok(Some(request)) => handle(request, &core, &token, &lockout),
                    Ok(None) => continue,
                    Err(_) => break,
                }
            }
        })
        .ok();
    Ok(ApiHandle { stop })
}

fn handle(mut request: Request, core: &Core, token: &str, lockout: &Mutex<Lockout>) {
    // Refuse browser-originated requests outright (defense in depth; we only bind loopback).
    if header(&request, "origin").is_some() {
        return respond(request, 403, json!({"error": "Requests with an Origin header are refused."}));
    }
    if lockout.lock().unwrap_or_else(|p| p.into_inner()).locked() {
        return respond(request, 429, json!({"error": "Too many failed authentications. Try again shortly."}));
    }
    match header(&request, "authorization") {
        Some(header) if constant_time_eq(header.trim_start_matches("Bearer ").trim(), token) => {}
        Some(_) => {
            // A token was presented but wrong: count it toward the lockout.
            lockout.lock().unwrap_or_else(|p| p.into_inner()).failures.push(Instant::now());
            return respond(request, 401, json!({"error": "Invalid API token."}));
        }
        // A missing token is not counted, so a drive-by page cannot lock out the owner.
        None => return respond(request, 401, json!({"error": "Missing API token."})),
    }
    // Read the exposure setting live so toggling it off takes effect without a restart.
    let expose = core.settings().api_expose_secrets;

    let method = request.method().clone();
    let path = request.url().split('?').next().unwrap_or("").to_owned();

    let result: AppResult<Value> = match (&method, path.as_str()) {
        (Method::Get, "/") | (Method::Get, "/tools") => Ok(tool_list()),
        (Method::Get, "/accounts") => commands::dispatch(core, "list_accounts", &json!({}), expose),
        (Method::Get, "/status") => commands::dispatch(core, "status", &json!({}), expose),
        (Method::Post, p) if p.starts_with("/tools/") => {
            let name = p.trim_start_matches("/tools/").to_owned();
            match read_body(&mut request) {
                Ok(body) => commands::dispatch(core, &name, &body, expose),
                Err(err) => Err(err),
            }
        }
        _ => Err(AppError::not_found("Route", &path)),
    };

    match result {
        Ok(value) => respond(request, 200, value),
        Err(err) => respond(request, status_for(&err.code), json!({"error": err.message, "code": err.code})),
    }
}

fn read_body(request: &mut Request) -> AppResult<Value> {
    let mut buf = String::new();
    request
        .as_reader()
        .read_to_string(&mut buf)
        .map_err(|e| AppError::new("API_BAD_BODY", "Bad Request", "Could not read the request body.").with_detail(e.to_string()))?;
    if buf.trim().is_empty() {
        Ok(json!({}))
    } else {
        serde_json::from_str(&buf).map_err(|e| AppError::invalid("API_BAD_JSON", format!("Invalid JSON: {e}")))
    }
}

fn tool_list() -> Value {
    json!({
        "app": crate::APP_NAME,
        "version": crate::VERSION,
        "tools": commands::tools().into_iter().map(|t| json!({
            "name": t.name, "description": t.description, "destructive": t.destructive, "schema": t.schema,
        })).collect::<Vec<_>>(),
    })
}

fn header(request: &Request, name: &str) -> Option<String> {
    request.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_owned())
}

fn status_for(code: &str) -> u16 {
    match code {
        "NOT_FOUND" => 404,
        "CONFIRM_REQUIRED" | "ARG_MISSING" | "API_BAD_JSON" | "PLACE_ID_INVALID" => 400,
        "COOKIE_INVALID" => 401,
        "RATE_LIMITED" => 429,
        _ => 500,
    }
}

fn respond(request: Request, status: u16, value: Value) {
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    let _ = request.respond(Response::from_string(value.to_string()).with_status_code(status).with_header(header));
}

/// Constant-time comparison for the bearer token (no early return on mismatch).
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
