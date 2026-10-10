//! Local automation surface: a loopback HTTP API and an MCP server over stdio,
//! both built on the shared [`commands`] registry.

pub mod commands;
pub mod http;
pub mod mcp;

use crate::core::Core;
use std::sync::Arc;

/// Ensures an API token exists in the vault, creating one on first use.
pub fn ensure_token(core: &Core) -> String {
    if let Some(token) = core.secret("api_token").filter(|t| !t.is_empty()) {
        return token;
    }
    let token = crate::store::crypto::b64(&crate::store::crypto::random_bytes::<24>());
    let _ = core.set_secret("api_token", &token);
    token
}

/// Starts the HTTP API if enabled in settings. Returns a handle that stops it on drop.
pub fn start_http(core: Arc<Core>) -> Option<http::ApiHandle> {
    let settings = core.settings();
    if !settings.api_enabled {
        return None;
    }
    let token = ensure_token(&core);
    match http::start(core, settings.api_port, token, settings.api_expose_secrets, settings.api_max_auth_failures) {
        Ok(handle) => Some(handle),
        Err(err) => {
            crate::log_warn!("Local API did not start: {err}");
            None
        }
    }
}
