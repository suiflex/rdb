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

/// One node of the collapsible JSON tree. `path` is a stable id (e.g.
/// `0.headers.host`) used as the collapse key; `expandable` marks objects/arrays.
#[derive(Debug, Clone)]
pub struct DocNode {
    pub depth: usize,
    pub key: String,
    pub preview: String,
    pub expandable: bool,
    pub path: String,
}

fn scalar_preview(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn push_doc_node(out: &mut Vec<DocNode>, depth: usize, key: &str, path: &str, v: &serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            out.push(DocNode {
                depth,
                key: key.into(),
                preview: format!("{{ {} fields }}", m.len()),
                expandable: !m.is_empty(),
                path: path.into(),
            });
            for (k, val) in m {
                push_doc_node(out, depth + 1, k, &format!("{path}.{k}"), val);
            }
        }
        serde_json::Value::Array(a) => {
            out.push(DocNode {
                depth,
                key: key.into(),
                preview: format!("[ {} ]", a.len()),
                expandable: !a.is_empty(),
                path: path.into(),
            });
            for (j, val) in a.iter().enumerate() {
                push_doc_node(out, depth + 1, &format!("[{j}]"), &format!("{path}[{j}]"), val);
            }
        }
        scalar => out.push(DocNode {
            depth,
            key: key.into(),
            preview: scalar_preview(scalar),
            expandable: false,
            path: path.into(),
        }),
    }
}

/// Flatten the documents into a fully-expanded depth-first node list for the
/// collapsible JSON tree view.
pub fn to_doc_tree(docs: &[serde_json::Value]) -> Vec<DocNode> {
    let mut out = Vec::new();
    for (i, doc) in docs.iter().enumerate() {
        push_doc_node(&mut out, 0, &format!("[{i}]"), &i.to_string(), doc);
    }
    out
}

/// Branches to collapse initially: everything below the top-level documents, so
/// each document shows its own keys but nested objects/arrays start folded.
pub fn default_doc_collapsed(full: &[DocNode]) -> std::collections::HashSet<String> {
    full.iter()
        .filter(|n| n.expandable && n.depth >= 1)
        .map(|n| n.path.clone())
        .collect()
}

/// The rows currently visible given the collapsed set, in display order. Returns
/// each node with whether it is currently expanded. A node hidden under a
/// collapsed ancestor is skipped.
pub fn visible_doc_rows<'a>(
    full: &'a [DocNode],
    collapsed: &std::collections::HashSet<String>,
) -> Vec<(&'a DocNode, bool)> {
    let mut out = Vec::new();
    let mut skip_below: Option<usize> = None;
    for n in full {
        if let Some(d) = skip_below {
            if n.depth > d {
                continue;
            }
            skip_below = None;
        }
        let expanded = n.expandable && !collapsed.contains(&n.path);
        out.push((n, expanded));
        if n.expandable && !expanded {
            skip_below = Some(n.depth);
        }
    }
    out
}

/// Presentation-ready view of a `ResultSet`, one arm per data shape. Redis
/// key/value results flatten into `Table` so every row-bearing view shares the
/// grid's selection/filter/edit machinery.
#[derive(Debug, Clone)]
pub enum ResultView {
    Table(GridModel),
    Documents(DocModel),
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

/// (label, value-text, frac) triples for the results bar chart: first
/// non-numeric column is the label, first numeric column the value, capped
/// at 30 rows and normalized by the largest absolute value. Empty when the
/// grid has no numeric column.
pub fn chart_data(g: &GridModel) -> Vec<(String, String, f32)> {
    let ncols = g.columns.len();
    if ncols == 0 || g.rows.is_empty() {
        return Vec::new();
    }
    let is_numeric = |c: usize| {
        let mut any = false;
        for row in &g.rows {
            match row.get(c) {
                Some(cell) if cell.is_null || cell.text.is_empty() => {}
                Some(cell) if cell.text.parse::<f64>().is_ok() => any = true,
                _ => return false,
            }
        }
        any
    };
    let Some(value_col) = (0..ncols).find(|&c| is_numeric(c)) else {
        return Vec::new();
    };
    let label_col = (0..ncols).find(|&c| !is_numeric(c));
    let mut out: Vec<(String, String, f64)> = Vec::new();
    for (i, row) in g.rows.iter().take(30).enumerate() {
        let raw = row
            .get(value_col)
            .map(|c| c.text.clone())
            .unwrap_or_default();
        let value: f64 = raw.parse().unwrap_or(0.0);
        let label = label_col
            .and_then(|lc| row.get(lc))
            .map(|c| c.text.clone())
            .unwrap_or_else(|| format!("row {}", i + 1));
        out.push((label, raw, value));
    }
    let max = out.iter().fold(0.0_f64, |m, (_, _, v)| m.max(v.abs()));
    if max <= 0.0 {
        return Vec::new();
    }
    out.into_iter()
        .map(|(l, s, v)| (l, s, (v.abs() / max) as f32))
        .collect()
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
        // Key/value results render through the same tabular grid as SQL so
        // they inherit selection, filtering, and inline editing. Column 0 is
        // the row identity (key / field / index / member), column 1 the value.
        ResultSet::KeyValue(pairs) => ResultView::Table(GridModel {
            columns: vec![
                VmColumn {
                    name: "key".into(),
                    type_name: "text".into(),
                },
                VmColumn {
                    name: "value".into(),
                    type_name: "text".into(),
                },
            ],
            rows: pairs
                .iter()
                .map(|(k, v)| {
                    vec![
                        VmCell {
                            text: k.clone(),
                            is_null: false,
                        },
                        redis_cell(v),
                    ]
                })
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
        for f in &db.functions {
            out.push(VmTreeNode {
                label: f.name.clone(),
                kind: "function".into(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdbs_core::result::{Cell, Column, RedisValue, ResultSet};
    use rdbs_core::schema::{Container, ContainerKind, Database, Field, Schema};

    #[test]
    fn doc_tree_folds_nested_and_toggles() {
        let docs = vec![serde_json::json!({
            "_id": "abc",
            "headers": { "host": "h", "port": 8080 }
        })];
        let full = to_doc_tree(&docs);
        // doc + _id + headers + host + port
        assert_eq!(full.len(), 5);
        let headers = full.iter().find(|n| n.key == "headers").unwrap();
        assert!(headers.expandable);
        assert_eq!(headers.path, "0.headers");

        // Default: doc open, nested `headers` folded -> host/port hidden.
        let collapsed = default_doc_collapsed(&full);
        let vis = visible_doc_rows(&full, &collapsed);
        assert_eq!(vis.len(), 3); // [0], _id, headers
        assert!(vis.iter().all(|(n, _)| n.key != "host"));

        // Expand headers -> host/port appear.
        let mut open = collapsed.clone();
        open.remove("0.headers");
        let vis = visible_doc_rows(&full, &open);
        assert_eq!(vis.len(), 5);
        assert!(vis.iter().any(|(n, _)| n.key == "host"));
    }

    #[test]
    fn chart_data_picks_label_and_numeric_columns() {
        let g = GridModel {
            columns: vec![
                VmColumn {
                    name: "sector".into(),
                    type_name: "text".into(),
                },
                VmColumn {
                    name: "total".into(),
                    type_name: "int".into(),
                },
            ],
            rows: vec![
                vec![
                    VmCell {
                        text: "energy".into(),
                        is_null: false,
                    },
                    VmCell {
                        text: "10".into(),
                        is_null: false,
                    },
                ],
                vec![
                    VmCell {
                        text: "tech".into(),
                        is_null: false,
                    },
                    VmCell {
                        text: "5".into(),
                        is_null: false,
                    },
                ],
            ],
        };
        let bars = chart_data(&g);
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0], ("energy".into(), "10".into(), 1.0));
        assert_eq!(bars[1].2, 0.5);
    }

    #[test]
    fn chart_data_empty_without_numeric_column() {
        let g = GridModel {
            columns: vec![VmColumn {
                name: "name".into(),
                type_name: "text".into(),
            }],
            rows: vec![vec![VmCell {
                text: "a".into(),
                is_null: false,
            }]],
        };
        assert!(chart_data(&g).is_empty());
    }

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
    fn keyvalue_maps_to_two_column_grid() {
        let rs = ResultSet::KeyValue(vec![
            ("k1".into(), RedisValue::Str("v1".into())),
            ("k2".into(), RedisValue::Int(9)),
            ("k3".into(), RedisValue::Nil),
        ]);
        let g = match to_result_view(&rs) {
            ResultView::Table(g) => g,
            other => panic!("expected Table, got {other:?}"),
        };
        assert_eq!(g.columns.len(), 2);
        assert_eq!(g.columns[0].name, "key");
        assert_eq!(g.rows.len(), 3);
        assert_eq!(g.rows[0][0].text, "k1");
        assert_eq!(g.rows[0][1].text, "v1");
        assert_eq!(g.rows[1][1].text, "9");
        assert!(g.rows[2][1].is_null);
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
                functions: Vec::new(),
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
                    if self.pk_cols.len() == 1 {
                        Some(0)
                    } else {
                        None
                    }
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
                VmColumn {
                    name: "id".into(),
                    type_name: "int4".into(),
                },
                VmColumn {
                    name: "name".into(),
                    type_name: "text".into(),
                },
            ],
            rows: vec![
                vec![
                    VmCell {
                        text: "1".into(),
                        is_null: false,
                    },
                    VmCell {
                        text: "alice".into(),
                        is_null: false,
                    },
                ],
                vec![
                    VmCell {
                        text: "2".into(),
                        is_null: false,
                    },
                    VmCell {
                        text: "bob".into(),
                        is_null: false,
                    },
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
                VmColumn {
                    name: "key".into(),
                    type_name: "text".into(),
                },
                VmColumn {
                    name: "value".into(),
                    type_name: "text".into(),
                },
            ],
            rows: vec![vec![
                VmCell {
                    text: "color".into(),
                    is_null: false,
                },
                VmCell {
                    text: "red".into(),
                    is_null: false,
                },
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
                VmColumn {
                    name: "a".into(),
                    type_name: "int4".into(),
                },
                VmColumn {
                    name: "b".into(),
                    type_name: "text".into(),
                },
                VmColumn {
                    name: "v".into(),
                    type_name: "text".into(),
                },
            ],
            rows: vec![vec![
                VmCell {
                    text: "1".into(),
                    is_null: false,
                },
                VmCell {
                    text: "x".into(),
                    is_null: false,
                },
                VmCell {
                    text: "old".into(),
                    is_null: false,
                },
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
