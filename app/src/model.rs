//! Pure conversion from dbm-core result/schema types into flat view-model
//! structs the UI binds to. No Slint imports here so it stays unit-testable;
//! `main.rs` maps these into the Slint-generated structs.

use dbm_core::result::{Cell, RedisValue, ResultSet};
use dbm_core::schema::{ContainerKind, Schema};

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

/// Flat grid model covering all four ResultSet variants.
#[derive(Debug, Default, Clone)]
pub struct GridModel {
    pub columns: Vec<VmColumn>,
    pub rows: Vec<Vec<VmCell>>,
    /// Pretty JSON for Documents.
    pub json: String,
    pub is_documents: bool,
    /// Status text for Affected (e.g. "3 rows affected").
    pub status: String,
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

/// Convert any ResultSet into the grid view-model.
pub fn to_grid_model(rs: &ResultSet) -> GridModel {
    match rs {
        ResultSet::Tabular { cols, rows } => GridModel {
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
            ..Default::default()
        },
        ResultSet::KeyValue(pairs) => GridModel {
            columns: vec![
                VmColumn {
                    name: "key".into(),
                    type_name: "".into(),
                },
                VmColumn {
                    name: "value".into(),
                    type_name: "".into(),
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
            ..Default::default()
        },
        ResultSet::Documents(docs) => {
            let json = serde_json::to_string_pretty(docs).unwrap_or_else(|_| "[]".into());
            GridModel {
                json,
                is_documents: true,
                ..Default::default()
            }
        }
        ResultSet::Affected(n) => GridModel {
            status: format!("{} rows affected", n),
            ..Default::default()
        },
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
    use dbm_core::result::{Cell, Column, RedisValue, ResultSet};
    use dbm_core::schema::{Container, ContainerKind, Database, Field, Schema};

    #[test]
    fn tabular_maps_cols_and_rows_with_null_flag() {
        let rs = ResultSet::Tabular {
            cols: vec![Column {
                name: "id".into(),
                type_name: "int4".into(),
            }],
            rows: vec![vec![Cell::Int(7)], vec![Cell::Null]],
        };
        let grid = to_grid_model(&rs);
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
        let grid = to_grid_model(&ResultSet::Affected(3));
        assert_eq!(grid.columns.len(), 0);
        assert_eq!(grid.status, "3 rows affected");
    }

    #[test]
    fn keyvalue_maps_redis_pairs() {
        let rs = ResultSet::KeyValue(vec![
            ("k1".into(), RedisValue::Str("v1".into())),
            ("k2".into(), RedisValue::Int(9)),
            ("k3".into(), RedisValue::Nil),
        ]);
        let grid = to_grid_model(&rs);
        assert_eq!(grid.columns.len(), 2); // key, value
        assert_eq!(grid.rows[0][0].text, "k1");
        assert_eq!(grid.rows[0][1].text, "v1");
        assert_eq!(grid.rows[1][1].text, "9");
        assert_eq!(grid.rows[2][1].text, "(nil)");
        assert!(grid.rows[2][1].is_null);
    }

    #[test]
    fn documents_render_as_json_text_block() {
        let rs = ResultSet::Documents(vec![serde_json::json!({"a": 1})]);
        let grid = to_grid_model(&rs);
        assert!(grid.json.contains("\"a\""));
        assert!(grid.is_documents);
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
        assert_eq!(nodes[0].depth, 0);
        assert_eq!(nodes[0].kind, "database");
        assert_eq!(nodes[1].label, "users");
        assert_eq!(nodes[1].depth, 1);
        assert_eq!(nodes[1].kind, "table");
        assert_eq!(nodes[2].label, "id: int4");
        assert_eq!(nodes[2].depth, 2);
        assert_eq!(nodes[2].kind, "field");
    }
}
