use std::path::PathBuf;

use directories::ProjectDirs;

use crate::error::{ConnStoreError, Result};
use crate::model::SavedConnection;
use crate::secret::SecretBackend;

/// Owns the saved-connections JSON file and the secret backend used for
/// passwords. Connections are held in memory and flushed to disk on each
/// mutation (the set is small — tens of entries — so full rewrite is fine).
pub struct ConnStore {
    path: PathBuf,
    conns: Vec<SavedConnection>,
    secrets: Box<dyn SecretBackend>,
}

impl ConnStore {
    /// Construct with an explicit path and secret backend (used by tests and by
    /// callers that select a backend at runtime). Starts with an empty set.
    pub fn new(path: PathBuf, secrets: Box<dyn SecretBackend>) -> Self {
        ConnStore {
            path,
            conns: Vec::new(),
            secrets,
        }
    }

    /// Construct and load existing connections from `path` (empty set if the
    /// file does not exist yet).
    pub fn load(path: PathBuf, secrets: Box<dyn SecretBackend>) -> Result<Self> {
        let conns = if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            serde_json::from_str(&raw)?
        } else {
            Vec::new()
        };
        Ok(ConnStore {
            path,
            conns,
            secrets,
        })
    }

    /// Default location of `connections.json`: the platform config dir for
    /// qualifier `dev`, org `dbm`, app `dbm`.
    pub fn default_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("dev", "dbm", "dbm").ok_or(ConnStoreError::NoConfigDir)?;
        Ok(dirs.config_dir().join("connections.json"))
    }

    /// Location of `recent_queries.json` in the same config dir as the
    /// connection store — the on-disk cap-limited query history.
    pub fn recent_queries_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("dev", "dbm", "dbm").ok_or(ConnStoreError::NoConfigDir)?;
        Ok(dirs.config_dir().join("recent_queries.json"))
    }

    /// Location of `query_tabs.json` in the same config dir — the persisted
    /// open query tabs and their SQL text, restored on launch.
    pub fn query_tabs_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("dev", "dbm", "dbm").ok_or(ConnStoreError::NoConfigDir)?;
        Ok(dirs.config_dir().join("query_tabs.json"))
    }

    /// Open the store at the platform default path with the runtime-selected
    /// secret backend (keychain, or encrypted-file fallback). Convenience
    /// wrapper over `default_path` + `secret::select_backend` + `load`.
    pub fn open_default() -> Result<Self> {
        let path = Self::default_path()?;
        let dir = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let backend = crate::secret::select_backend(&dir)?;
        Self::load(path, backend)
    }

    fn flush(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.conns)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.conns.iter().position(|c| c.id == id)
    }

    pub fn list(&self) -> &[SavedConnection] {
        &self.conns
    }

    pub fn get(&self, id: &str) -> Option<&SavedConnection> {
        self.conns.iter().find(|c| c.id == id)
    }

    pub fn add(&mut self, conn: SavedConnection) -> Result<()> {
        self.conns.push(conn);
        self.flush()
    }

    pub fn update(&mut self, conn: SavedConnection) -> Result<()> {
        match self.index_of(&conn.id) {
            Some(i) => {
                self.conns[i] = conn;
                self.flush()
            }
            None => Err(ConnStoreError::NotFound(conn.id)),
        }
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        match self.index_of(id) {
            Some(i) => {
                self.conns.remove(i);
                self.flush()
            }
            None => Err(ConnStoreError::NotFound(id.to_string())),
        }
    }

    /// Toggle/set the starred flag on a connection.
    pub fn set_favorite(&mut self, id: &str, favorite: bool) -> Result<()> {
        match self.index_of(id) {
            Some(i) => {
                self.conns[i].favorite = favorite;
                self.flush()
            }
            None => Err(ConnStoreError::NotFound(id.to_string())),
        }
    }

    /// Move a connection to `new_index` in the list and renumber every
    /// connection's `order` to its new position, so the stored order is the
    /// source of truth for the UI.
    ///
    /// `new_index` is clamped to the valid range.
    // ponytail: renumber-all on each move; O(n), fine for a hand-managed list.
    pub fn reorder(&mut self, id: &str, new_index: usize) -> Result<()> {
        let from = self
            .index_of(id)
            .ok_or_else(|| ConnStoreError::NotFound(id.to_string()))?;
        let to = new_index.min(self.conns.len() - 1);
        let conn = self.conns.remove(from);
        self.conns.insert(to, conn);
        for (i, c) in self.conns.iter_mut().enumerate() {
            c.order = i as i64;
        }
        self.flush()
    }

    /// Store `password` in the secret backend keyed by the connection id.
    /// Errors if the connection id is unknown.
    pub fn set_password(&self, id: &str, password: &str) -> Result<()> {
        self.index_of(id)
            .ok_or_else(|| ConnStoreError::NotFound(id.to_string()))?;
        self.secrets.set(id, password)
    }

    /// Fetch the password for a connection from the secret backend, if any.
    pub fn get_password(&self, id: &str) -> Result<Option<String>> {
        self.secrets.get(id)
    }

    /// Remove the password from the secret backend.
    /// Idempotent: succeeds even if no password was stored.
    pub fn delete_password(&self, id: &str) -> Result<()> {
        self.secrets.delete(id)
    }

    /// Build a `rdb-core::ConnConfig` for a saved connection with its stored
    /// password injected. Errors if the connection id is unknown.
    pub fn conn_config_for(&self, id: &str) -> Result<rdb_core::conn::ConnConfig> {
        let conn = self
            .get(id)
            .ok_or_else(|| ConnStoreError::NotFound(id.to_string()))?;
        let password = self.get_password(id)?;
        Ok(conn.to_conn_config(password))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Engine, SavedConnection};

    fn temp_store() -> (tempfile::TempDir, ConnStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.json");
        let secrets = Box::new(crate::secret::EncryptedFileBackend::new(dir.path()).unwrap());
        let store = ConnStore::new(path, secrets);
        (dir, store)
    }

    fn pg(name: &str) -> SavedConnection {
        SavedConnection::new(name, Engine::Postgres, "localhost", 5432, "postgres")
    }

    #[test]
    fn add_list_get_update_remove() {
        let (_dir, mut store) = temp_store();
        assert_eq!(store.list().len(), 0);

        let c = pg("one");
        let id = c.id.clone();
        store.add(c).unwrap();
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.get(&id).unwrap().name, "one");

        let mut updated = store.get(&id).unwrap().clone();
        updated.name = "renamed".into();
        store.update(updated).unwrap();
        assert_eq!(store.get(&id).unwrap().name, "renamed");

        store.remove(&id).unwrap();
        assert!(store.get(&id).is_none());
        assert_eq!(store.list().len(), 0);
    }

    #[test]
    fn persists_to_disk_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.json");
        let id;
        {
            let secrets = Box::new(crate::secret::EncryptedFileBackend::new(dir.path()).unwrap());
            let mut store = ConnStore::new(path.clone(), secrets);
            let c = pg("persisted");
            id = c.id.clone();
            store.add(c).unwrap();
        }
        let secrets = Box::new(crate::secret::EncryptedFileBackend::new(dir.path()).unwrap());
        let store = ConnStore::load(path, secrets).unwrap();
        assert_eq!(store.get(&id).unwrap().name, "persisted");
    }

    #[test]
    fn set_favorite_toggles_flag() {
        let (_dir, mut store) = temp_store();
        let c = pg("star me");
        let id = c.id.clone();
        store.add(c).unwrap();
        assert!(!store.get(&id).unwrap().favorite);

        store.set_favorite(&id, true).unwrap();
        assert!(store.get(&id).unwrap().favorite);

        store.set_favorite(&id, false).unwrap();
        assert!(!store.get(&id).unwrap().favorite);
    }

    #[test]
    fn reorder_moves_and_renumbers_order() {
        let (_dir, mut store) = temp_store();
        let (a, b, c) = (pg("a"), pg("b"), pg("c"));
        let (ia, ic) = (a.id.clone(), c.id.clone());
        store.add(a).unwrap();
        store.add(b).unwrap();
        store.add(c).unwrap();

        // Move "c" (index 2) to the front.
        store.reorder(&ic, 0).unwrap();

        let list = store.list();
        assert_eq!(list[0].id, ic);
        assert_eq!(list[1].id, ia);
        // order rewritten to match position.
        for (i, conn) in list.iter().enumerate() {
            assert_eq!(conn.order, i as i64);
        }
    }

    #[test]
    fn reorder_clamps_out_of_range_index() {
        let (_dir, mut store) = temp_store();
        let (a, b) = (pg("a"), pg("b"));
        let ia = a.id.clone();
        store.add(a).unwrap();
        store.add(b).unwrap();

        store.reorder(&ia, 99).unwrap();
        assert_eq!(store.list()[1].id, ia);
    }

    #[test]
    fn update_missing_id_is_not_found() {
        let (_dir, mut store) = temp_store();
        let err = store.update(pg("ghost")).unwrap_err();
        assert!(matches!(err, ConnStoreError::NotFound(_)));
    }

    #[test]
    fn saved_json_has_no_password_field() {
        let (dir, mut store) = temp_store();
        store.add(pg("one")).unwrap();
        let raw = std::fs::read_to_string(dir.path().join("connections.json")).unwrap();
        assert!(!raw.contains("password"));
    }

    #[test]
    fn set_get_then_delete_password() {
        let (_dir, mut store) = temp_store();
        let c = pg("secured");
        let id = c.id.clone();
        store.add(c).unwrap();

        assert!(store.get_password(&id).unwrap().is_none());

        store.set_password(&id, "hunter2").unwrap();
        assert_eq!(store.get_password(&id).unwrap().as_deref(), Some("hunter2"));

        store.delete_password(&id).unwrap();
        assert!(store.get_password(&id).unwrap().is_none());
    }

    #[test]
    fn set_password_on_missing_connection_is_not_found() {
        let (_dir, store) = temp_store();
        let err = store.set_password("nope", "x").unwrap_err();
        assert!(matches!(err, ConnStoreError::NotFound(_)));
    }

    #[test]
    fn conn_config_for_injects_stored_password() {
        let (_dir, mut store) = temp_store();
        let c = pg("c");
        let id = c.id.clone();
        store.add(c).unwrap();
        store.set_password(&id, "pw").unwrap();
        let cfg = store.conn_config_for(&id).unwrap();
        assert_eq!(cfg.password.as_deref(), Some("pw"));
        assert_eq!(cfg.port, 5432);
    }

    #[test]
    fn open_default_returns_a_store_or_clean_error() {
        // We cannot assume a writable config dir / keychain in CI, so we only
        // assert the call resolves to a Result without panicking and, on Ok,
        // yields a usable (possibly empty) list.
        match ConnStore::open_default() {
            Ok(store) => {
                let _ = store.list().len();
            }
            Err(_) => { /* acceptable on headless CI */ }
        }
    }
}
