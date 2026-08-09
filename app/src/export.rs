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
    match e {
        Engine::Postgres => "postgres",
        Engine::MySql => "mysql",
        Engine::Redis => "redis",
        Engine::Mongo => "mongo",
        Engine::Sqlite => "sqlite",
        Engine::Cassandra => "cassandra",
        Engine::Mssql => "mssql",
    }
}

/// URI scheme for a connection-string URL. Distinct from `engine_label`:
/// postgres/mongo use the canonical `postgresql`/`mongodb` schemes.
fn engine_scheme(e: Engine) -> &'static str {
    match e {
        Engine::Postgres => "postgresql",
        Engine::MySql => "mysql",
        Engine::Redis => "redis",
        Engine::Mongo => "mongodb",
        Engine::Sqlite => "sqlite",
        Engine::Cassandra => "cassandra",
        Engine::Mssql => "sqlserver",
    }
}

/// Percent-encode a URL userinfo component (RFC 3986 unreserved set kept as-is,
/// everything else `%XX`). Mirrors `percent_decode` in connstore's URL parser so
/// an exported password round-trips back through the "import URL" flow.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
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
            Some(p) if !p.is_empty() => format!("{}:{}@", c.user, percent_encode(p)),
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
    fn clean(s: &str) -> String {
        s.replace(['\t', '\n', '\r'], " ")
    }
    let mut out = String::new();
    let header: Vec<String> = g.columns.iter().map(|c| clean(&c.name)).collect();
    out.push_str(&header.join("\t"));
    out.push('\n');
    for row in &g.rows {
        let cells: Vec<String> = row.iter().map(|c| clean(&c.text)).collect();
        out.push_str(&cells.join("\t"));
        out.push('\n');
    }
    out
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
        let mut c = SavedConnection::new("c", Engine::Postgres, "10.2.238.22", 5432, "app");
        c.database = Some("oss_rba".into());
        assert_eq!(
            conn_to_url(&c, Some("secret")),
            "postgresql://app:secret@10.2.238.22:5432/oss_rba"
        );

        // No stored password → bare `user@`, still re-importable.
        let m = SavedConnection::new("m", Engine::Mongo, "10.2.238.111", 27017, "root");
        assert_eq!(conn_to_url(&m, None), "mongodb://root@10.2.238.111:27017");
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
}
