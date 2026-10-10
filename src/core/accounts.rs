//! Account and library operations on the core.

use super::Core;
use crate::error::{AppError, AppResult};
use crate::import::{ImportBatch, ImportedAccount};
use crate::roblox::account as api;
use crate::store::model::{Account, CookieStatus, Favorite, RecentGame, SavedPrivateServer};
use chrono::Utc;

/// Outcome of importing a batch of accounts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportOutcome {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    /// Accounts with only username+password; the caller may offer a browser sign-in.
    pub need_sign_in: usize,
}

impl Core {
    pub fn accounts(&self) -> Vec<Account> {
        self.snapshot().accounts.clone()
    }

    pub fn account(&self, reference: &str) -> Option<Account> {
        self.snapshot().find(reference).cloned()
    }

    pub fn groups(&self) -> Vec<String> {
        self.snapshot().groups.clone()
    }

    /// Adds an account from a cookie, resolving its identity from Roblox first.
    pub fn add_cookie(&self, cookie: &str) -> AppResult<Account> {
        let cookie = cookie.trim();
        if !crate::import::looks_like_cookie(cookie) {
            return Err(AppError::invalid("COOKIE_FORMAT_INVALID", "That text is not a Roblox security cookie."));
        }
        let identity = api::whoami(cookie)?;
        let account = Account {
            user_id: identity.user_id,
            username: identity.username,
            display_name: identity.display_name,
            cookie: cookie.to_owned(),
            added_at: Some(Utc::now()),
            cookie_status: CookieStatus::Valid,
            cookie_checked_at: Some(Utc::now()),
            ..Default::default()
        };
        let saved = account.clone();
        self.edit(|data| {
            data.upsert(account);
        })?;
        crate::log_info!("Added account {}", saved.username);
        Ok(saved)
    }

    /// Stores already-resolved imported accounts (identity resolution happens in the caller).
    pub fn apply_import(&self, batch: &ImportBatch) -> AppResult<ImportOutcome> {
        let mut outcome = ImportOutcome { skipped: batch.skipped, need_sign_in: batch.needs_sign_in().len(), ..Default::default() };
        let importable: Vec<Account> = batch.accounts.iter().filter(|a| a.has_cookie()).cloned().map(ImportedAccount::into_account).collect();
        let favorites = batch.favorites.clone();
        let recents = batch.recent_games.clone();
        let groups = batch.groups.clone();
        self.edit(|data| {
            for group in &groups {
                data.ensure_group(group);
            }
            for account in importable {
                if data.upsert(account) {
                    outcome.added += 1;
                } else {
                    outcome.updated += 1;
                }
            }
            merge_favorites(&mut data.favorites, favorites);
            merge_recents(&mut data.recent_games, recents);
        })?;
        Ok(outcome)
    }

    pub fn delete_account(&self, reference: &str) -> AppResult<bool> {
        let reference = reference.to_owned();
        self.edit(|data| match data.index_of(&reference) {
            Some(index) => {
                data.accounts.remove(index);
                true
            }
            None => false,
        })
    }

    pub fn set_note(&self, reference: &str, note: &str) -> AppResult<()> {
        self.mutate_account(reference, |a| a.note = note.to_owned())
    }

    pub fn set_alias(&self, reference: &str, alias: &str) -> AppResult<()> {
        self.mutate_account(reference, |a| a.alias = alias.to_owned())
    }

    pub fn set_starred(&self, reference: &str, starred: bool) -> AppResult<()> {
        self.mutate_account(reference, |a| a.starred = starred)
    }

    pub fn set_group(&self, reference: &str, group: &str) -> AppResult<()> {
        let group = group.trim().to_owned();
        let reference = reference.to_owned();
        self.edit(|data| {
            if !group.is_empty() {
                data.ensure_group(&group);
            }
            if let Some(account) = data.find_mut(&reference) {
                account.group = group;
            }
        })
    }

    fn mutate_account(&self, reference: &str, change: impl FnOnce(&mut Account)) -> AppResult<()> {
        let reference = reference.to_owned();
        let changed = self.edit(|data| match data.find_mut(&reference) {
            Some(account) => {
                change(account);
                true
            }
            None => false,
        })?;
        if changed { Ok(()) } else { Err(AppError::not_found("Account", &reference)) }
    }

    pub fn set_cookie_status(&self, reference: &str, status: CookieStatus) -> AppResult<()> {
        let reference = reference.to_owned();
        self.edit(|data| {
            if let Some(account) = data.find_mut(&reference) {
                account.cookie_status = status;
                account.cookie_checked_at = Some(Utc::now());
            }
        })
    }

    /// Replaces an account's cookie after a re-login, refreshing its identity next check.
    pub fn update_cookie(&self, reference: &str, cookie: &str) -> AppResult<()> {
        let cookie = cookie.to_owned();
        self.mutate_account(reference, |a| {
            a.cookie = cookie;
            a.cookie_status = CookieStatus::Valid;
            a.cookie_checked_at = Some(Utc::now());
        })
    }

    pub fn reorder(&self, order: &[String]) -> AppResult<()> {
        let order = order.to_vec();
        self.edit(|data| {
            let position = |key: &str| order.iter().position(|k| k == key).unwrap_or(usize::MAX);
            data.accounts.sort_by_key(|a| position(&a.key()));
        })
    }

    // ---- Groups ----

    pub fn create_group(&self, name: &str) -> AppResult<bool> {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err(AppError::invalid("GROUP_NAME_EMPTY", "Enter a group name."));
        }
        self.edit(|data| {
            if data.groups.iter().any(|g| g == &name) {
                false
            } else {
                data.groups.push(name.clone());
                true
            }
        })
    }

    pub fn rename_group(&self, old: &str, new: &str) -> AppResult<bool> {
        let (old, new) = (old.to_owned(), new.trim().to_owned());
        if new.is_empty() {
            return Err(AppError::invalid("GROUP_NAME_EMPTY", "Enter a group name."));
        }
        self.edit(|data| {
            let Some(index) = data.groups.iter().position(|g| g == &old) else { return false };
            if data.groups.iter().any(|g| g == &new) {
                return false;
            }
            data.groups[index] = new.clone();
            for account in &mut data.accounts {
                if account.group == old {
                    account.group = new.clone();
                }
            }
            true
        })
    }

    pub fn delete_group(&self, name: &str) -> AppResult<()> {
        let name = name.to_owned();
        self.edit(|data| {
            data.groups.retain(|g| g != &name);
            for account in &mut data.accounts {
                if account.group == name {
                    account.group.clear();
                }
            }
        })
    }

    // ---- Favorites, recent games, private servers ----

    pub fn favorites(&self) -> Vec<Favorite> {
        self.snapshot().favorites.clone()
    }

    pub fn add_favorite(&self, favorite: Favorite) -> AppResult<()> {
        self.edit(|data| merge_favorites(&mut data.favorites, vec![favorite]))
    }

    pub fn remove_favorite(&self, place_id: u64, private_server: &str) -> AppResult<()> {
        let ps = private_server.to_owned();
        self.edit(|data| data.favorites.retain(|f| !(f.place_id == place_id && f.private_server == ps)))
    }

    pub fn recent_games(&self) -> Vec<RecentGame> {
        self.snapshot().recent_games.clone()
    }

    pub fn record_recent_game(&self, place_id: u64, name: &str, private_server: &str) -> AppResult<()> {
        let max = self.settings().max_recent_games;
        let game = RecentGame { place_id, name: name.to_owned(), private_server: private_server.to_owned(), at: Some(Utc::now()) };
        self.edit(|data| {
            data.recent_games.retain(|g| !(g.place_id == place_id && g.private_server == game.private_server));
            data.recent_games.insert(0, game);
            data.recent_games.truncate(max);
        })
    }

    pub fn private_servers(&self) -> Vec<SavedPrivateServer> {
        self.snapshot().private_servers.clone()
    }

    pub fn save_private_servers(&self, servers: Vec<SavedPrivateServer>) -> AppResult<()> {
        self.edit(|data| {
            for server in servers {
                match data.private_servers.iter_mut().find(|s| s.id == server.id) {
                    Some(existing) => {
                        let note = existing.note.clone();
                        *existing = SavedPrivateServer { note, ..server };
                    }
                    None => data.private_servers.push(server),
                }
            }
        })
    }

    pub fn remove_private_server(&self, id: &str) -> AppResult<()> {
        let id = id.to_owned();
        self.edit(|data| data.private_servers.retain(|s| s.id != id))
    }

    pub fn server_history(&self) -> Vec<crate::store::model::ServerVisit> {
        self.snapshot().server_history.clone()
    }
}

fn merge_favorites(target: &mut Vec<Favorite>, incoming: Vec<Favorite>) {
    for favorite in incoming {
        if favorite.place_id == 0 {
            continue;
        }
        target.retain(|f| !(f.place_id == favorite.place_id && f.private_server == favorite.private_server));
        target.insert(0, favorite);
    }
}

fn merge_recents(target: &mut Vec<RecentGame>, incoming: Vec<RecentGame>) {
    for game in incoming {
        if game.place_id == 0 {
            continue;
        }
        if !target.iter().any(|g| g.place_id == game.place_id && g.private_server == game.private_server) {
            target.push(game);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_core;

    fn imported(id: u64, name: &str) -> ImportedAccount {
        ImportedAccount { user_id: id, username: name.into(), cookie: "_|WARNING:-x|c".into(), ..Default::default() }
    }

    #[test]
    fn import_counts_added_and_updated() {
        let (_dir, core) = test_core();
        let mut batch = ImportBatch::default();
        batch.accounts = vec![imported(1, "a"), imported(2, "b")];
        batch.groups = vec!["Farm".into()];
        assert_eq!(core.apply_import(&batch).unwrap(), ImportOutcome { added: 2, ..Default::default() });
        // Re-import one: counts as updated.
        batch.accounts = vec![imported(1, "a")];
        let outcome = core.apply_import(&batch).unwrap();
        assert_eq!((outcome.added, outcome.updated), (0, 1));
        assert!(core.groups().contains(&"Farm".to_string()));
    }

    #[test]
    fn notes_groups_and_delete() {
        let (_dir, core) = test_core();
        let mut batch = ImportBatch::default();
        batch.accounts = vec![imported(7, "bob")];
        core.apply_import(&batch).unwrap();
        core.set_note("bob", "main").unwrap();
        core.set_group("7", "Mules").unwrap();
        let a = core.account("bob").unwrap();
        assert_eq!((a.note.as_str(), a.group.as_str()), ("main", "Mules"));
        assert!(core.groups().contains(&"Mules".to_string()));
        assert!(core.delete_account("bob").unwrap());
        assert!(core.account("bob").is_none());
    }

    #[test]
    fn rename_group_moves_members() {
        let (_dir, core) = test_core();
        let mut batch = ImportBatch::default();
        batch.accounts = vec![imported(1, "a")];
        batch.groups = vec!["Old".into()];
        core.apply_import(&batch).unwrap();
        core.set_group("a", "Old").unwrap();
        assert!(core.rename_group("Old", "New").unwrap());
        assert_eq!(core.account("a").unwrap().group, "New");
    }

    #[test]
    fn recent_games_capped_and_deduplicated() {
        let (_dir, core) = test_core();
        core.edit_settings(|s| s.max_recent_games = 2).unwrap();
        core.record_recent_game(1, "one", "").unwrap();
        core.record_recent_game(2, "two", "").unwrap();
        core.record_recent_game(1, "one-again", "").unwrap();
        core.record_recent_game(3, "three", "").unwrap();
        let recents = core.recent_games();
        assert_eq!(recents.len(), 2);
        assert_eq!(recents[0].place_id, 3);
    }

    #[test]
    fn secrets_round_trip() {
        let (_dir, core) = test_core();
        core.set_secret("api_token", "abc").unwrap();
        assert_eq!(core.secret("api_token").as_deref(), Some("abc"));
    }
}
