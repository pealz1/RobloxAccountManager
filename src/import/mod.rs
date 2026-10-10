//! Account import from files, pasted text and other account managers.
//!
//! Sources: plain text / `.txt` (one account per line), CSV with a header row,
//! JSON arrays or objects, Evanovar RAM `saved_accounts.json` and its backups,
//! ic3w0lf Roblox Account Manager `AccountData.json`, and Nova backups.
//!
//! Only files and text the user hands us are read. Browser profiles are never touched.

pub mod backup;
pub mod evanovar;
pub mod ic3w0lf;

use crate::store::model::{Account, Favorite, RecentGame};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub const COOKIE_PREFIX: &str =
    "_|WARNING:-DO-NOT-SHARE-THIS.--Sharing-this-will-allow-someone-to-log-in-as-you-and-to-steal-your-ROBUX-and-items.|";

static COOKIE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"_\|WARNING:-DO-NOT-SHARE-THIS\.--Sharing-this-will-allow-someone-to-log-in-as-you-and-to-steal-your-ROBUX-and-items\.\|[A-Za-z0-9_+/=.\-]+")
        .expect("cookie regex")
});

/// One account read from a source. Missing fields are filled in from Roblox later.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportedAccount {
    pub cookie: String,
    pub username: String,
    pub user_id: u64,
    pub password: String,
    pub note: String,
    pub alias: String,
    pub group: String,
    pub fields: BTreeMap<String, String>,
    pub added_at: Option<DateTime<Utc>>,
    pub last_used: Option<DateTime<Utc>>,
}

impl ImportedAccount {
    pub fn has_cookie(&self) -> bool {
        !self.cookie.is_empty()
    }

    pub fn into_account(self) -> Account {
        Account {
            user_id: self.user_id,
            username: self.username,
            cookie: self.cookie,
            password: self.password,
            note: self.note,
            alias: self.alias,
            group: self.group,
            fields: self.fields,
            added_at: self.added_at.or_else(|| Some(Utc::now())),
            last_used: self.last_used,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImportBatch {
    pub source: String,
    pub accounts: Vec<ImportedAccount>,
    pub groups: Vec<String>,
    pub favorites: Vec<Favorite>,
    pub recent_games: Vec<RecentGame>,
    pub skipped: usize,
}

impl ImportBatch {
    pub fn with_cookies(&self) -> usize {
        self.accounts.iter().filter(|a| a.has_cookie()).count()
    }

    /// Accounts that arrived with only a username and password; they need a browser sign-in.
    pub fn needs_sign_in(&self) -> Vec<&ImportedAccount> {
        self.accounts.iter().filter(|a| !a.has_cookie() && !a.password.is_empty()).collect()
    }
}

/// Finds Roblox session tokens inside a blob of text.
pub fn find_cookies(text: &str) -> Vec<String> {
    let mut seen = Vec::new();
    for m in COOKIE_RE.find_iter(text) {
        let cookie = m.as_str().to_owned();
        if !seen.contains(&cookie) {
            seen.push(cookie);
        }
    }
    seen
}

pub fn looks_like_cookie(text: &str) -> bool {
    let t = text.trim();
    COOKIE_RE.find(t).is_some_and(|m| m.start() == 0 && m.end() == t.len())
}

/// Parses pasted text or a text / CSV / JSON file.
pub fn parse_text(text: &str) -> ImportBatch {
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            return parse_json_value(&value);
        }
    }
    if let Some(first) = trimmed.lines().find(|l| !l.trim().is_empty()) {
        if let Some(batch) = parse_csv(first, trimmed) {
            return batch;
        }
    }
    parse_lines(trimmed)
}

/// One record per line. Cookies are detected anywhere on the line; otherwise the
/// line is read as colon- or comma-separated `username`, `password` fields.
fn parse_lines(text: &str) -> ImportBatch {
    let mut batch = ImportBatch { source: "text".into(), ..Default::default() };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cookies = find_cookies(line);
        if let Some(cookie) = cookies.first() {
            let mut account = ImportedAccount { cookie: cookie.clone(), ..Default::default() };
            // Fields before the cookie, if any, are treated as username[:password].
            if let Some(prefix) = line.split(cookie.as_str()).next() {
                let parts: Vec<&str> = prefix.split([':', ',']).map(str::trim).filter(|p| !p.is_empty()).collect();
                if let Some(user) = parts.first() {
                    account.username = (*user).to_owned();
                }
                if let Some(pass) = parts.get(1) {
                    account.password = (*pass).to_owned();
                }
            }
            batch.accounts.push(account);
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, [':', ',', '\t']).map(str::trim).collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            batch.accounts.push(ImportedAccount {
                username: parts[0].to_owned(),
                password: parts[1].to_owned(),
                ..Default::default()
            });
        } else {
            batch.skipped += 1;
        }
    }
    batch
}

fn parse_csv(header_line: &str, text: &str) -> Option<ImportBatch> {
    let header: Vec<String> = header_line.split(',').map(|c| c.trim().to_lowercase()).collect();
    let known = ["username", "user", "name", "password", "pass", "cookie", "token", "note", "group", "alias", "user_id", "userid", "id"];
    let matches = header.iter().filter(|h| known.contains(&h.as_str())).count();
    // Require a header that is mostly recognised columns, and at least a cookie or user column.
    if matches < 2 || !header.iter().any(|h| ["cookie", "token", "username", "user", "name"].contains(&h.as_str())) {
        return None;
    }
    let mut batch = ImportBatch { source: "csv".into(), ..Default::default() };
    for line in text.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        let get = |names: &[&str]| -> String {
            names
                .iter()
                .find_map(|n| header.iter().position(|h| h == n).and_then(|i| cells.get(i)))
                .map(|s| s.trim().to_owned())
                .unwrap_or_default()
        };
        let account = ImportedAccount {
            cookie: {
                let raw = get(&["cookie", "token"]);
                find_cookies(&raw).into_iter().next().unwrap_or(raw)
            },
            username: get(&["username", "user", "name"]),
            password: get(&["password", "pass"]),
            note: get(&["note"]),
            group: get(&["group"]),
            alias: get(&["alias"]),
            user_id: get(&["user_id", "userid", "id"]).parse().unwrap_or(0),
            ..Default::default()
        };
        if account.has_cookie() || (!account.username.is_empty() && !account.password.is_empty()) {
            batch.accounts.push(account);
        } else {
            batch.skipped += 1;
        }
    }
    Some(batch)
}

/// Reads a JSON array of account objects, or an object wrapping such an array.
pub fn parse_json_value(value: &Value) -> ImportBatch {
    let mut batch = ImportBatch { source: "json".into(), ..Default::default() };
    let array = match value {
        Value::Array(items) => Some(items.clone()),
        Value::Object(map) => map
            .get("accounts")
            .and_then(Value::as_array)
            .cloned()
            .or_else(|| Some(map.values().cloned().collect())),
        _ => None,
    };
    let Some(items) = array else {
        batch.skipped += 1;
        return batch;
    };
    for item in &items {
        let Some(obj) = item.as_object() else {
            batch.skipped += 1;
            continue;
        };
        let string = |keys: &[&str]| -> String {
            keys.iter()
                .find_map(|k| obj.get(*k).and_then(Value::as_str))
                .map(str::to_owned)
                .unwrap_or_default()
        };
        let cookie_raw = string(&["cookie", "securityToken", "SecurityToken", ".ROBLOSECURITY", "roblosecurity", "token"]);
        let account = ImportedAccount {
            cookie: find_cookies(&cookie_raw).into_iter().next().unwrap_or(cookie_raw),
            username: string(&["username", "Username", "name", "Name"]),
            password: string(&["password", "Password"]),
            note: string(&["note", "Description", "description"]),
            alias: string(&["alias", "Alias"]),
            group: string(&["group", "Group"]),
            user_id: obj.get("user_id").or_else(|| obj.get("UserID")).or_else(|| obj.get("userId")).and_then(as_u64).unwrap_or(0),
            ..Default::default()
        };
        if account.has_cookie() || (!account.username.is_empty() && !account.password.is_empty()) {
            batch.accounts.push(account);
        } else {
            batch.skipped += 1;
        }
    }
    batch
}

pub(crate) fn as_u64(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const COOKIE: &str = "_|WARNING:-DO-NOT-SHARE-THIS.--Sharing-this-will-allow-someone-to-log-in-as-you-and-to-steal-your-ROBUX-and-items.|ABC123def";

    #[test]
    fn plain_cookie_lines() {
        let batch = parse_text(&format!("{COOKIE}\n\n# comment\n{COOKIE}2"));
        assert_eq!(batch.accounts.len(), 2);
        assert!(batch.accounts[0].has_cookie());
    }

    #[test]
    fn user_pass_lines() {
        let batch = parse_text("alice:secret1\nbob,secret2\nbad-line");
        assert_eq!(batch.accounts.len(), 2);
        assert_eq!(batch.skipped, 1);
        assert_eq!(batch.needs_sign_in().len(), 2);
    }

    #[test]
    fn user_pass_cookie_line() {
        let batch = parse_text(&format!("alice:secret:{COOKIE}"));
        let a = &batch.accounts[0];
        assert_eq!(a.username, "alice");
        assert_eq!(a.password, "secret");
        assert!(a.has_cookie());
    }

    #[test]
    fn csv_with_header() {
        let batch = parse_text(&format!("username,password,cookie,group\nalice,pw,{COOKIE},Farm"));
        assert_eq!(batch.source, "csv");
        assert_eq!(batch.accounts[0].group, "Farm");
        assert!(batch.accounts[0].has_cookie());
    }

    #[test]
    fn json_array() {
        let json = format!(r#"[{{"username":"bob","cookie":"{COOKIE}","user_id":42}}]"#);
        let batch = parse_text(&json);
        assert_eq!(batch.source, "json");
        assert_eq!(batch.accounts[0].user_id, 42);
    }
}
