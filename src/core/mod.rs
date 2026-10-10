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
use std::sync::{Arc, Mutex, RwLock};

pub struct Core {
    data_dir: PathBuf,
    vault: Mutex<Vault>,
    settings: RwLock<Settings>,
    /// The live multi-instance guard, when Multi Roblox is on.
    multi: Mutex<Option<Arc<crate::win::multi::MultiGuard>>>,
}

impl Core {
    /// Opens the vault (creating a plain one if none exists) and loads settings.
    pub fn open(data_dir: &Path, password: Option<&str>) -> AppResult<Arc<Core>> {
        let settings = Settings::load(data_dir);
        let vault = match Vault::peek_protection(data_dir) {
            Some(_) => Vault::open(data_dir, password)?,
            None => Vault::create(data_dir, Protection::None, None)?,
        };
        Ok(Arc::new(Core {
            data_dir: data_dir.to_path_buf(),
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

    /// A consistent snapshot of the vault data, reloaded if another process changed it.
    pub fn snapshot(&self) -> Arc<VaultData> {
        self.vault.lock().unwrap_or_else(|p| p.into_inner()).data()
    }

    /// Monotonic counter that bumps on every change, for the UI to detect updates.
    pub fn revision(&self) -> u64 {
        self.vault.lock().unwrap_or_else(|p| p.into_inner()).revision()
    }

    /// Runs `change` against the vault and persists it.
    pub fn edit<R>(&self, change: impl FnOnce(&mut VaultData) -> R) -> AppResult<R> {
        self.vault.lock().unwrap_or_else(|p| p.into_inner()).update(change)
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
        self.vault.lock().unwrap_or_else(|p| p.into_inner()).set_protection(protection, password)
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
