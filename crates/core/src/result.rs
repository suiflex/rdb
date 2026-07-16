use serde_json::Value as Json;

/// What a driver returns. The UI renders by variant: grid for `Tabular`,
/// tree/JSON for `Documents`, key-list for `KeyValue`, toast for `Affected`.
#[derive(Debug, Clone)]
pub enum ResultSet {
    Tabular { cols: Vec<Column>, rows: Vec<Row> },
    Documents(Vec<Json>),
    KeyValue(Vec<(String, RedisValue)>),
    Affected(u64),
}

#[derive(Debug, Clone)]
pub struct Column {
    pub name: String,
    pub type_name: String,
}

pub type Row = Vec<Cell>;

/// One message from a streaming query ([`crate::driver::Driver::query_stream`]).
/// `Meta` (the column headers) always arrives first, then any number of
/// `Batch`es of rows. Lets the UI render a huge result progressively instead of
/// buffering every row before the first paint.
#[derive(Debug, Clone)]
pub enum StreamItem {
    Meta(Vec<Column>),
    Batch(Vec<Row>),
}

/// One grid cell. Engine-native types are normalized into this set.
#[derive(Debug, Clone)]
pub enum Cell {
    Null,
    Int(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    Bytes(Vec<u8>),
}

impl Cell {
    /// Display string for the result grid.
    pub fn render(&self) -> String {
        match self {
            Cell::Null => "NULL".to_string(),
            Cell::Int(i) => i.to_string(),
            Cell::Float(f) => f.to_string(),
            Cell::Text(s) => s.clone(),
            Cell::Bool(b) => b.to_string(),
            Cell::Bytes(b) => format!("({} bytes)", b.len()),
        }
    }
}

/// Redis values are their own shape, kept separate from SQL `Cell`.
#[derive(Debug, Clone)]
pub enum RedisValue {
    Str(String),
    Int(i64),
    List(Vec<String>),
    Nil,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_renders_as_null_marker() {
        assert_eq!(Cell::Null.render(), "NULL");
    }

    #[test]
    fn text_renders_verbatim() {
        assert_eq!(Cell::Text("hello".into()).render(), "hello");
    }

    #[test]
    fn int_and_bool_render() {
        assert_eq!(Cell::Int(42).render(), "42");
        assert_eq!(Cell::Bool(true).render(), "true");
    }

    #[test]
    fn bytes_render_as_size_summary_not_raw() {
        assert_eq!(Cell::Bytes(vec![0u8; 3]).render(), "(3 bytes)");
    }

    #[test]
    fn tabular_result_holds_cols_and_rows() {
        let rs = ResultSet::Tabular {
            cols: vec![Column {
                name: "id".into(),
                type_name: "int4".into(),
            }],
            rows: vec![vec![Cell::Int(1)]],
        };
        match rs {
            ResultSet::Tabular { cols, rows } => {
                assert_eq!(cols.len(), 1);
                assert_eq!(rows[0][0].render(), "1");
            }
            _ => panic!("wrong variant"),
        }
    }
}
