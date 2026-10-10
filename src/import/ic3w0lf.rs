//! Reads ic3w0lf's Roblox Account Manager `AccountData.json`.
//!
//! Three on-disk forms:
//! * Password-locked: `RAMHeader | salt | nonce | secretbox` (handled in `crypto`).
//! * DPAPI: Windows-protected bytes with a fixed entropy string.
//! * Plain UTF-8 JSON (very old files).
//!
//! The decrypted payload is a JSON array of `{SecurityToken, Username, UserID, Alias,
//! Description, Group, Fields}` objects.

use super::{ImportBatch, ImportedAccount};
use crate::error::{AppError, AppResult};
use crate::store::crypto;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub fn is_password_locked(bytes: &[u8]) -> bool {
    bytes.starts_with(crypto::IC3_HEADER)
}

/// Reads `AccountData.json`. `password` is required only for password-locked files.
pub fn read_accounts(path: &Path, password: Option<&str>) -> AppResult<ImportBatch> {
    let bytes = std::fs::read(path).map_err(|e| AppError::io("Reading AccountData.json", &e))?;
    let plain = decrypt(&bytes, password)?;
    let value: Value = serde_json::from_slice(&plain).map_err(|e| {
        AppError::new("IC3_INVALID", "Not An ic3w0lf File", "This is not a valid Roblox Account Manager file.").with_detail(e.to_string())
    })?;
    Ok(batch_from_array(&value))
}

fn decrypt(bytes: &[u8], password: Option<&str>) -> AppResult<Vec<u8>> {
    if is_password_locked(bytes) {
        let password = password.ok_or_else(|| {
            AppError::new(
                "IC3_LOCKED",
                "Password Needed",
                "This Roblox Account Manager file is password-locked. Enter its password to import.",
            )
        })?;
        return crypto::ic3_password_decrypt(bytes, password);
    }
    // Plain JSON?
    if serde_json::from_slice::<Value>(bytes).is_ok() {
        return Ok(bytes.to_vec());
    }
    // Otherwise DPAPI (CurrentUser scope, fixed entropy) — only works on the original PC.
    crypto::dpapi_unprotect(bytes, crypto::IC3_ENTROPY).map_err(|_| AppError::new(
        "IC3_DPAPI",
        "Tied To Another Computer",
        "This file was encrypted for a different Windows account or computer. Open it in Roblox Account Manager there and set a password, then import that file.",
    ))
}

fn batch_from_array(value: &Value) -> ImportBatch {
    let mut batch = ImportBatch { source: "Roblox Account Manager (ic3w0lf)".into(), ..Default::default() };
    let Some(items) = value.as_array() else {
        batch.skipped += 1;
        return batch;
    };
    for item in items {
        let Some(obj) = item.as_object() else {
            batch.skipped += 1;
            continue;
        };
        let cookie = obj.get("SecurityToken").and_then(Value::as_str).unwrap_or_default().to_owned();
        if cookie.is_empty() {
            batch.skipped += 1;
            continue;
        }
        let mut fields = BTreeMap::new();
        if let Some(map) = obj.get("Fields").and_then(Value::as_object) {
            for (k, v) in map {
                if let Some(text) = v.as_str() {
                    fields.insert(k.clone(), text.to_owned());
                }
            }
        }
        let group = obj.get("Group").and_then(Value::as_str).unwrap_or("Default").to_owned();
        batch.accounts.push(ImportedAccount {
            cookie,
            username: obj.get("Username").and_then(Value::as_str).unwrap_or_default().to_owned(),
            user_id: obj.get("UserID").and_then(super::as_u64).unwrap_or(0),
            password: obj.get("Password").and_then(Value::as_str).unwrap_or_default().to_owned(),
            alias: obj.get("Alias").and_then(Value::as_str).unwrap_or_default().to_owned(),
            note: obj.get("Description").and_then(Value::as_str).unwrap_or_default().to_owned(),
            group: if group == "Default" { String::new() } else { group },
            fields,
            ..Default::default()
        });
    }
    batch
}

#[cfg(test)]
mod tests {
    use super::*;

    const COOKIE: &str =
        "_|WARNING:-DO-NOT-SHARE-THIS.--Sharing-this-will-allow-someone-to-log-in-as-you-and-to-steal-your-ROBUX-and-items.|TOK";

    #[test]
    fn reads_plain_json_array() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("AccountData.json");
        let json = format!(
            r#"[{{"SecurityToken":"{COOKIE}","Username":"bob","UserID":5,"Group":"Mules","Description":"alt","Fields":{{"pin":"1234"}}}}]"#
        );
        std::fs::write(&path, json).unwrap();
        let batch = read_accounts(&path, None).unwrap();
        let a = &batch.accounts[0];
        assert_eq!((a.user_id, a.group.as_str(), a.note.as_str()), (5, "Mules", "alt"));
        assert_eq!(a.fields.get("pin").map(String::as_str), Some("1234"));
    }

    #[test]
    fn detects_password_locked_header() {
        let mut bytes = crypto::IC3_HEADER.to_vec();
        bytes.extend_from_slice(&[0u8; 60]);
        assert!(is_password_locked(&bytes));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("AccountData.json");
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(read_accounts(&path, None).err().unwrap().code, "IC3_LOCKED");
    }
}
