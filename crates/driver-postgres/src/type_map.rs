use rdb_core::result::Cell;
use std::error::Error;
use tokio_postgres::types::{FromSql, Type};
use tokio_postgres::Row;

/// Which `Cell` variant a pg column type maps to. Pragmatic, not exhaustive:
/// unknown types fall back to a string read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Int,
    Float,
    Bool,
    Text,
    Bytes,
    Uuid,
    Timestamp,
    Date,
    Numeric,
    Money,
    Json,
    Time,
    TimeTz,
    Interval,
}

/// Classify a pg `Type` into a `CellKind`. Unknown types -> `Text` (string
/// fallback), which is always safe to display.
pub fn classify(ty: &Type) -> CellKind {
    match *ty {
        Type::INT2 | Type::INT4 | Type::INT8 => CellKind::Int,
        Type::FLOAT4 | Type::FLOAT8 => CellKind::Float,
        Type::BOOL => CellKind::Bool,
        Type::BYTEA => CellKind::Bytes,
        Type::UUID => CellKind::Uuid,
        Type::TIMESTAMPTZ | Type::TIMESTAMP => CellKind::Timestamp,
        Type::DATE => CellKind::Date,
        Type::TIME => CellKind::Time,
        Type::TIMETZ => CellKind::TimeTz,
        Type::INTERVAL => CellKind::Interval,
        Type::NUMERIC => CellKind::Numeric,
        Type::MONEY => CellKind::Money,
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => CellKind::Text,
        Type::JSON | Type::JSONB => CellKind::Json,
        _ => CellKind::Text,
    }
}

/// `postgres-types` has no built-in `FromSql` for `MONEY` (unlike `NUMERIC`,
/// nothing in the crate ecosystem claims it), so without this columns of
/// this type silently became `Cell::Null` — the same string_fallback used
/// everywhere else can't decode it either (`String`'s `FromSql` only
/// accepts text-family OIDs). Postgres's binary wire format for `money` is
/// documented as a plain big-endian `i64` scaled by 100 (the currency's
/// minor unit), independent of `lc_monetary`.
struct PgMoney(i64);

impl<'a> FromSql<'a> for PgMoney {
    fn from_sql(_ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        let bytes: [u8; 8] = raw.try_into()?;
        Ok(PgMoney(i64::from_be_bytes(bytes)))
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::MONEY
    }
}

impl std::fmt::Display for PgMoney {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cents = self.0.unsigned_abs() % 100;
        let whole = self.0 / 100;
        if self.0 < 0 && whole == 0 {
            write!(f, "-0.{cents:02}")
        } else {
            write!(f, "{whole}.{cents:02}")
        }
    }
}

/// Render a microsecond count as `HH:MM:SS[.ffffff]`, trailing zeros of the
/// fractional part trimmed the way Postgres itself prints them.
fn fmt_hms(micros: u64) -> String {
    let secs = micros / 1_000_000;
    let frac = micros % 1_000_000;
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if frac == 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        let frac = format!("{frac:06}");
        format!("{h:02}:{m:02}:{s:02}.{}", frac.trim_end_matches('0'))
    }
}

/// `timetz`, like `money`, has no `FromSql` anywhere in the crate ecosystem
/// (`chrono`'s integration covers `time` but not `timetz`), so the column
/// silently decoded to `Cell::Null`. The binary wire format is a big-endian
/// `i64` of microseconds since midnight followed by a big-endian `i32` zone
/// offset in seconds *west* of UTC — the displayed offset is its negation.
struct PgTimeTz {
    micros: i64,
    zone_secs_west: i32,
}

impl<'a> FromSql<'a> for PgTimeTz {
    fn from_sql(_ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        let bytes: [u8; 12] = raw.try_into()?;
        Ok(PgTimeTz {
            micros: i64::from_be_bytes(bytes[..8].try_into()?),
            zone_secs_west: i32::from_be_bytes(bytes[8..].try_into()?),
        })
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::TIMETZ
    }
}

impl std::fmt::Display for PgTimeTz {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let utc_offset = -self.zone_secs_west;
        let sign = if utc_offset < 0 { '-' } else { '+' };
        let off = utc_offset.unsigned_abs();
        let (oh, om) = (off / 3600, (off % 3600) / 60);
        write!(
            f,
            "{}{sign}{oh:02}:{om:02}",
            fmt_hms(self.micros.unsigned_abs())
        )
    }
}

/// `interval` has the same gap. Wire format is a big-endian `i64` of
/// microseconds, then `i32` days, then `i32` months. Rendered in Postgres's
/// own default `postgres` interval style.
struct PgInterval {
    micros: i64,
    days: i32,
    months: i32,
}

impl<'a> FromSql<'a> for PgInterval {
    fn from_sql(_ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        let bytes: [u8; 16] = raw.try_into()?;
        Ok(PgInterval {
            micros: i64::from_be_bytes(bytes[..8].try_into()?),
            days: i32::from_be_bytes(bytes[8..12].try_into()?),
            months: i32::from_be_bytes(bytes[12..].try_into()?),
        })
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::INTERVAL
    }
}

impl std::fmt::Display for PgInterval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        let (years, mons) = (self.months / 12, self.months % 12);
        if years != 0 {
            parts.push(format!(
                "{years} year{}",
                if years.abs() == 1 { "" } else { "s" }
            ));
        }
        if mons != 0 {
            parts.push(format!(
                "{mons} mon{}",
                if mons.abs() == 1 { "" } else { "s" }
            ));
        }
        if self.days != 0 {
            parts.push(format!(
                "{} day{}",
                self.days,
                if self.days.abs() == 1 { "" } else { "s" }
            ));
        }
        if self.micros != 0 || parts.is_empty() {
            let sign = if self.micros < 0 { "-" } else { "" };
            parts.push(format!("{sign}{}", fmt_hms(self.micros.unsigned_abs())));
        }
        write!(f, "{}", parts.join(" "))
    }
}

/// Extract column `idx` of `row` into a `Cell`, honoring NULLs.
///
/// Reads via `try_get::<Option<T>>`: `Ok(None)` -> `Cell::Null`. If the typed
/// read fails (type we did not special-case, or a decode mismatch), fall back
/// to reading the value as `Option<String>`; if even that fails the value is
/// represented as `Cell::Null` so one odd column never aborts a whole result.
pub fn extract_cell(row: &Row, idx: usize) -> Cell {
    let ty = row.columns()[idx].type_();
    match classify(ty) {
        CellKind::Int => match row.try_get::<_, Option<i16>>(idx) {
            Ok(Some(v)) => Cell::Int(v as i64),
            Ok(None) => Cell::Null,
            Err(_) => match row.try_get::<_, Option<i32>>(idx) {
                Ok(Some(v)) => Cell::Int(v as i64),
                Ok(None) => Cell::Null,
                Err(_) => match row.try_get::<_, Option<i64>>(idx) {
                    Ok(Some(v)) => Cell::Int(v),
                    Ok(None) => Cell::Null,
                    Err(_) => string_fallback(row, idx),
                },
            },
        },
        CellKind::Float => match row.try_get::<_, Option<f64>>(idx) {
            Ok(Some(v)) => Cell::Float(v),
            Ok(None) => Cell::Null,
            Err(_) => match row.try_get::<_, Option<f32>>(idx) {
                Ok(Some(v)) => Cell::Float(v as f64),
                Ok(None) => Cell::Null,
                Err(_) => string_fallback(row, idx),
            },
        },
        CellKind::Bool => match row.try_get::<_, Option<bool>>(idx) {
            Ok(Some(v)) => Cell::Bool(v),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
        },
        CellKind::Bytes => match row.try_get::<_, Option<Vec<u8>>>(idx) {
            Ok(Some(v)) => Cell::Bytes(v),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
        },
        CellKind::Uuid => match row.try_get::<_, Option<uuid::Uuid>>(idx) {
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
        },
        CellKind::Timestamp => match row.try_get::<_, Option<chrono::DateTime<chrono::Utc>>>(idx) {
            Ok(Some(v)) => Cell::Text(v.format("%Y-%m-%d %H:%M:%S%.3f").to_string()),
            Ok(None) => Cell::Null,
            Err(_) => match row.try_get::<_, Option<chrono::NaiveDateTime>>(idx) {
                Ok(Some(v)) => Cell::Text(v.format("%Y-%m-%d %H:%M:%S%.3f").to_string()),
                Ok(None) => Cell::Null,
                Err(_) => string_fallback(row, idx),
            },
        },
        CellKind::Date => match row.try_get::<_, Option<chrono::NaiveDate>>(idx) {
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
        },
        // string_fallback can't recover a NUMERIC that overflows Decimal's
        // range (String's FromSql doesn't accept the NUMERIC OID either, so
        // that fallback would silently produce Cell::Null — indistinguishable
        // from a real NULL). A placeholder that's visibly not-a-value is
        // more honest than pretending the cell is empty.
        // ponytail: shows a placeholder instead of the real (rare,
        // >28-29 significant digits) out-of-range value; upgrade to a raw
        // binary NUMERIC parser if that's ever actually needed.
        CellKind::Numeric => match row.try_get::<_, Option<rust_decimal::Decimal>>(idx) {
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => Cell::Text("<numeric value out of range>".to_string()),
        },
        CellKind::Money => match row.try_get::<_, Option<PgMoney>>(idx) {
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => Cell::Text("<unreadable money value>".to_string()),
        },
        CellKind::Json => match row.try_get::<_, Option<serde_json::Value>>(idx) {
            // Compact single-line render; the UI inspector pretty-prints on demand.
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
        },
        // A decode failure here must not fall through to string_fallback:
        // String's FromSql rejects these OIDs, so that path would produce a
        // phantom Cell::Null indistinguishable from a real NULL. Same
        // reasoning as Numeric and Money above.
        CellKind::Time => match row.try_get::<_, Option<chrono::NaiveTime>>(idx) {
            Ok(Some(v)) => Cell::Text(v.format("%H:%M:%S%.f").to_string()),
            Ok(None) => Cell::Null,
            Err(_) => Cell::Text("<unreadable time value>".to_string()),
        },
        CellKind::TimeTz => match row.try_get::<_, Option<PgTimeTz>>(idx) {
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => Cell::Text("<unreadable time value>".to_string()),
        },
        CellKind::Interval => match row.try_get::<_, Option<PgInterval>>(idx) {
            Ok(Some(v)) => Cell::Text(v.to_string()),
            Ok(None) => Cell::Null,
            Err(_) => Cell::Text("<unreadable interval value>".to_string()),
        },
        CellKind::Text => string_fallback(row, idx),
    }
}

/// Read column `idx` as an optional string; failure or NULL -> `Cell::Null`,
/// otherwise `Cell::Text`.
fn string_fallback(row: &Row, idx: usize) -> Cell {
    match row.try_get::<_, Option<String>>(idx) {
        Ok(Some(s)) => Cell::Text(s),
        _ => Cell::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_postgres::types::Type;

    #[test]
    fn integer_types_classify_as_int() {
        assert_eq!(classify(&Type::INT2), CellKind::Int);
        assert_eq!(classify(&Type::INT4), CellKind::Int);
        assert_eq!(classify(&Type::INT8), CellKind::Int);
    }

    #[test]
    fn postgres_integer_widths_map_to_int() {
        let values: [i16; 1] = [7];
        assert_eq!(values[0] as i64, 7);
    }

    #[test]
    fn float_types_classify_as_float() {
        assert_eq!(classify(&Type::FLOAT4), CellKind::Float);
        assert_eq!(classify(&Type::FLOAT8), CellKind::Float);
    }

    #[test]
    fn bool_text_bytea_classify() {
        assert_eq!(classify(&Type::BOOL), CellKind::Bool);
        assert_eq!(classify(&Type::TEXT), CellKind::Text);
        assert_eq!(classify(&Type::VARCHAR), CellKind::Text);
        assert_eq!(classify(&Type::BYTEA), CellKind::Bytes);
    }

    #[test]
    fn uuid_timestamp_date_numeric_have_branches() {
        assert_eq!(classify(&Type::UUID), CellKind::Uuid);
        assert_eq!(classify(&Type::TIMESTAMPTZ), CellKind::Timestamp);
        assert_eq!(classify(&Type::TIMESTAMP), CellKind::Timestamp);
        assert_eq!(classify(&Type::DATE), CellKind::Date);
        assert_eq!(classify(&Type::NUMERIC), CellKind::Numeric);
    }

    #[test]
    fn money_classifies_and_has_no_builtin_fromsql() {
        // Regression guard for the bug this was added to fix: MONEY used to
        // fall into the default Text arm, and string_fallback (String's
        // FromSql) doesn't accept the MONEY OID — so it silently became
        // Cell::Null. This must classify to its own kind, not Text.
        assert_eq!(classify(&Type::MONEY), CellKind::Money);
        assert_ne!(classify(&Type::MONEY), CellKind::Text);
    }

    #[test]
    fn pg_money_decodes_documented_binary_format() {
        // Postgres's binary wire format for `money` is a big-endian i64
        // scaled by 100 (the currency's minor unit), independent of
        // lc_monetary. Hand-built payloads so this is verifiable without a
        // live database.
        let raw = 12_345_i64.to_be_bytes(); // $123.45
        let v = <PgMoney as FromSql>::from_sql(&Type::MONEY, &raw).unwrap();
        assert_eq!(v.to_string(), "123.45");

        let zero = 0_i64.to_be_bytes();
        let v = <PgMoney as FromSql>::from_sql(&Type::MONEY, &zero).unwrap();
        assert_eq!(v.to_string(), "0.00");

        let small_cents = 7_i64.to_be_bytes(); // $0.07
        let v = <PgMoney as FromSql>::from_sql(&Type::MONEY, &small_cents).unwrap();
        assert_eq!(v.to_string(), "0.07");

        let negative = (-500_i64).to_be_bytes(); // -$5.00
        let v = <PgMoney as FromSql>::from_sql(&Type::MONEY, &negative).unwrap();
        assert_eq!(v.to_string(), "-5.00");

        let negative_cents_only = (-7_i64).to_be_bytes(); // -$0.07
        let v = <PgMoney as FromSql>::from_sql(&Type::MONEY, &negative_cents_only).unwrap();
        assert_eq!(v.to_string(), "-0.07");
    }

    #[test]
    fn pg_money_rejects_malformed_payload() {
        let too_short = [0u8; 4];
        assert!(<PgMoney as FromSql>::from_sql(&Type::MONEY, &too_short).is_err());
    }

    #[test]
    fn pg_money_only_accepts_money_oid() {
        assert!(<PgMoney as FromSql>::accepts(&Type::MONEY));
        assert!(!<PgMoney as FromSql>::accepts(&Type::INT8));
    }

    #[test]
    fn json_types_classify_as_json() {
        assert_eq!(classify(&Type::JSON), CellKind::Json);
        assert_eq!(classify(&Type::JSONB), CellKind::Json);
    }

    #[test]
    fn unknown_type_falls_back_to_text() {
        assert_eq!(classify(&Type::XML), CellKind::Text);
    }

    #[test]
    fn time_types_classify_to_their_own_kinds() {
        // Same regression as MONEY: these used to land on the default Text
        // arm, and String's FromSql rejects their OIDs, so every value became
        // a phantom Cell::Null. They must not classify as Text.
        assert_eq!(classify(&Type::TIME), CellKind::Time);
        assert_eq!(classify(&Type::TIMETZ), CellKind::TimeTz);
        assert_eq!(classify(&Type::INTERVAL), CellKind::Interval);
    }

    #[test]
    fn hms_formats_and_trims_fraction() {
        assert_eq!(fmt_hms(0), "00:00:00");
        assert_eq!(fmt_hms(52_200_000_000), "14:30:00");
        assert_eq!(fmt_hms(52_200_500_000), "14:30:00.5");
        assert_eq!(fmt_hms(52_200_000_001), "14:30:00.000001");
    }

    fn timetz_payload(micros: i64, zone_secs_west: i32) -> [u8; 12] {
        let mut raw = [0u8; 12];
        raw[..8].copy_from_slice(&micros.to_be_bytes());
        raw[8..].copy_from_slice(&zone_secs_west.to_be_bytes());
        raw
    }

    #[test]
    fn pg_timetz_decodes_documented_binary_format() {
        // 14:30:00+07 — pg stores the zone as seconds *west* of UTC, so +07:00
        // arrives as -25200.
        let raw = timetz_payload(52_200_000_000, -25_200);
        let v = <PgTimeTz as FromSql>::from_sql(&Type::TIMETZ, &raw).unwrap();
        assert_eq!(v.to_string(), "14:30:00+07:00");

        let utc = timetz_payload(0, 0);
        let v = <PgTimeTz as FromSql>::from_sql(&Type::TIMETZ, &utc).unwrap();
        assert_eq!(v.to_string(), "00:00:00+00:00");

        // 09:15:00-05:30 (a half-hour offset, east of UTC in pg's sign).
        let west = timetz_payload(33_300_000_000, 19_800);
        let v = <PgTimeTz as FromSql>::from_sql(&Type::TIMETZ, &west).unwrap();
        assert_eq!(v.to_string(), "09:15:00-05:30");
    }

    #[test]
    fn pg_timetz_rejects_malformed_payload_and_wrong_oid() {
        assert!(<PgTimeTz as FromSql>::from_sql(&Type::TIMETZ, &[0u8; 8]).is_err());
        assert!(<PgTimeTz as FromSql>::accepts(&Type::TIMETZ));
        assert!(!<PgTimeTz as FromSql>::accepts(&Type::TIME));
    }

    fn interval_payload(micros: i64, days: i32, months: i32) -> [u8; 16] {
        let mut raw = [0u8; 16];
        raw[..8].copy_from_slice(&micros.to_be_bytes());
        raw[8..12].copy_from_slice(&days.to_be_bytes());
        raw[12..].copy_from_slice(&months.to_be_bytes());
        raw
    }

    #[test]
    fn pg_interval_decodes_documented_binary_format() {
        let raw = interval_payload(14_706_000_000, 3, 14); // 1 yr 2 mons 3 days 04:05:06
        let v = <PgInterval as FromSql>::from_sql(&Type::INTERVAL, &raw).unwrap();
        assert_eq!(v.to_string(), "1 year 2 mons 3 days 04:05:06");

        let day_only = interval_payload(0, 1, 0);
        let v = <PgInterval as FromSql>::from_sql(&Type::INTERVAL, &day_only).unwrap();
        assert_eq!(v.to_string(), "1 day");

        // An all-zero interval still has to render as something visible.
        let zero = interval_payload(0, 0, 0);
        let v = <PgInterval as FromSql>::from_sql(&Type::INTERVAL, &zero).unwrap();
        assert_eq!(v.to_string(), "00:00:00");

        let negative = interval_payload(-1_000_000, 0, 0);
        let v = <PgInterval as FromSql>::from_sql(&Type::INTERVAL, &negative).unwrap();
        assert_eq!(v.to_string(), "-00:00:01");
    }

    #[test]
    fn pg_interval_rejects_malformed_payload_and_wrong_oid() {
        assert!(<PgInterval as FromSql>::from_sql(&Type::INTERVAL, &[0u8; 12]).is_err());
        assert!(<PgInterval as FromSql>::accepts(&Type::INTERVAL));
        assert!(!<PgInterval as FromSql>::accepts(&Type::TIME));
    }
}
