//! `oracledb::Row` column -> `rdb_core::Cell`.
//!
//! The rule this module exists to enforce: **nothing silently becomes
//! `Cell::Null`.** The Postgres driver once dropped money/enum/timetz values
//! to `Null` because no `FromSql` impl matched, and the grid showed blanks
//! with no hint that a value was there. Only a genuine SQL NULL yields
//! `Cell::Null`; a value this driver cannot decode yields a typed marker, so
//! "empty" and "unreadable" never look the same.
//!
//! **Dispatch is on the column, not on the value.** `oracledb::Row` exposes
//! only `get::<T>()` over `FromDbValue` — there is no dynamic value type in
//! its public API (`DbValue` exists but its module is private), and `String`
//! is *not* a universal fallback: its `FromDbValue` impl accepts only
//! `VARCHAR`-family and `ROWID` data, so `get::<String>()` on a NUMBER is an
//! error, not a rendering. Every branch below therefore asks
//! `Metadata::db_type()` first and requests the one Rust type that column can
//! actually produce.
//!
//! Dates and timestamps are formatted from their components rather than read
//! as strings, because reading them as strings would hand back whatever
//! `NLS_DATE_FORMAT` the session happens to carry.

use std::collections::HashMap;

use oracledb::{
    DbType, FromDbValue, JsonValue, Metadata, OracleIntervalDS, OracleIntervalYM, OracleNumber,
    OracleTimestamp, Row, Vector, VectorData, DB_TYPE_BFILE, DB_TYPE_BINARY_DOUBLE,
    DB_TYPE_BINARY_FLOAT, DB_TYPE_BINARY_INTEGER, DB_TYPE_BOOLEAN, DB_TYPE_CHAR, DB_TYPE_CLOB,
    DB_TYPE_CURSOR, DB_TYPE_DATE, DB_TYPE_INTERVAL_DS, DB_TYPE_INTERVAL_YM, DB_TYPE_JSON,
    DB_TYPE_LONG, DB_TYPE_LONG_NVARCHAR, DB_TYPE_LONG_RAW, DB_TYPE_NCHAR, DB_TYPE_NCLOB,
    DB_TYPE_NUMBER, DB_TYPE_NVARCHAR, DB_TYPE_OBJECT, DB_TYPE_RAW, DB_TYPE_ROWID,
    DB_TYPE_TIMESTAMP, DB_TYPE_TIMESTAMP_LTZ, DB_TYPE_TIMESTAMP_TZ, DB_TYPE_UROWID,
    DB_TYPE_VARCHAR, DB_TYPE_VECTOR, DB_TYPE_XMLTYPE,
};
use rdb_core::result::Cell;

/// One column of one row, as a `Cell`.
pub fn cell_at(row: &Row, idx: usize, md: &Metadata) -> Cell {
    let ty = md.db_type();

    // The three broad classes Oracle itself groups, taken from `DbType`'s own
    // predicates so a type added upstream lands in the right bucket without a
    // change here. CLOB/NCLOB are string types and arrive as `String`: LOBs
    // come back as values unless the statement asked for locators via
    // `fetch_lobs()`, which this driver does not.
    if ty.is_string_type() {
        return cell(row, idx, Cell::Text, "[text]");
    }
    if ty.is_binary_type() {
        return cell(row, idx, Cell::Bytes, "[binary]");
    }
    if ty.is_date_type() {
        // Whether a zone is part of the value is a property of the column,
        // not of the timestamp struct — `OracleTimestamp` carries an offset
        // either way, and a plain TIMESTAMP's is a meaningless zero.
        let zoned = ty == &DB_TYPE_TIMESTAMP_TZ || ty == &DB_TYPE_TIMESTAMP_LTZ;
        return cell(
            row,
            idx,
            |t: OracleTimestamp| Cell::Text(format_timestamp(&t, zoned)),
            "[timestamp]",
        );
    }

    if ty == &DB_TYPE_NUMBER || ty == &DB_TYPE_BINARY_INTEGER {
        return cell(row, idx, number_cell, "[number]");
    }
    if ty == &DB_TYPE_BINARY_FLOAT {
        return cell(row, idx, |f: f32| Cell::Float(f as f64), "[binary float]");
    }
    if ty == &DB_TYPE_BINARY_DOUBLE {
        return cell(row, idx, Cell::Float, "[binary double]");
    }
    if ty == &DB_TYPE_BOOLEAN {
        return cell(row, idx, Cell::Bool, "[boolean]");
    }
    if ty == &DB_TYPE_INTERVAL_DS {
        return cell(
            row,
            idx,
            |v: OracleIntervalDS| Cell::Text(v.to_string()),
            "[interval]",
        );
    }
    if ty == &DB_TYPE_INTERVAL_YM {
        return cell(
            row,
            idx,
            |v: OracleIntervalYM| Cell::Text(v.to_string()),
            "[interval]",
        );
    }
    // Oracle 21c's native JSON type. The previous ODPI-C driver could not read
    // this at all and had to tell users to wrap the column in
    // `JSON_SERIALIZE`; here it decodes to a value.
    if ty == &DB_TYPE_JSON {
        return cell(
            row,
            idx,
            |v: JsonValue| Cell::Text(render_json(&v)),
            "[json]",
        );
    }
    if ty == &DB_TYPE_VECTOR {
        return cell(
            row,
            idx,
            |v: Vector| Cell::Text(describe_vector(&v)),
            "[vector]",
        );
    }

    // BFILE points at a file on the database server's own disk, a REF CURSOR
    // is a nested result set, and an OBJECT is a user-defined type: each is a
    // separate feature rather than a value, so name it instead of showing a
    // blank cell.
    if ty == &DB_TYPE_BFILE {
        return Cell::Text("[BFILE]".into());
    }
    if ty == &DB_TYPE_CURSOR {
        return Cell::Text("[REF CURSOR]".into());
    }
    if ty == &DB_TYPE_OBJECT {
        return Cell::Text("[OBJECT]".into());
    }

    // XMLTYPE and anything upstream adds later: text is the likeliest shape,
    // and a failed attempt still lands on a marker rather than a blank.
    cell(row, idx, Cell::Text, "[unsupported type]")
}

/// Read one column as `T` and map it, distinguishing the three outcomes that
/// must never be confused: a real value, a SQL NULL, and a value that exists
/// but could not be decoded.
///
/// `Option<T>` is what separates the last two — a bare `get::<T>()` reports a
/// NULL as `ErrorKind::ValueWasNull`, which would land on the marker path and
/// claim an empty column held something unreadable.
fn cell<'a, T, F>(row: &'a Row, idx: usize, f: F, marker: &str) -> Cell
where
    T: FromDbValue<'a>,
    F: FnOnce(T) -> Cell,
{
    match row.get::<Option<T>>(idx) {
        Ok(Some(v)) => f(v),
        Ok(None) => Cell::Null,
        Err(_) => Cell::Text(marker.to_string()),
    }
}

/// Oracle NUMBER carries up to 38 significant digits — wider than `i64` and
/// wider than `f64` represents exactly — so narrowing is only safe when it
/// round-trips. Otherwise the decimal text is the true value.
fn number_cell(n: OracleNumber) -> Cell {
    let s = n.to_string();
    if let Ok(i) = s.parse::<i64>() {
        return Cell::Int(i);
    }
    // A round-trip check, because f64 silently drops digits past ~15
    // significant ones: a NUMBER(38) would otherwise come back as a
    // different number than the database holds.
    if let Ok(f) = s.parse::<f64>() {
        if f.is_finite() && format!("{f}") == s {
            return Cell::Float(f);
        }
    }
    Cell::Text(s)
}

/// `YYYY-MM-DD HH:MM:SS[.ffffff][ ±HH:MM]`, built from the components so the
/// session's NLS settings cannot change it. Oracle's `DATE` carries a time
/// component (unlike the SQL standard), so it is never rendered date-only.
fn format_timestamp(t: &OracleTimestamp, zoned: bool) -> String {
    let mut s = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    );
    let micros = t.nanoseconds() / 1_000;
    if micros > 0 {
        s.push_str(&format!(".{micros:06}"));
    }
    if zoned {
        let (h, m) = (t.tz_hour_offset(), t.tz_minute_offset());
        let sign = if h < 0 || m < 0 { '-' } else { '+' };
        s.push_str(&format!(" {sign}{:02}:{:02}", h.abs(), m.abs()));
    }
    s
}

/// A `VECTOR` column holds hundreds or thousands of components; pasting them
/// into a grid cell is noise, not data.
///
/// ponytail: shape and element type only. Render the components when there is
/// somewhere to render them — a cell-detail pane — not in a table row.
fn describe_vector(v: &Vector) -> String {
    match v {
        Vector::Dense(d) => {
            let (kind, n) = match d {
                VectorData::Float32(x) => ("FLOAT32", x.len()),
                VectorData::Float64(x) => ("FLOAT64", x.len()),
                VectorData::Int8(x) => ("INT8", x.len()),
                VectorData::Binary(x) => ("BINARY", x.len()),
            };
            format!("[VECTOR {kind} · {n} dims]")
        }
        // `SparseVector`'s dimensions and indices have no public accessors, so
        // there is nothing more to say about one from outside the crate.
        Vector::Sparse(_) => "[SPARSE VECTOR]".to_string(),
    }
}

/// `JsonValue` as JSON text.
///
/// The crate decodes Oracle's binary OSON into a value tree but does not
/// render it, and it carries Oracle types (`OracleNumber`, `OracleTimestamp`)
/// that JSON has no syntax for — those spell as their Oracle text, quoted,
/// which is what the grid should show.
fn render_json(v: &JsonValue) -> String {
    let mut out = String::new();
    write_json(v, &mut out);
    out
}

fn write_json(v: &JsonValue, out: &mut String) {
    match v {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Boolean(b) => out.push_str(if *b { "true" } else { "false" }),
        JsonValue::Number(n) => out.push_str(&n.to_string()),
        JsonValue::BinaryFloat(f) => out.push_str(&f.to_string()),
        JsonValue::BinaryDouble(f) => out.push_str(&f.to_string()),
        JsonValue::String(s) => write_json_string(s, out),
        JsonValue::Timestamp(t) => write_json_string(&format_timestamp(t, true), out),
        JsonValue::IntervalDS(i) => write_json_string(&i.to_string(), out),
        JsonValue::IntervalYM(i) => write_json_string(&i.to_string(), out),
        JsonValue::Vector(vec) => write_json_string(&describe_vector(vec), out),
        JsonValue::Raw(b) | JsonValue::JsonId(b) => {
            let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
            write_json_string(&hex, out);
        }
        JsonValue::JsonArray(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        JsonValue::JsonObject(map) => write_json_object(map, out),
    }
}

/// Oracle's OSON decodes into a `HashMap`, whose iteration order is arbitrary
/// and would reshuffle a cell between two reads of the same row. Sorting the
/// keys costs nothing at object sizes a grid cell holds and makes the output
/// stable — and therefore diffable and testable.
fn write_json_object(map: &HashMap<String, JsonValue>, out: &mut String) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    out.push('{');
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_json_string(k, out);
        out.push(':');
        write_json(&map[*k], out);
    }
    out.push('}');
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Human-readable type name for a result column header.
///
/// `DbType::name()` cannot be used here: it returns the Rust constant's name
/// (`"DB_TYPE_VARCHAR"`), and the Oracle spelling it also holds (`ora_name`,
/// `"VARCHAR2"`) is crate-private. So the mapping is kept here, and the size
/// and precision come off `Metadata`.
pub fn column_type_name(md: &Metadata) -> String {
    let ty = md.db_type();
    let (size, precision, scale) = (md.max_size(), md.precision(), md.scale());

    if ty == &DB_TYPE_VARCHAR {
        return format!("VARCHAR2({size})");
    }
    if ty == &DB_TYPE_NVARCHAR {
        return format!("NVARCHAR2({size})");
    }
    if ty == &DB_TYPE_CHAR {
        return format!("CHAR({size})");
    }
    if ty == &DB_TYPE_NCHAR {
        return format!("NCHAR({size})");
    }
    if ty == &DB_TYPE_RAW {
        return format!("RAW({size})");
    }
    if ty == &DB_TYPE_NUMBER {
        // Oracle reports an unconstrained NUMBER as precision 0 with scale
        // -127 ("no scale specified"), which would render as the nonsense
        // `NUMBER(0,-127)` in a column header.
        return match (precision, scale) {
            (0, _) => "NUMBER".to_string(),
            (p, 0) => format!("NUMBER({p})"),
            (p, s) => format!("NUMBER({p},{s})"),
        };
    }
    if ty == &DB_TYPE_TIMESTAMP {
        return format!("TIMESTAMP({})", timestamp_precision(scale));
    }
    if ty == &DB_TYPE_TIMESTAMP_TZ {
        return format!("TIMESTAMP({}) WITH TIME ZONE", timestamp_precision(scale));
    }
    if ty == &DB_TYPE_TIMESTAMP_LTZ {
        return format!(
            "TIMESTAMP({}) WITH LOCAL TIME ZONE",
            timestamp_precision(scale)
        );
    }

    simple_type_name(ty).to_string()
}

/// Oracle reports a timestamp's fractional-second digits in the scale field.
/// A describe that omits it (scale 0 on a type that always has one) means the
/// default, which is 6.
fn timestamp_precision(scale: i8) -> u8 {
    if (1..=9).contains(&scale) {
        scale as u8
    } else {
        6
    }
}

/// Types whose name carries no size or precision.
fn simple_type_name(ty: &'static DbType) -> &'static str {
    for (t, name) in [
        (&DB_TYPE_DATE, "DATE"),
        (&DB_TYPE_BINARY_FLOAT, "BINARY_FLOAT"),
        (&DB_TYPE_BINARY_DOUBLE, "BINARY_DOUBLE"),
        (&DB_TYPE_BINARY_INTEGER, "BINARY_INTEGER"),
        (&DB_TYPE_BOOLEAN, "BOOLEAN"),
        (&DB_TYPE_CLOB, "CLOB"),
        (&DB_TYPE_NCLOB, "NCLOB"),
        (&DB_TYPE_BFILE, "BFILE"),
        (&DB_TYPE_LONG, "LONG"),
        (&DB_TYPE_LONG_RAW, "LONG RAW"),
        (&DB_TYPE_LONG_NVARCHAR, "LONG NVARCHAR"),
        (&DB_TYPE_ROWID, "ROWID"),
        (&DB_TYPE_UROWID, "UROWID"),
        (&DB_TYPE_INTERVAL_DS, "INTERVAL DAY TO SECOND"),
        (&DB_TYPE_INTERVAL_YM, "INTERVAL YEAR TO MONTH"),
        (&DB_TYPE_JSON, "JSON"),
        (&DB_TYPE_XMLTYPE, "XMLTYPE"),
        (&DB_TYPE_VECTOR, "VECTOR"),
        (&DB_TYPE_CURSOR, "REF CURSOR"),
        (&DB_TYPE_OBJECT, "OBJECT"),
    ] {
        if ty == t {
            return name;
        }
    }
    "UNKNOWN"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(y: i16, mo: u8, d: u8, h: u8, mi: u8, s: u8, ns: u32) -> OracleTimestamp {
        OracleTimestamp::new_timestamp(y, mo, d, h, mi, s, ns)
    }

    #[test]
    fn date_keeps_its_time_component() {
        assert_eq!(
            format_timestamp(&ts(2024, 3, 7, 9, 5, 1, 0), false),
            "2024-03-07 09:05:01"
        );
    }

    #[test]
    fn timestamp_renders_fractional_seconds() {
        assert_eq!(
            format_timestamp(&ts(2024, 3, 7, 9, 5, 1, 250_000_000), false),
            "2024-03-07 09:05:01.250000"
        );
    }

    #[test]
    fn timestamp_with_zone_keeps_its_offset() {
        let east = OracleTimestamp::new_timestamp_tz(2024, 3, 7, 9, 5, 1, 0, 7, 30);
        assert_eq!(format_timestamp(&east, true), "2024-03-07 09:05:01 +07:30");
        let west = OracleTimestamp::new_timestamp_tz(2024, 3, 7, 9, 5, 1, 0, -5, 0);
        assert_eq!(format_timestamp(&west, true), "2024-03-07 09:05:01 -05:00");
    }

    #[test]
    fn an_unzoned_column_shows_no_offset_even_though_one_is_carried() {
        // `OracleTimestamp` always has offset fields; a plain TIMESTAMP's are
        // zero and must not render as a spurious `+00:00`.
        assert_eq!(
            format_timestamp(&ts(2024, 3, 7, 9, 5, 1, 0), false),
            "2024-03-07 09:05:01"
        );
    }

    #[test]
    fn a_number_narrows_only_when_it_round_trips() {
        // `Cell` is not `PartialEq`, so these match on the shape.
        assert!(matches!(number_cell("42".parse().unwrap()), Cell::Int(42)));
        assert!(matches!(
            number_cell("1.5".parse().unwrap()),
            Cell::Float(f) if f == 1.5
        ));
        // 38 significant digits: f64 would round this, so it stays text.
        let wide = "12345678901234567890123456789012345678";
        assert!(matches!(
            number_cell(wide.parse().unwrap()),
            Cell::Text(s) if s == wide
        ));
    }

    #[test]
    fn json_objects_render_with_stable_key_order() {
        let mut map = HashMap::new();
        map.insert("b".to_string(), JsonValue::Boolean(true));
        map.insert("a".to_string(), JsonValue::String("x".into()));
        map.insert("c".to_string(), JsonValue::Null);
        assert_eq!(
            render_json(&JsonValue::JsonObject(map)),
            r#"{"a":"x","b":true,"c":null}"#
        );
    }

    #[test]
    fn json_strings_escape_quotes_and_control_characters() {
        let v = JsonValue::String("he said \"hi\"\n\tbye\\".into());
        assert_eq!(render_json(&v), r#""he said \"hi\"\n\tbye\\""#);
    }

    #[test]
    fn json_arrays_nest() {
        let v = JsonValue::JsonArray(vec![
            JsonValue::Boolean(false),
            JsonValue::JsonArray(vec![JsonValue::Null]),
        ]);
        assert_eq!(render_json(&v), "[false,[null]]");
    }

    #[test]
    fn a_vector_is_summarised_not_dumped() {
        let v = Vector::Dense(VectorData::Float32(vec![0.0; 1536]));
        assert_eq!(describe_vector(&v), "[VECTOR FLOAT32 · 1536 dims]");
    }

    #[test]
    fn a_timestamp_without_a_reported_precision_defaults_to_six() {
        assert_eq!(timestamp_precision(0), 6);
        assert_eq!(timestamp_precision(-127), 6);
        assert_eq!(timestamp_precision(3), 3);
    }

    #[test]
    fn type_names_use_oracle_spelling_not_the_rust_constant() {
        // `DbType::name()` would say "DB_TYPE_VARCHAR" here.
        assert_eq!(simple_type_name(&DB_TYPE_DATE), "DATE");
        assert_eq!(simple_type_name(&DB_TYPE_BINARY_DOUBLE), "BINARY_DOUBLE");
        assert_eq!(simple_type_name(&DB_TYPE_JSON), "JSON");
        assert_eq!(simple_type_name(&DB_TYPE_XMLTYPE), "XMLTYPE");
    }
}
