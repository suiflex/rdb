//! `oracle::SqlValue` -> `rdb_core::Cell`.
//!
//! The rule this module exists to enforce: **nothing silently becomes
//! `Cell::Null`.** The Postgres driver once dropped money/enum/timetz values
//! to `Null` because no `FromSql` impl matched, and the grid showed blanks
//! with no hint that a value was there. So every branch here either produces
//! the real value or falls back through `get::<String>()` — Oracle can render
//! nearly anything as text — and only a genuine SQL NULL yields `Cell::Null`.
//!
//! Dates and timestamps are formatted from their components rather than read
//! as strings, because reading them as strings would hand back whatever
//! `NLS_DATE_FORMAT` the session happens to carry.

use oracle::sql_type::{OracleType, Timestamp};
use oracle::SqlValue;
use rdb_core::result::Cell;

pub fn sql_value_to_cell(v: &SqlValue) -> Cell {
    if v.is_null().unwrap_or(false) {
        return Cell::Null;
    }
    let Ok(ty) = v.oracle_type() else {
        return text_or_marker(v, "[unreadable]");
    };

    match ty {
        // Oracle NUMBER carries up to 38 significant digits — wider than i64
        // and wider than f64 represents exactly — so narrowing is only safe
        // when it round-trips. Otherwise the decimal text is the true value.
        OracleType::Number(_, _) | OracleType::Float(_) => number_cell(v),
        OracleType::Int64 => v
            .get::<i64>()
            .map(Cell::Int)
            .unwrap_or_else(|_| number_cell(v)),
        OracleType::UInt64 => number_cell(v),
        OracleType::BinaryFloat | OracleType::BinaryDouble => v
            .get::<f64>()
            .map(Cell::Float)
            .unwrap_or_else(|_| text_or_marker(v, "[binary float]")),
        OracleType::Boolean => v
            .get::<bool>()
            .map(Cell::Bool)
            .unwrap_or_else(|_| text_or_marker(v, "[boolean]")),

        OracleType::Date
        | OracleType::Timestamp(_)
        | OracleType::TimestampTZ(_)
        | OracleType::TimestampLTZ(_) => v
            .get::<Timestamp>()
            .map(|t| Cell::Text(format_timestamp(&t)))
            .unwrap_or_else(|_| text_or_marker(v, "[timestamp]")),

        // BFILE points at a file on the database server's own disk; reading
        // its contents is a separate feature, so it is named rather than
        // shown as an empty cell.
        OracleType::BFILE => Cell::Text("[BFILE]".into()),
        OracleType::Raw(_) | OracleType::LongRaw | OracleType::BLOB => v
            .get::<Vec<u8>>()
            .map(Cell::Bytes)
            .unwrap_or_else(|_| text_or_marker(v, "[binary]")),

        OracleType::RefCursor => Cell::Text("[REF CURSOR]".into()),

        // Everything textual, plus the types Oracle renders as text on its
        // own: CLOB/NCLOB, XML, JSON, ROWID, both INTERVAL kinds, and object
        // types / collections.
        _ => text_or_marker(v, "[unsupported type]"),
    }
}

fn number_cell(v: &SqlValue) -> Cell {
    let Ok(s) = v.get::<String>() else {
        return text_or_marker(v, "[number]");
    };
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

/// Last resort before a blank: ask Oracle for the value as text, and only if
/// even that fails show a typed marker. Never `Cell::Null` — that would claim
/// the database holds no value here, which is a different fact.
fn text_or_marker(v: &SqlValue, marker: &str) -> Cell {
    match v.get::<String>() {
        Ok(s) => Cell::Text(s),
        Err(_) => Cell::Text(marker.to_string()),
    }
}

/// `YYYY-MM-DD HH:MM:SS[.ffffff][ ±HH:MM]`, built from the components so the
/// session's NLS settings cannot change it. Oracle's `DATE` carries a time
/// component (unlike the SQL standard), so it is never rendered date-only.
fn format_timestamp(t: &Timestamp) -> String {
    let mut s = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    );
    let micros = t.nanosecond() / 1_000;
    if micros > 0 {
        s.push_str(&format!(".{micros:06}"));
    }
    if t.with_tz() {
        let (h, m) = (t.tz_hour_offset(), t.tz_minute_offset());
        let sign = if h < 0 || m < 0 { '-' } else { '+' };
        s.push_str(&format!(" {sign}{:02}:{:02}", h.abs(), m.abs()));
    }
    s
}

/// Human-readable type name for a result column header.
pub fn column_type_name(t: &OracleType) -> String {
    match t {
        OracleType::Varchar2(n) => format!("VARCHAR2({n})"),
        OracleType::NVarchar2(n) => format!("NVARCHAR2({n})"),
        OracleType::Char(n) => format!("CHAR({n})"),
        OracleType::NChar(n) => format!("NCHAR({n})"),
        OracleType::Raw(n) => format!("RAW({n})"),
        // Oracle reports an unconstrained NUMBER as precision 0 with scale
        // -127 ("no scale specified"), which would render as the nonsense
        // `NUMBER(0,-127)` in a column header.
        OracleType::Number(p, s) => match (p, s) {
            (0, _) => "NUMBER".to_string(),
            (p, 0) => format!("NUMBER({p})"),
            (p, s) => format!("NUMBER({p},{s})"),
        },
        OracleType::Float(p) => format!("FLOAT({p})"),
        OracleType::Timestamp(p) => format!("TIMESTAMP({p})"),
        OracleType::TimestampTZ(p) => format!("TIMESTAMP({p}) WITH TIME ZONE"),
        OracleType::TimestampLTZ(p) => format!("TIMESTAMP({p}) WITH LOCAL TIME ZONE"),
        OracleType::IntervalDS(d, s) => format!("INTERVAL DAY({d}) TO SECOND({s})"),
        OracleType::IntervalYM(p) => format!("INTERVAL YEAR({p}) TO MONTH"),
        OracleType::Rowid => "ROWID".into(),
        OracleType::BinaryFloat => "BINARY_FLOAT".into(),
        OracleType::BinaryDouble => "BINARY_DOUBLE".into(),
        OracleType::Date => "DATE".into(),
        OracleType::CLOB => "CLOB".into(),
        OracleType::NCLOB => "NCLOB".into(),
        OracleType::BLOB => "BLOB".into(),
        OracleType::BFILE => "BFILE".into(),
        OracleType::RefCursor => "REF CURSOR".into(),
        OracleType::Boolean => "BOOLEAN".into(),
        OracleType::Long => "LONG".into(),
        OracleType::LongRaw => "LONG RAW".into(),
        OracleType::Json => "JSON".into(),
        OracleType::Xml => "XMLTYPE".into(),
        OracleType::Int64 => "INTEGER".into(),
        OracleType::UInt64 => "UNSIGNED INTEGER".into(),
        // Object types and anything the crate adds later: its own Display is
        // already a readable Oracle type name.
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32, ns: u32) -> Timestamp {
        Timestamp::new(y, mo, d, h, mi, s, ns).unwrap()
    }

    #[test]
    fn date_keeps_its_time_component() {
        assert_eq!(
            format_timestamp(&ts(2024, 3, 7, 9, 5, 1, 0)),
            "2024-03-07 09:05:01"
        );
    }

    #[test]
    fn timestamp_renders_fractional_seconds() {
        assert_eq!(
            format_timestamp(&ts(2024, 3, 7, 9, 5, 1, 250_000_000)),
            "2024-03-07 09:05:01.250000"
        );
    }

    #[test]
    fn timestamp_with_zone_keeps_its_offset() {
        let t = ts(2024, 3, 7, 9, 5, 1, 0).and_tz_hm_offset(7, 30).unwrap();
        assert_eq!(format_timestamp(&t), "2024-03-07 09:05:01 +07:30");
        let west = ts(2024, 3, 7, 9, 5, 1, 0).and_tz_hm_offset(-5, 0).unwrap();
        assert_eq!(format_timestamp(&west), "2024-03-07 09:05:01 -05:00");
    }

    #[test]
    fn type_names_carry_their_precision() {
        assert_eq!(
            column_type_name(&OracleType::Varchar2(100)),
            "VARCHAR2(100)"
        );
        assert_eq!(column_type_name(&OracleType::Number(10, 2)), "NUMBER(10,2)");
        assert_eq!(column_type_name(&OracleType::Number(38, 0)), "NUMBER(38)");
        assert_eq!(column_type_name(&OracleType::Number(0, 0)), "NUMBER");
        // What Oracle actually reports for a bare `NUMBER` column.
        assert_eq!(column_type_name(&OracleType::Number(0, -127)), "NUMBER");
        assert_eq!(column_type_name(&OracleType::BinaryDouble), "BINARY_DOUBLE");
        assert_eq!(
            column_type_name(&OracleType::TimestampTZ(6)),
            "TIMESTAMP(6) WITH TIME ZONE"
        );
    }
}
