use tiberius::{ColumnData, ColumnType, FromSql};

use rdb_core::result::Cell;

/// Map a tiberius cell value into rdb-core's `Cell`.
///
/// Numeric/decimal and date/time values stringify (`Numeric` has its own
/// `Display`; date/time go through tiberius's `chrono` feature rather than
/// hand-decoding the raw TDS day-count/tick fields, which is error-prone).
/// XML falls back to `Debug` — `XmlData` has no cheap borrowed accessor to
/// its text, and XML columns are rare enough not to warrant one.
pub fn column_data_to_cell(data: &ColumnData<'static>) -> Cell {
    match data {
        ColumnData::U8(v) => v.map(|n| Cell::Int(n as i64)).unwrap_or(Cell::Null),
        ColumnData::I16(v) => v.map(|n| Cell::Int(n as i64)).unwrap_or(Cell::Null),
        ColumnData::I32(v) => v.map(|n| Cell::Int(n as i64)).unwrap_or(Cell::Null),
        ColumnData::I64(v) => v.map(Cell::Int).unwrap_or(Cell::Null),
        ColumnData::F32(v) => v.map(|n| Cell::Float(n as f64)).unwrap_or(Cell::Null),
        ColumnData::F64(v) => v.map(Cell::Float).unwrap_or(Cell::Null),
        ColumnData::Bit(v) => v.map(Cell::Bool).unwrap_or(Cell::Null),
        ColumnData::String(v) => v
            .as_ref()
            .map(|s| Cell::Text(s.to_string()))
            .unwrap_or(Cell::Null),
        ColumnData::Guid(v) => v.map(|u| Cell::Text(u.to_string())).unwrap_or(Cell::Null),
        ColumnData::Binary(v) => v
            .as_ref()
            .map(|b| Cell::Bytes(b.to_vec()))
            .unwrap_or(Cell::Null),
        ColumnData::Numeric(v) => v.map(|n| Cell::Text(n.to_string())).unwrap_or(Cell::Null),
        ColumnData::Xml(v) => v
            .as_ref()
            .map(|x| Cell::Text(format!("{x:?}")))
            .unwrap_or(Cell::Null),
        ColumnData::SmallDateTime(_) | ColumnData::DateTime(_) | ColumnData::DateTime2(_) => {
            <chrono::NaiveDateTime as FromSql>::from_sql(data)
                .ok()
                .flatten()
                .map(|d| Cell::Text(d.to_string()))
                .unwrap_or(Cell::Null)
        }
        ColumnData::Time(_) => <chrono::NaiveTime as FromSql>::from_sql(data)
            .ok()
            .flatten()
            .map(|t| Cell::Text(t.to_string()))
            .unwrap_or(Cell::Null),
        ColumnData::Date(_) => <chrono::NaiveDate as FromSql>::from_sql(data)
            .ok()
            .flatten()
            .map(|d| Cell::Text(d.to_string()))
            .unwrap_or(Cell::Null),
        ColumnData::DateTimeOffset(_) => <chrono::DateTime<chrono::Utc> as FromSql>::from_sql(data)
            .ok()
            .flatten()
            .map(|d| Cell::Text(d.to_string()))
            .unwrap_or(Cell::Null),
    }
}

/// Human-readable type name for a result column, used to fill `Column.type_name`.
pub fn column_type_name(ct: ColumnType) -> String {
    format!("{ct:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_maps_to_cell_null() {
        assert!(matches!(
            column_data_to_cell(&ColumnData::I32(None)),
            Cell::Null
        ));
    }

    #[test]
    fn int_variants_map_to_cell_int() {
        assert!(matches!(
            column_data_to_cell(&ColumnData::I32(Some(42))),
            Cell::Int(42)
        ));
        assert!(matches!(
            column_data_to_cell(&ColumnData::U8(Some(7))),
            Cell::Int(7)
        ));
        assert!(matches!(
            column_data_to_cell(&ColumnData::I64(Some(9))),
            Cell::Int(9)
        ));
    }

    #[test]
    fn float_variants_map_to_cell_float() {
        assert!(matches!(
            column_data_to_cell(&ColumnData::F64(Some(1.5))),
            Cell::Float(_)
        ));
        assert!(matches!(
            column_data_to_cell(&ColumnData::F32(Some(1.5))),
            Cell::Float(_)
        ));
    }

    #[test]
    fn bit_maps_to_cell_bool() {
        assert!(matches!(
            column_data_to_cell(&ColumnData::Bit(Some(true))),
            Cell::Bool(true)
        ));
    }

    #[test]
    fn string_maps_to_cell_text() {
        let v = column_data_to_cell(&ColumnData::String(Some("hi".into())));
        assert!(matches!(v, Cell::Text(ref s) if s == "hi"));
    }

    #[test]
    fn binary_maps_to_cell_bytes() {
        let v = column_data_to_cell(&ColumnData::Binary(Some(vec![1u8, 2].into())));
        assert!(matches!(v, Cell::Bytes(ref b) if b == &[1, 2]));
    }

    #[test]
    fn numeric_stringifies() {
        let n = tiberius::numeric::Numeric::new_with_scale(1050, 2);
        let v = column_data_to_cell(&ColumnData::Numeric(Some(n)));
        assert!(matches!(v, Cell::Text(ref s) if s == "10.50"));
    }
}
