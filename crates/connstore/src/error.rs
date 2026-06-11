use thiserror::Error;

/// All fallible operations in connstore return this error.
#[derive(Error, Debug)]
pub enum ConnStoreError {
    #[error("connection not found: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("secret backend error: {0}")]
    Secret(String),
    #[error("no config directory available on this platform")]
    NoConfigDir,
}

pub type Result<T> = std::result::Result<T, ConnStoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_message_includes_id() {
        let e = ConnStoreError::NotFound("abc".into());
        assert_eq!(e.to_string(), "connection not found: abc");
    }

    #[test]
    fn secret_message_includes_detail() {
        let e = ConnStoreError::Secret("locked".into());
        assert_eq!(e.to_string(), "secret backend error: locked");
    }
}
