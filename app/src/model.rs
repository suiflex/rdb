//! Pure conversion from rdbs-core result/schema types into flat view-model
//! structs the UI binds to. No Slint imports here so it stays unit-testable;
//! `main.rs` maps these into the Slint-generated structs.

use rdbs_core::result::{Cell, RedisValue, ResultSet};
use rdbs_core::schema::{ContainerKind, Schema};

#[derive(Debug, Default, Clone)]
pub struct VmCell {
    pub text: String,
    pub is_null: bool,
}

#[derive(Debug, Default, Clone)]
pub struct VmColumn {
    pub name: String,
    pub type_name: String,
}

/// Tabular grid: real columns + rows. Used directly for SQL results and as the
/// "Table" rendering of Mongo documents.
#[derive(Debug, Default, Clone)]
pub struct GridModel {
    pub columns: Vec<VmColumn>,
    pub rows: Vec<Vec<VmCell>>,
}

/// Mongo documents: the raw pretty JSON plus a flattened grid the UI toggles to.
#[derive(Debug, Default, Clone)]
pub struct DocModel {
    pub json: String,
    pub grid: GridModel,
}

/// Redis key/value rows: each row is a key (or list index / hash field) and its
/// value cell. Kept separate from the SQL grid so Redis presentation can diverge.
#[derive(Debug, Default, Clone)]
pub struct KvModel {
    pub rows: Vec<(String, VmCell)>,
}

/// Presentation-ready view of a `ResultSet`, one arm per data shape. Drives the
/// per-kind result region in the UI instead of collapsing everything to a grid.
#[derive(Debug, Clone)]
pub enum ResultView {
    Table(GridModel),
    Documents(DocModel),
    KeyValue(KvModel),
    /// Status toast for writes, e.g. "3 rows affected".
    Affected(String),
}

#[derive(Debug, Default, Clone)]
pub struct VmTreeNode {
    pub label: String,
    pub kind: String,
}

/// One column definition for the Structure tab (Feature B).
#[derive(Debug, Default, Clone)]
pub struct VmStructField {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
}

/// Column definitions for the Structure tab. MVP: the fields of the first
/// container (table/collection) found in the schema. TODO: drive this from the
/// table the user selects in the sidebar rather than always the first one.
pub fn to_structure_model(schema: &Schema) -> Vec<VmStructField> {
    schema
        .databases
        .iter()
        .flat_map(|db| db.containers.iter())
        .find(|c| !c.fields.is_empty())
        .map(|c| {
            c.fields
                .iter()
                .map(|f| VmStructField {
                    name: f.name.clone(),
                    type_name: f.type_name.clone(),
                    nullable: f.nullable,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn redis_cell(v: &RedisValue) -> VmCell {
    match v {
        RedisValue::Str(s) => VmCell {
            text: s.clone(),
            is_null: false,
        },
        RedisValue::Int(i) => VmCell {
            text: i.to_string(),
            is_null: false,
        },
        RedisValue::List(items) => VmCell {
            text: items.join(", "),
            is_null: false,
        },
        RedisValue::Nil => VmCell {
            text: "(nil)".into(),
            is_null: true,
        },
    }
}

/// Flatten Mongo documents into a tabular grid: columns are the ordered union
/// of top-level keys (`_id` first), cells are the matching values. A missing key
/// is a null cell; a nested object/array is rendered as compact JSON so the row
/// still reads as one line. The raw pretty JSON is kept separately for the
/// JSON-view toggle. ponytail: top-level keys only, no deep column expansion.
fn flatten_documents(docs: &[serde_json::Value]) -> (Vec<VmColumn>, Vec<Vec<VmCell>>) {
    let mut keys: Vec<String> = Vec::new();
    for doc in docs {
        if let Some(obj) = doc.as_object() {
            for k in obj.keys() {
                if !keys.iter().any(|e| e == k) {
                    keys.push(k.clone());
                }
            }
        }
    }
    // _id reads first, like every Mongo client.
    if let Some(pos) = keys.iter().position(|k| k == "_id") {
        keys.swap(0, pos);
    }

    let columns = keys
        .iter()
        .map(|k| VmColumn {
            name: k.clone(),
            type_name: String::new(),
        })
        .collect();

    let rows = docs
        .iter()
        .map(|doc| {
            keys.iter()
                .map(|k| match doc.get(k) {
                    None | Some(serde_json::Value::Null) => VmCell {
                        text: String::new(),
                        is_null: true,
                    },
                    Some(serde_json::Value::String(s)) => VmCell {
                        text: s.clone(),
                        is_null: false,
                    },
                    Some(v) => VmCell {
                        text: v.to_string(),
                        is_null: false,
                    },
                })
                .collect()
        })
        .collect();

    (columns, rows)
}

/// Convert any ResultSet into its presentation-ready view. Each variant keeps
/// its own shape instead of collapsing to a single grid.
pub fn to_result_view(rs: &ResultSet) -> ResultView {
    match rs {
        ResultSet::Tabular { cols, rows } => ResultView::Table(GridModel {
            columns: cols
                .iter()
                .map(|c| VmColumn {
                    name: c.name.clone(),
                    type_name: c.type_name.clone(),
                })
                .collect(),
            rows: rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|cell| VmCell {
                            text: cell.render(),
                            is_null: matches!(cell, Cell::Null),
                        })
                        .collect()
                })
                .collect(),
        }),
        ResultSet::KeyValue(pairs) => ResultView::KeyValue(KvModel {
            rows: pairs
                .iter()
                .map(|(k, v)| (k.clone(), redis_cell(v)))
                .collect(),
        }),
        ResultSet::Documents(docs) => {
            let json = serde_json::to_string_pretty(docs).unwrap_or_else(|_| "[]".into());
            let (columns, rows) = flatten_documents(docs);
            ResultView::Documents(DocModel {
                json,
                grid: GridModel { columns, rows },
            })
        }
        ResultSet::Affected(n) => ResultView::Affected(format!("{} rows affected", n)),
    }
}

/// Flatten a Schema into depth-tagged tree nodes for the sidebar.
pub fn to_tree_model(schema: &Schema) -> Vec<VmTreeNode> {
    let mut out = Vec::new();
    for db in &schema.databases {
        out.push(VmTreeNode {
            label: db.name.clone(),
            kind: "database".into(),
        });
        for c in &db.containers {
            let kind = match c.kind {
                ContainerKind::Table => "table",
                ContainerKind::Collection => "collection",
                ContainerKind::Keyspace => "keyspace",
            };
            out.push(VmTreeNode {
                label: c.name.clone(),
                kind: kind.into(),
            });
            for f in &c.fields {
                out.push(VmTreeNode {
                    label: format!("{}: {}", f.name, f.type_name),
                    kind: "field".into(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdbs_core::result::{Cell, Column, RedisValue, ResultSet};
    use rdbs_core::schema::{Container, ContainerKind, Database, Field, Schema};

    fn expect_table(rs: &ResultSet) -> GridModel {
        match to_result_view(rs) {
            ResultView::Table(g) => g,
            other => panic!("expected Table, got {other:?}"),
        }
    }

    #[test]
    fn tabular_maps_cols_and_rows_with_null_flag() {
        let rs = ResultSet::Tabular {
            cols: vec![Column {
                name: "id".into(),
                type_name: "int4".into(),
            }],
            rows: vec![vec![Cell::Int(7)], vec![Cell::Null]],
        };
        let grid = expect_table(&rs);
        assert_eq!(grid.columns.len(), 1);
        assert_eq!(grid.columns[0].name, "id");
        assert_eq!(grid.rows.len(), 2);
        assert_eq!(grid.rows[0][0].text, "7");
        assert!(!grid.rows[0][0].is_null);
        assert_eq!(grid.rows[1][0].text, "NULL");
        assert!(grid.rows[1][0].is_null);
    }

    #[test]
    fn affected_maps_to_status_text() {
        match to_result_view(&ResultSet::Affected(3)) {
            ResultView::Affected(s) => assert_eq!(s, "3 rows affected"),
            other => panic!("expected Affected, got {other:?}"),
        }
    }

    #[test]
    fn keyvalue_maps_redis_pairs() {
        let rs = ResultSet::KeyValue(vec![
            ("k1".into(), RedisValue::Str("v1".into())),
            ("k2".into(), RedisValue::Int(9)),
            ("k3".into(), RedisValue::Nil),
        ]);
        let kv = match to_result_view(&rs) {
            ResultView::KeyValue(kv) => kv,
            other => panic!("expected KeyValue, got {other:?}"),
        };
        assert_eq!(kv.rows.len(), 3);
        assert_eq!(kv.rows[0].0, "k1");
        assert_eq!(kv.rows[0].1.text, "v1");
        assert_eq!(kv.rows[1].1.text, "9");
        assert_eq!(kv.rows[2].1.text, "(nil)");
        assert!(kv.rows[2].1.is_null);
    }

    #[test]
    fn documents_render_as_json_text_block() {
        let rs = ResultSet::Documents(vec![serde_json::json!({"a": 1})]);
        match to_result_view(&rs) {
            ResultView::Documents(d) => assert!(d.json.contains("\"a\"")),
            other => panic!("expected Documents, got {other:?}"),
        }
    }

    #[test]
    fn documents_flatten_to_grid_with_id_first() {
        let rs = ResultSet::Documents(vec![
            serde_json::json!({"_id": "x", "name": "ada", "tags": [1, 2]}),
            serde_json::json!({"name": "lin", "age": 30}),
        ]);
        let grid = match to_result_view(&rs) {
            ResultView::Documents(d) => d.grid,
            other => panic!("expected Documents, got {other:?}"),
        };
        // union of keys, _id first
        let names: Vec<&str> = grid.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names[0], "_id");
        assert!(names.contains(&"name") && names.contains(&"tags") && names.contains(&"age"));
        // row 0: _id plain string, nested array as compact JSON
        assert_eq!(grid.rows[0][0].text, "x");
        let tags_idx = names.iter().position(|n| *n == "tags").unwrap();
        assert_eq!(grid.rows[0][tags_idx].text, "[1,2]");
        // row 1 missing _id -> null cell
        assert!(grid.rows[1][0].is_null);
    }

    #[test]
    fn schema_flattens_to_indented_tree_nodes() {
        let schema = Schema {
            databases: vec![Database {
                name: "app".into(),
                containers: vec![Container {
                    name: "users".into(),
                    kind: ContainerKind::Table,
                    fields: vec![Field {
                        name: "id".into(),
                        type_name: "int4".into(),
                        nullable: false,
                    }],
                }],
            }],
        };
        let nodes = to_tree_model(&schema);
        assert_eq!(nodes[0].label, "app");
        assert_eq!(nodes[0].kind, "database");
        assert_eq!(nodes[1].label, "users");
        assert_eq!(nodes[1].kind, "table");
        assert_eq!(nodes[2].label, "id: int4");
        assert_eq!(nodes[2].kind, "field");
    }
}

// ===== buffered edits (TablePlus-style pending writes) =====

use rdbs_core::write::{TableRef, WriteOp};
use std::collections::{BTreeSet, HashMap};

/// Buffered, uncommitted grid mutations for the open browse container.
/// `changes` keys are (row, col) into the CURRENT page grid; rows at
/// `grid.rows.len()..` address `inserts` instead. ⌘S turns the buffer into
/// `WriteOp`s via [`EditBuffer::to_ops`]; Esc/Discard drops it.
#[derive(Debug, Default, Clone)]
pub struct EditBuffer {
    pub table: Option<TableRef>,
    pub pk_cols: Vec<String>,
    pub changes: HashMap<(usize, usize), String>,
    pub deletes: BTreeSet<usize>,
    pub inserts: Vec<Vec<String>>,
}

impl EditBuffer {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.deletes.is_empty() && self.inserts.is_empty()
    }

    /// Rows whose edits still count: changes on delete-marked rows die with
    /// the row, so they don't inflate the pending badge.
    pub fn pending_count(&self) -> usize {
        let live_changes = self
            .changes
            .keys()
            .filter(|(r, _)| !self.deletes.contains(r))
            .count();
        live_changes + self.deletes.len() + self.inserts.len()
    }

    /// Reset for a new container/page.
    pub fn clear(&mut self) {
        self.changes.clear();
        self.deletes.clear();
        self.inserts.clear();
    }

    /// The effective column name for op payloads. Redis kv grids label the
    /// identity column "key" while the driver addresses it by its typed
    /// identity ("field"/"index"/"member"); a single-pk grid whose first
    /// column is "key" maps column 0 to that pk name.
    fn col_name<'a>(&'a self, grid: &'a GridModel, idx: usize) -> &'a str {
        if idx == 0
            && self.pk_cols.len() == 1
            && grid.columns.first().map(|c| c.name.as_str()) == Some("key")
            && self.pk_cols[0] != "key"
        {
            return &self.pk_cols[0];
        }
        grid.columns.get(idx).map(|c| c.name.as_str()).unwrap_or("")
    }

    /// Identity pairs of one ORIGINAL row (pre-edit values), typed by column.
    fn row_pk(&self, grid: &GridModel, row: usize) -> Result<Vec<(String, Cell)>, String> {
        let mut pk = Vec::new();
        for name in &self.pk_cols {
            let idx = grid
                .columns
                .iter()
                .position(|c| &c.name == name)
                .or({
                    // single-identity grids (Redis): identity lives in column 0
                    if self.pk_cols.len() == 1 { Some(0) } else { None }
                })
                .ok_or_else(|| format!("primary key column \"{name}\" not in result"))?;
            let cell = grid
                .rows
                .get(row)
                .and_then(|r| r.get(idx))
                .ok_or_else(|| format!("row {row} out of range"))?;
            let value = if cell.is_null {
                Cell::Null
            } else {
                coerce(&cell.text, &grid.columns[idx].type_name)
            };
            pk.push((name.clone(), value));
        }
        Ok(pk)
    }

    /// Turn the buffer into driver ops against `grid` (the ORIGINAL page as
    /// fetched — pk values must predate the user's edits). Delete beats edit
    /// on the same row. Insert rows skip empty cells so column defaults apply.
    pub fn to_ops(&self, grid: &GridModel) -> Result<Vec<WriteOp>, String> {
        let table = self.table.clone().ok_or("no table open")?;
        if self.pk_cols.is_empty() {
            return Err("container has no primary key (read-only)".into());
        }
        let mut ops = Vec::new();

        // updates: group live per-row changes
        let mut by_row: HashMap<usize, Vec<(usize, &String)>> = HashMap::new();
        for (&(r, c), text) in &self.changes {
            if r < grid.rows.len() && !self.deletes.contains(&r) {
                by_row.entry(r).or_default().push((c, text));
            }
        }
        let mut rows: Vec<_> = by_row.into_iter().collect();
        rows.sort_by_key(|(r, _)| *r);
        for (r, mut cells) in rows {
            cells.sort_by_key(|(c, _)| *c);
            let changes = cells
                .into_iter()
                .map(|(c, text)| {
                    let type_name = grid
                        .columns
                        .get(c)
                        .map(|col| col.type_name.as_str())
                        .unwrap_or("");
                    (self.col_name(grid, c).to_string(), coerce(text, type_name))
                })
                .collect();
            ops.push(WriteOp::Update {
                table: table.clone(),
                pk: self.row_pk(grid, r)?,
                changes,
            });
        }

        // deletes
        for &r in &self.deletes {
            ops.push(WriteOp::Delete {
                table: table.clone(),
                pk: self.row_pk(grid, r)?,
            });
        }

        // inserts
        for row in &self.inserts {
            let values: Vec<(String, Cell)> = row
                .iter()
                .enumerate()
                .filter(|(_, text)| !text.is_empty())
                .map(|(c, text)| {
                    let type_name = grid
                        .columns
                        .get(c)
                        .map(|col| col.type_name.as_str())
                        .unwrap_or("");
                    (self.col_name(grid, c).to_string(), coerce(text, type_name))
                })
                .collect();
            if values.is_empty() {
                continue;
            }
            ops.push(WriteOp::Insert {
                table: table.clone(),
                values,
            });
        }
        Ok(ops)
    }
}

/// Edited text → typed `Cell`, guided by the column type. The literal `NULL`
/// (any case) is SQL NULL — TablePlus convention. Unparseable numerics fall
/// back to text so the server reports the real error.
pub fn coerce(text: &str, type_name: &str) -> Cell {
    if text.eq_ignore_ascii_case("null") {
        return Cell::Null;
    }
    let t = type_name.to_ascii_lowercase();
    if t.contains("int") || t.contains("serial") {
        if let Ok(i) = text.trim().parse::<i64>() {
            return Cell::Int(i);
        }
    }
    if ["float", "double", "numeric", "decimal", "real"]
        .iter()
        .any(|k| t.contains(k))
    {
        if let Ok(f) = text.trim().parse::<f64>() {
            return Cell::Float(f);
        }
    }
    if t.contains("bool") {
        match text.trim().to_ascii_lowercase().as_str() {
            "true" | "t" | "1" | "yes" => return Cell::Bool(true),
            "false" | "f" | "0" | "no" => return Cell::Bool(false),
            _ => {}
        }
    }
    Cell::Text(text.to_string())
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    fn grid() -> GridModel {
        GridModel {
            columns: vec![
                VmColumn { name: "id".into(), type_name: "int4".into() },
                VmColumn { name: "name".into(), type_name: "text".into() },
            ],
            rows: vec![
                vec![
                    VmCell { text: "1".into(), is_null: false },
                    VmCell { text: "alice".into(), is_null: false },
                ],
                vec![
                    VmCell { text: "2".into(), is_null: false },
                    VmCell { text: "bob".into(), is_null: false },
                ],
            ],
        }
    }

    fn buf() -> EditBuffer {
        EditBuffer {
            table: Some(TableRef::named("users")),
            pk_cols: vec!["id".into()],
            ..Default::default()
        }
    }

    #[test]
    fn single_edit_becomes_update_with_original_pk() {
        let mut b = buf();
        b.changes.insert((0, 1), "carol".into());
        let ops = b.to_ops(&grid()).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            WriteOp::Update { pk, changes, .. } => {
                assert_eq!(pk[0].0, "id");
                assert!(matches!(pk[0].1, Cell::Int(1)));
                assert_eq!(changes[0].0, "name");
                assert!(matches!(&changes[0].1, Cell::Text(s) if s == "carol"));
            }
            _ => panic!("expected update"),
        }
    }

    #[test]
    fn delete_wins_over_edit_on_same_row() {
        let mut b = buf();
        b.changes.insert((1, 1), "x".into());
        b.deletes.insert(1);
        let ops = b.to_ops(&grid()).unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], WriteOp::Delete { .. }));
        assert_eq!(b.pending_count(), 1);
    }

    #[test]
    fn insert_skips_empty_cells() {
        let mut b = buf();
        b.inserts.push(vec![String::new(), "dave".into()]);
        let ops = b.to_ops(&grid()).unwrap();
        match &ops[0] {
            WriteOp::Insert { values, .. } => {
                assert_eq!(values.len(), 1);
                assert_eq!(values[0].0, "name");
            }
            _ => panic!("expected insert"),
        }
    }

    #[test]
    fn null_keyword_coerces_to_null() {
        assert!(matches!(coerce("NULL", "text"), Cell::Null));
        assert!(matches!(coerce("null", "int4"), Cell::Null));
        assert!(matches!(coerce("42", "int4"), Cell::Int(42)));
        assert!(matches!(coerce("4.5", "numeric"), Cell::Float(_)));
        assert!(matches!(coerce("true", "boolean"), Cell::Bool(true)));
        assert!(matches!(coerce("notanum", "int4"), Cell::Text(_)));
    }

    #[test]
    fn missing_pk_is_an_error() {
        let mut b = buf();
        b.pk_cols.clear();
        b.changes.insert((0, 1), "x".into());
        assert!(b.to_ops(&grid()).is_err());
    }

    #[test]
    fn redis_style_key_column_renames_to_pk_identity() {
        let g = GridModel {
            columns: vec![
                VmColumn { name: "key".into(), type_name: "text".into() },
                VmColumn { name: "value".into(), type_name: "text".into() },
            ],
            rows: vec![vec![
                VmCell { text: "color".into(), is_null: false },
                VmCell { text: "red".into(), is_null: false },
            ]],
        };
        let mut b = EditBuffer {
            table: Some(TableRef::named("prefs")),
            pk_cols: vec!["field".into()],
            ..Default::default()
        };
        b.changes.insert((0, 1), "blue".into());
        let ops = b.to_ops(&g).unwrap();
        match &ops[0] {
            WriteOp::Update { pk, changes, .. } => {
                assert_eq!(pk[0].0, "field");
                assert!(matches!(&pk[0].1, Cell::Text(s) if s == "color"));
                assert_eq!(changes[0].0, "value");
            }
            _ => panic!("expected update"),
        }
    }

    #[test]
    fn composite_pk_reads_both_columns() {
        let g = GridModel {
            columns: vec![
                VmColumn { name: "a".into(), type_name: "int4".into() },
                VmColumn { name: "b".into(), type_name: "text".into() },
                VmColumn { name: "v".into(), type_name: "text".into() },
            ],
            rows: vec![vec![
                VmCell { text: "1".into(), is_null: false },
                VmCell { text: "x".into(), is_null: false },
                VmCell { text: "old".into(), is_null: false },
            ]],
        };
        let mut b = EditBuffer {
            table: Some(TableRef::named("t")),
            pk_cols: vec!["a".into(), "b".into()],
            ..Default::default()
        };
        b.changes.insert((0, 2), "new".into());
        let ops = b.to_ops(&g).unwrap();
        match &ops[0] {
            WriteOp::Update { pk, .. } => {
                assert_eq!(pk.len(), 2);
                assert!(matches!(pk[0].1, Cell::Int(1)));
                assert!(matches!(&pk[1].1, Cell::Text(s) if s == "x"));
            }
            _ => panic!("expected update"),
        }
    }
}
