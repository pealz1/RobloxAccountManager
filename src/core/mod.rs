//! The application core: everything the UI, the local API and the MCP server call.
//!
//! `Core` owns the vault and settings behind locks so the GUI thread, API threads
//! and background workers can all use it. Methods are synchronous; callers that
//! must not block (the GUI) run them on a worker thread and post results back.

pub mod accounts;
pub mod launcher;

use crate::error::{AppError, AppResult};
use crate::store::model::VaultData;
use crate::store::settings::Settings;
use crate::store::vault::{Protection, Vault};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

pub struct Core {
    data_dir: PathBuf,
    /// Serializes this process's writes; its `update()` takes the blocking
    /// cross-process file lock, so it is only ever locked off the GUI thread
    /// (background workers) or on a click — never per frame.
    vault: Mutex<Vault>,
    /// A fast read cache the GUI hits every frame. It is never held across I/O,
    /// so a stalled cross-process file lock can't freeze rendering.
    cache: RwLock<Arc<VaultData>>,
    /// Bumps on every change; the GUI reads it lock-free to detect updates.
    revision: AtomicU64,
    settings: RwLock<Settings>,
    /// The live multi-instance guard, when Multi Roblox is on.
    multi: Mutex<Option<Arc<crate::win::multi::MultiGuard>>>,
}

impl Core {
    /// Opens the vault (creating a plain one only when none exists) and loads settings.
    pub fn open(data_dir: &Path, password: Option<&str>) -> AppResult<Arc<Core>> {
        let settings = Settings::load(data_dir);
        // Only create a fresh vault when no vault file exists at all. If a vault is
        // present but can't be read right now (a transient I/O error), propagate the
        // error instead of creating an empty vault over it.
        let vault =
            if Vault::exists(data_dir) { Vault::open(data_dir, password)? } else { Vault::create(data_dir, Protection::None, None)? };
        Ok(Arc::new(Core {
            data_dir: data_dir.to_path_buf(),
            cache: RwLock::new(vault.cached()),
            revision: AtomicU64::new(vault.revision()),
            vault: Mutex::new(vault),
            settings: RwLock::new(settings),
            multi: Mutex::new(None),
        }))
    }

    /// Protection of the existing vault without opening it (None when there is no vault yet).
    pub fn vault_protection(data_dir: &Path) -> Option<Protection> {
        Vault::peek_protection(data_dir)
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn protection(&self) -> Protection {
        self.vault.lock().unwrap_or_else(|p| p.into_inner()).protection()
    }

    /// A consistent snapshot of the vault data. Served from the in-memory cache, so
    /// it never blocks on the cross-process file lock.
    pub fn snapshot(&self) -> Arc<VaultData> {
        Arc::clone(&self.cache.read().unwrap_or_else(|p| p.into_inner()))
    }

    /// Monotonic counter that bumps on every change, for the UI to detect updates.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// Refreshes the cache from disk if another process changed the vault. Call this
    /// from a background worker, never per frame — it may briefly take the file lock.
    pub fn refresh_from_disk(&self) {
        let mut vault = self.vault.lock().unwrap_or_else(|p| p.into_inner());
        if vault.refresh().unwrap_or(false) {
            self.publish(&vault);
        }
    }

    /// Updates the read cache and revision from the vault after a change.
    fn publish(&self, vault: &Vault) {
        *self.cache.write().unwrap_or_else(|p| p.into_inner()) = vault.cached();
        self.revision.store(vault.revision(), Ordering::Release);
    }

    /// Runs `change` against the vault and persists it, then refreshes the read cache.
    pub fn edit<R>(&self, change: impl FnOnce(&mut VaultData) -> R) -> AppResult<R> {
        let mut vault = self.vault.lock().unwrap_or_else(|p| p.into_inner());
        let result = vault.update(change)?;
        self.publish(&vault);
        Ok(result)
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Updates settings in memory and on disk.
    pub fn edit_settings<R>(&self, change: impl FnOnce(&mut Settings) -> R) -> AppResult<R> {
        let mut guard = self.settings.write().unwrap_or_else(|p| p.into_inner());
        let result = change(&mut guard);
        guard.save(&self.data_dir).map_err(|e| AppError::io("Saving settings", &e))?;
        Ok(result)
    }

    pub fn set_protection(&self, protection: Protection, password: Option<&str>) -> AppResult<()> {
        let mut vault = self.vault.lock().unwrap_or_else(|p| p.into_inner());
        vault.set_protection(protection, password)?;
        self.publish(&vault);
        Ok(())
    }

    /// A small secret stored encrypted in the vault (for example the API token).
    pub fn secret(&self, key: &str) -> Option<String> {
        self.snapshot().secrets.get(key).cloned()
    }

    pub fn set_secret(&self, key: &str, value: &str) -> AppResult<()> {
        let key = key.to_owned();
        let value = value.to_owned();
        self.edit(|data| {
            data.secrets.insert(key, value);
        })
    }

    // ---- Multi Roblox lifecycle ----

    pub fn multi_roblox_running(&self) -> bool {
        self.multi.lock().unwrap_or_else(|p| p.into_inner()).is_some()
    }

    pub fn multi_guard(&self) -> Option<Arc<crate::win::multi::MultiGuard>> {
        self.multi.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn enable_multi_roblox(&self) -> AppResult<()> {
        let settings = self.settings();
        let guard = crate::win::multi::enable(settings.multi_method, settings.cookie_lock_773)?;
        *self.multi.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(guard));
        Ok(())
    }

    pub fn disable_multi_roblox(&self) {
        self.multi.lock().unwrap_or_else(|p| p.into_inner()).take();
    }
}

#[cfg(test)]
pub(crate) fn test_core() -> (tempfile::TempDir, Arc<Core>) {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::open(dir.path(), None).unwrap();
    (dir, core)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::model::Account;

    #[test]
    fn open_does_not_create_over_an_existing_unopenable_vault() {
        let dir = tempfile::tempdir().unwrap();
        {
            let core = Core::open(dir.path(), Some("pw-123456")).unwrap();
            core.set_protection(Protection::Password, Some("pw-123456")).unwrap();
            core.edit(|d| {
                d.upsert(Account { user_id: 1, username: "a".into(), cookie: "c".into(), ..Default::default() });
            })
            .unwrap();
        }
        // Opening a present password vault without the password must error, not wipe it.
        assert!(Core::open(dir.path(), None).is_err());
        // The data is intact and still unlocks with the password.
        let reopened = Core::open(dir.path(), Some("pw-123456")).unwrap();
        assert_eq!(reopened.accounts().len(), 1);
    }

    #[test]
    fn edit_updates_the_lock_free_read_cache() {
        let (_dir, core) = test_core();
        assert_eq!(core.accounts().len(), 0);
        let before = core.revision();
        core.edit(|d| {
            d.upsert(Account { user_id: 7, username: "b".into(), cookie: "c".into(), ..Default::default() });
        })
        .unwrap();
        assert_eq!(core.snapshot().accounts.len(), 1);
        assert!(core.revision() > before);
    }
}
