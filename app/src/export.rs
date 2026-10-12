//! File-export serializers: turn a result grid or the saved-connection list
//! into CSV / TSV / JSON / SQL / Markdown text. The caller picks the path via a
//! native save dialog.

use rdb_connstore::{Engine, SavedConnection};

use crate::model::GridModel;

/// RFC-4180 field escaping shared by the CSV serializers.
fn csv_esc(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn engine_label(e: Engine) -> &'static str {
    e.key()
}

/// URI scheme for a connection-string URL. Distinct from `engine_label`:
/// postgres/mongo use the canonical `postgresql`/`mongodb` schemes.
fn engine_scheme(e: Engine) -> &'static str {
    e.scheme()
}

/// Connection-string URL, e.g. `postgresql://user:pass@host:5432/db`. `password`
/// is the real secret (percent-encoded) so the export can be pasted back into
/// the "import URL" field and re-used; when absent the credential is `user@`.
/// The user segment is omitted when empty and the `/database` when absent.
pub fn conn_to_url(c: &SavedConnection, password: Option<&str>) -> String {
    let cred = if c.user.is_empty() {
        String::new()
    } else {
        match password {
            Some(p) if !p.is_empty() => {
                format!("{}:{}@", c.user, rdb_core::conn::percent_encode_userinfo(p))
            }
            _ => format!("{}@", c.user),
        }
    };
    let db = c
        .database
        .as_deref()
        .map(|d| format!("/{d}"))
        .unwrap_or_default();
    format!(
        "{}://{cred}{}:{}{db}",
        engine_scheme(c.engine),
        c.host,
        c.port
    )
}

/// Saved connections as CSV. The `url` column embeds the real password
/// (percent-encoded) via `password_for` so the export is a re-usable backup —
/// it therefore contains plaintext secrets and should be treated as sensitive.
pub fn conns_to_csv(
    conns: &[SavedConnection],
    password_for: impl Fn(&SavedConnection) -> Option<String>,
) -> String {
    let mut out = String::from("name,engine,host,port,database,user,url\n");
    for c in conns {
        let pw = password_for(c);
        let row = [
            csv_esc(&c.name),
            engine_label(c.engine).to_string(),
            csv_esc(&c.host),
            c.port.to_string(),
            csv_esc(c.database.as_deref().unwrap_or("")),
            csv_esc(&c.user),
            csv_esc(&conn_to_url(c, pw.as_deref())),
        ];
        out.push_str(&row.join(","));
        out.push('\n');
    }
    out
}

/// Saved connections as a pretty JSON array. Mirrors `conns_to_csv`: the `url`
/// embeds the real password (percent-encoded) via `password_for` for re-import,
/// so the output contains plaintext secrets and is sensitive. Built by hand (not
/// via `Serialize`) so the computed `url` is included.
pub fn conns_to_json(
    conns: &[SavedConnection],
    password_for: impl Fn(&SavedConnection) -> Option<String>,
) -> String {
    use serde_json::{json, Value};
    let arr: Vec<Value> = conns
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "engine": engine_label(c.engine),
                "host": c.host,
                "port": c.port,
                "database": c.database,
                "user": c.user,
                "url": conn_to_url(c, password_for(c).as_deref()),
            })
        })
        .collect();
    serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".into())
}

/// RFC-4180-style CSV: fields quoted when they contain a comma, quote or
/// newline; quotes doubled.
pub fn to_csv(g: &GridModel) -> String {
    let mut out = String::new();
    let header: Vec<String> = g.columns.iter().map(|c| csv_esc(&c.name)).collect();
    out.push_str(&header.join(","));
    out.push('\n');
    for row in &g.rows {
        let cells: Vec<String> = row.iter().map(|c| csv_esc(&c.text)).collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

/// TSV for the clipboard (pastes into spreadsheets); tabs/newlines inside
/// cells become spaces.
pub fn to_tsv(g: &GridModel) -> String {
    tsv(g, true)
}

/// TSV without the header row — what a partial selection copies, so pasting a
/// couple of cells doesn't drag a column name along with them.
pub fn to_tsv_body(g: &GridModel) -> String {
    tsv(g, false)
}

fn tsv(g: &GridModel, header_row: bool) -> String {
    fn clean(s: &str) -> String {
        s.replace(['\t', '\n', '\r'], " ")
    }
    let mut out = String::new();
    if header_row {
        let header: Vec<String> = g.columns.iter().map(|c| clean(&c.name)).collect();
        out.push_str(&header.join("\t"));
        out.push('\n');
    }
    for row in &g.rows {
        let cells: Vec<String> = row.iter().map(|c| clean(&c.text)).collect();
        out.push_str(&cells.join("\t"));
        out.push('\n');
    }
    out
}

/// Clipboard flavour of a TSV: the same text without the trailing newline a
/// text file wants. Copying a single cell out of the result grid otherwise
/// put the value plus an Enter on the clipboard, so pasting it into the query
/// editor opened a new line.
pub fn for_clipboard(mut s: String) -> String {
    if s.ends_with('\n') {
        s.pop();
    }
    s
}

/// JSON array of row objects (`[{ "col": "val", "nullcol": null }]`). Built
/// through `serde_json` so escaping is correct.
pub fn to_json(g: &GridModel) -> String {
    use serde_json::{Map, Value};
    let rows: Vec<Value> = g
        .rows
        .iter()
        .map(|row| {
            let mut obj = Map::new();
            for (col, cell) in g.columns.iter().zip(row) {
                let v = if cell.is_null {
                    Value::Null
                } else {
                    Value::String(cell.text.clone())
                };
                obj.insert(col.name.clone(), v);
            }
            Value::Object(obj)
        })
        .collect();
    serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".into())
}

/// GitHub-flavored Markdown table. Pipes and newlines inside cells are escaped
/// so the table stays intact.
pub fn to_markdown(g: &GridModel) -> String {
    fn cell(s: &str) -> String {
        s.replace('|', "\\|").replace(['\n', '\r'], " ")
    }
    let mut out = String::new();
    let names: Vec<String> = g.columns.iter().map(|c| cell(&c.name)).collect();
    out.push_str(&format!("| {} |\n", names.join(" | ")));
    let sep: Vec<&str> = g.columns.iter().map(|_| "---").collect();
    out.push_str(&format!("| {} |\n", sep.join(" | ")));
    for row in &g.rows {
        let cells: Vec<String> = row.iter().map(|c| cell(&c.text)).collect();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    out
}

/// `INSERT INTO <table> (...) VALUES (...);` — one statement per row.
// ponytail: every non-null value is emitted as a single-quoted string literal
// (quotes doubled); add per-column type formatting if a driver needs typed
// literals (numbers/bools unquoted).
pub fn to_sql_insert(g: &GridModel, table: &str) -> String {
    let cols: Vec<&str> = g.columns.iter().map(|c| c.name.as_str()).collect();
    let cols_sql = cols.join(", ");
    let mut out = String::new();
    for row in &g.rows {
        let vals: Vec<String> = row
            .iter()
            .map(|c| {
                if c.is_null {
                    "NULL".to_string()
                } else {
                    format!("'{}'", c.text.replace('\'', "''"))
                }
            })
            .collect();
        out.push_str(&format!(
            "INSERT INTO {table} ({cols_sql}) VALUES ({});\n",
            vals.join(", ")
        ));
    }
    out
}

/// Parsed table import payload ready for execution.
#[derive(Debug)]
pub enum ImportPayload {
    WriteOps(Vec<rdb_core::write::WriteOp>),
    SqlStatements(Vec<String>),
}

fn json_val_to_cell(v: &serde_json::Value) -> rdb_core::result::Cell {
    use rdb_core::result::Cell;
    match v {
        serde_json::Value::Null => Cell::Null,
        serde_json::Value::Bool(b) => Cell::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Cell::Int(i)
            } else if let Some(f) = n.as_f64() {
                Cell::Float(f)
            } else {
                Cell::Text(n.to_string())
            }
        }
        serde_json::Value::String(s) => Cell::Text(s.clone()),
        other => Cell::Text(other.to_string()),
    }
}

fn json_obj_to_op(
    table: &rdb_core::write::TableRef,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Option<rdb_core::write::WriteOp> {
    let values: Vec<(String, rdb_core::result::Cell)> = obj
        .iter()
        .map(|(k, v)| (k.clone(), json_val_to_cell(v)))
        .collect();
    (!values.is_empty()).then(|| rdb_core::write::WriteOp::Insert {
        table: table.clone(),
        values,
    })
}

/// Parse a JSON array of objects (or single object / NDJSON) into `WriteOp::Insert`s.
pub fn parse_import_json(
    table: &rdb_core::write::TableRef,
    content: &str,
) -> Result<Vec<rdb_core::write::WriteOp>, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("empty JSON file".into());
    }
    let val: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| format!("invalid JSON: {e}"))?;
    let mut ops = Vec::new();
    match &val {
        serde_json::Value::Array(arr) => {
            for item in arr {
                let obj = item
                    .as_object()
                    .ok_or_else(|| "JSON array items must be objects".to_string())?;
                if let Some(op) = json_obj_to_op(table, obj) {
                    ops.push(op);
                }
            }
        }
        serde_json::Value::Object(obj) => {
            if let Some(op) = json_obj_to_op(table, obj) {
                ops.push(op);
            }
        }
        _ => return Err("JSON root must be an array of objects or an object".into()),
    }
    if ops.is_empty() {
        return Err("no rows found in JSON".into());
    }
    Ok(ops)
}

fn parse_csv_records(content: &str) -> Vec<Vec<(String, bool)>> {
    let mut rows: Vec<Vec<(String, bool)>> = Vec::new();
    let mut cur_row: Vec<(String, bool)> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut was_quoted = false;
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' && field.is_empty() {
            in_quotes = true;
            was_quoted = true;
        } else if c == ',' {
            cur_row.push((std::mem::take(&mut field), was_quoted));
            was_quoted = false;
        } else if c == '\r' || c == '\n' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            cur_row.push((std::mem::take(&mut field), was_quoted));
            was_quoted = false;
            if cur_row.len() > 1 || !cur_row[0].0.is_empty() || cur_row[0].1 {
                rows.push(std::mem::take(&mut cur_row));
            } else {
                cur_row.clear();
            }
        } else {
            field.push(c);
        }
    }
    if !field.is_empty() || was_quoted || !cur_row.is_empty() {
        cur_row.push((field, was_quoted));
        if cur_row.len() > 1 || !cur_row[0].0.is_empty() || cur_row[0].1 {
            rows.push(cur_row);
        }
    }
    rows
}

fn csv_cell_value(raw: &str, was_quoted: bool) -> rdb_core::result::Cell {
    use rdb_core::result::Cell;
    if !was_quoted && (raw.is_empty() || raw.eq_ignore_ascii_case("null")) {
        return Cell::Null;
    }
    if !was_quoted {
        if raw.eq_ignore_ascii_case("true") {
            return Cell::Bool(true);
        }
        if raw.eq_ignore_ascii_case("false") {
            return Cell::Bool(false);
        }
        if let Ok(i) = raw.parse::<i64>() {
            return Cell::Int(i);
        }
        if let Ok(f) = raw.parse::<f64>() {
            return Cell::Float(f);
        }
    }
    Cell::Text(raw.to_string())
}

fn csv_row_to_op(
    table: &rdb_core::write::TableRef,
    headers: &[String],
    row: &[(String, bool)],
) -> Option<rdb_core::write::WriteOp> {
    let values: Vec<(String, rdb_core::result::Cell)> = headers
        .iter()
        .zip(row.iter())
        .filter(|(h, _)| !h.trim().is_empty())
        .map(|(h, (val, quoted))| (h.trim().to_string(), csv_cell_value(val, *quoted)))
        .collect();
    (!values.is_empty()).then(|| rdb_core::write::WriteOp::Insert {
        table: table.clone(),
        values,
    })
}

/// Parse an RFC-4180 CSV document (header row + data rows) into `WriteOp::Insert`s.
pub fn parse_import_csv(
    table: &rdb_core::write::TableRef,
    content: &str,
) -> Result<Vec<rdb_core::write::WriteOp>, String> {
    let records = parse_csv_records(content);
    let Some((header_row, data_rows)) = records.split_first() else {
        return Err("empty CSV file".into());
    };
    let headers: Vec<String> = header_row.iter().map(|(h, _)| h.clone()).collect();
    let mut ops = Vec::with_capacity(data_rows.len());
    for (row_idx, r) in data_rows.iter().enumerate() {
        if r.len() > headers.len() {
            return Err(format!(
                "CSV row {} has {} column(s), expected {}",
                row_idx + 2,
                r.len(),
                headers.len()
            ));
        }
        if let Some(op) = csv_row_to_op(table, &headers, r) {
            ops.push(op);
        }
    }
    if ops.is_empty() {
        return Err("no data rows found in CSV".into());
    }
    Ok(ops)
}

/// Dispatch an imported file by extension (`.sql`, `.json`, `.csv`) into an `ImportPayload`.
pub fn parse_import_file(
    table: &rdb_core::write::TableRef,
    path: &str,
    content: &str,
) -> Result<ImportPayload, String> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let trimmed = content.trim_start();
    if ext == "sql" {
        let stmts = crate::editor::split_statements(content);
        if stmts.is_empty() {
            return Err("no SQL statements found in file".into());
        }
        Ok(ImportPayload::SqlStatements(stmts))
    } else if ext == "json"
        || (ext.is_empty() && (trimmed.starts_with('[') || trimmed.starts_with('{')))
    {
        parse_import_json(table, content).map(ImportPayload::WriteOps)
    } else {
        parse_import_csv(table, content).map(ImportPayload::WriteOps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> GridModel {
        use crate::model::{VmCell, VmColumn};
        GridModel {
            columns: vec![
                VmColumn {
                    name: "id".into(),
                    type_name: "int".into(),
                },
                VmColumn {
                    name: "name".into(),
                    type_name: "text".into(),
                },
            ],
            rows: vec![vec![
                VmCell {
                    text: "1".into(),
                    is_null: false,
                },
                VmCell {
                    text: "a,\"b\"\n".into(),
                    is_null: false,
                },
            ]],
        }
    }

    #[test]
    fn csv_quotes_special_chars() {
        assert_eq!(to_csv(&grid()), "id,name\n1,\"a,\"\"b\"\"\n\"\n");
    }

    #[test]
    fn tsv_strips_control_chars() {
        assert_eq!(to_tsv(&grid()), "id\tname\n1\ta,\"b\" \n");
    }

    #[test]
    fn tsv_body_drops_the_header() {
        assert_eq!(to_tsv_body(&grid()), "1\ta,\"b\" \n");
    }

    #[test]
    fn clipboard_tsv_drops_only_the_final_newline() {
        assert_eq!(for_clipboard(to_tsv_body(&grid())), "1\ta,\"b\" ");
        // An empty trailing row is still a row: only one newline comes off.
        assert_eq!(for_clipboard("a\n\n".to_string()), "a\n");
        assert_eq!(for_clipboard(String::new()), "");
    }

    #[test]
    fn conns_csv_embeds_encoded_password_and_escapes() {
        let mut c =
            SavedConnection::new("db, prod", Engine::Postgres, "localhost", 5432, "postgres");
        c.database = Some("app".into());
        // Special chars in the password must be percent-encoded so the URL parses.
        let csv = conns_to_csv(&[c], |_| Some("p@ss:1".into()));
        assert_eq!(
            csv,
            "name,engine,host,port,database,user,url\n\"db, prod\",postgres,localhost,5432,app,postgres,postgresql://postgres:p%40ss%3A1@localhost:5432/app\n"
        );
    }

    #[test]
    fn conn_url_embeds_password_per_engine() {
        let mut c = SavedConnection::new("c", Engine::Postgres, "203.0.113.22", 5432, "app");
        c.database = Some("analytics".into());
        assert_eq!(
            conn_to_url(&c, Some("secret")),
            "postgresql://app:secret@203.0.113.22:5432/analytics"
        );

        // No stored password → bare `user@`, still re-importable.
        let m = SavedConnection::new("m", Engine::Mongo, "203.0.113.111", 27017, "root");
        assert_eq!(conn_to_url(&m, None), "mongodb://root@203.0.113.111:27017");
    }

    #[test]
    fn conn_url_omits_empty_user_and_missing_db() {
        let c = SavedConnection::new("c", Engine::Redis, "localhost", 6379, "");
        assert_eq!(conn_to_url(&c, Some("x")), "redis://localhost:6379");
    }

    #[test]
    fn conns_json_embeds_password_in_url() {
        let mut c = SavedConnection::new("prod", Engine::Postgres, "localhost", 5432, "app");
        c.database = Some("db".into());
        let json = conns_to_json(&[c], |_| Some("secret".into()));
        assert!(json.contains("\"url\": \"postgresql://app:secret@localhost:5432/db\""));
    }

    #[test]
    fn json_nulls_and_escaping() {
        use crate::model::{VmCell, VmColumn};
        let g = GridModel {
            columns: vec![VmColumn {
                name: "note".into(),
                type_name: "text".into(),
            }],
            rows: vec![
                vec![VmCell {
                    text: "he said \"hi\"".into(),
                    is_null: false,
                }],
                vec![VmCell {
                    text: String::new(),
                    is_null: true,
                }],
            ],
        };
        assert_eq!(
            to_json(&g),
            "[\n  {\n    \"note\": \"he said \\\"hi\\\"\"\n  },\n  {\n    \"note\": null\n  }\n]"
        );
    }

    #[test]
    fn markdown_escapes_pipes() {
        assert_eq!(
            to_markdown(&grid()),
            "| id | name |\n| --- | --- |\n| 1 | a,\"b\"  |\n"
        );
    }

    #[test]
    fn sql_insert_quotes_and_nulls() {
        use crate::model::{VmCell, VmColumn};
        let g = GridModel {
            columns: vec![
                VmColumn {
                    name: "id".into(),
                    type_name: "int".into(),
                },
                VmColumn {
                    name: "name".into(),
                    type_name: "text".into(),
                },
            ],
            rows: vec![vec![
                VmCell {
                    text: "1".into(),
                    is_null: false,
                },
                VmCell {
                    text: "O'Brien".into(),
                    is_null: true,
                },
            ]],
        };
        assert_eq!(
            to_sql_insert(&g, "users"),
            "INSERT INTO users (id, name) VALUES ('1', NULL);\n"
        );
        let mut g2 = g.clone();
        g2.rows[0][1].is_null = false;
        assert_eq!(
            to_sql_insert(&g2, "users"),
            "INSERT INTO users (id, name) VALUES ('1', 'O''Brien');\n"
        );
    }

    #[test]
    fn import_csv_parses_headers_quotes_and_types() {
        let table = rdb_core::write::TableRef::named("users");
        let csv = "id,name,note\n1,\"Ada, Lovelace\",NULL\n2,Lin,\"\"\n";
        let ops = parse_import_csv(&table, csv).unwrap();
        assert_eq!(ops.len(), 2);
        match &ops[0] {
            rdb_core::write::WriteOp::Insert { values, .. } => {
                assert_eq!(values[0].0, "id");
                assert!(matches!(values[0].1, rdb_core::result::Cell::Int(1)));
                assert_eq!(values[1].0, "name");
                assert!(
                    matches!(&values[1].1, rdb_core::result::Cell::Text(s) if s == "Ada, Lovelace")
                );
                assert_eq!(values[2].0, "note");
                assert!(matches!(values[2].1, rdb_core::result::Cell::Null));
            }
            other => panic!("expected Insert, got {other:?}"),
        }
    }

    #[test]
    fn import_json_parses_array_of_objects() {
        let table = rdb_core::write::TableRef::named("users");
        let json = r#"[{"id": 10, "name": "Budi", "active": true, "deleted_at": null}]"#;
        let ops = parse_import_json(&table, json).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            rdb_core::write::WriteOp::Insert { values, .. } => {
                assert!(values
                    .iter()
                    .any(|(k, v)| k == "id" && matches!(v, rdb_core::result::Cell::Int(10))));
                assert!(values.iter().any(|(k, v)| k == "name"
                    && matches!(v, rdb_core::result::Cell::Text(s) if s == "Budi")));
                assert!(
                    values
                        .iter()
                        .any(|(k, v)| k == "active"
                            && matches!(v, rdb_core::result::Cell::Bool(true)))
                );
                assert!(values
                    .iter()
                    .any(|(k, v)| k == "deleted_at" && matches!(v, rdb_core::result::Cell::Null)));
            }
            other => panic!("expected Insert, got {other:?}"),
        }
    }

    #[test]
    fn import_sql_splits_dump_statements() {
        let table = rdb_core::write::TableRef::named("users");
        let sql = "INSERT INTO users (id, name) VALUES (1, 'a');\nINSERT INTO users (id, name) VALUES (2, 'b');";
        match parse_import_file(&table, "dump.sql", sql).unwrap() {
            ImportPayload::SqlStatements(stmts) => {
                assert_eq!(
                    stmts,
                    vec![
                        "INSERT INTO users (id, name) VALUES (1, 'a')",
                        "INSERT INTO users (id, name) VALUES (2, 'b')",
                    ]
                );
            }
            other => panic!("expected SqlStatements, got {other:?}"),
        }
    }
}
