#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use nova::core::Core;
use std::sync::Arc;

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    let data_override = nova::paths::take_data_dir_arg(&mut args);
    let data_dir = nova::paths::init_data_dir(data_override).to_path_buf();

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("{} {}", nova::APP_NAME, nova::VERSION);
        return;
    }

    nova::logging::init(nova::store::Settings::load(&data_dir).log_retention);

    // Headless MCP server for AI tools (Claude, Codex) spawned over stdio.
    if args.iter().any(|a| a == "--mcp") {
        match open_core(&data_dir, mcp_password()) {
            Ok(core) => nova::api::mcp::serve_stdio(core),
            Err(err) => eprintln!("Nova RAM could not open its vault: {err}"),
        }
        return;
    }

    let core = match open_core(&data_dir, None) {
        Ok(core) => core,
        Err(err) if err.code == "VAULT_LOCKED" => match nova::ui::unlock_prompt(&data_dir) {
            Some(core) => core,
            None => return,
        },
        Err(err) => {
            nova::log_error!("Startup failed: {err}");
            nova::ui::fatal_dialog(&err.message);
            return;
        }
    };

    let services = nova::services::Services::new(Arc::clone(&core));
    if let Err(err) = nova::ui::run(core, services) {
        nova::log_error!("UI exited with error: {err}");
    }
}

fn open_core(data_dir: &std::path::Path, password: Option<String>) -> nova::error::AppResult<Arc<Core>> {
    Core::open(data_dir, password.as_deref())
}

/// A password for the MCP server's vault, from `NOVA_VAULT_PASSWORD` if set.
fn mcp_password() -> Option<String> {
    std::env::var("NOVA_VAULT_PASSWORD").ok().filter(|p| !p.is_empty())
}
