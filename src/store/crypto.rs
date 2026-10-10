//! Encryption primitives.
//!
//! * New vaults: Windows DPAPI (tied to the Windows account) or a password
//!   (Argon2id → AES-256-GCM).
//! * Compatibility readers for Evanovar RAM (PBKDF2-SHA1 → AES-GCM with 16-byte
//!   nonces) and ic3w0lf's Roblox Account Manager (DPAPI with fixed entropy, or
//!   SHA-512 → Argon2 → XSalsa20-Poly1305).

use crate::error::{AppError, AppResult};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, AesGcm, Nonce, aes::Aes256};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha1::Sha1;

/// Entropy mixed into Nova's own DPAPI blobs.
const NOVA_DPAPI_ENTROPY: &[u8] = b"NovaRAM vault v1";

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    rand::rng().fill_bytes(&mut out);
    out
}

pub fn b64(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn unb64(text: &str) -> AppResult<Vec<u8>> {
    B64.decode(text.trim())
        .map_err(|e| AppError::new("DATA_MALFORMED", "Damaged Data", "Encrypted data is not valid base64.").with_detail(e.to_string()))
}

fn decrypt_failed() -> AppError {
    AppError::new("DECRYPT_FAILED", "Could Not Decrypt", "The data could not be decrypted with this key.")
}

// ---------- DPAPI ----------

#[cfg(windows)]
pub fn dpapi_protect(plain: &[u8], entropy: &[u8]) -> AppResult<Vec<u8>> {
    dpapi_call(plain, entropy, true)
}

#[cfg(windows)]
pub fn dpapi_unprotect(blob: &[u8], entropy: &[u8]) -> AppResult<Vec<u8>> {
    dpapi_call(blob, entropy, false)
}

#[cfg(windows)]
fn dpapi_call(input: &[u8], entropy: &[u8], protect: bool) -> AppResult<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    let data_in = CRYPT_INTEGER_BLOB { cbData: input.len() as u32, pbData: input.as_ptr() as *mut u8 };
    let entropy_blob = CRYPT_INTEGER_BLOB { cbData: entropy.len() as u32, pbData: entropy.as_ptr() as *mut u8 };
    let entropy_ptr = if entropy.is_empty() { std::ptr::null() } else { &entropy_blob as *const _ };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: all blobs point at live buffers for the duration of the call; the output
    // buffer is allocated by Windows and released with LocalFree below.
    let ok = unsafe {
        if protect {
            CryptProtectData(&data_in, std::ptr::null(), entropy_ptr, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&data_in, std::ptr::null_mut(), entropy_ptr, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        }
    };
    if ok == 0 || out.pbData.is_null() {
        let code = std::io::Error::last_os_error();
        return Err(if protect {
            AppError::new("DPAPI_FAILED", "Windows Encryption Failed", "Windows could not encrypt the data.").with_detail(code.to_string())
        } else {
            AppError::new("DPAPI_DECRYPT_FAILED", "Windows Decryption Failed", "This data was encrypted for a different Windows account or computer.").with_detail(code.to_string())
        });
    }
    // SAFETY: Windows returned a buffer of cbData bytes.
    let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
    unsafe { LocalFree(out.pbData as _) };
    Ok(bytes)
}

pub fn nova_dpapi_protect(plain: &[u8]) -> AppResult<Vec<u8>> {
    dpapi_protect(plain, NOVA_DPAPI_ENTROPY)
}

pub fn nova_dpapi_unprotect(blob: &[u8]) -> AppResult<Vec<u8>> {
    dpapi_unprotect(blob, NOVA_DPAPI_ENTROPY)
}

// ---------- Password vaults (Argon2id + AES-256-GCM) ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub alg: String,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub salt: String,
}

impl KdfParams {
    pub fn fresh() -> Self {
        Self { alg: "argon2id".into(), m_kib: 64 * 1024, t: 3, p: 1, salt: b64(&random_bytes::<16>()) }
    }

    pub fn derive(&self, password: &str) -> AppResult<[u8; 32]> {
        if self.alg != "argon2id" {
            return Err(AppError::new("KDF_UNSUPPORTED", "Unsupported Vault", "This vault uses an unknown key derivation."));
        }
        let salt = unb64(&self.salt)?;
        argon2_raw(Algorithm::Argon2id, password.as_bytes(), &salt, self.m_kib, self.t, self.p)
    }
}

fn argon2_raw(alg: Algorithm, password: &[u8], salt: &[u8], m_kib: u32, t: u32, p: u32) -> AppResult<[u8; 32]> {
    let params = Params::new(m_kib, t, p, Some(32))
        .map_err(|e| AppError::unexpected("argon2 params", e))?;
    let mut key = [0u8; 32];
    Argon2::new(alg, Version::V0x13, params)
        .hash_password_into(password, salt, &mut key)
        .map_err(|e| AppError::unexpected("argon2", e))?;
    Ok(key)
}

/// AES-256-GCM with a random 12-byte nonce. Returns (nonce, ciphertext||tag).
pub fn aes_encrypt(key: &[u8; 32], plain: &[u8]) -> AppResult<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| AppError::unexpected("aes key", e))?;
    let nonce = random_bytes::<12>();
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plain)
        .map_err(|e| AppError::unexpected("aes encrypt", e))?;
    Ok((nonce.to_vec(), ct))
}

pub fn aes_decrypt(key: &[u8; 32], nonce: &[u8], ct: &[u8]) -> AppResult<Vec<u8>> {
    if nonce.len() != 12 {
        return Err(decrypt_failed());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| AppError::unexpected("aes key", e))?;
    cipher.decrypt(Nonce::from_slice(nonce), ct).map_err(|_| decrypt_failed())
}

// ---------- Evanovar RAM compatibility ----------

type Aes256Gcm16 = AesGcm<Aes256, aes_gcm::aead::consts::U16>;

/// PBKDF2-HMAC-SHA1, 100 000 rounds, 32 bytes (PyCryptodome's defaults).
pub fn evanovar_pbkdf2(secret: &[u8], salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha1>(secret, salt, 100_000, &mut key);
    key
}

/// Decrypts a `{nonce, tag, ciphertext}` package written by PyCryptodome AES-GCM.
pub fn evanovar_decrypt(key: &[u8; 32], nonce: &[u8], tag: &[u8], ciphertext: &[u8]) -> AppResult<Vec<u8>> {
    if nonce.len() != 16 || tag.len() != 16 {
        return Err(decrypt_failed());
    }
    let cipher = Aes256Gcm16::new_from_slice(key).map_err(|e| AppError::unexpected("aes key", e))?;
    let mut joined = ciphertext.to_vec();
    joined.extend_from_slice(tag);
    cipher
        .decrypt(aes_gcm::aead::generic_array::GenericArray::from_slice(nonce), joined.as_slice())
        .map_err(|_| decrypt_failed())
}

/// Python's `base64.b64decode(text)` without `validate`: characters outside the
/// alphabet are dropped before decoding. Evanovar stores hex salts and decodes them this way.
pub fn python_lenient_b64(text: &str) -> AppResult<Vec<u8>> {
    let filtered: String = text
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
        .collect();
    let trimmed = filtered.trim_end_matches('=');
    let padded = format!("{trimmed}{}", "=".repeat((4 - trimmed.len() % 4) % 4));
    unb64(&padded)
}

// ---------- ic3w0lf RAM compatibility ----------

/// `ROBLOX ACCOUNT MANAGER | :) | BROUGHT TO YOU BUY ic3w0lf`
pub const IC3_ENTROPY: &[u8] = b"ROBLOX ACCOUNT MANAGER | :) | BROUGHT TO YOU BUY ic3w0lf";
/// Header of password-locked `AccountData.json` files.
pub const IC3_HEADER: &[u8] = b"Roblox Account Manager created by ic3w0lf22 @ github.com .......";

/// Opens a password-locked ic3w0lf file: header | salt(16) | nonce(24) | secretbox.
/// The key is Argon2 ("moderate") of SHA-512(password); both Argon2i and Argon2id
/// parameter sets are tried because libsodium-net's default changed between releases.
pub fn ic3_password_decrypt(file: &[u8], password: &str) -> AppResult<Vec<u8>> {
    use crypto_secretbox::{XSalsa20Poly1305, aead::Aead as _, aead::KeyInit as _};
    use sha2::{Digest, Sha512};
    let body = file.strip_prefix(IC3_HEADER).ok_or_else(decrypt_failed)?;
    if body.len() < 16 + 24 + 16 {
        return Err(decrypt_failed());
    }
    let (salt, rest) = body.split_at(16);
    let (nonce, boxed) = rest.split_at(24);
    let hash = Sha512::digest(password.as_bytes());
    let attempts = [
        (Algorithm::Argon2i, 128 * 1024, 6),
        (Algorithm::Argon2id, 256 * 1024, 3),
    ];
    for (alg, m_kib, t) in attempts {
        let key = argon2_raw(alg, &hash, salt, m_kib, t, 1)?;
        let cipher = XSalsa20Poly1305::new_from_slice(&key).map_err(|e| AppError::unexpected("secretbox key", e))?;
        if let Ok(plain) = cipher.decrypt(crypto_secretbox::Nonce::from_slice(nonce), boxed) {
            return Ok(plain);
        }
    }
    Err(AppError::new("PASSWORD_INVALID", "Wrong Password", "The password did not unlock this file."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_round_trip() {
        let key = random_bytes::<32>();
        let (nonce, ct) = aes_encrypt(&key, b"hello").unwrap();
        assert_eq!(aes_decrypt(&key, &nonce, &ct).unwrap(), b"hello");
        let other = random_bytes::<32>();
        assert!(aes_decrypt(&other, &nonce, &ct).is_err());
    }

    #[test]
    fn dpapi_round_trip() {
        let blob = nova_dpapi_protect(b"secret").unwrap();
        assert_eq!(nova_dpapi_unprotect(&blob).unwrap(), b"secret");
        assert!(dpapi_unprotect(&blob, b"wrong entropy").is_err());
    }

    #[test]
    fn kdf_is_deterministic_per_salt() {
        let params = KdfParams { m_kib: 1024, t: 1, ..KdfParams::fresh() };
        assert_eq!(params.derive("pw").unwrap(), params.derive("pw").unwrap());
        assert_ne!(params.derive("pw").unwrap(), params.derive("pw2").unwrap());
    }

    #[test]
    fn lenient_b64_matches_python_for_hex_salts() {
        // python: base64.b64decode("00ff"*16) -> 48 bytes
        assert_eq!(python_lenient_b64(&"00ff".repeat(16)).unwrap().len(), 48);
    }
}
