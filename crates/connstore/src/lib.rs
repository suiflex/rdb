//! rdb-connstore: saved connections (JSON, no passwords) + OS keychain secrets
//! with an encrypted-file fallback for headless Linux.

pub mod conn_url;
pub mod error;
pub mod group_path;
pub mod model;
pub mod secret;
pub mod settings;
pub mod store;

pub use conn_url::{parse_conn_url, ConnUrlError, ParsedUrl};
pub use error::{ConnStoreError, Result};
pub use group_path::{
    group_ancestors, group_leaf, group_parent, is_descendant, normalize_group_path,
};
pub use model::{Engine, EnvTag, QueryLanguage, SavedConnection};
pub use secret::{EncryptedFileBackend, KeyringBackend, SecretBackend};
pub use settings::{AppSettings, EditorPrefs, SettingsStore, ThemeMode, UiState};
pub use store::ConnStore;
