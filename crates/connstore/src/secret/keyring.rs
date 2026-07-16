use keyring::{Entry, Error as KeyringError};

use crate::error::{ConnStoreError, Result};
use crate::secret::SecretBackend;

/// OS keychain backend. Each connection's password is stored as a credential
/// under service `"dbm"` with the connection id as the username.
pub struct KeyringBackend {
    service: String,
}

impl KeyringBackend {
    pub fn new() -> Self {
        KeyringBackend {
            service: "dbm".to_string(),
        }
    }

    fn entry(&self, conn_id: &str) -> Result<Entry> {
        Entry::new(&self.service, conn_id).map_err(|e| ConnStoreError::Secret(e.to_string()))
    }
}

impl Default for KeyringBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretBackend for KeyringBackend {
    fn set(&self, conn_id: &str, password: &str) -> Result<()> {
        self.entry(conn_id)?
            .set_password(password)
            .map_err(|e| ConnStoreError::Secret(e.to_string()))
    }

    fn get(&self, conn_id: &str) -> Result<Option<String>> {
        match self.entry(conn_id)?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(ConnStoreError::Secret(e.to_string())),
        }
    }

    fn delete(&self, conn_id: &str) -> Result<()> {
        match self.entry(conn_id)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(KeyringError::NoEntry) => Ok(()), // idempotent
            Err(e) => Err(ConnStoreError::Secret(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::SecretBackend;

    // Requires a real OS keychain / desktop session. Run locally with:
    //   cargo test -p rdb-connstore keyring -- --ignored
    #[test]
    #[ignore]
    fn keyring_roundtrip_on_a_real_desktop_session() {
        let backend = KeyringBackend::new();
        let id = "rdb-test-keyring-roundtrip";
        backend.set(id, "hunter2").unwrap();
        assert_eq!(backend.get(id).unwrap().as_deref(), Some("hunter2"));
        backend.delete(id).unwrap();
        assert!(backend.get(id).unwrap().is_none());
        // deleting again is idempotent
        backend.delete(id).unwrap();
    }
}
