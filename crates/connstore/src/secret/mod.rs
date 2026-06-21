//! Secret storage abstraction. Two backends:
//! - `KeyringBackend`: OS keychain (macOS Keychain, Windows Credential Manager,
//!   Linux Secret Service). Preferred.
//! - `EncryptedFileBackend`: AES-GCM file, used when Secret Service is absent
//!   (headless Linux). See its module docs for the security tradeoff.

mod file;
mod keyring;

pub use file::EncryptedFileBackend;
pub use keyring::KeyringBackend;

use crate::error::Result;

/// Stores/retrieves a single password per connection id. Implementations must
/// treat `delete` of a missing entry as success (idempotent).
pub trait SecretBackend {
    fn set(&self, conn_id: &str, password: &str) -> Result<()>;
    fn get(&self, conn_id: &str) -> Result<Option<String>>;
    fn delete(&self, conn_id: &str) -> Result<()>;
}

use std::path::Path;

/// Pick a secret backend at runtime: prefer the OS keychain, fall back to the
/// encrypted file when the keychain is unavailable (e.g. headless Linux with no
/// Secret Service provider). `fallback_dir` is where the encrypted file lives
/// when the fallback is taken.
pub fn select_backend(fallback_dir: &Path) -> crate::error::Result<Box<dyn SecretBackend>> {
    let keyring = KeyringBackend::new();
    // Full set->get->delete roundtrip probe. A bare `get` is not enough: on some
    // environments (e.g. an unsigned macOS binary) `set` reports success but the
    // value never persists, so `get` keeps returning None and every saved
    // password is silently lost. Only trust the keychain if a written sentinel
    // reads back identical; otherwise fall back to the encrypted file.
    const PROBE_ID: &str = "__dbm_probe__";
    const PROBE_VAL: &str = "dbm-probe-value";
    let keyring_ok = keyring.set(PROBE_ID, PROBE_VAL).is_ok()
        && matches!(keyring.get(PROBE_ID), Ok(Some(v)) if v == PROBE_VAL);
    let _ = keyring.delete(PROBE_ID);
    if keyring_ok {
        Ok(Box::new(keyring))
    } else {
        Ok(Box::new(EncryptedFileBackend::new(fallback_dir)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_dir_backend_is_usable() {
        // We cannot assume a keyring exists in CI, so test the file fallback
        // path the selector returns when the keyring probe fails.
        let dir = tempfile::tempdir().unwrap();
        let backend: Box<dyn SecretBackend> =
            Box::new(EncryptedFileBackend::new(dir.path()).unwrap());
        backend.set("c1", "pw").unwrap();
        assert_eq!(backend.get("c1").unwrap().as_deref(), Some("pw"));
    }

    #[test]
    fn select_backend_returns_a_backend_that_roundtrips() {
        // Whatever backend is chosen (keyring or file fallback) must actually
        // persist a password and read it back — guards against selecting a
        // backend whose `set` silently no-ops.
        let dir = tempfile::tempdir().unwrap();
        let backend = select_backend(dir.path()).unwrap();
        backend.set("rt", "secret-value").unwrap();
        assert_eq!(backend.get("rt").unwrap().as_deref(), Some("secret-value"));
        backend.delete("rt").unwrap();
    }
}
