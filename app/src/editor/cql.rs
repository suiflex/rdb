//! CQL keyword set for the editor lexer — Cassandra/ScyllaDB. Deliberately
//! excludes JOIN/GROUP BY/HAVING/subqueries: CQL doesn't support them, and
//! keeping them out is what actually distinguishes this from the SQL set.
//! `app/src/completion/cql.rs` re-exports this list so highlighting and
//! completion can't drift apart.

pub const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "ALTER",
    "DROP",
    "TABLE",
    "KEYSPACE",
    "PRIMARY",
    "KEY",
    "ALLOW",
    "FILTERING",
    "WITH",
    "USING",
    "TTL",
    "TOKEN",
    "AND",
    "OR",
    "IN",
    "CONTAINS",
    "LIMIT",
    "ORDER",
    "BY",
    "ASC",
    "DESC",
    "IF",
    "EXISTS",
    "COUNT",
    "NULL",
];

/// True when `word` (already uppercased) is a CQL keyword.
pub fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_key_and_allow_filtering_are_keywords() {
        assert!(is_keyword("PRIMARY"));
        assert!(is_keyword("ALLOW"));
        assert!(is_keyword("FILTERING"));
    }

    #[test]
    fn sql_only_keywords_are_not_cql_keywords() {
        // JOIN/HAVING/GROUP have no CQL equivalent — this is what actually
        // proves the split happened, not just a rename of the SQL list.
        assert!(!is_keyword("JOIN"));
        assert!(!is_keyword("HAVING"));
        assert!(!is_keyword("GROUP"));
    }
}
