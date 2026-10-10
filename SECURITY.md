# Security Policy

Nova RAM handles sensitive data: Roblox session cookies, optionally saved
passwords, and notes. Please report vulnerabilities privately so they can be
fixed before details are public.

## How Nova RAM protects local data

- Accounts live in an **encrypted vault** (`vault.json`): Windows DPAPI tied to
  your Windows account, or Argon2id + AES-256-GCM behind a password. "No
  encryption" is available but must be chosen explicitly.
- The vault is written **atomically** with a `.bak` copy and a cross-process lock,
  so a crash or a second instance can't corrupt or lose it.
- Logs are **redacted**: cookies, passwords, tokens and private-server codes are
  stripped before anything is written to disk.
- The **local API** binds `127.0.0.1` only, requires a bearer token, refuses any
  request with a browser `Origin`, and locks out after repeated bad tokens.
- The **MCP server** and API never return cookies or passwords unless you turn on
  *Expose secrets*, and destructive actions require an explicit confirmation.
- The **updater** downloads only from this repository's GitHub releases and
  verifies a published SHA-256 before replacing the executable, keeping a rollback
  copy.

## Supported versions

Security fixes land in the latest release. Older releases are not patched
separately.

## Reporting a vulnerability

Use GitHub's **Report a vulnerability** option on the repository's
[Security page](https://github.com/pealz1/RobloxAccountManager/security/advisories/new).
Please include steps to reproduce and the impact. We aim to acknowledge reports
quickly and will credit reporters who want it once a fix ships.
