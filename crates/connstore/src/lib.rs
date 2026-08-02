//! rdb-connstore: saved connections (JSON, no passwords) + OS keychain secrets
//! with an encrypted-file fallback for headless Linux.

pub mod conn_url;
pub mod error;
pub mod model;
pub mod secret;
pub mod settings;
pub mod store;

pub use conn_url::{parse_conn_url, ConnUrlError, ParsedUrl};
pub use error::{ConnStoreError, Result};
pub use model::{Engine, EnvTag, SavedConnection};
pub use secret::{EncryptedFileBackend, KeyringBackend, SecretBackend};
pub use settings::{AppSettings, EditorPrefs, SettingsStore, ThemeMode, UiState};
pub use store::ConnStore;
