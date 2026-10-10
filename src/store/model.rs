//! Everything the user owns that is stored encrypted in the vault.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CookieStatus {
    #[default]
    Unknown,
    Valid,
    Invalid,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Account {
    pub user_id: u64,
    pub username: String,
    pub display_name: String,
    /// Optional nickname shown instead of the username.
    pub alias: String,
    pub note: String,
    /// Group name, empty when ungrouped.
    pub group: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cookie: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub password: String,
    pub added_at: Option<DateTime<Utc>>,
    pub last_used: Option<DateTime<Utc>>,
    pub cookie_status: CookieStatus,
    pub cookie_checked_at: Option<DateTime<Utc>>,
    pub robux: Option<i64>,
    pub premium: Option<bool>,
    pub starred: bool,
    /// Free-form key/value pairs (imported from ic3w0lf "Fields").
    pub fields: BTreeMap<String, String>,
}

impl Account {
    /// Stable identity: the Roblox user id when known, otherwise the lowercase username.
    pub fn key(&self) -> String {
        if self.user_id > 0 { self.user_id.to_string() } else { self.username.to_lowercase() }
    }

    pub fn label(&self) -> &str {
        if self.alias.trim().is_empty() { &self.username } else { &self.alias }
    }

    /// Matches a user id, username (case-insensitive) or alias.
    pub fn matches_ref(&self, reference: &str) -> bool {
        let r = reference.trim();
        (self.user_id > 0 && r == self.user_id.to_string())
            || self.username.eq_ignore_ascii_case(r)
            || (!self.alias.is_empty() && self.alias.eq_ignore_ascii_case(r))
    }

    /// Copy without secrets, safe to hand to scripts and AI tools. Clears the cookie,
    /// the password, and the free-form `fields` (ic3w0lf stores PINs there).
    pub fn redacted(&self) -> Account {
        Account { cookie: String::new(), password: String::new(), fields: BTreeMap::new(), ..self.clone() }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Favorite {
    pub place_id: u64,
    pub name: String,
    pub private_server: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentGame {
    pub place_id: u64,
    pub name: String,
    pub private_server: String,
    pub at: Option<DateTime<Utc>>,
}

/// A private server link the user saved (their own or one shared with them).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedPrivateServer {
    pub id: String,
    pub name: String,
    pub place_id: u64,
    pub game_name: String,
    pub link: String,
    pub note: String,
    /// Roblox VIP server id when known (owner-side actions).
    pub vip_server_id: u64,
    /// Account key that owns the server, if it is one of ours.
    pub owner: String,
    pub added_at: Option<DateTime<Utc>>,
    pub last_joined: Option<DateTime<Utc>>,
}

/// One server an account joined, read from the Roblox client logs or our own launches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerVisit {
    pub at: Option<DateTime<Utc>>,
    pub user_id: u64,
    pub username: String,
    pub place_id: u64,
    pub universe_id: u64,
    pub job_id: String,
    pub server_ip: String,
    pub game_name: String,
    /// Set when the visit was a private server launched from Nova.
    pub private_server: String,
    pub left_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchTarget {
    pub place_id: u64,
    pub private_server: String,
    pub job_id: String,
}

/// A named, one-click launch: these accounts into this place.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchPreset {
    pub name: String,
    pub accounts: Vec<String>,
    pub target: LaunchTarget,
    pub tile_windows: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RejoinConfig {
    pub account: String,
    pub place_id: u64,
    pub private_server: String,
    pub job_id: String,
    pub check_interval_secs: u32,
    pub max_retries: u32,
    pub check_presence: bool,
    pub check_place_id: bool,
    pub check_internet: bool,
}

impl Default for RejoinConfig {
    fn default() -> Self {
        Self {
            account: String::new(),
            place_id: 0,
            private_server: String::new(),
            job_id: String::new(),
            check_interval_secs: 10,
            max_retries: 5,
            check_presence: true,
            check_place_id: true,
            check_internet: true,
        }
    }
}

pub const MAX_SERVER_HISTORY: usize = 1000;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultData {
    pub accounts: Vec<Account>,
    pub groups: Vec<String>,
    pub favorites: Vec<Favorite>,
    pub recent_games: Vec<RecentGame>,
    pub private_servers: Vec<SavedPrivateServer>,
    pub server_history: Vec<ServerVisit>,
    pub presets: Vec<LaunchPreset>,
    pub auto_rejoin: Vec<RejoinConfig>,
    /// Small secrets such as the local API token.
    pub secrets: BTreeMap<String, String>,
}

impl VaultData {
    pub fn find(&self, reference: &str) -> Option<&Account> {
        let r = reference.trim();
        self.accounts.iter().find(|a| a.key() == r).or_else(|| self.accounts.iter().find(|a| a.matches_ref(r)))
    }

    pub fn find_mut(&mut self, reference: &str) -> Option<&mut Account> {
        let index = self.index_of(reference)?;
        self.accounts.get_mut(index)
    }

    pub fn index_of(&self, reference: &str) -> Option<usize> {
        let r = reference.trim();
        self.accounts.iter().position(|a| a.key() == r).or_else(|| self.accounts.iter().position(|a| a.matches_ref(r)))
    }

    /// Adds or updates an account. Existing notes, groups, aliases and passwords are kept
    /// when the incoming record leaves them empty. Returns true when the account was new.
    pub fn upsert(&mut self, incoming: Account) -> bool {
        let existing = self.accounts.iter().position(|a| {
            (incoming.user_id > 0 && a.user_id == incoming.user_id)
                || (!incoming.username.is_empty() && a.username.eq_ignore_ascii_case(&incoming.username))
        });
        match existing {
            Some(index) => {
                let current = &self.accounts[index];
                let merged = Account {
                    note: pick(&incoming.note, &current.note),
                    group: pick(&incoming.group, &current.group),
                    alias: pick(&incoming.alias, &current.alias),
                    password: pick(&incoming.password, &current.password),
                    cookie: pick(&incoming.cookie, &current.cookie),
                    display_name: pick(&incoming.display_name, &current.display_name),
                    user_id: if incoming.user_id > 0 { incoming.user_id } else { current.user_id },
                    added_at: current.added_at.or(incoming.added_at),
                    last_used: incoming.last_used.or(current.last_used),
                    starred: current.starred || incoming.starred,
                    fields: {
                        let mut f = current.fields.clone();
                        f.extend(incoming.fields.clone());
                        f
                    },
                    ..incoming
                };
                self.ensure_group(&merged.group);
                self.accounts[index] = merged;
                false
            }
            None => {
                self.ensure_group(&incoming.group);
                self.accounts.push(incoming);
                true
            }
        }
    }

    pub fn ensure_group(&mut self, group: &str) {
        let name = group.trim();
        if !name.is_empty() && !self.groups.iter().any(|g| g == name) {
            self.groups.push(name.to_owned());
        }
    }

    pub fn push_visit(&mut self, visit: ServerVisit) {
        let duplicate = self.server_history.iter().any(|v| v.job_id == visit.job_id && v.user_id == visit.user_id && v.at == visit.at);
        if !duplicate {
            self.server_history.push(visit);
            self.server_history.sort_by_key(|v| v.at);
            let overflow = self.server_history.len().saturating_sub(MAX_SERVER_HISTORY);
            self.server_history.drain(..overflow);
        }
    }
}

fn pick(incoming: &str, current: &str) -> String {
    if incoming.trim().is_empty() { current.to_owned() } else { incoming.to_owned() }
}

/// Moves `moved` before `before` (or after the last visible account when `before` is None).
/// Port of `account_order.move_account`.
pub fn move_account(order: &[String], moved: &str, before: Option<&str>, visible: &[String]) -> Vec<String> {
    if !order.iter().any(|k| k == moved) || before == Some(moved) || before.is_some_and(|b| !order.iter().any(|k| k == b)) {
        return order.to_vec();
    }
    let mut result: Vec<String> = order.iter().filter(|k| *k != moved).cloned().collect();
    let index = match before {
        Some(b) => result.iter().position(|k| k == b).unwrap_or(result.len()),
        None => visible
            .iter()
            .rfind(|k| result.contains(k))
            .and_then(|last| result.iter().position(|k| k == last))
            .map_or(result.len(), |i| i + 1),
    };
    result.insert(index, moved.to_owned());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc(id: u64, name: &str) -> Account {
        Account { user_id: id, username: name.into(), cookie: "c".into(), ..Default::default() }
    }

    #[test]
    fn upsert_keeps_existing_note_and_group() {
        let mut data = VaultData::default();
        assert!(data.upsert(Account { note: "main".into(), group: "Farm".into(), ..acc(1, "a") }));
        assert!(!data.upsert(Account { cookie: "new".into(), ..acc(1, "a") }));
        let a = data.find("1").unwrap();
        assert_eq!((a.note.as_str(), a.group.as_str(), a.cookie.as_str()), ("main", "Farm", "new"));
        assert_eq!(data.groups, vec!["Farm".to_string()]);
    }

    #[test]
    fn find_by_username_alias_or_id() {
        let mut data = VaultData::default();
        data.upsert(Account { alias: "Mule".into(), ..acc(7, "Bob") });
        assert!(data.find("bob").is_some());
        assert!(data.find("mule").is_some());
        assert!(data.find("7").is_some());
        assert!(data.find("nobody").is_none());
    }

    #[test]
    fn move_account_matches_python_semantics() {
        let order: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert_eq!(move_account(&order, "d", Some("b"), &order), ["a", "d", "b", "c"]);
        let visible: Vec<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        assert_eq!(move_account(&order, "d", None, &visible), ["a", "b", "d", "c"]);
        assert_eq!(move_account(&order, "x", Some("a"), &order), order);
    }

    #[test]
    fn redacted_hides_secrets() {
        let mut fields = BTreeMap::new();
        fields.insert("pin".to_string(), "1234".to_string());
        let a = Account { password: "p".into(), fields, ..acc(1, "a") };
        let r = a.redacted();
        assert!(r.cookie.is_empty() && r.password.is_empty() && r.fields.is_empty());
    }
}
