//! CQL value → unified `Cell`. Pragmatic, not exhaustive: collections, UDTs
//! and the temporal/decimal types fall back to their debug string, which is
//! always safe to display in the grid.

use rdb_core::result::Cell;
use scylla::value::CqlValue;

/// One column of a row (`None` = NULL) into a `Cell`.
pub fn cell(value: &Option<CqlValue>) -> Cell {
    match value {
        None => Cell::Null,
        Some(v) => cql_to_cell(v),
    }
}

fn cql_to_cell(v: &CqlValue) -> Cell {
    match v {
        CqlValue::Boolean(b) => Cell::Bool(*b),
        CqlValue::Int(i) => Cell::Int(*i as i64),
        CqlValue::BigInt(i) => Cell::Int(*i),
        CqlValue::SmallInt(i) => Cell::Int(*i as i64),
        CqlValue::TinyInt(i) => Cell::Int(*i as i64),
        CqlValue::Counter(c) => Cell::Int(c.0),
        CqlValue::Float(f) => Cell::Float(*f as f64),
        CqlValue::Double(f) => Cell::Float(*f),
        CqlValue::Text(s) | CqlValue::Ascii(s) => Cell::Text(s.clone()),
        CqlValue::Blob(b) => Cell::Bytes(b.clone()),
        CqlValue::Uuid(u) => Cell::Text(u.to_string()),
        CqlValue::Timeuuid(u) => Cell::Text(u.to_string()),
        CqlValue::Inet(a) => Cell::Text(a.to_string()),
        CqlValue::Empty => Cell::Null,
        other => Cell::Text(format!("{other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_and_scalars_map() {
        assert!(matches!(cell(&None), Cell::Null));
        assert!(matches!(cell(&Some(CqlValue::Int(7))), Cell::Int(7)));
        assert!(matches!(cell(&Some(CqlValue::BigInt(9))), Cell::Int(9)));
        assert!(matches!(
            cell(&Some(CqlValue::Boolean(true))),
            Cell::Bool(true)
        ));
        match cell(&Some(CqlValue::Text("hi".into()))) {
            Cell::Text(s) => assert_eq!(s, "hi"),
            _ => panic!("wrong"),
        }
    }
}
