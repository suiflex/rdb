use thiserror::Error;

/// All fallible operations in dbm return this error.
#[derive(Error, Debug)]
pub enum DbmError {
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("query failed: {0}")]
    Query(String),
    #[error("unsupported query for this driver")]
    UnsupportedQuery,
    #[error("schema error: {0}")]
    Schema(String),
}

pub type Result<T> = std::result::Result<T, DbmError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_query_has_stable_message() {
        let e = DbmError::UnsupportedQuery;
        assert_eq!(e.to_string(), "unsupported query for this driver");
    }

    #[test]
    fn connection_error_includes_detail() {
        let e = DbmError::Connection("refused".into());
        assert_eq!(e.to_string(), "connection failed: refused");
    }
}
