## Nova RAM: full Rust rewrite

This replaces the Python app with **Nova RAM**, a native Windows app written
entirely in Rust — one small static executable (~15 MB), no Python runtime, instant
startup. Feature parity with the Python app is tracked file-by-file in
[docs/PARITY.md](docs/PARITY.md); nothing was dropped except the Console and
Donations tabs (removed on request).

### What's new beyond parity

- **Servers page** — searchable history of every server each account joined (read
  from the Roblox logs) with one-click rejoin.
- **Private Servers page** — list owned servers, refresh join links, save and launch.
- **Local HTTP API + MCP server** — manage accounts from scripts or AI tools
  (Claude, Codex): list, add, import, launch, group, and more. Loopback-only,
  token-auth, secrets hidden by default, destructive actions gated by `confirm`.
  Docs: [docs/API.md](docs/API.md).
- **Quick sign-in** — add accounts with a cross-device login code (no password typed).
- **Broader import** — cookies, `user:pass`, CSV, JSON, Evanovar `saved_accounts.json`
  (password + hardware), ic3w0lf `AccountData.json` (plain/DPAPI/password), and backups.
  Decryption tested against vectors produced by the original Python crypto.
- **In-process Multi Roblox handle mode** — closes the singleton handles directly,
  so no external `handle64.exe` is downloaded or run.
- **Encrypted vault** — Windows DPAPI or Argon2id + AES-256-GCM, atomic writes with
  a `.bak` copy under a cross-process lock.
- **Redesigned UI** — a cleaner, more readable egui interface with light/dark themes.
- **Reworked updater** — fork releases only, SHA-256 verified, with a rollback copy.

### Upstream issues & PRs addressed

- **Issue #79 / PR #82** (Window Grid clips windows behind the taskbar): the Rust
  grid uses the monitor work area, measures window borders, keeps 16:9 client areas,
  drops a column when Roblox enforces a minimum width, and only stacks rows when
  needed. Covered by tests in `src/win/grid.rs`.
- **Issue #80 / PR #81** (unreliable Windows builds): the release is a single
  statically-linked exe (`+crt-static`) with no bundled DLLs, built from `Cargo.lock`
  and smoke-tested (`--version`) in CI before release.

### Quality

- 78 unit/integration tests; CI runs `cargo fmt --check`, `cargo clippy -D warnings`
  and `cargo test`.
- A scheduled workflow watches upstream and opens an issue listing new commits to
  port (since a rewrite can't merge upstream diffs).

GPL-3.0 is unchanged; the README and About screen credit Evanovar RAM and ic3w0lf's
original Roblox Account Manager.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
