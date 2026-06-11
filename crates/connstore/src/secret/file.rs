//! Encrypted-file secret backend (Linux Secret Service fallback).
//!
//! SECURITY TRADEOFF: secrets are AES-256-GCM encrypted under a key stored in a
//! `0600` key file alongside the encrypted blob. A local attacker who can read
//! the user's files can read the key and decrypt the secrets. This is strictly
//! WEAKER than an OS keychain and exists only so the app keeps working on a
//! headless Linux box with no Secret Service provider — never as an equal peer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;

use crate::error::{ConnStoreError, Result};
use crate::secret::SecretBackend;

const KEY_FILE: &str = "secret.key";
const STORE_FILE: &str = "secrets.enc";
const NONCE_LEN: usize = 12;

/// AES-GCM file backend. The on-disk format of `secrets.enc` is:
/// `nonce (12 bytes) || ciphertext`, where the plaintext is a JSON map of
/// `conn_id -> password`.
pub struct EncryptedFileBackend {
    key_path: PathBuf,
    store_path: PathBuf,
}

impl EncryptedFileBackend {
    /// Create/open a backend rooted at `dir`. Generates a fresh 32-byte key on
    /// first use and writes it with `0600` perms (best-effort on non-unix).
    pub fn new(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let backend = EncryptedFileBackend {
            key_path: dir.join(KEY_FILE),
            store_path: dir.join(STORE_FILE),
        };
        backend.ensure_key()?;
        Ok(backend)
    }

    fn ensure_key(&self) -> Result<()> {
        if self.key_path.exists() {
            return Ok(());
        }
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        std::fs::write(&self.key_path, key)?;
        self.restrict_perms(&self.key_path)?;
        Ok(())
    }

    #[cfg(unix)]
    fn restrict_perms(&self, path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn restrict_perms(&self, _path: &Path) -> Result<()> {
        // Windows/macOS file ACLs default to user-only for files in the user
        // config dir; no portable chmod equivalent needed here.
        Ok(())
    }

    fn cipher(&self) -> Result<Aes256Gcm> {
        let bytes = std::fs::read(&self.key_path)?;
        if bytes.len() != 32 {
            return Err(ConnStoreError::Secret("corrupt key file".into()));
        }
        let key = Key::<Aes256Gcm>::from_slice(&bytes);
        Ok(Aes256Gcm::new(key))
    }

    fn load(&self) -> Result<HashMap<String, String>> {
        if !self.store_path.exists() {
            return Ok(HashMap::new());
        }
        let raw = std::fs::read(&self.store_path)?;
        if raw.len() < NONCE_LEN {
            return Err(ConnStoreError::Secret("corrupt secrets file".into()));
        }
        let (nonce_bytes, ciphertext) = raw.split_at(NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);
        let plaintext = self
            .cipher()?
            .decrypt(nonce, ciphertext)
            .map_err(|_| ConnStoreError::Secret("decryption failed".into()))?;
        let map = serde_json::from_slice(&plaintext)?;
        Ok(map)
    }

    fn save(&self, map: &HashMap<String, String>) -> Result<()> {
        let plaintext = serde_json::to_vec(map)?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher()?
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|_| ConnStoreError::Secret("encryption failed".into()))?;
        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        std::fs::write(&self.store_path, out)?;
        self.restrict_perms(&self.store_path)?;
        Ok(())
    }
}

impl SecretBackend for EncryptedFileBackend {
    fn set(&self, conn_id: &str, password: &str) -> Result<()> {
        let mut map = self.load()?;
        map.insert(conn_id.to_string(), password.to_string());
        self.save(&map)
    }

    fn get(&self, conn_id: &str) -> Result<Option<String>> {
        Ok(self.load()?.get(conn_id).cloned())
    }

    fn delete(&self, conn_id: &str) -> Result<()> {
        let mut map = self.load()?;
        if map.remove(conn_id).is_some() {
            self.save(&map)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::SecretBackend;

    #[test]
    fn file_backend_roundtrips_set_get_delete() {
        let dir = tempfile::tempdir().unwrap();
        let backend = EncryptedFileBackend::new(dir.path()).unwrap();

        assert!(backend.get("c1").unwrap().is_none());
        backend.set("c1", "hunter2").unwrap();
        backend.set("c2", "other").unwrap();
        assert_eq!(backend.get("c1").unwrap().as_deref(), Some("hunter2"));
        assert_eq!(backend.get("c2").unwrap().as_deref(), Some("other"));

        backend.delete("c1").unwrap();
        assert!(backend.get("c1").unwrap().is_none());
        assert_eq!(backend.get("c2").unwrap().as_deref(), Some("other"));
        // deleting a missing entry is idempotent
        backend.delete("c1").unwrap();
    }

    #[test]
    fn secrets_persist_across_backend_instances() {
        let dir = tempfile::tempdir().unwrap();
        {
            let b = EncryptedFileBackend::new(dir.path()).unwrap();
            b.set("c1", "persisted").unwrap();
        }
        let b2 = EncryptedFileBackend::new(dir.path()).unwrap();
        assert_eq!(b2.get("c1").unwrap().as_deref(), Some("persisted"));
    }

    #[test]
    fn ciphertext_does_not_contain_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let backend = EncryptedFileBackend::new(dir.path()).unwrap();
        backend.set("c1", "topsecret").unwrap();
        let raw = std::fs::read(dir.path().join("secrets.enc")).unwrap();
        assert!(!raw.windows(9).any(|w| w == b"topsecret"));
    }
}
