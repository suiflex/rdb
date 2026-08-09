use rdb_core::result::Cell;
use serde_json::Value as Json;

/// Map one cell from ClickHouse's `FORMAT JSON` response into `Cell`.
///
/// ClickHouse's JSON output format already stringifies anything that could
/// lose precision as a plain JS number (`UInt64`/`Int64`/`Decimal`/`Date`/
/// `DateTime`/`UUID`/`Enum`, …), so this collapses to one generic mapping
/// instead of a per-ClickHouse-type table: everything that isn't a JSON
/// primitive is text already, and `Array`/`Object` (ClickHouse `Array`/`Map`/
/// `Tuple`/`Nested`) degrade to their JSON text form.
pub fn json_value_to_cell(v: &Json) -> Cell {
    match v {
        Json::Null => Cell::Null,
        Json::Bool(b) => Cell::Bool(*b),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                Cell::Int(i)
            } else if let Some(u) = n.as_u64() {
                // Only reachable for u64 values above i64::MAX; ClickHouse
                // renders UInt64 as a JSON string precisely to avoid this
                // case, so this is a defensive fallback, not the common path.
                Cell::Int(u as i64)
            } else {
                Cell::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        Json::String(s) => Cell::Text(s.clone()),
        Json::Array(_) | Json::Object(_) => Cell::Text(v.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_maps_to_cell_null() {
        assert!(matches!(json_value_to_cell(&Json::Null), Cell::Null));
    }

    #[test]
    fn bool_maps_to_cell_bool() {
        assert!(matches!(
            json_value_to_cell(&Json::Bool(true)),
            Cell::Bool(true)
        ));
    }

    #[test]
    fn int_maps_to_cell_int() {
        assert!(matches!(
            json_value_to_cell(&serde_json::json!(42)),
            Cell::Int(42)
        ));
    }

    #[test]
    fn float_maps_to_cell_float() {
        assert!(matches!(
            json_value_to_cell(&serde_json::json!(1.5)),
            Cell::Float(_)
        ));
    }

    #[test]
    fn string_maps_to_cell_text() {
        // Covers UInt64/Decimal/Date/UUID/etc, which ClickHouse's JSON
        // format already renders as strings.
        let v = json_value_to_cell(&serde_json::json!("18446744073709551615"));
        assert!(matches!(v, Cell::Text(ref s) if s == "18446744073709551615"));
    }

    #[test]
    fn array_degrades_to_text() {
        let v = json_value_to_cell(&serde_json::json!([1, 2, 3]));
        assert!(matches!(v, Cell::Text(ref s) if s == "[1,2,3]"));
    }

    #[test]
    fn object_degrades_to_text() {
        let v = json_value_to_cell(&serde_json::json!({"a": 1}));
        assert!(matches!(v, Cell::Text(ref s) if s == "{\"a\":1}"));
    }
}
