use dbm_core::result::Cell;
use mysql_async::consts::ColumnType;
use mysql_async::Value;

/// Map a mysql_async cell value into dbm-core's `Cell`.
///
/// Bytes are treated as text when valid UTF-8 (covers CHAR/VARCHAR/TEXT and
/// DECIMAL, which mysql returns as bytes), otherwise as raw `Bytes`
/// (covers BLOB/BINARY). Date/time values come back as `Bytes` from the
/// driver in their default form, so they render as text here.
pub fn value_to_cell(v: &Value) -> Cell {
    match v {
        Value::NULL => Cell::Null,
        Value::Int(i) => Cell::Int(*i),
        Value::UInt(u) => Cell::Int(*u as i64),
        Value::Float(f) => Cell::Float(*f as f64),
        Value::Double(d) => Cell::Float(*d),
        Value::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) => Cell::Text(s.to_string()),
            Err(_) => Cell::Bytes(b.clone()),
        },
        // Date/time variants: stringify via Value's as_sql.
        other => Cell::Text(other.as_sql(true).trim_matches('\'').to_string()),
    }
}

/// Human-readable type name for a result column, used to fill `Column.type_name`.
pub fn column_type_name(ct: ColumnType) -> String {
    format!("{ct:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbm_core::result::Cell;
    use mysql_async::Value;

    #[test]
    fn null_maps_to_cell_null() {
        assert!(matches!(value_to_cell(&Value::NULL), Cell::Null));
    }

    #[test]
    fn int_maps_to_cell_int() {
        assert!(matches!(value_to_cell(&Value::Int(42)), Cell::Int(42)));
    }

    #[test]
    fn uint_maps_to_cell_int() {
        assert!(matches!(value_to_cell(&Value::UInt(7)), Cell::Int(7)));
    }

    #[test]
    fn float_and_double_map_to_cell_float() {
        assert!(matches!(value_to_cell(&Value::Float(1.5)), Cell::Float(_)));
        assert!(matches!(value_to_cell(&Value::Double(2.5)), Cell::Float(_)));
    }

    #[test]
    fn utf8_bytes_map_to_text_and_binary_maps_to_bytes() {
        let t = value_to_cell(&Value::Bytes(b"hello".to_vec()));
        assert!(matches!(t, Cell::Text(ref s) if s == "hello"));
        let b = value_to_cell(&Value::Bytes(vec![0xff, 0xfe]));
        assert!(matches!(b, Cell::Bytes(_)));
    }
}
