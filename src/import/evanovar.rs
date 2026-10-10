//! Reads data from the Python Evanovar RAM so users can migrate.
//!
//! `saved_accounts.json` is either plain `{accounts, secure_settings}` or
//! `{encrypted: true, data: {nonce, tag, ciphertext}}`. Encrypted files use the
//! hardware key (PBKDF2-SHA1 of a WMI machine id) or a password key. Hardware
//! files can only be opened on the computer that wrote them.

use super::{ImportBatch, ImportedAccount};
use crate::error::{AppError, AppResult};
use crate::store::crypto;
use crate::store::model::{Favorite, RecentGame};
use serde_json::Value;
use std::path::Path;

const HARDWARE_SALT: &[u8] = b"roblox_account_manager_salt_v1";

/// Opens `saved_accounts.json`. `password` is required for password-encrypted files;
/// `machine_id` (SHA-256 hex of the WMI identifiers) unlocks hardware files when supplied.
pub fn read_accounts(path: &Path, password: Option<&str>, machine_id: Option<&str>) -> AppResult<ImportBatch> {
    let bytes = std::fs::read(path).map_err(|e| AppError::io("Reading saved_accounts.json", &e))?;
    let document: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::new("EVANOVAR_INVALID", "Not An Evanovar File", "This is not a valid Evanovar RAM account file.").with_detail(e.to_string()))?;
    let payload = decode_payload(&document, password, machine_id)?;
    Ok(batch_from_payload(&payload))
}

fn decode_payload(document: &Value, password: Option<&str>, machine_id: Option<&str>) -> AppResult<Value> {
    if document.get("encrypted").and_then(Value::as_bool) != Some(true) {
        return Ok(document.clone());
    }
    let package = document.get("data").ok_or_else(|| malformed("missing encrypted data"))?;
    let nonce = crypto::unb64(field(package, "nonce")?)?;
    let tag = crypto::unb64(field(package, "tag")?)?;
    let ciphertext = crypto::unb64(field(package, "ciphertext")?)?;

    // Password mode is marked by a `password_encoding` field.
    if let Some(password) = password.filter(|_| package.get("password_encoding").is_some() || password.is_some()) {
        if let Some(salt_hex) = document.get("salt").and_then(Value::as_str) {
            let salt = crypto::python_lenient_b64(salt_hex)?;
            let key = crypto::evanovar_pbkdf2(password.as_bytes(), &salt);
            if let Ok(plain) = crypto::evanovar_decrypt(&key, &nonce, &tag, &ciphertext) {
                return parse_plain(&plain);
            }
            // Older builds derived the key from the Latin-1 bytes of the password.
            if let Some(latin1) = to_latin1(password) {
                let legacy = crypto::evanovar_pbkdf2(&latin1, &salt);
                if let Ok(plain) = crypto::evanovar_decrypt(&legacy, &nonce, &tag, &ciphertext) {
                    return parse_plain(&plain);
                }
            }
            return Err(AppError::new("PASSWORD_INVALID", "Wrong Password", "The password did not unlock this Evanovar file."));
        }
    }

    // Hardware mode: the PBKDF2 secret is the machine id hex string.
    let machine_id = machine_id.ok_or_else(|| AppError::new(
        "EVANOVAR_HARDWARE",
        "Hardware-Encrypted File",
        "This file is tied to the computer that made it. Open Evanovar RAM there and export a password backup, then import that.",
    ))?;
    let key = crypto::evanovar_pbkdf2(machine_id.as_bytes(), HARDWARE_SALT);
    let plain = crypto::evanovar_decrypt(&key, &nonce, &tag, &ciphertext)
        .map_err(|_| AppError::new("EVANOVAR_HARDWARE", "Could Not Decrypt", "The hardware key for this computer did not match the file."))?;
    parse_plain(&plain)
}

fn parse_plain(plain: &[u8]) -> AppResult<Value> {
    serde_json::from_slice(plain).map_err(|e| malformed(&e.to_string()))
}

/// Evanovar stores accounts as `{ "<username>": {username, cookie, user_id, note, ...} }`,
/// optionally wrapped as `{accounts: {...}, secure_settings: {...}}`.
fn batch_from_payload(payload: &Value) -> ImportBatch {
    let accounts_obj = payload.get("accounts").filter(|v| v.is_object()).unwrap_or(payload);
    let mut batch = ImportBatch { source: "Evanovar RAM".into(), ..Default::default() };
    if let Some(map) = accounts_obj.as_object() {
        for (name, record) in map {
            let Some(obj) = record.as_object() else { continue };
            let cookie = obj.get("cookie").and_then(Value::as_str).unwrap_or_default().to_owned();
            if cookie.is_empty() {
                batch.skipped += 1;
                continue;
            }
            batch.accounts.push(ImportedAccount {
                cookie,
                username: obj.get("username").and_then(Value::as_str).unwrap_or(name).to_owned(),
                user_id: obj.get("user_id").and_then(super::as_u64).unwrap_or(0),
                password: obj.get("password").and_then(Value::as_str).unwrap_or_default().to_owned(),
                note: obj.get("note").and_then(Value::as_str).unwrap_or_default().to_owned(),
                ..Default::default()
            });
        }
    }
    batch
}

/// Reads a sibling `groups.json`, `favorites.json` and `recent_games.json` if present.
pub fn read_side_files(dir: &Path, batch: &mut ImportBatch) {
    if let Ok(text) = std::fs::read_to_string(dir.join("groups.json")) {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) {
            if let Some(groups) = map.get("groups").and_then(Value::as_array) {
                batch.groups.extend(groups.iter().filter_map(|g| g.as_str().map(str::to_owned)));
            }
            if let Some(assignments) = map.get("assignments").and_then(Value::as_object) {
                for account in &mut batch.accounts {
                    if let Some(group) = assignments.get(&account.username).and_then(Value::as_str) {
                        account.group = group.to_owned();
                    }
                }
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string(dir.join("favorites.json")) {
        if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text) {
            for item in items {
                batch.favorites.push(Favorite {
                    place_id: item.get("place_id").and_then(super::as_u64).unwrap_or(0),
                    name: item.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    private_server: item.get("private_server").and_then(Value::as_str).unwrap_or_default().to_owned(),
                });
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string(dir.join("recent_games.json")) {
        if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text) {
            for item in items {
                batch.recent_games.push(RecentGame {
                    place_id: item.get("place_id").and_then(super::as_u64).unwrap_or(0),
                    name: item.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    private_server: item.get("private_server").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    at: None,
                });
            }
        }
    }
}

fn field<'a>(value: &'a Value, key: &str) -> AppResult<&'a str> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| malformed(&format!("missing {key}")))
}

fn malformed(detail: &str) -> AppError {
    AppError::new("EVANOVAR_MALFORMED", "Damaged File", "The Evanovar account file is damaged.").with_detail(detail.to_owned())
}

fn to_latin1(text: &str) -> Option<Vec<u8>> {
    text.chars().map(|c| (c as u32 <= 0xFF).then_some(c as u8)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn vectors() -> Value {
        let text = std::fs::read_to_string(fixtures().join("evanovar_vectors.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    /// Decrypts the real PyCryptodome password package produced by Evanovar's code.
    #[test]
    fn decrypts_password_package() {
        let v = vectors();
        let p = &v["password"];
        let salt = crypto::python_lenient_b64(p["salt_config"].as_str().unwrap()).unwrap();
        let key = crypto::evanovar_pbkdf2(p["password"].as_str().unwrap().as_bytes(), &salt);
        let pkg = &p["package"];
        let plain = crypto::evanovar_decrypt(
            &key,
            &crypto::unb64(pkg["nonce"].as_str().unwrap()).unwrap(),
            &crypto::unb64(pkg["tag"].as_str().unwrap()).unwrap(),
            &crypto::unb64(pkg["ciphertext"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        let payload: Value = serde_json::from_slice(&plain).unwrap();
        let batch = batch_from_payload(&payload);
        assert_eq!(batch.accounts.len(), 1);
        assert_eq!(batch.accounts[0].username, "alice");
        assert_eq!(batch.accounts[0].note, "né");
    }

    /// Decrypts the hardware package using the fixed machine id from the fixture.
    #[test]
    fn decrypts_hardware_package() {
        let v = vectors();
        let h = &v["hardware"];
        let key = crypto::evanovar_pbkdf2(h["machine_id"].as_str().unwrap().as_bytes(), HARDWARE_SALT);
        let pkg = &h["package"];
        let plain = crypto::evanovar_decrypt(
            &key,
            &crypto::unb64(pkg["nonce"].as_str().unwrap()).unwrap(),
            &crypto::unb64(pkg["tag"].as_str().unwrap()).unwrap(),
            &crypto::unb64(pkg["ciphertext"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert!(String::from_utf8_lossy(&plain).contains("alice"));
    }

    #[test]
    fn decrypts_legacy_latin1_password() {
        let v = vectors();
        let legacy = &v["password_legacy"];
        let salt = crypto::python_lenient_b64(legacy["salt_config"].as_str().unwrap()).unwrap();
        let password = legacy["password"].as_str().unwrap();
        let key = crypto::evanovar_pbkdf2(&to_latin1(password).unwrap(), &salt);
        let pkg = &legacy["package"];
        let plain = crypto::evanovar_decrypt(
            &key,
            &crypto::unb64(pkg["nonce"].as_str().unwrap()).unwrap(),
            &crypto::unb64(pkg["tag"].as_str().unwrap()).unwrap(),
            &crypto::unb64(pkg["ciphertext"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert!(String::from_utf8_lossy(&plain).contains("alice"));
    }

    #[test]
    fn reads_plain_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saved_accounts.json");
        std::fs::write(&path, r#"{"accounts":{"bob":{"username":"bob","cookie":"c","user_id":7,"note":"x"}}}"#).unwrap();
        let batch = read_accounts(&path, None, None).unwrap();
        assert_eq!(batch.accounts[0].user_id, 7);
    }
}
