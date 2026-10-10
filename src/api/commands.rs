//! The shared command surface for the local HTTP API and the MCP server.
//!
//! One registry of tools, one dispatcher. Account secrets (cookies, passwords) are
//! stripped from every response unless the `api_expose_secrets` setting is on, and
//! destructive tools require an explicit `confirm: true`.

use crate::core::Core;
use crate::error::{AppError, AppResult};
use crate::roblox::launch::LaunchRequest;
use serde_json::{Value, json};

/// A tool the API/MCP exposes.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub destructive: bool,
    /// JSON Schema for the arguments object.
    pub schema: Value,
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "list_accounts",
            destructive: false,
            description: "List saved accounts (username, id, group, note, status, running/RAM). Cookies are never included unless the server is configured to expose secrets.",
            schema: json!({"type":"object","properties":{"group":{"type":"string","description":"Only accounts in this group"},"search":{"type":"string","description":"Filter by username, note, group or id"}}}),
        },
        Tool {
            name: "get_account",
            destructive: false,
            description: "Get one account by username or user id.",
            schema: json!({"type":"object","properties":{"account":{"type":"string"}},"required":["account"]}),
        },
        Tool {
            name: "add_account",
            destructive: false,
            description: "Add an account from a .ROBLOSECURITY cookie. Resolves its username and id from Roblox.",
            schema: json!({"type":"object","properties":{"cookie":{"type":"string"}},"required":["cookie"]}),
        },
        Tool {
            name: "import_accounts",
            destructive: false,
            description: "Import accounts from pasted text (cookies, user:pass lines, CSV or JSON). Only cookie accounts are added; user:pass lines are reported as needing a browser sign-in.",
            schema: json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
        },
        Tool {
            name: "delete_account",
            destructive: true,
            description: "Remove an account from Nova (does not affect the Roblox account). Requires confirm:true.",
            schema: json!({"type":"object","properties":{"account":{"type":"string"},"confirm":{"type":"boolean"}},"required":["account","confirm"]}),
        },
        Tool {
            name: "launch",
            destructive: false,
            description: "Launch one or more accounts into a game. Provide place_id and/or private_server and/or job_id. Omit all for the Roblox home page.",
            schema: json!({"type":"object","properties":{"accounts":{"type":"array","items":{"type":"string"}},"place_id":{"type":"string"},"private_server":{"type":"string"},"job_id":{"type":"string"}},"required":["accounts"]}),
        },
        Tool {
            name: "join_user",
            destructive: false,
            description: "Launch accounts into the game a target user is currently in.",
            schema: json!({"type":"object","properties":{"accounts":{"type":"array","items":{"type":"string"}},"target":{"type":"string"}},"required":["accounts","target"]}),
        },
        Tool {
            name: "join_small_server",
            destructive: false,
            description: "Launch accounts into the emptiest public server of a place.",
            schema: json!({"type":"object","properties":{"accounts":{"type":"array","items":{"type":"string"}},"place_id":{"type":"string"}},"required":["accounts","place_id"]}),
        },
        Tool {
            name: "set_note",
            destructive: false,
            description: "Set an account's note.",
            schema: json!({"type":"object","properties":{"account":{"type":"string"},"note":{"type":"string"}},"required":["account","note"]}),
        },
        Tool {
            name: "set_group",
            destructive: false,
            description: "Move an account to a group (empty string clears it).",
            schema: json!({"type":"object","properties":{"account":{"type":"string"},"group":{"type":"string"}},"required":["account","group"]}),
        },
        Tool {
            name: "list_groups",
            destructive: false,
            description: "List group names.",
            schema: json!({"type":"object","properties":{}}),
        },
        Tool {
            name: "list_servers",
            destructive: false,
            description: "Recent servers each account has joined (from the Roblox logs).",
            schema: json!({"type":"object","properties":{"account":{"type":"string","description":"Only this account"},"limit":{"type":"integer"}}}),
        },
        Tool {
            name: "list_private_servers",
            destructive: false,
            description: "List the private servers owned by an account.",
            schema: json!({"type":"object","properties":{"account":{"type":"string"},"place_id":{"type":"string"}},"required":["account"]}),
        },
        Tool {
            name: "multi_roblox",
            destructive: false,
            description: "Control multi-instance support. action is 'enable', 'disable' or 'status'.",
            schema: json!({"type":"object","properties":{"action":{"type":"string","enum":["enable","disable","status"]}},"required":["action"]}),
        },
        Tool {
            name: "kill_all_roblox",
            destructive: true,
            description: "Close every running Roblox client. Requires confirm:true.",
            schema: json!({"type":"object","properties":{"confirm":{"type":"boolean"}},"required":["confirm"]}),
        },
        Tool {
            name: "status",
            destructive: false,
            description: "App status: version, account count, running clients, multi-instance and update state.",
            schema: json!({"type":"object","properties":{}}),
        },
    ]
}

fn account_json(account: &crate::store::model::Account, core: &Core, expose: bool) -> Value {
    let activity = json!({});
    let mut value = serde_json::to_value(if expose { account.clone() } else { account.redacted() }).unwrap_or(json!({}));
    if let Value::Object(map) = &mut value {
        map.insert("key".into(), json!(account.key()));
        map.insert("label".into(), json!(account.label()));
        let _ = core;
        let _ = activity;
    }
    value
}

fn str_arg<'a>(args: &'a Value, key: &str) -> AppResult<&'a str> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| AppError::invalid("ARG_MISSING", format!("Missing '{key}'.")))
}

fn str_list(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::Array(items)) => items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn require_confirm(args: &Value) -> AppResult<()> {
    if args.get("confirm").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(AppError::invalid("CONFIRM_REQUIRED", "This action changes or removes data. Call again with confirm: true."))
    }
}

/// Runs a tool by name. `expose_secrets` comes from settings and gates cookie output.
pub fn dispatch(core: &Core, name: &str, args: &Value, expose_secrets: bool) -> AppResult<Value> {
    match name {
        "status" => Ok(json!({
            "app": crate::APP_NAME,
            "version": crate::VERSION,
            "accounts": core.accounts().len(),
            "groups": core.groups(),
            "running_clients": crate::win::process::list_roblox().len(),
            "multi_roblox": core.multi_roblox_running(),
        })),
        "list_accounts" => {
            let group = args.get("group").and_then(Value::as_str);
            let terms: Vec<String> =
                args.get("search").and_then(Value::as_str).unwrap_or("").to_lowercase().split_whitespace().map(str::to_owned).collect();
            let accounts: Vec<Value> = core
                .accounts()
                .into_iter()
                .filter(|a| group.is_none_or(|g| a.group == g))
                .filter(|a| {
                    let hay = format!("{} {} {} {}", a.username, a.note, a.group, a.user_id).to_lowercase();
                    terms.iter().all(|t| hay.contains(t))
                })
                .map(|a| account_json(&a, core, expose_secrets))
                .collect();
            Ok(json!({ "accounts": accounts }))
        }
        "get_account" => {
            let account = core.account(str_arg(args, "account")?).ok_or_else(|| AppError::not_found("Account", ""))?;
            Ok(account_json(&account, core, expose_secrets))
        }
        "add_account" => {
            let account = core.add_cookie(str_arg(args, "cookie")?)?;
            Ok(json!({ "added": account.username, "user_id": account.user_id }))
        }
        "import_accounts" => {
            let batch = crate::import::parse_text(str_arg(args, "text")?);
            let mut resolved = batch.clone();
            for a in &mut resolved.accounts {
                if a.has_cookie()
                    && a.user_id == 0
                    && let Ok(id) = crate::roblox::account::whoami(&a.cookie)
                {
                    a.user_id = id.user_id;
                    if a.username.is_empty() {
                        a.username = id.username;
                    }
                }
            }
            let outcome = core.apply_import(&resolved)?;
            Ok(
                json!({ "added": outcome.added, "updated": outcome.updated, "skipped": outcome.skipped, "need_sign_in": outcome.need_sign_in }),
            )
        }
        "delete_account" => {
            require_confirm(args)?;
            let removed = core.delete_account(str_arg(args, "account")?)?;
            Ok(json!({ "removed": removed }))
        }
        "launch" => {
            let accounts = str_list(args, "accounts");
            let request = LaunchRequest {
                place_id: args.get("place_id").and_then(Value::as_str).unwrap_or("").to_owned(),
                private_server: args.get("private_server").and_then(Value::as_str).unwrap_or("").to_owned(),
                job_id: args.get("job_id").and_then(Value::as_str).unwrap_or("").to_owned(),
            };
            let result = core.launch_batch(&accounts, &request);
            Ok(batch_json(&result))
        }
        "join_user" => {
            let result = core.join_user(&str_list(args, "accounts"), str_arg(args, "target")?)?;
            Ok(batch_json(&result))
        }
        "join_small_server" => {
            let place =
                str_arg(args, "place_id")?.parse().map_err(|_| AppError::invalid("PLACE_ID_INVALID", "place_id must be numeric."))?;
            let result = core.join_small_server(&str_list(args, "accounts"), place)?;
            Ok(batch_json(&result))
        }
        "set_note" => {
            core.set_note(str_arg(args, "account")?, str_arg(args, "note")?)?;
            Ok(json!({ "ok": true }))
        }
        "set_group" => {
            core.set_group(str_arg(args, "account")?, str_arg(args, "group")?)?;
            Ok(json!({ "ok": true }))
        }
        "list_groups" => Ok(json!({ "groups": core.groups() })),
        "list_servers" => {
            let account = args.get("account").and_then(Value::as_str);
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;
            let mut visits = core.server_history();
            visits.reverse();
            let filtered: Vec<Value> = visits
                .into_iter()
                .filter(|v| account.is_none_or(|a| v.username.eq_ignore_ascii_case(a) || v.user_id.to_string() == a))
                .take(limit)
                .map(|v| serde_json::to_value(v).unwrap_or(json!({})))
                .collect();
            Ok(json!({ "servers": filtered }))
        }
        "list_private_servers" => {
            let account = core.account(str_arg(args, "account")?).ok_or_else(|| AppError::not_found("Account", ""))?;
            let place = args.get("place_id").and_then(Value::as_str).and_then(|s| s.parse().ok());
            let servers = crate::roblox::private_servers::list_servers(&account.cookie, account.user_id, place)?;
            Ok(json!({ "servers": serde_json::to_value(servers).unwrap_or(json!([])) }))
        }
        "multi_roblox" => match str_arg(args, "action")? {
            "enable" => {
                core.enable_multi_roblox()?;
                Ok(json!({ "running": true }))
            }
            "disable" => {
                core.disable_multi_roblox();
                Ok(json!({ "running": false }))
            }
            _ => Ok(json!({ "running": core.multi_roblox_running() })),
        },
        "kill_all_roblox" => {
            require_confirm(args)?;
            let (closed, remaining) = crate::win::process::kill_all();
            Ok(json!({ "closed": closed, "remaining": remaining }))
        }
        _ => Err(AppError::not_found("Tool", name)),
    }
}

fn batch_json(result: &crate::core::launcher::BatchResult) -> Value {
    json!({
        "launched": result.launched,
        "total": result.total,
        "ok": result.ok(),
        "failures": result.failures.iter().map(|(account, err)| json!({"account": account, "error": err.message})).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_core;

    #[test]
    fn destructive_tools_require_confirm() {
        let (_dir, core) = test_core();
        let err = dispatch(&core, "kill_all_roblox", &json!({}), false).unwrap_err();
        assert_eq!(err.code, "CONFIRM_REQUIRED");
    }

    #[test]
    fn list_accounts_never_leaks_cookie_without_expose() {
        let (_dir, core) = test_core();
        core.edit(|d| {
            d.upsert(crate::store::model::Account { user_id: 1, username: "a".into(), cookie: "SECRET".into(), ..Default::default() });
        })
        .unwrap();
        let out = dispatch(&core, "list_accounts", &json!({}), false).unwrap();
        assert!(!out.to_string().contains("SECRET"));
        let exposed = dispatch(&core, "list_accounts", &json!({}), true).unwrap();
        assert!(exposed.to_string().contains("SECRET"));
    }

    #[test]
    fn unknown_tool_is_not_found() {
        let (_dir, core) = test_core();
        assert_eq!(dispatch(&core, "nope", &json!({}), false).unwrap_err().code, "NOT_FOUND");
    }

    #[test]
    fn every_tool_has_object_schema() {
        for tool in tools() {
            assert_eq!(tool.schema["type"], "object", "{}", tool.name);
        }
    }
}
