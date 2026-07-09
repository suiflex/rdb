use rdbs_core::result::Cell;
use tokio_postgres::types::Type;
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
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => CellKind::Text,
        Type::JSON | Type::JSONB => CellKind::Json,
        _ => CellKind::Text,
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
        CellKind::Int => match row.try_get::<_, Option<i64>>(idx) {
            Ok(Some(v)) => Cell::Int(v),
            Ok(None) => Cell::Null,
            Err(_) => match row.try_get::<_, Option<i32>>(idx) {
                Ok(Some(v)) => Cell::Int(v as i64),
                Ok(None) => Cell::Null,
                Err(_) => string_fallback(row, idx),
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
        CellKind::Numeric => match row.try_get::<_, Option<rust_decimal::Decimal>>(idx) {
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
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
    fn json_types_classify_as_json() {
        assert_eq!(classify(&Type::JSON), CellKind::Json);
        assert_eq!(classify(&Type::JSONB), CellKind::Json);
    }

    #[test]
    fn unknown_type_falls_back_to_text() {
        assert_eq!(classify(&Type::INTERVAL), CellKind::Text);
    }
}
