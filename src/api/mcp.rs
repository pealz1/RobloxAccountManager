//! Model Context Protocol server over stdio, so Claude, Codex and other MCP
//! clients can manage accounts. Line-delimited JSON-RPC 2.0.
//!
//! Launch with `NovaRAM --mcp`. It opens the same vault as the GUI; the
//! cross-process lock keeps writes consistent between them.

use super::commands;
use crate::core::Core;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::Arc;

const PROTOCOL_VERSION: &str = "2024-11-05";

/// Runs the server loop until stdin closes.
pub fn serve_stdio(core: Arc<Core>) {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let expose = core.settings().api_expose_secrets;
    crate::log_info!("MCP server started on stdio");
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle(&core, &request, expose),
            Err(err) => Some(error_response(Value::Null, -32700, &format!("Parse error: {err}"))),
        };
        if let Some(response) = response {
            let mut out = stdout.lock();
            let _ = writeln!(out, "{response}");
            let _ = out.flush();
        }
    }
}

/// Handles one JSON-RPC request. Returns None for notifications (no id).
fn handle(core: &Core, request: &Value, expose: bool) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(json!({}));

    // Notifications carry no id and get no response.
    if id.is_none() {
        return None;
    }
    let id = id.unwrap();

    match method {
        "initialize" => Some(result_response(id, json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "nova-ram", "version": crate::VERSION },
            "instructions": "Manage Roblox accounts in Nova RAM: list, add, import, launch, group and more. Cookies are never returned unless the server is configured to expose secrets. Destructive tools need confirm:true.",
        }))),
        "ping" => Some(result_response(id, json!({}))),
        "tools/list" => Some(result_response(id, json!({
            "tools": commands::tools().into_iter().map(|t| json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.schema,
            })).collect::<Vec<_>>(),
        }))),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match commands::dispatch(core, name, &args, expose) {
                Ok(value) => Some(result_response(id, tool_result(&value, false))),
                Err(err) => Some(result_response(id, tool_result(&json!({"error": err.message, "code": err.code}), true))),
            }
        }
        other => Some(error_response(id, -32601, &format!("Unknown method: {other}"))),
    }
}

/// MCP tool results wrap content; `is_error` flags a tool-level failure (not transport).
fn tool_result(value: &Value, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(value).unwrap_or_default() }],
        "isError": is_error,
    })
}

fn result_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_core;

    #[test]
    fn initialize_reports_protocol_and_tools() {
        let (_dir, core) = test_core();
        let response = handle(&core, &json!({"jsonrpc":"2.0","id":1,"method":"initialize"}), false).unwrap();
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
        let list = handle(&core, &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}), false).unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().len() >= 10);
    }

    #[test]
    fn notifications_get_no_response() {
        let (_dir, core) = test_core();
        assert!(handle(&core, &json!({"jsonrpc":"2.0","method":"notifications/initialized"}), false).is_none());
    }

    #[test]
    fn tool_call_dispatches_and_flags_errors() {
        let (_dir, core) = test_core();
        let ok = handle(&core, &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"status","arguments":{}}}), false).unwrap();
        assert_eq!(ok["result"]["isError"], false);
        let err = handle(&core, &json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"delete_account","arguments":{"account":"x"}}}), false).unwrap();
        assert_eq!(err["result"]["isError"], true);
    }
}
