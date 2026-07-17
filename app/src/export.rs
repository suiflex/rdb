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
    }
}

/// Saved connections as CSV. Non-secret fields only — passwords live in the
/// keychain and are never part of `SavedConnection`, so the dump is safe.
pub fn conns_to_csv(conns: &[SavedConnection]) -> String {
    let mut out = String::from("name,engine,host,port,database,user\n");
    for c in conns {
        let row = [
            csv_esc(&c.name),
            engine_label(c.engine).to_string(),
            csv_esc(&c.host),
            c.port.to_string(),
            csv_esc(c.database.as_deref().unwrap_or("")),
            csv_esc(&c.user),
        ];
        out.push_str(&row.join(","));
        out.push('\n');
    }
    out
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
    fn conns_csv_has_no_secrets_and_escapes() {
        let mut c =
            SavedConnection::new("db, prod", Engine::Postgres, "localhost", 5432, "postgres");
        c.database = Some("app".into());
        let csv = conns_to_csv(&[c]);
        assert_eq!(
            csv,
            "name,engine,host,port,database,user\n\"db, prod\",postgres,localhost,5432,app,postgres\n"
        );
        assert!(!csv.to_lowercase().contains("password"));
    }
}
