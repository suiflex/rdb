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

fn json_val_to_cell(v: &serde_json::Value, large_integer_prefix: &str) -> rdb_core::result::Cell {
    use rdb_core::result::Cell;
    match v {
        serde_json::Value::Null => Cell::Null,
        serde_json::Value::Bool(b) => Cell::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Cell::Int(i)
            } else if n.is_u64() {
                Cell::Text(n.to_string())
            } else if let Some(f) = n.as_f64().filter(|f| f.is_finite()) {
                Cell::Float(f)
            } else {
                Cell::Text(n.to_string())
            }
        }
        serde_json::Value::String(s) => Cell::Text(
            s.strip_prefix(large_integer_prefix)
                .unwrap_or(s)
                .to_string(),
        ),
        other => Cell::Text(other.to_string()),
    }
}

const LARGE_JSON_INTEGER_PREFIX: &str = "__rdb_large_json_integer_";

fn json_strings_contain(content: &str, needle: &str) -> bool {
    let mut string_start = None;
    let mut escaped = false;

    for (end, c) in content.char_indices() {
        if let Some(start) = string_start {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                if serde_json::from_str::<String>(&content[start..=end])
                    .is_ok_and(|value| value.contains(needle))
                {
                    return true;
                }
                string_start = None;
            }
        } else if c == '"' {
            string_start = Some(end);
        }
    }
    false
}

fn append_json_non_number_char(
    c: char,
    output: &mut String,
    in_string: &mut bool,
    escaped: &mut bool,
) {
    output.push(c);
    if *in_string {
        if *escaped {
            *escaped = false;
        } else if c == '\\' {
            *escaped = true;
        } else if c == '"' {
            *in_string = false;
        }
    } else if c == '"' {
        *in_string = true;
    }
}

fn append_json_number(token: &str, prefix: &str, output: &mut String) {
    let digits = token.strip_prefix('-').unwrap_or(token);
    let valid_integer = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    if valid_integer && token.parse::<i64>().is_err() && token.parse::<u64>().is_err() {
        output.push('"');
        output.push_str(prefix);
        output.push_str(token);
        output.push('"');
    } else {
        output.push_str(token);
    }
}

fn preserve_large_json_integers(content: &str, prefix: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut number = String::new();

    for c in content.chars() {
        if in_string {
            append_json_non_number_char(c, &mut output, &mut in_string, &mut escaped);
        } else if !number.is_empty()
            && (c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        {
            number.push(c);
        } else {
            if !number.is_empty() {
                append_json_number(&number, prefix, &mut output);
                number.clear();
            }
            if c == '-' || c.is_ascii_digit() {
                number.push(c);
            } else {
                append_json_non_number_char(c, &mut output, &mut in_string, &mut escaped);
            }
        }
    }
    if !number.is_empty() {
        append_json_number(&number, prefix, &mut output);
    }
    output
}

fn parse_json_objects(
    content: &str,
) -> Result<(Vec<serde_json::Map<String, serde_json::Value>>, String), String> {
    let mut large_integer_prefix = LARGE_JSON_INTEGER_PREFIX.to_string();
    while content.contains(&large_integer_prefix)
        || json_strings_contain(content, &large_integer_prefix)
    {
        large_integer_prefix.push('_');
    }
    let preserved = preserve_large_json_integers(content, &large_integer_prefix);
    let trimmed = preserved.trim();
    if trimmed.is_empty() {
        return Err("empty JSON file".into());
    }
    let objs = if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        match val {
            serde_json::Value::Array(arr) => arr
                .into_iter()
                .map(|item| match item {
                    serde_json::Value::Object(obj) => Ok(obj),
                    _ => Err("JSON array items must be objects".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()?,
            serde_json::Value::Object(obj) => vec![obj],
            _ => return Err("JSON root must be an array of objects or an object".into()),
        }
    } else {
        let mut objs = Vec::new();
        for (idx, line) in trimmed.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let val: serde_json::Value = serde_json::from_str(line)
                .map_err(|e| format!("invalid JSON on line {}: {e}", idx + 1))?;
            match val {
                serde_json::Value::Object(obj) => objs.push(obj),
                _ => return Err(format!("NDJSON line {} must be a JSON object", idx + 1)),
            }
        }
        objs
    };
    Ok((objs, large_integer_prefix))
}

#[derive(Clone, Copy)]
enum CsvState {
    Unquoted,
    Quoted,
    AfterQuote,
}

struct CsvParser {
    rows: Vec<Vec<(String, bool)>>,
    row: Vec<(String, bool)>,
    field: String,
    was_quoted: bool,
    record_number: usize,
}

impl CsvParser {
    fn new() -> Self {
        Self {
            rows: Vec::new(),
            row: Vec::new(),
            field: String::new(),
            was_quoted: false,
            record_number: 1,
        }
    }

    fn finish_field(&mut self) {
        self.row
            .push((std::mem::take(&mut self.field), self.was_quoted));
        self.was_quoted = false;
    }

    fn finish_record(&mut self, skip_empty: bool) {
        self.finish_field();
        if !skip_empty || self.row.len() > 1 || !self.row[0].0.is_empty() || self.row[0].1 {
            self.rows.push(std::mem::take(&mut self.row));
        } else {
            self.row.clear();
        }
        self.record_number += 1;
    }

    fn consume_quoted(
        &mut self,
        c: char,
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    ) -> CsvState {
        if c != '"' {
            self.field.push(c);
            return CsvState::Quoted;
        }
        if chars.peek() == Some(&'"') {
            chars.next();
            self.field.push('"');
            CsvState::Quoted
        } else {
            CsvState::AfterQuote
        }
    }

    fn consume_after_quote(
        &mut self,
        c: char,
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    ) -> Result<CsvState, String> {
        match c {
            ',' => {
                self.finish_field();
                Ok(CsvState::Unquoted)
            }
            '\r' | '\n' => {
                consume_csv_newline(c, chars);
                self.finish_record(false);
                Ok(CsvState::Unquoted)
            }
            _ => Err(format!(
                "invalid CSV character {c:?} after closing quote in record {}",
                self.record_number
            )),
        }
    }

    fn consume_unquoted(
        &mut self,
        c: char,
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    ) -> Result<CsvState, String> {
        match c {
            '"' if self.field.is_empty() => {
                self.was_quoted = true;
                Ok(CsvState::Quoted)
            }
            '"' => Err(format!(
                "unexpected quote in unquoted CSV field in record {}",
                self.record_number
            )),
            ',' => {
                self.finish_field();
                Ok(CsvState::Unquoted)
            }
            '\r' | '\n' => {
                consume_csv_newline(c, chars);
                self.finish_record(true);
                Ok(CsvState::Unquoted)
            }
            _ => {
                self.field.push(c);
                Ok(CsvState::Unquoted)
            }
        }
    }

    fn finish(mut self, state: CsvState) -> Result<Vec<Vec<(String, bool)>>, String> {
        if matches!(state, CsvState::Quoted) {
            return Err(format!(
                "unclosed quoted CSV field in record {}",
                self.record_number
            ));
        }
        let has_pending_field = !self.field.is_empty()
            || self.was_quoted
            || !self.row.is_empty()
            || matches!(state, CsvState::AfterQuote);
        if has_pending_field {
            self.finish_field();
            if self.row.len() > 1 || !self.row[0].0.is_empty() || self.row[0].1 {
                self.rows.push(self.row);
            }
        }
        Ok(self.rows)
    }
}

fn consume_csv_newline(c: char, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    if c == '\r' && chars.peek() == Some(&'\n') {
        chars.next();
    }
}

fn parse_csv_records(content: &str) -> Result<Vec<Vec<(String, bool)>>, String> {
    let mut parser = CsvParser::new();
    let mut state = CsvState::Unquoted;
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        state = match state {
            CsvState::Quoted => parser.consume_quoted(c, &mut chars),
            CsvState::AfterQuote => parser.consume_after_quote(c, &mut chars)?,
            CsvState::Unquoted => parser.consume_unquoted(c, &mut chars)?,
        };
    }
    parser.finish(state)
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
        let digits = raw
            .strip_prefix('+')
            .or_else(|| raw.strip_prefix('-'))
            .unwrap_or(raw);
        let is_integer = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
        let has_leading_zero = digits.len() > 1 && digits.starts_with('0');
        if is_integer {
            if !has_leading_zero {
                if let Ok(i) = raw.parse::<i64>() {
                    return Cell::Int(i);
                }
            }
            return Cell::Text(raw.to_string());
        }
        if let Ok(f) = raw.parse::<f64>() {
            if f.is_finite() {
                return Cell::Float(f);
            }
        }
    }
    Cell::Text(raw.to_string())
}

/// Parsed tabular/SQL import sheet backing the Import JSON / CSV Wizard preview.
#[derive(Debug, Clone)]
pub struct ImportSheet {
    pub format_label: &'static str, // "JSON" | "CSV" | "SQL"
    pub file_name: String,
    pub size_bytes: usize,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<rdb_core::result::Cell>>,
    pub sql_statements: Vec<String>,
}

pub fn format_byte_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn collect_json_headers(objs: &[serde_json::Map<String, serde_json::Value>]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    objs.iter()
        .flat_map(|obj| obj.keys())
        .filter(|k| seen.insert((*k).clone()))
        .cloned()
        .collect()
}

fn build_json_sheet_row(
    headers: &[String],
    obj: &serde_json::Map<String, serde_json::Value>,
    large_integer_prefix: &str,
) -> Vec<rdb_core::result::Cell> {
    headers
        .iter()
        .map(|h| match obj.get(h) {
            Some(v) => json_val_to_cell(v, large_integer_prefix),
            None => rdb_core::result::Cell::Null,
        })
        .collect()
}

fn parse_json_sheet(
    file_name: String,
    size_bytes: usize,
    content: &str,
) -> Result<ImportSheet, String> {
    let (objs, large_integer_prefix) = parse_json_objects(content)?;
    let headers = collect_json_headers(&objs);
    if headers.is_empty() {
        return Err("no columns found in JSON".into());
    }
    let rows: Vec<Vec<rdb_core::result::Cell>> = objs
        .iter()
        .map(|obj| build_json_sheet_row(&headers, obj, &large_integer_prefix))
        .collect();
    Ok(ImportSheet {
        format_label: "JSON",
        file_name,
        size_bytes,
        headers,
        rows,
        sql_statements: Vec::new(),
    })
}

fn build_csv_sheet_row(headers: &[String], rec: &[(String, bool)]) -> Vec<rdb_core::result::Cell> {
    (0..headers.len())
        .map(|i| match rec.get(i) {
            Some((v, q)) => csv_cell_value(v, *q),
            None => rdb_core::result::Cell::Null,
        })
        .collect()
}

fn parse_csv_sheet(
    file_name: String,
    size_bytes: usize,
    content: &str,
) -> Result<ImportSheet, String> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let records = parse_csv_records(content)?;
    let Some((header_row, data_rows)) = records.split_first() else {
        return Err("empty CSV file".into());
    };
    let headers: Vec<String> = header_row
        .iter()
        .map(|(h, _)| h.trim().to_string())
        .collect();
    if headers.is_empty() {
        return Err("no header columns found in CSV".into());
    }
    let mut rows = Vec::with_capacity(data_rows.len());
    for (row_idx, rec) in data_rows.iter().enumerate() {
        if rec.len() != headers.len() {
            return Err(format!(
                "CSV row {} has {} column(s), expected {}",
                row_idx + 2,
                rec.len(),
                headers.len()
            ));
        }
        rows.push(build_csv_sheet_row(&headers, rec));
    }
    if rows.is_empty() {
        return Err("no data rows found in CSV".into());
    }
    Ok(ImportSheet {
        format_label: "CSV",
        file_name,
        size_bytes,
        headers,
        rows,
        sql_statements: Vec::new(),
    })
}

pub fn parse_import_sheet(path: &str, content: &str) -> Result<ImportSheet, String> {
    let p = std::path::Path::new(path);
    let file_name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("import")
        .to_string();
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let size_bytes = content.len();
    let trimmed = content.trim_start();
    if ext == "sql" {
        let stmts = crate::editor::split_statements(content);
        if stmts.is_empty() {
            return Err("no SQL statements found in file".into());
        }
        let rows = stmts
            .iter()
            .map(|s| vec![rdb_core::result::Cell::Text(s.clone())])
            .collect();
        Ok(ImportSheet {
            format_label: "SQL",
            file_name,
            size_bytes,
            headers: vec!["statement".into()],
            rows,
            sql_statements: stmts,
        })
    } else if ext == "json"
        || (ext.is_empty() && (trimmed.starts_with('[') || trimmed.starts_with('{')))
    {
        parse_json_sheet(file_name, size_bytes, content)
    } else {
        parse_csv_sheet(file_name, size_bytes, content)
    }
}

/// Auto-match file columns against `table_cols` according to `mode`
/// (`"Match Columns by Name and Order"`, `"Match Columns by Name"`, `"Match Columns by Order"`).
pub fn match_import_columns(
    file_cols: &[String],
    table_cols: &[String],
    mode: &str,
    create_table: bool,
) -> Vec<String> {
    if create_table {
        return file_cols.to_vec();
    }
    file_cols
        .iter()
        .enumerate()
        .map(|(i, fc)| {
            if mode == "Match Columns by Order" {
                return table_cols
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| "Do not import".into());
            }
            if let Some(found) = table_cols.iter().find(|tc| tc.eq_ignore_ascii_case(fc)) {
                return found.clone();
            }
            if mode == "Match Columns by Name and Order" {
                if let Some(by_pos) = table_cols.get(i) {
                    if !file_cols
                        .iter()
                        .any(|other| other.eq_ignore_ascii_case(by_pos))
                    {
                        return by_pos.clone();
                    }
                }
            }
            "Do not import".into()
        })
        .collect()
}

/// Build `WriteOp::Insert` operations from an `ImportSheet` and user-chosen `target_cols`.
pub fn build_import_ops_from_sheet(
    sheet: &ImportSheet,
    table: &rdb_core::write::TableRef,
    target_cols: &[String],
) -> Result<Vec<rdb_core::write::WriteOp>, String> {
    if !sheet.sql_statements.is_empty() {
        return Ok(Vec::new());
    }
    let active_cols: Vec<(usize, String)> = target_cols
        .iter()
        .enumerate()
        .filter(|(_, tc)| !tc.is_empty() && tc.as_str() != "Do not import")
        .map(|(i, tc)| (i, tc.clone()))
        .collect();
    if active_cols.is_empty() {
        return Err(
            "No columns matched — select target column(s) or enable 'Create new table'".into(),
        );
    }
    let mut ops = Vec::with_capacity(sheet.rows.len());
    for row in &sheet.rows {
        let values: Vec<(String, rdb_core::result::Cell)> = active_cols
            .iter()
            .map(|(idx, col_name)| {
                let cell = row
                    .get(*idx)
                    .cloned()
                    .unwrap_or(rdb_core::result::Cell::Null);
                (col_name.clone(), cell)
            })
            .collect();
        ops.push(rdb_core::write::WriteOp::Insert {
            table: table.clone(),
            values,
        });
    }
    Ok(ops)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_import_json(
        table: &rdb_core::write::TableRef,
        content: &str,
    ) -> Result<Vec<rdb_core::write::WriteOp>, String> {
        let sheet = parse_json_sheet("data.json".into(), content.len(), content)?;
        build_import_ops_from_sheet(&sheet, table, &sheet.headers)
    }

    fn parse_import_csv(
        table: &rdb_core::write::TableRef,
        content: &str,
    ) -> Result<Vec<rdb_core::write::WriteOp>, String> {
        let sheet = parse_csv_sheet("data.csv".into(), content.len(), content)?;
        build_import_ops_from_sheet(&sheet, table, &sheet.headers)
    }

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
        let sql = "INSERT INTO users (id, name) VALUES (1, 'a');\nINSERT INTO users (id, name) VALUES (2, 'b');";
        let sheet = parse_import_sheet("dump.sql", sql).unwrap();
        assert_eq!(
            sheet.sql_statements,
            vec![
                "INSERT INTO users (id, name) VALUES (1, 'a')",
                "INSERT INTO users (id, name) VALUES (2, 'b')",
            ]
        );
        let quoted_sql = "INSERT INTO \"my;table\" /* a;b */ VALUES (1, E'it\\'s;ok');\nSELECT 2;";
        let quoted_sheet = parse_import_sheet("quoted.sql", quoted_sql).unwrap();
        assert_eq!(
            quoted_sheet.sql_statements,
            vec![
                "INSERT INTO \"my;table\" /* a;b */ VALUES (1, E'it\\'s;ok')",
                "SELECT 2",
            ]
        );
    }

    #[test]
    fn import_csv_preserves_text_and_exact_large_integers() {
        use rdb_core::result::Cell;

        let sheet = parse_csv_sheet(
            "data.csv".into(),
            0,
            "id,leading,nan,pos_inf,neg_inf,big,empty\n1,007,NaN,inf,-Infinity,9223372036854775808,\"\"\n",
        )
        .unwrap();
        assert!(matches!(sheet.rows[0][0], Cell::Int(1)));
        for (index, expected) in [
            (1, "007"),
            (2, "NaN"),
            (3, "inf"),
            (4, "-Infinity"),
            (5, "9223372036854775808"),
        ] {
            assert!(matches!(&sheet.rows[0][index], Cell::Text(value) if value == expected));
        }
        assert!(matches!(&sheet.rows[0][6], Cell::Text(value) if value.is_empty()));
    }

    #[test]
    fn import_csv_strips_bom_and_keeps_rfc4180_quoted_content() {
        let clean = parse_csv_sheet(
            "clean.csv".into(),
            0,
            "name,note\nAda,\"said \"\"hello\"\"\nand left\"\n",
        )
        .unwrap();
        let bom = parse_csv_sheet(
            "bom.csv".into(),
            0,
            "\u{feff}name,note\nAda,\"said \"\"hello\"\"\nand left\"\n",
        )
        .unwrap();
        assert_eq!(bom.headers, clean.headers);
        assert_eq!(bom.headers[0], "name");
        assert!(matches!(
            &bom.rows[0][0],
            rdb_core::result::Cell::Text(value) if value == "Ada"
        ));
        assert!(matches!(
            &bom.rows[0][1],
            rdb_core::result::Cell::Text(value) if value == "said \"hello\"\nand left"
        ));
    }

    #[test]
    fn import_csv_rejects_malformed_quotes() {
        for input in [
            "id,note\n1,\"unclosed\n",
            "id,note\n1,ab\"cd\n",
            "id,note\n1,\"closed\"tail\n",
        ] {
            let err = parse_csv_sheet("data.csv".into(), input.len(), input).unwrap_err();
            assert!(err.contains("CSV") || err.contains("quote"), "{err}");
        }
    }

    #[test]
    fn import_json_preserves_large_integer_digits() {
        let sheet =
            parse_json_sheet("data.json".into(), 0, r#"[{"value":18446744073709551616}]"#).unwrap();
        assert!(matches!(
            &sheet.rows[0][0],
            rdb_core::result::Cell::Text(value) if value == "18446744073709551616"
        ));
    }

    #[test]
    fn import_json_preserves_strings_that_look_like_integer_markers() {
        let sheet = parse_json_sheet(
            "data.json".into(),
            0,
            r#"[{"value":"\u0000rdb-large-integer:123"}]"#,
        )
        .unwrap();
        assert!(matches!(
            &sheet.rows[0][0],
            rdb_core::result::Cell::Text(value) if value == "\0rdb-large-integer:123"
        ));
    }

    #[test]
    fn import_json_preserves_unicode_escaped_marker_like_strings() {
        let sheet = parse_json_sheet(
            "data.json".into(),
            0,
            r#"[{"value":"\u005f\u005f\u0072\u0064\u0062\u005f\u006c\u0061\u0072\u0067\u0065\u005f\u006a\u0073\u006f\u006e\u005f\u0069\u006e\u0074\u0065\u0067\u0065\u0072\u005f123"}]"#,
        )
        .unwrap();
        assert!(matches!(
            &sheet.rows[0][0],
            rdb_core::result::Cell::Text(value) if value == "__rdb_large_json_integer_123"
        ));
    }

    #[test]
    fn import_csv_rejects_short_rows() {
        let table = rdb_core::write::TableRef::named("users");
        let err = parse_import_csv(&table, "id,name,email\n1,Alice\n").unwrap_err();
        assert!(err.contains("CSV row 2 has 2 column(s), expected 3"));
    }

    #[test]
    fn import_json_parses_ndjson_lines() {
        let table = rdb_core::write::TableRef::named("users");
        let ndjson = "{\"id\": 1, \"name\": \"A\"}\n{\"id\": 2, \"name\": \"B\"}\n";
        let ops = parse_import_json(&table, ndjson).unwrap();
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn import_sheet_column_matching_and_ops_respect_mode() {
        let sheet = parse_import_sheet("clean.json", "[{\"id\":1,\"code\":\"BBCA\"}]").unwrap();
        assert_eq!(sheet.headers, vec!["id", "code"]);
        assert_eq!(sheet.rows.len(), 1);
        let schema_cols = vec!["code".to_string(), "name".to_string()];
        let matched =
            match_import_columns(&sheet.headers, &schema_cols, "Match Columns by Name", false);
        assert_eq!(matched, vec!["Do not import", "code"]);
        let table = rdb_core::write::TableRef::named("emiten");
        let ops = build_import_ops_from_sheet(&sheet, &table, &matched).unwrap();
        assert_eq!(ops.len(), 1);
    }
}
