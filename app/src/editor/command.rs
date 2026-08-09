//! Redis command words for the editor lexer. Canonical list — reused by
//! `app/src/completion/command.rs` so highlighting and completion agree.
//! Minimal, common commands only; not an exhaustive Redis command reference.

pub const KEYWORDS: &[&str] = &[
    "GET", "SET", "DEL", "EXISTS", "EXPIRE", "TTL", "KEYS", "SCAN", "TYPE", "MGET", "MSET", "HGET",
    "HSET", "HGETALL", "HDEL", "LPUSH", "RPUSH", "LRANGE", "LLEN", "SADD", "SMEMBERS", "SREM",
    "ZADD", "ZRANGE", "ZSCORE", "INCR", "DECR", "RENAME", "FLUSHDB", "PING",
];

/// True when `word` (already uppercased) is a Redis command word.
pub fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}
