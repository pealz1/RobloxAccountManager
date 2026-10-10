# Nova RAM automation: local API & MCP

Nova RAM exposes the same set of actions two ways:

- a **local HTTP API** on `127.0.0.1` for scripts, and
- an **MCP server** over stdio so AI tools (Claude, Codex, and other MCP clients)
  can manage accounts directly.

Both share one command set and the same safety rules:

- **Loopback only.** The HTTP API binds `127.0.0.1`; it never listens on the network.
- **Token auth.** Every HTTP request needs `Authorization: Bearer <token>`.
- **No browser calls.** Any request carrying an `Origin` header is refused, and
  repeated bad tokens trigger a short lockout.
- **Secrets stay hidden.** Cookies and passwords are never returned unless you turn
  on *Settings → Local API → Expose secrets*.
- **Destructive actions confirm.** `delete_account` and `kill_all_roblox` require
  `"confirm": true`.

---

## Local HTTP API

Enable it under **Settings → Local API & AI (MCP)** and copy the token shown there
(regenerate it any time). It listens on the configured port (default `7963`).

### Endpoints

| Method & path | Purpose |
| :--- | :--- |
| `GET /` or `GET /tools` | Server info and the full tool list with JSON schemas |
| `GET /accounts` | Shorthand for `list_accounts` |
| `GET /status` | App status |
| `POST /tools/<name>` | Run a tool; the JSON body is its arguments |

### Example

```bash
TOKEN="paste-from-settings"
# List accounts
curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:7963/accounts

# Launch two accounts into a place
curl -s -X POST http://127.0.0.1:7963/tools/launch \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"accounts":["Builderman","1234567"],"place_id":"606849621"}'

# Delete an account (needs confirm)
curl -s -X POST http://127.0.0.1:7963/tools/delete_account \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"account":"OldAlt","confirm":true}'
```

Errors return `{ "error": "...", "code": "..." }` with a matching HTTP status
(`400` bad request, `401` unauthorized, `404` not found, `429` rate limited/locked).

---

## MCP server (Claude, Codex, …)

Run Nova RAM as an MCP server over stdio:

```
NovaRAM.exe --mcp
```

It opens the same vault as the app (a cross-process lock keeps writes consistent
if both run at once). If the vault is password-protected, set `NOVA_VAULT_PASSWORD`
in the server's environment.

### Claude Desktop / Claude Code

Add to your MCP config (`claude_desktop_config.json`, or `.mcp.json` for Claude Code):

```json
{
  "mcpServers": {
    "nova-ram": {
      "command": "C:\\\\Path\\\\To\\\\NovaRAM.exe",
      "args": ["--mcp"]
    }
  }
}
```

### Codex

```toml
[mcp_servers.nova-ram]
command = "C:\\Path\\To\\NovaRAM.exe"
args = ["--mcp"]
```

The client will call `initialize`, then `tools/list`, and can then `tools/call`
any tool below. Results come back as pretty-printed JSON; tool-level failures set
`isError: true`.

---

## Tools

Every tool takes a JSON object. `account` accepts a username or a Roblox user id.

| Tool | Arguments | Notes |
| :--- | :--- | :--- |
| `status` | — | Version, account count, running clients, multi-instance state |
| `list_accounts` | `group?`, `search?` | Cookies omitted unless secrets are exposed |
| `get_account` | `account` | One account |
| `add_account` | `cookie` | Resolves username and id from Roblox |
| `import_accounts` | `text` | Cookies / `user:pass` / CSV / JSON; cookie accounts are added |
| `delete_account` | `account`, `confirm` | **Destructive** |
| `launch` | `accounts[]`, `place_id?`, `private_server?`, `job_id?` | Omit all targets for the home page |
| `join_user` | `accounts[]`, `target` | Joins the target's current game |
| `join_small_server` | `accounts[]`, `place_id` | Emptiest public server |
| `set_note` | `account`, `note` | |
| `set_group` | `account`, `group` | Empty string clears the group |
| `list_groups` | — | |
| `list_servers` | `account?`, `limit?` | Server-join history from the logs |
| `list_private_servers` | `account`, `place_id?` | |
| `multi_roblox` | `action` = `enable`/`disable`/`status` | |
| `kill_all_roblox` | `confirm` | **Destructive** |

### Tips for agents

- Call `status` first to confirm the server is reachable and see how many accounts exist.
- Prefer `list_accounts` with `search`/`group` over fetching everything.
- Treat account cookies as secret: they are not returned by default, and you should
  not ask the user to expose them unless a task truly needs it.
- Destructive tools fail closed — send `confirm: true` only when the user has agreed.
