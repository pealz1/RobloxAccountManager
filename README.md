[![CI](https://img.shields.io/github/actions/workflow/status/pealz1/RobloxAccountManager/ci.yml?label=ci)](https://github.com/pealz1/RobloxAccountManager/actions)
[![Latest release](https://img.shields.io/github/v/release/pealz1/RobloxAccountManager?label=release)](https://github.com/pealz1/RobloxAccountManager/releases/latest)
[![License](https://img.shields.io/github/license/pealz1/RobloxAccountManager)](LICENSE)
![OS](https://img.shields.io/badge/os-windows-0078D4)
![Built with Rust](https://img.shields.io/badge/built%20with-rust-ce412b)

# Nova RAM

**Nova RAM** is a fast, native Windows app for managing many Roblox accounts:
organize them, launch them into games and private servers, run multiple clients
at once, and automate the tedious parts. It is a full **Rust** rewrite — one small
static executable, no runtime to install, instant startup.

It is a fork of [Evanovar RAM](https://github.com/evanovar/RobloxAccountManager)
(a Python app), which was itself inspired by
[ic3w0lf's original Roblox Account Manager](https://github.com/ic3w0lf22/Roblox-Account-Manager).
Nova RAM can import directly from both.

> [!IMPORTANT]
> - **Open source.** Every line is here to read; if you don't trust the `.exe`, build it yourself.
> - **Reproducible.** Release binaries are built from the tagged source by GitHub Actions and ship with a published SHA-256.
> - **Unsigned.** Windows or antivirus may warn about a new unsigned binary. That is expected for small open-source releases.

## Highlights

- **Accounts** — groups, notes, aliases, stars, avatars, search, and multi-select
  actions. Status and live memory use show next to each account.
- **Launching** — join a Place, a private server (VIP/share link or code), a
  specific Job ID, the emptiest public server, another user's game, or the home
  page. A built-in game search resolves Place IDs by name.
- **Servers** *(new)* — a searchable history of every server each account has
  joined, read from the Roblox logs, with one-click rejoin.
- **Private Servers** — list the servers an account owns, refresh join links, save
  the ones you use, and launch straight into them.
- **Multi Roblox** — run many clients with the mutex method (no admin) or by
  closing singleton handles in-process (admin), plus the Error-773 cookie-lock fix.
  No external `handle64.exe` download.
- **Window tools** — tile all clients into a clean grid (fixed so windows never
  hide behind the taskbar), rename windows per account, hide/show/close clients.
- **Auto-Rejoin & Anti-AFK** — keep accounts in a game and nudge them so they
  aren't kicked for inactivity.
- **Import** — cookies, `user:pass`, CSV, JSON, Evanovar `saved_accounts.json`
  (incl. password/hardware), ic3w0lf `AccountData.json` (plain/DPAPI/password), and
  portable backups.
- **Automation** — a loopback HTTP API and an **MCP server** so tools like Claude
  and Codex can list, add, launch and manage accounts. See [docs/API.md](docs/API.md).
- **Security** — accounts are stored in an encrypted vault (Windows account key or
  a password), written atomically with a backup copy.

## Installation

1. Open the [latest release](https://github.com/pealz1/RobloxAccountManager/releases/latest).
2. Download `NovaRAM-v<version>.exe` (and, if you like, verify it against the
   published `.sha256`).
3. Put it in a folder where it may keep its data, and run it.

## Run from source

Requirements: Windows 10/11 and a [Rust toolchain](https://rustup.rs).

```powershell
git clone https://github.com/pealz1/RobloxAccountManager.git
cd RobloxAccountManager
cargo run --release
```

The build produces a single `target/release/NovaRAM.exe`.

## Data & privacy

Nova RAM stores everything under `NovaData` next to the executable. Put it
elsewhere with `--data-dir "D:/Profiles/Alt"` or the `NOVA_DATA_DIR` environment
variable (handy for separate profiles).

Network use is limited to what you ask for: Roblox APIs for account, game,
presence, launch and private-server features; GitHub for update checks; and the
connectivity check used by Auto-Rejoin. Account cookies never leave your machine
unless you explicitly enable the API and turn on *Expose secrets*.

## Command-line

| Flag | Effect |
| :--- | :--- |
| `--data-dir <path>` | Use a specific data folder |
| `--mcp` | Run the MCP server over stdio (for AI tools) |
| `--version` | Print the version and exit |

## Contributing

Issues and pull requests are welcome. Before opening a PR, run:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all
```

[docs/PARITY.md](docs/PARITY.md) maps every module of the original Python app to
its place here.

## Relationship to upstream

Because Nova RAM is a full-language rewrite, upstream changes can't be merged as
diffs. A scheduled job watches `evanovar/RobloxAccountManager` and opens an issue
listing new upstream commits worth porting, so nothing is silently missed.

## Disclaimer

Provided for account-management and educational purposes. You are responsible for
following Roblox's Terms of Use. The maintainers are not responsible for account
actions, moderation, or data loss.

## License

Nova RAM is licensed under the [GNU General Public License v3.0](LICENSE), the same
license as the projects it builds on.
