//! The encrypted vault: accounts and everything else that is private.
//!
//! Every change goes through [`Vault::update`], which takes a cross-process file lock,
//! reloads the file if another process (for example the MCP bridge or a second window)
//! changed it, applies the change, and writes the file atomically while keeping the
//! previous good copy as `vault.json.bak`.

use super::atomic::write_atomic;
use super::crypto::{self, KdfParams};
use super::model::VaultData;
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

const FORMAT: &str = "nova-vault";
const VERSION: u32 = 1;
pub const FILE_NAME: &str = "vault.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protection {
    /// Plain JSON. Only when the user explicitly picks it.
    None,
    /// Windows DPAPI: unlocks automatically for this Windows account only.
    Windows,
    /// Argon2id + AES-256-GCM: asks for a password at startup, portable between PCs.
    Password,
}

impl Protection {
    pub fn label(self) -> &'static str {
        match self {
            Protection::None => "Not encrypted",
            Protection::Windows => "Windows account",
            Protection::Password => "Password",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct VaultFile {
    format: String,
    version: u32,
    protection: Protection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kdf: Option<KdfParams>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    nonce: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payload: Option<VaultData>,
}

#[derive(Clone)]
enum Key {
    None,
    Windows,
    Password { key: [u8; 32], kdf: KdfParams },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: SystemTime,
}

pub struct Vault {
    path: PathBuf,
    key: Key,
    data: Arc<VaultData>,
    stamp: Option<Stamp>,
    revision: u64,
    using_backup: bool,
}

pub fn locked_error() -> AppError {
    AppError::new("VAULT_LOCKED", "Vault Locked", "Enter the vault password to unlock your accounts.")
}

pub fn wrong_password() -> AppError {
    AppError::new("PASSWORD_INVALID", "Wrong Password", "The password did not unlock the vault.")
}

impl Vault {
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join(FILE_NAME)
    }

    /// Returns the protection of an existing vault without decrypting it.
    pub fn peek_protection(dir: &Path) -> Option<Protection> {
        let path = Self::path_in(dir);
        read_file(&path)
            .or_else(|_| read_file(&backup_path(&path)))
            .ok()
            .map(|f| f.protection)
    }

    pub fn create(dir: &Path, protection: Protection, password: Option<&str>) -> AppResult<Vault> {
        let key = make_key(protection, password)?;
        let mut vault = Vault {
            path: Self::path_in(dir),
            key,
            data: Arc::new(VaultData::default()),
            stamp: None,
            revision: 1,
            using_backup: false,
        };
        let _guard = vault.lock()?;
        vault.write(&VaultData::default())?;
        Ok(vault)
    }

    pub fn open(dir: &Path, password: Option<&str>) -> AppResult<Vault> {
        let path = Self::path_in(dir);
        let (file, using_backup) = match read_file(&path) {
            Ok(file) => (file, false),
            Err(main_error) => match read_file(&backup_path(&path)) {
                Ok(file) => {
                    crate::log_warn!("vault.json could not be read; recovered from vault.json.bak");
                    (file, true)
                }
                Err(_) => return Err(main_error),
            },
        };
        let key = match file.protection {
            Protection::None => Key::None,
            Protection::Windows => Key::Windows,
            Protection::Password => {
                let password = password.ok_or_else(locked_error)?;
                let kdf = file.kdf.clone().ok_or_else(|| damaged("missing key parameters"))?;
                Key::Password { key: kdf.derive(password)?, kdf }
            }
        };
        let data = decode(&file, &key)?;
        let stamp = if using_backup { None } else { stamp_of(&path) };
        Ok(Vault { path, key, data: Arc::new(data), stamp, revision: 1, using_backup })
    }

    pub fn protection(&self) -> Protection {
        match self.key {
            Key::None => Protection::None,
            Key::Windows => Protection::Windows,
            Key::Password { .. } => Protection::Password,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Current data, reloaded first when another process changed the file.
    pub fn data(&mut self) -> Arc<VaultData> {
        if let Err(err) = self.refresh() {
            crate::log_warn!("Vault reload failed: {err}");
        }
        Arc::clone(&self.data)
    }

    /// Cached data without touching the disk.
    pub fn cached(&self) -> Arc<VaultData> {
        Arc::clone(&self.data)
    }

    /// Reloads the vault if its file changed since we last read or wrote it.
    pub fn refresh(&mut self) -> AppResult<bool> {
        let current = stamp_of(&self.path);
        if current.is_none() || current == self.stamp || self.using_backup {
            return Ok(false);
        }
        let file = read_file(&self.path)?;
        let data = decode(&file, &self.key)?;
        self.data = Arc::new(data);
        self.stamp = current;
        self.revision += 1;
        Ok(true)
    }

    /// Applies `change` to a fresh copy of the data and saves it. Nothing changes in
    /// memory unless the write succeeds.
    pub fn update<R>(&mut self, change: impl FnOnce(&mut VaultData) -> R) -> AppResult<R> {
        let _guard = self.lock()?;
        self.refresh()?;
        let mut next = (*self.data).clone();
        let result = change(&mut next);
        if next != *self.data {
            self.write(&next)?;
            self.data = Arc::new(next);
            self.revision += 1;
        }
        Ok(result)
    }

    /// Re-encrypts the vault with a different protection.
    pub fn set_protection(&mut self, protection: Protection, password: Option<&str>) -> AppResult<()> {
        let _guard = self.lock()?;
        self.refresh()?;
        let previous = self.key.clone();
        self.key = make_key(protection, password)?;
        let data = (*self.data).clone();
        if let Err(err) = self.write(&data) {
            self.key = previous;
            return Err(err);
        }
        // The backup was written with the old key; replace it so it stays readable.
        let _ = fs::copy(&self.path, backup_path(&self.path));
        self.revision += 1;
        Ok(())
    }

    fn write(&mut self, data: &VaultData) -> AppResult<()> {
        let file = encode(data, &self.key)?;
        let bytes = serde_json::to_vec_pretty(&file).map_err(|e| AppError::unexpected("vault encode", e))?;
        if self.path.exists() && !self.using_backup {
            let _ = fs::copy(&self.path, backup_path(&self.path));
        }
        write_atomic(&self.path, &bytes).map_err(|e| AppError::io("Saving the vault", &e))?;
        self.stamp = stamp_of(&self.path);
        self.using_backup = false;
        Ok(())
    }

    fn lock(&self) -> AppResult<FileLock> {
        let lock_path = self.path.with_file_name("vault.lock");
        if let Some(dir) = lock_path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|e| AppError::io("Opening the vault lock", &e))?;
        file.lock().map_err(|e| AppError::io("Locking the vault", &e))?;
        Ok(FileLock(file))
    }
}

struct FileLock(File);

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn backup_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.bak", path.display()))
}

fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some(Stamp { len: meta.len(), modified: meta.modified().ok()? })
}

fn damaged(detail: &str) -> AppError {
    AppError::new("VAULT_DAMAGED", "Vault Damaged", "The account vault could not be read.").with_detail(detail.to_owned())
}

fn read_file(path: &Path) -> AppResult<VaultFile> {
    let bytes = fs::read(path).map_err(|e| AppError::io("Reading the vault", &e))?;
    let file: VaultFile = serde_json::from_slice(&bytes).map_err(|e| damaged(&e.to_string()))?;
    if file.format != FORMAT {
        return Err(damaged("not a Nova vault"));
    }
    if file.version > VERSION {
        return Err(AppError::new(
            "VAULT_TOO_NEW",
            "Vault From A Newer Version",
            "This vault was saved by a newer Nova RAM. Update the app to open it.",
        ));
    }
    Ok(file)
}

fn make_key(protection: Protection, password: Option<&str>) -> AppResult<Key> {
    Ok(match protection {
        Protection::None => Key::None,
        Protection::Windows => Key::Windows,
        Protection::Password => {
            let password = password.filter(|p| !p.is_empty()).ok_or_else(|| {
                AppError::invalid("PASSWORD_REQUIRED", "Choose a password for the vault.")
            })?;
            let kdf = KdfParams::fresh();
            Key::Password { key: kdf.derive(password)?, kdf }
        }
    })
}

fn encode(data: &VaultData, key: &Key) -> AppResult<VaultFile> {
    let mut file = VaultFile {
        format: FORMAT.into(),
        version: VERSION,
        protection: Protection::None,
        kdf: None,
        nonce: String::new(),
        data: String::new(),
        payload: None,
    };
    match key {
        Key::None => file.payload = Some(data.clone()),
        Key::Windows => {
            let plain = serde_json::to_vec(data).map_err(|e| AppError::unexpected("vault encode", e))?;
            file.protection = Protection::Windows;
            file.data = crypto::b64(&crypto::nova_dpapi_protect(&plain)?);
        }
        Key::Password { key, kdf } => {
            let plain = serde_json::to_vec(data).map_err(|e| AppError::unexpected("vault encode", e))?;
            let (nonce, ct) = crypto::aes_encrypt(key, &plain)?;
            file.protection = Protection::Password;
            file.kdf = Some(kdf.clone());
            file.nonce = crypto::b64(&nonce);
            file.data = crypto::b64(&ct);
        }
    }
    Ok(file)
}

fn decode(file: &VaultFile, key: &Key) -> AppResult<VaultData> {
    let plain = match (file.protection, key) {
        (Protection::None, _) => return file.payload.clone().ok_or_else(|| damaged("missing payload")),
        (Protection::Windows, _) => crypto::nova_dpapi_unprotect(&crypto::unb64(&file.data)?)?,
        (Protection::Password, Key::Password { key, .. }) => {
            crypto::aes_decrypt(key, &crypto::unb64(&file.nonce)?, &crypto::unb64(&file.data)?)
                .map_err(|_| wrong_password())?
        }
        (Protection::Password, _) => return Err(locked_error()),
    };
    serde_json::from_slice(&plain).map_err(|e| damaged(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::model::Account;

    fn account(id: u64) -> Account {
        Account { user_id: id, username: format!("u{id}"), cookie: "c".into(), ..Default::default() }
    }

    #[test]
    fn windows_vault_round_trip_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::create(dir.path(), Protection::Windows, None).unwrap();
        vault.update(|d| d.upsert(account(1))).unwrap();
        vault.update(|d| d.upsert(account(2))).unwrap();
        let reopened = Vault::open(dir.path(), None).unwrap();
        assert_eq!(reopened.cached().accounts.len(), 2);
        // Damage the main file: the backup (one write behind) is used.
        fs::write(dir.path().join(FILE_NAME), b"{garbage").unwrap();
        let recovered = Vault::open(dir.path(), None).unwrap();
        assert_eq!(recovered.cached().accounts.len(), 1);
        assert!(!fs::read_to_string(dir.path().join(FILE_NAME)).unwrap().contains("u1"));
    }

    #[test]
    fn password_vault_requires_password() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::create(dir.path(), Protection::Password, Some("hunter22")).unwrap();
        vault.update(|d| d.upsert(account(5))).unwrap();
        assert_eq!(Vault::open(dir.path(), None).err().unwrap().code, "VAULT_LOCKED");
        assert_eq!(Vault::open(dir.path(), Some("nope")).err().unwrap().code, "PASSWORD_INVALID");
        assert_eq!(Vault::open(dir.path(), Some("hunter22")).unwrap().cached().accounts.len(), 1);
        assert!(!fs::read_to_string(dir.path().join(FILE_NAME)).unwrap().contains("u5"));
    }

    #[test]
    fn second_process_changes_are_picked_up_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = Vault::create(dir.path(), Protection::None, None).unwrap();
        let mut b = Vault::open(dir.path(), None).unwrap();
        a.update(|d| d.upsert(account(1))).unwrap();
        // b has a stale copy but must not lose a's change.
        std::thread::sleep(std::time::Duration::from_millis(20));
        b.update(|d| d.upsert(account(2))).unwrap();
        let mut c = Vault::open(dir.path(), None).unwrap();
        assert_eq!(c.data().accounts.len(), 2);
    }

    #[test]
    fn switching_protection_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::create(dir.path(), Protection::None, None).unwrap();
        vault.update(|d| d.upsert(account(9))).unwrap();
        vault.set_protection(Protection::Password, Some("pw-123456")).unwrap();
        assert_eq!(Vault::peek_protection(dir.path()), Some(Protection::Password));
        let opened = Vault::open(dir.path(), Some("pw-123456")).unwrap();
        assert_eq!(opened.cached().accounts[0].user_id, 9);
    }
}
