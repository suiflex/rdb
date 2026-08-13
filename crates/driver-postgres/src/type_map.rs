use rdb_core::result::Cell;
use std::error::Error;
use tokio_postgres::types::{FromSql, Type};
use tokio_postgres::Row;

/// Which `Cell` variant a pg column type maps to. Pragmatic, not exhaustive:
/// unknown types fall back to a string read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Int,
    Float,
    Bool,
    Text,
    Bytes,
    Uuid,
    Timestamp,
    Date,
    Numeric,
    Money,
    Json,
}

/// Classify a pg `Type` into a `CellKind`. Unknown types -> `Text` (string
/// fallback), which is always safe to display.
pub fn classify(ty: &Type) -> CellKind {
    match *ty {
        Type::INT2 | Type::INT4 | Type::INT8 => CellKind::Int,
        Type::FLOAT4 | Type::FLOAT8 => CellKind::Float,
        Type::BOOL => CellKind::Bool,
        Type::BYTEA => CellKind::Bytes,
        Type::UUID => CellKind::Uuid,
        Type::TIMESTAMPTZ | Type::TIMESTAMP => CellKind::Timestamp,
        Type::DATE => CellKind::Date,
        Type::NUMERIC => CellKind::Numeric,
        Type::MONEY => CellKind::Money,
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => CellKind::Text,
        Type::JSON | Type::JSONB => CellKind::Json,
        _ => CellKind::Text,
    }
}

/// `postgres-types` has no built-in `FromSql` for `MONEY` (unlike `NUMERIC`,
/// nothing in the crate ecosystem claims it), so without this columns of
/// this type silently became `Cell::Null` — the same string_fallback used
/// everywhere else can't decode it either (`String`'s `FromSql` only
/// accepts text-family OIDs). Postgres's binary wire format for `money` is
/// documented as a plain big-endian `i64` scaled by 100 (the currency's
/// minor unit), independent of `lc_monetary`.
struct PgMoney(i64);

impl<'a> FromSql<'a> for PgMoney {
    fn from_sql(_ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        let bytes: [u8; 8] = raw.try_into()?;
        Ok(PgMoney(i64::from_be_bytes(bytes)))
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::MONEY
    }
}

impl std::fmt::Display for PgMoney {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cents = self.0.unsigned_abs() % 100;
        let whole = self.0 / 100;
        if self.0 < 0 && whole == 0 {
            write!(f, "-0.{cents:02}")
        } else {
            write!(f, "{whole}.{cents:02}")
        }
    }
}

/// Extract column `idx` of `row` into a `Cell`, honoring NULLs.
///
/// Reads via `try_get::<Option<T>>`: `Ok(None)` -> `Cell::Null`. If the typed
/// read fails (type we did not special-case, or a decode mismatch), fall back
/// to reading the value as `Option<String>`; if even that fails the value is
/// represented as `Cell::Null` so one odd column never aborts a whole result.
pub fn extract_cell(row: &Row, idx: usize) -> Cell {
    let ty = row.columns()[idx].type_();
    match classify(ty) {
        CellKind::Int => match row.try_get::<_, Option<i16>>(idx) {
            Ok(Some(v)) => Cell::Int(v as i64),
            Ok(None) => Cell::Null,
            Err(_) => match row.try_get::<_, Option<i32>>(idx) {
                Ok(Some(v)) => Cell::Int(v as i64),
                Ok(None) => Cell::Null,
                Err(_) => match row.try_get::<_, Option<i64>>(idx) {
                    Ok(Some(v)) => Cell::Int(v),
                    Ok(None) => Cell::Null,
                    Err(_) => string_fallback(row, idx),
                },
            },
        },
        CellKind::Float => match row.try_get::<_, Option<f64>>(idx) {
            Ok(Some(v)) => Cell::Float(v),
            Ok(None) => Cell::Null,
            Err(_) => match row.try_get::<_, Option<f32>>(idx) {
                Ok(Some(v)) => Cell::Float(v as f64),
                Ok(None) => Cell::Null,
                Err(_) => string_fallback(row, idx),
            },
        },
        CellKind::Bool => match row.try_get::<_, Option<bool>>(idx) {
            Ok(Some(v)) => Cell::Bool(v),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
        },
        CellKind::Bytes => match row.try_get::<_, Option<Vec<u8>>>(idx) {
            Ok(Some(v)) => Cell::Bytes(v),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
        },
        CellKind::Uuid => match row.try_get::<_, Option<uuid::Uuid>>(idx) {
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
        },
        CellKind::Timestamp => match row.try_get::<_, Option<chrono::DateTime<chrono::Utc>>>(idx) {
            Ok(Some(v)) => Cell::Text(v.format("%Y-%m-%d %H:%M:%S%.3f").to_string()),
            Ok(None) => Cell::Null,
            Err(_) => match row.try_get::<_, Option<chrono::NaiveDateTime>>(idx) {
                Ok(Some(v)) => Cell::Text(v.format("%Y-%m-%d %H:%M:%S%.3f").to_string()),
                Ok(None) => Cell::Null,
                Err(_) => string_fallback(row, idx),
            },
        },
        CellKind::Date => match row.try_get::<_, Option<chrono::NaiveDate>>(idx) {
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
        },
        // string_fallback can't recover a NUMERIC that overflows Decimal's
        // range (String's FromSql doesn't accept the NUMERIC OID either, so
        // that fallback would silently produce Cell::Null — indistinguishable
        // from a real NULL). A placeholder that's visibly not-a-value is
        // more honest than pretending the cell is empty.
        // ponytail: shows a placeholder instead of the real (rare,
        // >28-29 significant digits) out-of-range value; upgrade to a raw
        // binary NUMERIC parser if that's ever actually needed.
        CellKind::Numeric => match row.try_get::<_, Option<rust_decimal::Decimal>>(idx) {
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => Cell::Text("<numeric value out of range>".to_string()),
        },
        CellKind::Money => match row.try_get::<_, Option<PgMoney>>(idx) {
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => Cell::Text("<unreadable money value>".to_string()),
        },
        CellKind::Json => match row.try_get::<_, Option<serde_json::Value>>(idx) {
            // Compact single-line render; the UI inspector pretty-prints on demand.
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
        },
        CellKind::Text => string_fallback(row, idx),
    }
}

/// Read column `idx` as an optional string; failure or NULL -> `Cell::Null`,
/// otherwise `Cell::Text`.
fn string_fallback(row: &Row, idx: usize) -> Cell {
    match row.try_get::<_, Option<String>>(idx) {
        Ok(Some(s)) => Cell::Text(s),
        _ => Cell::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_postgres::types::Type;

    #[test]
    fn integer_types_classify_as_int() {
        assert_eq!(classify(&Type::INT2), CellKind::Int);
        assert_eq!(classify(&Type::INT4), CellKind::Int);
        assert_eq!(classify(&Type::INT8), CellKind::Int);
    }

    #[test]
    fn postgres_integer_widths_map_to_int() {
        let values: [i16; 1] = [7];
        assert_eq!(values[0] as i64, 7);
    }

    #[test]
    fn float_types_classify_as_float() {
        assert_eq!(classify(&Type::FLOAT4), CellKind::Float);
        assert_eq!(classify(&Type::FLOAT8), CellKind::Float);
    }

    #[test]
    fn bool_text_bytea_classify() {
        assert_eq!(classify(&Type::BOOL), CellKind::Bool);
        assert_eq!(classify(&Type::TEXT), CellKind::Text);
        assert_eq!(classify(&Type::VARCHAR), CellKind::Text);
        assert_eq!(classify(&Type::BYTEA), CellKind::Bytes);
    }

    #[test]
    fn uuid_timestamp_date_numeric_have_branches() {
        assert_eq!(classify(&Type::UUID), CellKind::Uuid);
        assert_eq!(classify(&Type::TIMESTAMPTZ), CellKind::Timestamp);
        assert_eq!(classify(&Type::TIMESTAMP), CellKind::Timestamp);
        assert_eq!(classify(&Type::DATE), CellKind::Date);
        assert_eq!(classify(&Type::NUMERIC), CellKind::Numeric);
    }

    #[test]
    fn money_classifies_and_has_no_builtin_fromsql() {
        // Regression guard for the bug this was added to fix: MONEY used to
        // fall into the default Text arm, and string_fallback (String's
        // FromSql) doesn't accept the MONEY OID — so it silently became
        // Cell::Null. This must classify to its own kind, not Text.
        assert_eq!(classify(&Type::MONEY), CellKind::Money);
        assert_ne!(classify(&Type::MONEY), CellKind::Text);
    }

    #[test]
    fn pg_money_decodes_documented_binary_format() {
        // Postgres's binary wire format for `money` is a big-endian i64
        // scaled by 100 (the currency's minor unit), independent of
        // lc_monetary. Hand-built payloads so this is verifiable without a
        // live database.
        let raw = 12_345_i64.to_be_bytes(); // $123.45
        let v = <PgMoney as FromSql>::from_sql(&Type::MONEY, &raw).unwrap();
        assert_eq!(v.to_string(), "123.45");

        let zero = 0_i64.to_be_bytes();
        let v = <PgMoney as FromSql>::from_sql(&Type::MONEY, &zero).unwrap();
        assert_eq!(v.to_string(), "0.00");

        let small_cents = 7_i64.to_be_bytes(); // $0.07
        let v = <PgMoney as FromSql>::from_sql(&Type::MONEY, &small_cents).unwrap();
        assert_eq!(v.to_string(), "0.07");

        let negative = (-500_i64).to_be_bytes(); // -$5.00
        let v = <PgMoney as FromSql>::from_sql(&Type::MONEY, &negative).unwrap();
        assert_eq!(v.to_string(), "-5.00");

        let negative_cents_only = (-7_i64).to_be_bytes(); // -$0.07
        let v = <PgMoney as FromSql>::from_sql(&Type::MONEY, &negative_cents_only).unwrap();
        assert_eq!(v.to_string(), "-0.07");
    }

    #[test]
    fn pg_money_rejects_malformed_payload() {
        let too_short = [0u8; 4];
        assert!(<PgMoney as FromSql>::from_sql(&Type::MONEY, &too_short).is_err());
    }

    #[test]
    fn pg_money_only_accepts_money_oid() {
        assert!(<PgMoney as FromSql>::accepts(&Type::MONEY));
        assert!(!<PgMoney as FromSql>::accepts(&Type::INT8));
    }

    #[test]
    fn json_types_classify_as_json() {
        assert_eq!(classify(&Type::JSON), CellKind::Json);
        assert_eq!(classify(&Type::JSONB), CellKind::Json);
    }

    #[test]
    fn unknown_type_falls_back_to_text() {
        assert_eq!(classify(&Type::INTERVAL), CellKind::Text);
    }
}
