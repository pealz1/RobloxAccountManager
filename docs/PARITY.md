# Parity checklist: Evanovar RAM (Python) → Nova RAM (Rust)

Every module of the Python code base (upstream `evanovar/RobloxAccountManager` at
`e8b5169`) and where its behaviour lives now. "Removed" entries were removed on
purpose; nothing else was dropped.

| Python module | Status | Rust location / note |
| :--- | :--- | :--- |
| `classes/account_manager.py` | Ported | `src/store/vault.rs` (load/modify/save under a cross-process lock, `.bak` recovery, encryption switching), `src/core/accounts.rs` |
| `classes/encryption.py` | Ported + replaced | New vaults use Windows DPAPI or Argon2id + AES-256-GCM (`src/store/crypto.rs`). The old hardware (WMI PBKDF2) and password formats are still read for migration (`src/import/evanovar.rs`). |
| `classes/roblox_api.py` | Ported | `src/roblox/*` (auth ticket, CSRF, presence, users, games, share links, launch URL, launchers incl. Voidstrap) |
| `classes/operation_result.py` | Ported | `src/error.rs` (`AppError` with code / title / message / detail / retryable) |
| `features/account_actions.py` | Ported | launch flows → `src/services/launcher.rs`; anti-AFK → `src/services/anti_afk.rs`; multi Roblox → `src/win/multi.rs`; kill all → `src/win/process.rs`; user:pass and JS login → `src/browser/login.rs` |
| `features/account_backup.py` | Ported | `src/import/backup.rs` (new format) and `src/import/evanovar.rs` (reads old backups) |
| `features/account_creator.py` | Ported as-is | `src/browser/creator.rs`. Not expanded: same limits (100 accounts, 5 browsers), the user still completes every CAPTCHA. |
| `features/account_filter.py` | Ported | `src/core/filter.rs` |
| `features/account_order.py` | Ported | `src/core/accounts.rs::move_account` |
| `features/auto_rejoin.py` | Ported | `src/services/auto_rejoin.rs` |
| `features/avatars.py` | Ported | `src/services/avatars.rs` |
| `features/browsers.py` | Replaced | `src/browser/discover.rs` finds Chrome, Edge, Brave, Vivaldi, Opera, Opera GX and portable Chromium and drives them over the DevTools protocol. No WebDriver or Selenium download is needed. Firefox is not supported (Firefox dropped CDP). |
| `features/chromium.py` | Ported | `src/browser/chromium.rs` (portable Chromium download) |
| `features/color_themes.py`, `features/themes.py` | Replaced | `src/ui/theme.rs` (light/dark/system, accent colour, density) |
| `features/cookie_validator.py` | Ported | `src/services/cookie_check.rs` |
| `features/diagnostics.py` | Ported | `src/logging.rs` (session + crash logs, secret redaction, retention) |
| `features/favorites.py` | Ported | `src/store/library.rs` (favourites) |
| `features/game_selector.py` | Ported | `src/roblox/games.rs` + game picker in the launch bar |
| `features/groups.py` | Ported | groups live in the vault (`src/core/accounts.rs`) |
| `features/handle64_trust.py` | Replaced | Handle mode closes the singleton handles in-process (`src/win/handles.rs`). No external `handle64.exe` is downloaded or run, so the signature check is no longer needed. |
| `features/headless_manager.py` | Ported | `src/services/instances.rs` (Instances page: hide/show/kill/rename per client) |
| `features/launch_monitor.py` | Ported | `src/services/launcher.rs::confirm_launch` |
| `features/presence.py` | Ported | `src/win/process.rs`, `src/roblox/logs.rs`, `src/services/activity.rs` |
| `features/private_servers.py` | Ported + extended | `src/roblox/private_servers.rs`, Private Servers page |
| `features/roblox_downloader.py` | Ported | `src/services/downloader.rs` |
| `features/roblox_settings.py` | Ported | `src/services/roblox_settings.rs` (basic presets, advanced editor, auto-apply) |
| `features/settings_store.py` | Ported | `src/store/settings.rs` |
| `features/updater.py` | Rewritten | `src/services/updater.rs` (fork releases only, SHA-256 verified, channel, rollback) |
| `features/websocket_server.py` | Replaced | Local REST API (`src/api/http.rs`) + MCP server (`src/api/mcp.rs`). Same protections: loopback only, token auth, browser origins refused, auth-failure lockout. |
| `features/window_grid.py` | Ported + fixed | `src/win/grid.rs`, includes the fix for upstream issue #79 / PR #82 |
| `features/window_log_probe.py`, `features/window_renamer.py` | Ported | `src/services/renamer.rs` |
| `features/window_operations.py` | Ported | `src/win/window.rs` |
| `features/windows_startup.py` | Ported | `src/win/startup.rs` |
| `utils/app_paths.py` | Ported | `src/paths.rs` (`--data-dir`, `NOVA_DATA_DIR`) |
| `utils/atomic_io.py` | Ported | `src/store/atomic.rs` |
| `utils/splash.py`, `utils/motion.py`, `utils/icons.py` | Replaced | egui renders instantly; no splash needed. Icons from Phosphor. |
| `utils/ui.py` Console tab | **Removed (requested)** | Logs still go to `NovaData/logs`; Settings → Diagnostics opens the folder. |
| `utils/ui.py` Donations tab | **Removed (requested)** | |
| `utils/ui.py` everything else | Rewritten | `src/ui/*` |
| Discord webhooks | Not ported | Upstream already removed them (`main.py` deletes the old setting). |
| Background image/video theme | Not ported | Replaced by the new theme system. |
| Detachable pages | Not ported | Pages are reachable from the sidebar and the command palette instead. |

## Upstream issues and pull requests

| Upstream | Where it is handled |
| :--- | :--- |
| Issue #79, PR #82: Window Grid clips windows behind the taskbar | `src/win/grid.rs`: uses the monitor work area, measures window borders, keeps 16:9 client areas, drops a column when Roblox enforces a minimum width, stacks rows only when needed |
| Issue #80, PR #81: unreliable Windows builds | Single static Rust executable (`+crt-static`), no bundled DLLs, CI smoke-tests `--version` |
