//! AES-256-GCM authenticated encryption for API keys at rest.
//!
//! Per `rules.md` §9: user API keys are never stored in plaintext.
//! This module provides encrypt/decrypt using a 256-bit key generated
//! on first launch and stored in the app data directory.

use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;

use crate::error::{AppError, AppResult};

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const KEY_FILENAME: &str = "encryption.key";

/// Holds the AES-256-GCM key in memory. Managed as Tauri state so
/// commands can borrow it without filesystem round-trips.
pub struct CryptoKey {
    cipher: Aes256Gcm,
}

impl std::fmt::Debug for CryptoKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptoKey")
            .field("cipher", &"[REDACTED]")
            .finish()
    }
}

impl CryptoKey {
    /// Build from raw 32-byte key material.
    #[must_use]
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        let key = Key::<Aes256Gcm>::from(bytes);
        Self {
            cipher: Aes256Gcm::new(&key),
        }
    }

    /// Load the key from `<dir>/encryption.key`, or generate + persist
    /// a new one if the file does not exist.
    ///
    /// # Errors
    ///
    /// - `AppError::Io` when the data directory cannot be created or
    ///   the key file cannot be read/written.
    /// - `AppError::Config` when an existing key file has wrong length.
    pub fn load_or_generate(data_dir: &Path) -> AppResult<Self> {
        let path = data_dir.join(KEY_FILENAME);
        if path.exists() {
            let bytes = fs::read(&path)?;
            let key: [u8; KEY_LEN] = bytes.try_into().map_err(|_| {
                AppError::Config(format!(
                    "encryption key file has wrong length (expected {KEY_LEN} bytes)"
                ))
            })?;
            Ok(Self::from_bytes(key))
        } else {
            fs::create_dir_all(data_dir)?;
            let mut key = [0u8; KEY_LEN];
            rand::thread_rng().fill_bytes(&mut key);
            fs::write(&path, key)?;
            Ok(Self::from_bytes(key))
        }
    }

    /// Resolve the key file path from a Tauri app handle.
    pub fn key_file_path(data_dir: &Path) -> PathBuf {
        data_dir.join(KEY_FILENAME)
    }

    /// Encrypt `plaintext` and return `(ciphertext, nonce)`.
    ///
    /// Each call generates a fresh random 96-bit nonce. The nonce must
    /// be stored alongside the ciphertext (e.g. in
    /// `user_provider_configs.api_key_nonce`).
    ///
    /// # Errors
    ///
    /// Returns `AppError::Internal` if AES-GCM encryption fails
    /// (should not happen with valid key + nonce).
    pub fn encrypt(&self, plaintext: &[u8]) -> AppResult<(Vec<u8>, Vec<u8>)> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("encryption failed: {e}")))?;

        Ok((ciphertext, nonce_bytes.to_vec()))
    }

    /// Decrypt `ciphertext` using the provided `nonce`.
    ///
    /// # Errors
    ///
    /// - `AppError::InvalidInput` when nonce length is wrong.
    /// - `AppError::Internal` when decryption fails (wrong key or
    ///   tampered ciphertext — AES-GCM is authenticated).
    pub fn decrypt(&self, ciphertext: &[u8], nonce_bytes: &[u8]) -> AppResult<Vec<u8>> {
        if nonce_bytes.len() != NONCE_LEN {
            return Err(AppError::InvalidInput(format!(
                "nonce must be {NONCE_LEN} bytes, got {}",
                nonce_bytes.len()
            )));
        }
        let nonce = Nonce::from_slice(nonce_bytes);
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("decryption failed: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> CryptoKey {
        CryptoKey::from_bytes([42u8; KEY_LEN])
    }

    #[test]
    fn encrypt_decrypt_round_trips() {
        let key = test_key();
        let plaintext = b"sk-test-secret-key-12345";
        let (ciphertext, nonce) = key.encrypt(plaintext).expect("encrypt");
        let decrypted = key.decrypt(&ciphertext, &nonce).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn ciphertext_differs_from_plaintext() {
        let key = test_key();
        let plaintext = b"sk-test-secret-key-12345";
        let (ciphertext, _) = key.encrypt(plaintext).expect("encrypt");
        assert_ne!(ciphertext, plaintext);
    }

    #[test]
    fn different_nonces_produce_different_ciphertext() {
        let key = test_key();
        let plaintext = b"same-input";
        let (ct1, _) = key.encrypt(plaintext).expect("encrypt 1");
        let (ct2, _) = key.encrypt(plaintext).expect("encrypt 2");
        assert_ne!(ct1, ct2);
    }

    #[test]
    fn wrong_key_fails_to_decrypt() {
        let key1 = CryptoKey::from_bytes([1u8; KEY_LEN]);
        let key2 = CryptoKey::from_bytes([2u8; KEY_LEN]);
        let (ciphertext, nonce) = key1.encrypt(b"secret").expect("encrypt");
        let err = key2.decrypt(&ciphertext, &nonce).expect_err("must fail");
        assert_eq!(err.code(), "INTERNAL_ERROR");
    }

    #[test]
    fn bad_nonce_length_rejected() {
        let key = test_key();
        let (ciphertext, _) = key.encrypt(b"data").expect("encrypt");
        let err = key
            .decrypt(&ciphertext, &[0u8; 8])
            .expect_err("must fail");
        assert_eq!(err.code(), "INVALID_INPUT");
    }

    #[test]
    fn load_or_generate_creates_and_reloads_key() {
        let dir = std::env::temp_dir().join(format!("crypto-test-{}", uuid::Uuid::new_v4()));
        let key1 = CryptoKey::load_or_generate(&dir).expect("first load");
        let key2 = CryptoKey::load_or_generate(&dir).expect("second load");

        let plaintext = b"round-trip test";
        let (ct, nonce) = key1.encrypt(plaintext).expect("encrypt");
        let decrypted = key2.decrypt(&ct, &nonce).expect("decrypt with reloaded key");
        assert_eq!(decrypted, plaintext);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn debug_redacts_key_material() {
        let key = test_key();
        let debug = format!("{key:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("42"));
    }
}
