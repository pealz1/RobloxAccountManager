//! Nova RAM core library: storage, Roblox API, Windows integration and services.
//! The GUI (`ui`), the local REST API and the MCP server all build on this crate.

pub mod error;
#[macro_use]
pub mod logging;
pub mod api;
pub mod core;
pub mod import;
pub mod paths;
pub mod roblox;
pub mod services;
pub mod store;
pub mod ui;
pub mod win;

pub const APP_NAME: &str = "Nova RAM";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// GitHub repository that publishes releases and receives update checks.
pub const REPO: &str = "pealz1/RobloxAccountManager";
pub const UPSTREAM_REPO: &str = "evanovar/RobloxAccountManager";
