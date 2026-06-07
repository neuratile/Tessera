//! AES-256-GCM authenticated encryption for API keys at rest.
//!
//! Per `rules.md` section 9: user API keys are never stored in plaintext.
//! The key material is derived deterministically from the configured
//! `JWT_SECRET`, so local provider configs remain decryptable across app
//! restarts without persisting a second secret on disk.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// Holds the AES-256-GCM key in memory. Managed as Tauri state so commands can
/// borrow it without recomputing the cipher on every call.
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

    /// Derive a stable AES-256-GCM key from the configured JWT signing secret.
    ///
    /// The JWT secret is already required to be high-entropy key material by
    /// `config.rs`, so SHA-256 is sufficient to turn it into the exact
    /// 32-byte key size AES-256-GCM expects without adding another dependency.
    #[must_use]
    pub fn derive_from_secret(secret: &str) -> Self {
        let digest = Sha256::digest(secret.as_bytes());
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(&digest);
        Self::from_bytes(key)
    }

    /// Encrypt `plaintext` and return `(ciphertext, nonce)`.
    ///
    /// Each call generates a fresh random 96-bit nonce. The nonce must be
    /// stored alongside the ciphertext.
    ///
    /// # Errors
    ///
    /// Returns `AppError::Internal` if AES-GCM encryption fails
    /// (should not happen with valid key + nonce).
    pub fn encrypt(&self, plaintext: &[u8]) -> AppResult<(Vec<u8>, Vec<u8>)> {
        // OsRng pulls directly from the OS CSPRNG without the thread-local
        // ReseedingRng layer used by `thread_rng()` — matches the source
        // already used for Argon2 salts in `auth/password.rs` so all
        // cryptographic randomness in this crate comes from the same
        // documented entropy source.
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|error| AppError::Internal(anyhow::anyhow!("encryption failed: {error}")))?;

        Ok((ciphertext, nonce_bytes.to_vec()))
    }

    /// Decrypt `ciphertext` using the provided `nonce`.
    ///
    /// # Errors
    ///
    /// - `AppError::InvalidInput` when nonce length is wrong.
    /// - `AppError::Internal` when decryption fails (wrong key or tampered
    ///   ciphertext - AES-GCM is authenticated).
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
            .map_err(|error| AppError::Internal(anyhow::anyhow!("decryption failed: {error}")))
    }

    /// Decrypt a stored API key into a UTF-8 string. Shared by
    /// `provider_config_service` and `embedding_config_service` so the
    /// decrypt-then-validate-UTF-8 mechanics have one definition.
    ///
    /// # Errors
    ///
    /// Same as [`Self::decrypt`], plus `AppError::Internal` when the
    /// plaintext is not valid UTF-8 (corrupt row or wrong key).
    pub fn decrypt_string(&self, ciphertext: &[u8], nonce_bytes: &[u8]) -> AppResult<String> {
        let plaintext = self.decrypt(ciphertext, nonce_bytes)?;
        String::from_utf8(plaintext)
            .map_err(|_| AppError::Internal(anyhow::anyhow!("decrypted key is not UTF-8")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> CryptoKey {
        CryptoKey::from_bytes([42u8; KEY_LEN])
    }

    #[test]
    fn derive_from_secret_is_stable() {
        let key_one = CryptoKey::derive_from_secret("phase-9-secret");
        let key_two = CryptoKey::derive_from_secret("phase-9-secret");

        let plaintext = b"sk-test-secret-key-12345";
        let (ciphertext, nonce) = key_one.encrypt(plaintext).expect("encrypt");
        let decrypted = key_two.decrypt(&ciphertext, &nonce).expect("decrypt");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn different_secrets_derive_different_keys() {
        let key_one = CryptoKey::derive_from_secret("secret-one");
        let key_two = CryptoKey::derive_from_secret("secret-two");
        let (ciphertext, nonce) = key_one.encrypt(b"secret").expect("encrypt");
        let err = key_two.decrypt(&ciphertext, &nonce).expect_err("must fail");
        assert_eq!(err.code(), "INTERNAL_ERROR");
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
        let (ciphertext_one, _) = key.encrypt(plaintext).expect("encrypt 1");
        let (ciphertext_two, _) = key.encrypt(plaintext).expect("encrypt 2");
        assert_ne!(ciphertext_one, ciphertext_two);
    }

    #[test]
    fn wrong_key_fails_to_decrypt() {
        let key_one = CryptoKey::from_bytes([1u8; KEY_LEN]);
        let key_two = CryptoKey::from_bytes([2u8; KEY_LEN]);
        let (ciphertext, nonce) = key_one.encrypt(b"secret").expect("encrypt");
        let err = key_two.decrypt(&ciphertext, &nonce).expect_err("must fail");
        assert_eq!(err.code(), "INTERNAL_ERROR");
    }

    #[test]
    fn bad_nonce_length_rejected() {
        let key = test_key();
        let (ciphertext, _) = key.encrypt(b"data").expect("encrypt");
        let err = key.decrypt(&ciphertext, &[0u8; 8]).expect_err("must fail");
        assert_eq!(err.code(), "INVALID_INPUT");
    }

    #[test]
    fn debug_redacts_key_material() {
        let key = test_key();
        let debug = format!("{key:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("42"));
    }
}
