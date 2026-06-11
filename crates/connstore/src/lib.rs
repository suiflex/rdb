//! dbm-connstore: saved connections (JSON, no passwords) + OS keychain secrets
//! with an encrypted-file fallback for headless Linux.

pub mod error;
pub mod model;
pub mod secret;
pub mod store;

pub use error::{ConnStoreError, Result};
pub use model::{Engine, SavedConnection};
pub use secret::{EncryptedFileBackend, KeyringBackend, SecretBackend};
pub use store::ConnStore;
