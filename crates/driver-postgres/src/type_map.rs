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
}

/// Classify a pg `Type` into a `CellKind`. Unknown types -> `Text` (string
/// fallback), which is always safe to display.
pub fn classify(ty: &Type) -> CellKind {
    match *ty {
        Type::INT2 | Type::INT4 | Type::INT8 => CellKind::Int,
        Type::FLOAT4 | Type::FLOAT8 => CellKind::Float,
        Type::BOOL => CellKind::Bool,
        Type::BYTEA => CellKind::Bytes,
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => CellKind::Text,
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
    fn unknown_type_falls_back_to_text() {
        // UUID has no dedicated branch -> string fallback bucket.
        assert_eq!(classify(&Type::UUID), CellKind::Text);
    }
}
