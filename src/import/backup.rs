//! Portable, password-protected account backups.
//!
//! Nova writes `{format:"nova-accounts-backup", version:1, kdf, nonce, data}` where
//! `data` is Argon2id + AES-256-GCM over the accounts JSON. It also reads Evanovar's
//! `{format:"ram-accounts-backup", version:1, salt, data:{nonce,tag,ciphertext}}`.

use super::{ImportBatch, ImportedAccount};
use crate::error::{AppError, AppResult};
use crate::store::crypto::{self, KdfParams};
use crate::store::model::Account;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

const NOVA_FORMAT: &str = "nova-accounts-backup";
const EVANOVAR_FORMAT: &str = "ram-accounts-backup";
pub const MIN_PASSWORD_LEN: usize = 8;

#[derive(Serialize, Deserialize)]
struct NovaBackup {
    format: String,
    version: u32,
    kdf: KdfParams,
    nonce: String,
    data: String,
}

/// Writes a password-protected backup of the given accounts.
pub fn export(accounts: &[Account], path: &Path, password: &str) -> AppResult<usize> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(AppError::invalid(
            "BACKUP_PASSWORD_SHORT",
            format!("Use a backup password with at least {MIN_PASSWORD_LEN} characters."),
        ));
    }
    if accounts.is_empty() {
        return Err(AppError::new("BACKUP_EMPTY", "Nothing To Export", "There are no accounts to export."));
    }
    let kdf = KdfParams::fresh();
    let key = kdf.derive(password)?;
    let plain = serde_json::to_vec(&serde_json::json!({
        "accounts": accounts,
        "exported_at": chrono::Utc::now().to_rfc3339(),
    }))
    .map_err(|e| AppError::unexpected("backup encode", e))?;
    let (nonce, ct) = crypto::aes_encrypt(&key, &plain)?;
    let backup = NovaBackup { format: NOVA_FORMAT.into(), version: 1, kdf, nonce: crypto::b64(&nonce), data: crypto::b64(&ct) };
    crate::store::atomic::write_json(path, &backup).map_err(|e| AppError::io("Writing the backup", &e))?;
    Ok(accounts.len())
}

/// Reads a Nova or Evanovar backup into an import batch.
pub fn import(path: &Path, password: &str) -> AppResult<ImportBatch> {
    let bytes = std::fs::read(path).map_err(|e| AppError::io("Reading the backup", &e))?;
    let document: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::new("BACKUP_INVALID", "Not A Backup", "This file is not an account backup.").with_detail(e.to_string()))?;
    let format = document.get("format").and_then(Value::as_str).unwrap_or_default();
    let plain = match format {
        NOVA_FORMAT => {
            let backup: NovaBackup = serde_json::from_value(document).map_err(|e| malformed(&e.to_string()))?;
            let key = backup.kdf.derive(password)?;
            crypto::aes_decrypt(&key, &crypto::unb64(&backup.nonce)?, &crypto::unb64(&backup.data)?).map_err(|_| wrong_password())?
        }
        EVANOVAR_FORMAT => decrypt_evanovar_backup(&document, password)?,
        _ => return Err(AppError::new("BACKUP_INVALID", "Not A Backup", "This file is not an account backup Nova can read.")),
    };
    let payload: Value = serde_json::from_slice(&plain).map_err(|e| malformed(&e.to_string()))?;
    Ok(batch_from_accounts(&payload))
}

fn decrypt_evanovar_backup(document: &Value, password: &str) -> AppResult<Vec<u8>> {
    let salt_text = document.get("salt").and_then(Value::as_str).ok_or_else(|| malformed("missing salt"))?;
    let salt = crypto::python_lenient_b64(salt_text)?;
    let data = document.get("data").ok_or_else(|| malformed("missing data"))?;
    let nonce = crypto::unb64(data.get("nonce").and_then(Value::as_str).ok_or_else(|| malformed("nonce"))?)?;
    let tag = crypto::unb64(data.get("tag").and_then(Value::as_str).ok_or_else(|| malformed("tag"))?)?;
    let ct = crypto::unb64(data.get("ciphertext").and_then(Value::as_str).ok_or_else(|| malformed("ciphertext"))?)?;
    let key = crypto::evanovar_pbkdf2(password.as_bytes(), &salt);
    crypto::evanovar_decrypt(&key, &nonce, &tag, &ct).map_err(|_| wrong_password())
}

fn batch_from_accounts(payload: &Value) -> ImportBatch {
    let mut batch = ImportBatch { source: "backup".into(), ..Default::default() };
    let accounts = payload.get("accounts");
    // Nova backups store an array of full Account objects.
    if let Some(array) = accounts.and_then(Value::as_array) {
        for item in array {
            if let Ok(account) = serde_json::from_value::<Account>(item.clone()) {
                if account.cookie.is_empty() {
                    batch.skipped += 1;
                    continue;
                }
                batch.accounts.push(ImportedAccount {
                    cookie: account.cookie,
                    username: account.username,
                    user_id: account.user_id,
                    password: account.password,
                    note: account.note,
                    alias: account.alias,
                    group: account.group,
                    fields: account.fields,
                    added_at: account.added_at,
                    last_used: account.last_used,
                });
            } else {
                batch.skipped += 1;
            }
        }
    } else if let Some(map) = accounts.and_then(Value::as_object) {
        // Evanovar backups store a username → record object.
        for (name, record) in map {
            let cookie = record.get("cookie").and_then(Value::as_str).unwrap_or_default().to_owned();
            if cookie.is_empty() {
                batch.skipped += 1;
                continue;
            }
            batch.accounts.push(ImportedAccount {
                cookie,
                username: record.get("username").and_then(Value::as_str).unwrap_or(name).to_owned(),
                user_id: record.get("user_id").and_then(super::as_u64).unwrap_or(0),
                note: record.get("note").and_then(Value::as_str).unwrap_or_default().to_owned(),
                ..Default::default()
            });
        }
    }
    batch
}

fn malformed(detail: &str) -> AppError {
    AppError::new("BACKUP_DAMAGED", "Backup Damaged", "The backup file is damaged.").with_detail(detail.to_owned())
}

fn wrong_password() -> AppError {
    AppError::new("BACKUP_PASSWORD_INVALID", "Wrong Backup Password", "The password did not unlock this backup.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn account(id: u64) -> Account {
        Account { user_id: id, username: format!("u{id}"), cookie: "_|WARNING:-x|c".into(), note: "n".into(), ..Default::default() }
    }

    #[test]
    fn nova_backup_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accts.novabackup");
        export(&[account(1), account(2)], &path, "longenough").unwrap();
        assert_eq!(import(&path, "longenough").unwrap().accounts.len(), 2);
        assert_eq!(import(&path, "wrongpass").err().unwrap().code, "BACKUP_PASSWORD_INVALID");
    }

    #[test]
    fn short_password_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(export(&[account(1)], &dir.path().join("b"), "short").err().unwrap().code, "BACKUP_PASSWORD_SHORT");
    }

    /// Reads the real Evanovar backup produced by the Python export in the fixture.
    #[test]
    fn reads_evanovar_backup() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/evanovar_backup.json");
        let batch = import(&path, "backup-pass-1").unwrap();
        assert_eq!(batch.accounts.len(), 1);
        assert_eq!(batch.accounts[0].username, "alice");
    }
}
